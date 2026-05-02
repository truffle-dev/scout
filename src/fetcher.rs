//! Async fetch orchestrator. Walks a [`Watchlist`] and gathers the
//! per-repo bundle of payloads the synchronous [`scan::plan`] stage
//! consumes: `RepoMeta`, optional CONTRIBUTING body, paginated open
//! issues, and per-issue comment + timeline pages.
//!
//! Concurrency is bounded by a shared `Semaphore` whose permit count
//! is the [`DEFAULT_CONCURRENCY`] knob (default 8). Repos run in
//! parallel; within each repo, issues run in parallel; within each
//! issue, comments and timeline run sequentially because the comment
//! fetch is the cheap fail-fast guard for the timeline fetch (a 503
//! on comments shouldn't burn another request slot on timeline). The
//! total in-flight HTTP request count never exceeds the configured
//! cap, regardless of how many repos and issues are being processed.
//!
//! PR-shaped items are dropped before their comment + timeline pages
//! would have been fetched, so we don't burn API quota on payloads
//! the planner is going to filter anyway. The same early-skip applies
//! to issues older than `max_age_days`: the issue listing has the
//! `created_at` we need, so we can decide before spawning the per-issue
//! task. `0` disables the age skip, matching the planner's "0 = no
//! filter" convention.
//!
//! Error policy follows the existing fetch layer: any non-2xx status
//! aborts the walk by returning the first error reached. Tasks
//! already spawned but not yet awaited may continue to consume
//! permits and complete their HTTP calls before the error reaches
//! the caller; this is intentional, since the cost is at most a few
//! extra requests on a failed scan and aborting them mid-flight
//! would leave half-finished request states in flight.
//!
//! Order of returned `FetchedRepo`s matches the order of
//! `watchlist.repos`. Within each repo, `issues` order matches the
//! order GitHub returned them with PR-shaped items removed.

use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

use crate::fetch::{
    DEFAULT_PAGE_CAP, FetchError, IssueMeta, contributing_md_at, list_issue_comments_at,
    list_issue_timeline_at, list_issues_paginated_at, repo_meta_at,
};
use crate::infer::days_since;
use crate::scan::{FetchedIssue, FetchedRepo};
use crate::watchlist::Watchlist;

/// Default cap on the number of HTTP requests in flight at once
/// across all repos and all issues. Tuned to stay well under
/// GitHub's authenticated rate ceiling on a typical personal-scan
/// watchlist; an unauthenticated caller should pass a lower value
/// via [`fetch_repos_at_with_concurrency`] to avoid the secondary
/// rate limit.
pub const DEFAULT_CONCURRENCY: usize = 8;

/// Pre-fetch issue age filter applied at the orchestrator level so
/// that issues the planner is going to drop don't pay for their
/// per-issue comments + timeline requests. `max_age_days = 0` matches
/// the planner's "no filter" convention; when it's 0 the `now_unix`
/// field is ignored. The filter compares against `IssueMeta.created_at`
/// for parity with `scan::plan`.
#[derive(Debug, Clone, Copy)]
pub struct AgeFilter {
    /// Maximum issue age in days. `0` disables the filter.
    pub max_age_days: u32,
    /// Reference timestamp the age check resolves against. Callers
    /// take a single wall-clock reading per scan so plan and fetch
    /// agree on what "today" means.
    pub now_unix: i64,
}

impl AgeFilter {
    /// No-op filter: every non-PR issue gets its bundle fetched.
    /// Convenient default for callers that don't want age skipping.
    pub const fn disabled() -> Self {
        Self {
            max_age_days: 0,
            now_unix: 0,
        }
    }
}

/// Fetch every payload the planner needs for a [`Watchlist`] from
/// `api.github.com`. Pagination cap defaults to [`DEFAULT_PAGE_CAP`];
/// concurrency cap defaults to [`DEFAULT_CONCURRENCY`].
///
/// `age_filter` carries the same `max_age_days` knob the planner applies
/// in `scan::plan`; passing it here lets the fetcher skip the per-issue
/// comments + timeline requests for issues the planner is going to drop
/// anyway. Pass [`AgeFilter::disabled`] to fetch every non-PR issue.
pub async fn fetch_repos(
    watchlist: &Watchlist,
    token: Option<&str>,
    age_filter: AgeFilter,
) -> Result<Vec<FetchedRepo>, FetchError> {
    let client = reqwest::Client::new();
    fetch_repos_at(
        "https://api.github.com",
        &client,
        watchlist,
        token,
        DEFAULT_PAGE_CAP,
        age_filter,
    )
    .await
}

