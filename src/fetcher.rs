//! Async fetch orchestrator. Walks a [`Watchlist`] and gathers the
//! per-repo bundle of payloads the synchronous [`scan::plan`] stage
//! consumes: `RepoMeta`, optional CONTRIBUTING body, paginated open
//! issues, and per-issue comment + timeline pages.
//!
//! This is the network-bound layer between the watchlist parse and
//! the synchronous planner. PR-shaped items are dropped before their
//! comment + timeline pages would have been fetched, so we don't burn
//! API quota on payloads the planner is going to filter anyway.
//!
//! Implementation is serial across repos and serial across issues
//! within a repo. A future slice replaces both loops with bounded
//! concurrency once rate-limit handling is in place; until then,
//! serial is the simpler shape and matches GitHub's secondary-rate-
//! limit guidance for an unauthenticated personal scan.
//!
//! Error policy follows the existing fetch layer: any non-2xx status
//! aborts the walk. Partial fetches would mislead the planner into
//! under-rating issues whose downstream pages happened to fail, so
//! the orchestrator returns the first error rather than a partial
//! `Vec<FetchedRepo>`. The caller decides whether to retry.

use crate::fetch::{
    DEFAULT_PAGE_CAP, FetchError, contributing_md_at, list_issue_comments_at,
    list_issue_timeline_at, list_issues_paginated_at, repo_meta_at,
};
use crate::scan::{FetchedIssue, FetchedRepo};
use crate::watchlist::Watchlist;

/// Fetch every payload the planner needs for a [`Watchlist`] from
/// `api.github.com`. Pagination cap defaults to [`DEFAULT_PAGE_CAP`].
pub async fn fetch_repos(
    watchlist: &Watchlist,
    token: Option<&str>,
) -> Result<Vec<FetchedRepo>, FetchError> {
    let client = reqwest::Client::new();
    fetch_repos_at(
        "https://api.github.com",
        &client,
        watchlist,
        token,
        DEFAULT_PAGE_CAP,
    )
    .await
}

/// Same orchestration as [`fetch_repos`], but parameterized on the
/// base URL, the reqwest client, and the page cap so wiremock-backed
/// tests and callers that want a custom client (timeouts, proxy,
/// connection pool) can inject their own. Order of returned
/// `FetchedRepo`s matches the order of `watchlist.repos`.
pub async fn fetch_repos_at(
    base_url: &str,
    client: &reqwest::Client,
    watchlist: &Watchlist,
    token: Option<&str>,
    max_pages: usize,
) -> Result<Vec<FetchedRepo>, FetchError> {
    let mut out = Vec::with_capacity(watchlist.repos.len());
    for entry in &watchlist.repos {
        let owner = &entry.owner;
        let repo_name = &entry.repo;
        let repo = repo_meta_at(base_url, client, owner, repo_name, token).await?;
        let contributing = contributing_md_at(base_url, client, owner, repo_name, token).await?;
        let raw_issues =
            list_issues_paginated_at(base_url, client, owner, repo_name, token, max_pages).await?;

        let mut issues = Vec::with_capacity(raw_issues.len());
        for issue in raw_issues {
            if issue.is_pull_request() {
                continue;
            }
            let comments =
                list_issue_comments_at(base_url, client, owner, repo_name, issue.number, token)
                    .await?;
            let timeline =
                list_issue_timeline_at(base_url, client, owner, repo_name, issue.number, token)
                    .await?;
            issues.push(FetchedIssue {
                issue,
                comments,
                timeline,
            });
        }

        out.push(FetchedRepo {
            repo,
            contributing,
            issues,
        });
    }
    Ok(out)
}
