//! Fetch-layer types, pure decoders, and async HTTP clients for the
//! GitHub endpoints scout depends on.
//!
//! The decode step is split from the HTTP call on every endpoint so the
//! serde shapes are testable from captured payloads without requiring a
//! network call or an async runtime. The HTTP calls are parameterized
//! on the base URL so integration tests can point them at a mock
//! server.
//!
//! On-the-wire responses from GitHub carry dozens of fields we don't
//! care about. We slice out only what the scoring layer needs; extra
//! fields are ignored because `#[derive(Deserialize)]` without
//! `deny_unknown_fields` tolerates them. GitHub adds fields over time;
//! we don't want a new upstream field to break the parse.

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Minimal repo metadata sliced out of a `/repos/{owner}/{repo}`
/// response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RepoMeta {
    /// `owner/repo` slug, same shape scout uses internally.
    pub full_name: String,
    /// Star count. Not used in the default ranking, surfaced for
    /// `scout explain`.
    pub stargazers_count: u32,
    /// Open-issue count at fetch time. Note: GitHub counts open PRs
    /// in this number too.
    pub open_issues_count: u32,
    /// ISO-8601 timestamp of the last push to the repo.
    pub pushed_at: String,
    /// GitHub marks archived repos as read-only.
    pub archived: bool,
}

/// Minimal issue metadata sliced out of a single element of a
/// `/repos/{owner}/{repo}/issues` response. GitHub's issues endpoint
/// returns both true issues and PRs; the `pull_request` field is only
/// present when the item is a PR. Callers filter via
/// [`IssueMeta::is_pull_request`] depending on whether they want the
/// issues-only subset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IssueMeta {
    /// Issue number as shown in the URL (e.g. `#1234`).
    pub number: u64,
    /// Issue title.
    pub title: String,
    /// Issue body (Markdown). `null` in the response if empty; we
    /// surface that as `None`.
    #[serde(default)]
    pub body: Option<String>,
    /// Browser URL for the issue.
    pub html_url: String,
    /// `"open"` or `"closed"`. The fetch layer defaults to open; we
    /// keep the field so a caller that wants closed issues still gets
    /// the truth.
    pub state: String,
    /// Label names. We don't care about color/description on the
    /// label objects, only the names, but we decode the full object
    /// to stay faithful to the API and let a future scoring pass
    /// reach for more fields without another migration.
    pub labels: Vec<Label>,
    /// Comment count at fetch time. Drives the `maintainer_touched`
    /// heuristic when paired with a comments-author fetch.
    pub comments: u32,
    /// ISO-8601 issue-creation timestamp.
    pub created_at: String,
    /// ISO-8601 last-activity timestamp.
    pub updated_at: String,
    /// Original reporter.
    pub user: UserRef,
    /// Present iff this "issue" is a PR. The `html_url` inside points
    /// at the PR page; existence alone is enough to classify.
    #[serde(default)]
    pub pull_request: Option<PullRequestRef>,
}

impl IssueMeta {
    /// Whether this item is a pull request. GitHub's issues endpoint
    /// returns both; scoring usually wants the issues-only subset.
    pub fn is_pull_request(&self) -> bool {
        self.pull_request.is_some()
    }
}

/// Single label name. We deserialize as an object (matching GitHub's
/// wire shape) and expose only the name, which is all the scoring
/// layer currently consumes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Label {
    /// Label text (`"bug"`, `"good first issue"`, `"P1"`, etc.).
    pub name: String,
}

/// Reference to a GitHub user. The wire shape is much larger; we
/// decode only what the scoring layer and `scout explain` need.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UserRef {
    /// GitHub username.
    pub login: String,
}

/// Reference to the PR side of a PR-shaped "issue". Presence of this
/// field in an `/issues` response means the item is a PR.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PullRequestRef {
    /// Browser URL of the pull request.
    pub html_url: String,
}

/// Parse a `/repos/{owner}/{repo}` JSON body into a `RepoMeta`. Pure:
/// no IO, no async.
pub fn decode_repo_meta(json: &str) -> Result<RepoMeta, serde_json::Error> {
    serde_json::from_str(json)
}

