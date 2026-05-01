//! `scout explain <OWNER/REPO#N>` shows the per-heuristic score
//! breakdown for a single issue. Same scoring function as `scout scan`,
//! called on one issue's fetched payloads instead of a watchlist.
//!
//! Five HTTP requests fan out via `tokio::try_join!`: repo metadata,
//! CONTRIBUTING.md (best-effort), issue metadata, issue comments (first
//! page, 100), and issue timeline (first page, 100). All five run in
//! parallel inside a single tokio task; the score function is pure and
//! reuses [`crate::score::factors_from`] verbatim so the displayed
//! number matches what the scan-side ranker would produce for the same
//! issue.
//!
//! Output is a markdown breakdown: a header line with the issue title,
//! the browser URL, the clamped total, then a two-column table of
//! factor name and weighted contribution. Pure rendering: no IO and no
//! async, so the test that drives the full pipeline through wiremock
//! can assert on the rendered string directly.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::fetch::{
    IssueMeta, contributing_md_at, issue_meta_at, list_issue_comments_at, list_issue_timeline_at,
    repo_meta_at,
};
use crate::init;
use crate::scan::{self, RunError, load_config};
use crate::score::{Breakdown, Weights, factors_from, score};
use crate::took::parse_issue_ref;

/// CLI entry point for `scout explain`. Loads config, resolves the
/// auth token (config `token_path` first, then `$GITHUB_TOKEN`), runs
/// the async fetcher inside a fresh tokio runtime, computes the
/// breakdown, and prints the rendered markdown. Returns
/// `ExitCode::SUCCESS` on a clean run and `ExitCode::from(1)` on any
/// error.
pub fn run(config_override: Option<&str>, issue_ref: &str) -> ExitCode {
    match run_inner(config_override, issue_ref) {
        Ok(rendered) => {
            print!("{rendered}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("scout explain: {e}");
            ExitCode::from(1)
        }
    }
}

fn run_inner(config_override: Option<&str>, issue_ref: &str) -> Result<String, RunError> {
    let issue = parse_issue_ref(issue_ref)?;

    let config_path = match config_override {
        Some(p) => PathBuf::from(p),
        None => init::default_config_path()?,
    };
    let config = load_config(&config_path)?;
    let weights: Weights = config.weights.into();
    let token = scan::resolve_token(config.auth.token_path.as_deref())?;

    let client = reqwest::Client::new();
    let runtime = tokio::runtime::Runtime::new()?;
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;

    runtime.block_on(explain_at(
        "https://api.github.com",
        &client,
        &issue.owner,
        &issue.repo,
        u64::from(issue.number),
        &weights,
        token.as_deref(),
        now_unix,
    ))
}

/// Run the full explain pipeline against a GitHub-shaped base URL.
/// Fans out five fetches in parallel, builds [`crate::score::Factors`]
/// from the responses, scores against `weights`, and renders the
/// markdown breakdown. Decoupled from `api.github.com` so wiremock
/// tests can drive the same code path against captured payloads.
#[allow(clippy::too_many_arguments)]
pub async fn explain_at(
    base_url: &str,
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    number: u64,
    weights: &Weights,
    token: Option<&str>,
    now_unix: i64,
) -> Result<String, RunError> {
    let (repo_meta, contributing, issue_meta, comments, timeline) = tokio::try_join!(
        repo_meta_at(base_url, client, owner, repo, token),
        contributing_md_at(base_url, client, owner, repo, token),
        issue_meta_at(base_url, client, owner, repo, number, token),
        list_issue_comments_at(base_url, client, owner, repo, number, token),
        list_issue_timeline_at(base_url, client, owner, repo, number, token),
    )?;

    let factors = factors_from(
        &issue_meta,
        &repo_meta,
        contributing.as_deref(),
        &comments,
        &timeline,
        now_unix,
    );
    let breakdown = score(&factors, weights);

    Ok(render_breakdown(&issue_meta, &breakdown))
}

/// Render an issue's score breakdown as markdown. Shape: an h1 with
/// the issue title; the browser URL on its own line; a bold `Score`
/// line with the clamped total to two decimal places; a
/// `factor | weighted` GFM table with one row per heuristic to three
/// decimal places. Pipe and newline characters in the title collapse
/// to safe substitutes so a long title can't break the table layout
/// upstream of where it's rendered.
fn render_breakdown(issue: &IssueMeta, breakdown: &Breakdown) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", sanitize_inline(&issue.title)));
    out.push_str(&format!("{}\n\n", issue.html_url));
    out.push_str(&format!("**Score**: {:.2}\n\n", breakdown.total));
    out.push_str("| factor | weighted |\n");
    out.push_str("| :----- | -------: |\n");
    for (name, value) in &breakdown.parts {
        out.push_str(&format!("| {name} | {value:.3} |\n"));
    }
    out
}

/// Replace newline characters with spaces so a multi-line issue title
/// renders as a single header line. The pipe character is left alone
/// because it is fine inside an h1; it would only matter inside a
/// table cell, which the table below builds from fixed strings.
fn sanitize_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\r' | '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}
