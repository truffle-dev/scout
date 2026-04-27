//! Pure ranking layer. Takes a slice of pre-fetched per-issue
//! payloads, derives `Factors` for each, scores them, and returns
//! the rows sorted by total score descending. No IO, no async; the
//! orchestrator that gathers the payloads and calls this module
//! lives separately so the network layer can be substituted in
//! tests.
//!
//! The ranker is the consumer of the factor-wiring phase: every
//! field in `Factors` now reads from real fetched payloads, and
//! `rank` is the place those payloads turn back into a single
//! ordered result that `scout scan` can render.

use crate::fetch::{CommentMeta, IssueMeta, RepoMeta, TimelineEvent};
use crate::score::{Breakdown, Weights, factors_from, score};

/// A single per-issue input bundle. Borrows the underlying
/// metadata so the orchestrator can keep ownership of its fetch
/// buffers across the rank call.
#[derive(Debug)]
pub struct RankInput<'a> {
    pub issue: &'a IssueMeta,
    pub repo: &'a RepoMeta,
    pub contributing: Option<&'a str>,
    pub comments: &'a [CommentMeta],
    pub timeline: &'a [TimelineEvent],
}

/// One row of the ranked output. Holds the breakdown alongside the
/// identifying fields a renderer needs (the repo's `full_name`,
/// issue number, title, browser URL).
#[derive(Debug, Clone)]
pub struct RankedRow {
    pub full_name: String,
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub breakdown: Breakdown,
}

/// Rank a slice of per-issue inputs. Returns a `Vec<RankedRow>`
/// sorted by `breakdown.total` descending. Ties preserve input
/// order (the underlying sort is stable). The caller decides
/// whether to truncate; cooldown filtering against the local
/// ledger is the orchestrator's job, not the ranker's, so a row
/// the caller wants to skip should be omitted from `inputs`
/// upstream.
///
/// `now_unix` is wall-clock seconds and is forwarded verbatim to
/// `factors_from`, so a single scan run scores every row against
/// one consistent "now."
pub fn rank(inputs: &[RankInput<'_>], weights: &Weights, now_unix: i64) -> Vec<RankedRow> {
    let mut rows: Vec<RankedRow> = inputs
        .iter()
        .map(|input| {
            let factors = factors_from(
                input.issue,
                input.repo,
                input.contributing,
                input.comments,
                input.timeline,
                now_unix,
            );
            let breakdown = score(&factors, weights);
            RankedRow {
                full_name: input.repo.full_name.clone(),
                number: input.issue.number,
                title: input.issue.title.clone(),
                html_url: input.issue.html_url.clone(),
                breakdown,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.breakdown
            .total
            .partial_cmp(&a.breakdown.total)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}