/// Parse a `/repos/{owner}/{repo}/issues` JSON body into a list of
/// `IssueMeta`. Pure: no IO, no async.
pub fn decode_issue_list(json: &str) -> Result<Vec<IssueMeta>, serde_json::Error> {
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
    /// JSON body did not match the expected shape. Usually means
    /// GitHub returned an error object instead of the payload we
    /// asked for, or the endpoint changed.
    #[error("decode failed: {0}")]
    Decode(#[from] serde_json::Error),
}

/// User-Agent header sent on every request. GitHub requires one;
/// requests without it are rejected.
const USER_AGENT: &str = concat!("scout/", env!("CARGO_PKG_VERSION"));

/// Shared GET helper for JSON endpoints. Handles the request-shaping
/// (User-Agent, Accept, X-GitHub-Api-Version, optional bearer auth),
/// status check, and decode. Extracted so each endpoint function stays
/// focused on its URL and the shape of the response.
async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<T, FetchError> {
    let (body, _next) = get_json_with_next(client, url, token).await?;
    Ok(body)
}

/// Like `get_json`, but also returns the `rel="next"` URL from the
/// response's `Link` header (if present). Used by paginated endpoints
/// to walk GitHub's cursor chain without hand-rolling page numbers.
async fn get_json_with_next<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<(T, Option<String>), FetchError> {
    let mut req = client
        .get(url)
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
            url: url.to_string(),
        });
    }

    let next = resp
        .headers()
        .get(reqwest::header::LINK)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_next_link);

    let body = resp.text().await?;
    Ok((serde_json::from_str(&body)?, next))
}

/// Default page cap for paginated list endpoints. At 100 items per
/// page this covers up to 1000 issues, which is more than enough for a
/// single-repo scan of the active working set. A repo with more than
/// 1000 open issues is a repo whose maintainers want triage, not a
/// whole-list crawl from us.
pub const DEFAULT_PAGE_CAP: usize = 10;

/// Extract the `rel="next"` URL from an RFC 8288 Link header value.
/// GitHub emits a narrow shape: comma-separated entries of the form
/// `<URL>; rel="value"` with no commas inside the angle brackets and
/// no other parameters we care about. Returns `None` if no `next`
/// relation is present, if the header is empty, or if the shape is
/// malformed enough that we can't confidently pull a URL out.
///
/// Public for the unit-test suite. Not part of the fetch-layer public
/// contract in the docs sense; callers should use the paginated
/// endpoint functions.
pub fn parse_next_link(header: &str) -> Option<String> {
    let mut rest = header;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            return None;
        }
        if !rest.starts_with('<') {
            return None;
        }
        let url_end = rest.find('>')?;
        let url = &rest[1..url_end];
        rest = &rest[url_end + 1..];

        let entry_end = rest.find(',').unwrap_or(rest.len());
        let params = &rest[..entry_end];
        rest = &rest[entry_end..];

        for part in params.split(';') {
            let part = part.trim();
            let Some(eq) = part.find('=') else {
                continue;
            };
            let (k, v) = part.split_at(eq);
            let k = k.trim();
            let v = v[1..].trim().trim_matches('"');
            if k == "rel" && v == "next" {
                return Some(url.to_string());
            }
        }
    }
}

/// Fetch a raw file body from a GitHub `/contents/{path}` URL.
/// Returns `Ok(Some(body))` on 200, `Ok(None)` on 404, and propagates
/// other non-2xx statuses or transport failures. The `Accept:
/// application/vnd.github.raw` media type asks GitHub to return the
/// file bytes directly instead of the default JSON envelope with
/// base64-encoded content, which keeps the decode side trivial.
async fn get_raw_or_none(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<Option<String>, FetchError> {
    let mut req = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github.raw")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }

    let resp = req.send().await?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(FetchError::Status {
            status: status.as_u16(),
            url: url.to_string(),
        });
    }
    Ok(Some(resp.text().await?))
}