/// Same orchestration as [`fetch_repos`], but parameterized on the
/// base URL, the reqwest client, and the page cap so wiremock-backed
/// tests and callers that want a custom client (timeouts, proxy,
/// connection pool) can inject their own. Concurrency cap defaults
/// to [`DEFAULT_CONCURRENCY`]. Order of returned `FetchedRepo`s
/// matches the order of `watchlist.repos`.
pub async fn fetch_repos_at(
    base_url: &str,
    client: &reqwest::Client,
    watchlist: &Watchlist,
    token: Option<&str>,
    max_pages: usize,
    age_filter: AgeFilter,
) -> Result<Vec<FetchedRepo>, FetchError> {
    fetch_repos_at_with_concurrency(
        base_url,
        client,
        watchlist,
        token,
        max_pages,
        DEFAULT_CONCURRENCY,
        age_filter,
    )
    .await
}

/// Same orchestration as [`fetch_repos_at`], but with an explicit
/// concurrency cap. `concurrency` is the maximum number of HTTP
/// requests in flight at once across all repos and all issues; a
/// value of `0` is clamped to `1`. Order of returned `FetchedRepo`s
/// matches the order of `watchlist.repos`.
pub async fn fetch_repos_at_with_concurrency(
    base_url: &str,
    client: &reqwest::Client,
    watchlist: &Watchlist,
    token: Option<&str>,
    max_pages: usize,
    concurrency: usize,
    age_filter: AgeFilter,
) -> Result<Vec<FetchedRepo>, FetchError> {
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));

    let mut repo_tasks: Vec<JoinHandle<Result<FetchedRepo, FetchError>>> =
        Vec::with_capacity(watchlist.repos.len());
    for entry in &watchlist.repos {
        let base = base_url.to_string();
        let client = client.clone();
        let owner = entry.owner.clone();
        let repo_name = entry.repo.clone();
        let token = token.map(|t| t.to_string());
        let sem = sem.clone();

        repo_tasks.push(tokio::spawn(async move {
            fetch_one_repo(
                &base,
                &client,
                &owner,
                &repo_name,
                token.as_deref(),
                max_pages,
                sem,
                age_filter,
            )
            .await
        }));
    }

    let mut out = Vec::with_capacity(repo_tasks.len());
    for task in repo_tasks {
        out.push(task.await.expect("repo task panicked")?);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn fetch_one_repo(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo_name: &str,
    token: Option<&str>,
    max_pages: usize,
    sem: Arc<Semaphore>,
    age_filter: AgeFilter,
) -> Result<FetchedRepo, FetchError> {
    let repo = {
        let _permit = sem.acquire().await.expect("semaphore closed");
        repo_meta_at(base_url, client, owner, repo_name, token).await?
    };
    let contributing = {
        let _permit = sem.acquire().await.expect("semaphore closed");
        contributing_md_at(base_url, client, owner, repo_name, token).await?
    };
    let raw_issues = {
        let _permit = sem.acquire().await.expect("semaphore closed");
        list_issues_paginated_at(base_url, client, owner, repo_name, token, max_pages).await?
    };

    let mut issue_tasks: Vec<JoinHandle<Result<FetchedIssue, FetchError>>> = Vec::new();
    for issue in raw_issues {
        if issue.is_pull_request() {
            continue;
        }
        if age_filter.max_age_days != 0
            && let Some(days) = days_since(&issue.created_at, age_filter.now_unix)
            && days > age_filter.max_age_days as i64
        {
            continue;
        }
        let base = base_url.to_string();
        let client = client.clone();
        let owner = owner.to_string();
        let repo_name = repo_name.to_string();
        let token = token.map(|t| t.to_string());
        let sem = sem.clone();

        issue_tasks.push(tokio::spawn(async move {
            fetch_one_issue(
                &base,
                &client,
                &owner,
                &repo_name,
                issue,
                token.as_deref(),
                sem,
            )
            .await
        }));
    }

    let mut issues = Vec::with_capacity(issue_tasks.len());
    for task in issue_tasks {
        issues.push(task.await.expect("issue task panicked")?);
    }

    Ok(FetchedRepo {
        repo,
        contributing,
        issues,
    })
}

async fn fetch_one_issue(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo_name: &str,
    issue: IssueMeta,
    token: Option<&str>,
    sem: Arc<Semaphore>,
) -> Result<FetchedIssue, FetchError> {
    let comments = {
        let _permit = sem.acquire().await.expect("semaphore closed");
        list_issue_comments_at(base_url, client, owner, repo_name, issue.number, token).await?
    };
    let timeline = {
        let _permit = sem.acquire().await.expect("semaphore closed");
        list_issue_timeline_at(base_url, client, owner, repo_name, issue.number, token).await?
    };
    Ok(FetchedIssue {
        issue,
        comments,
        timeline,
    })
}
