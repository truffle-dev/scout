//! Fetch-layer types + a pure JSON decoder for a single GitHub
//! `/repos/{owner}/{repo}` response. The HTTP client itself lands in a
//! later commit; splitting the decode step out keeps the serde shape
//! testable from a captured payload without requiring a network call or
//! an async runtime.
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
/// no IO, no async. Call this from both the live HTTP path (when that
/// ships) and from tests that want to exercise the decode against a
/// captured fixture.
pub fn decode_repo_meta(json: &str) -> Result<RepoMeta, serde_json::Error> {
    serde_json::from_str(json)
}

/// Errors the fetch layer can surface. `Decode` is the only variant
/// this commit can produce; HTTP + status variants land alongside the
/// async client.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    /// JSON body did not match `RepoMeta`'s shape. Usually means GitHub
    /// returned an error object (message + documentation_url) instead
    /// of a repo object.
    #[error("decode failed: {0}")]
    Decode(#[from] serde_json::Error),
}