/// Fetch repo metadata from `api.github.com`.
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
    get_json(client, &url, token).await
}

/// Fetch the first page of open issues for a repo from
/// `api.github.com`. Pagination lands in a follow-up commit; this
/// function returns whatever the first page contains (up to 100
/// items) and is enough for small repos or a "what's hot today"
/// sample.
pub async fn list_issues(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Vec<IssueMeta>, FetchError> {
    list_issues_at("https://api.github.com", client, owner, repo, token).await
}

/// Single-page issue list from an arbitrary GitHub-shaped base URL.
/// Returns up to 100 open issues (and PR-shaped items; caller filters
/// via [`IssueMeta::is_pull_request`] if they want issues-only).
pub async fn list_issues_at(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Vec<IssueMeta>, FetchError> {
    let url = format!("{base_url}/repos/{owner}/{repo}/issues?state=open&per_page=100");
    get_json(client, &url, token).await
}

/// Fetch all open issues for a repo from `api.github.com`, walking
/// GitHub's `Link: rel="next"` cursor chain up to `DEFAULT_PAGE_CAP`
/// pages. Stops early if a page has no `next` relation. Returns the
/// concatenated issue list.
pub async fn list_issues_paginated(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Vec<IssueMeta>, FetchError> {
    list_issues_paginated_at(
        "https://api.github.com",
        client,
        owner,
        repo,
        token,
        DEFAULT_PAGE_CAP,
    )
    .await
}

/// Candidate paths for CONTRIBUTING content, probed in order. Covers
/// the repo-root convention, the `.github/` modern convention, and
/// the `docs/`-subdir pattern. A repo with CONTRIBUTING under any
/// other path is treated as "no CONTRIBUTING" here; the classifier
/// downstream defaults that to contribution-friendly, which matches
/// the typical shape of such repos (small, no explicit gates).
pub const CONTRIBUTING_PATHS: &[&str] = &[
    "CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
];

/// Fetch a repo's CONTRIBUTING.md body from `api.github.com`,
/// trying the paths in [`CONTRIBUTING_PATHS`] in order. Returns the
/// raw Markdown body of the first path that resolves, `Ok(None)` if
/// none do, or an error on non-404 failures.
pub async fn contributing_md(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Option<String>, FetchError> {
    contributing_md_at("https://api.github.com", client, owner, repo, token).await
}

/// CONTRIBUTING fetch from an arbitrary GitHub-shaped base URL, used
/// by the wiremock integration tests and by callers who want to
/// point at a mirror. Walks the candidate path list and returns at
/// the first 200.
pub async fn contributing_md_at(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Option<String>, FetchError> {
    for path in CONTRIBUTING_PATHS {
        let url = format!("{base_url}/repos/{owner}/{repo}/contents/{path}");
        if let Some(body) = get_raw_or_none(client, &url, token).await? {
            return Ok(Some(body));
        }
    }
    Ok(None)
}

/// Paginated issue list from an arbitrary GitHub-shaped base URL,
/// with an explicit page cap. `max_pages` is a hard upper bound: the
/// walker returns the items collected so far if the cap is reached
/// before a page without `rel="next"` appears. A failure on any page
/// aborts the walk and discards prior pages; partial page sets would
/// mislead the scoring layer into under-rating issues on later pages,
/// so the caller should re-run the scan instead of coping with the
/// truncation.
pub async fn list_issues_paginated_at(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    max_pages: usize,
) -> Result<Vec<IssueMeta>, FetchError> {
    let first = format!("{base_url}/repos/{owner}/{repo}/issues?state=open&per_page=100");
    let mut url = first;
    let mut out: Vec<IssueMeta> = Vec::new();
    for _ in 0..max_pages {
        let (page, next): (Vec<IssueMeta>, Option<String>) =
            get_json_with_next(client, &url, token).await?;
        out.extend(page);
        match next {
            Some(n) => url = n,
            None => return Ok(out),
        }
    }
    Ok(out)
}
