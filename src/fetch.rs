//! Fetch-layer types, a pure JSON decoder, and an async HTTP client
//! for a single GitHub `/repos/{owner}/{repo}` response.
//!
//! The decode step is split from the HTTP call so the serde shape is
//! testable from a captured payload without requiring a network call
//! or an async runtime. The HTTP call itself is parameterized on the
//! base URL so integration tests can point it at a mock server.
//!
//! The on-the-wire response from GitHub carries dozens of fields we
//! don't care about. `RepoMeta` picks out only what the scoring layer
//! needs (pushed_at drives `active_repo` decay; archived + open_issues
//! drive the watchlist filter; full_name is the canonical slug).

use serde::Deserialize;

/// Minimal repo metadata sliced out of a `/repos/{owner}/{repo}`
/// response. Extra fields in the upstream JSON are ignored, which is
/// the behavior `#[derive(Deserialize)]` gives us without
/// `deny_unknown_fields`. GitHub adds fields over time; we don't want
/// a new upstream field to break the parse.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RepoMeta {
    /// `owner/repo` slug, same shape scout uses internally.
    pub full_name: String,
    /// Star count. Not used in the default ranking, surfaced for
    /// `scout explain`.
    pub stargazers_count: u32,
    /// Open-issue count at fetch time. Used by watchlist filtering to
    /// skip repos that have nothing triageable.
    pub open_issues_count: u32,
    /// ISO-8601 timestamp of the last push to the repo. Parsing into a
    /// datetime happens in the scoring layer so this module stays free
    /// of a chrono dependency.
    pub pushed_at: String,
    /// GitHub marks archived repos as read-only. We never rank issues
    /// in archived repos, regardless of other signals.
    pub archived: bool,
}

/// Parse a `/repos/{owner}/{repo}` JSON body into a `RepoMeta`. Pure:
/// no IO, no async. Call this from both the live HTTP path and from
/// tests that want to exercise the decode against a captured fixture.
pub fn decode_repo_meta(json: &str) -> Result<RepoMeta, serde_json::Error> {
    serde_json::from_str(json)
}

/// Errors the fetch layer can surface.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// Network layer failure: DNS, TLS, timeout, connection reset.
    #[error("http transport failed: {0}")]
    Http(#[from] reqwest::Error),
    /// Non-2xx status from GitHub. 404 means the repo is private,
    /// renamed, or doesn't exist; 403 usually means a rate-limit or
    /// auth issue.
    #[error("github returned {status} for {url}")]
    Status { status: u16, url: String },
    /// JSON body did not match `RepoMeta`'s shape. Usually means GitHub
    /// returned an error object (message + documentation_url) instead
    /// of a repo object, or the endpoint changed.
    #[error("decode failed: {0}")]
    Decode(#[from] serde_json::Error),
}

/// User-Agent header sent on every request. GitHub requires one;
/// requests without it are rejected.
const USER_AGENT: &str = concat!("scout/", env!("CARGO_PKG_VERSION"));

/// Fetch repo metadata from `api.github.com`. Thin convenience wrapper
/// around [`repo_meta_at`] with the production base URL baked in.
pub async fn repo_meta(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<RepoMeta, FetchError> {
    repo_meta_at("https://api.github.com", client, owner, repo, token).await
}

/// Fetch repo metadata from an arbitrary GitHub-shaped base URL. The
/// `base_url` should include scheme and host but no trailing slash
/// (e.g. `https://api.github.com`). Tests point this at a wiremock
/// server's `uri()`.
pub async fn repo_meta_at(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<RepoMeta, FetchError> {
    let url = format!("{base_url}/repos/{owner}/{repo}");

    let mut req = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }

    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(FetchError::Status {
            status: status.as_u16(),
            url,
        });
    }

    let body = resp.text().await?;
    Ok(decode_repo_meta(&body)?)
}
