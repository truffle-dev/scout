//! Scoring function for scout. Eight heuristics, six binary and two
//! linear-decay. Pure: no IO, no async, just `Factors + Weights ->
//! Breakdown`. Lives separate from the fetch/parse layers so the test
//! suite can exercise it without touching the network.
//!
//! The aggregator `factors_from` binds `IssueMeta + RepoMeta +
//! optional CONTRIBUTING body + now` into a `Factors` value. Two
//! fields (`no_crosslinked_pr`, `maintainer_touched`) still need
//! endpoints the fetch layer doesn't expose yet; those default to
//! `false` and will be filled in as the fetch layer grows.

use crate::fetch::{IssueMeta, RepoMeta};
use crate::infer::{
    contributing_looks_ok, days_since, has_effort_label, has_non_effort_label, has_reproducer,
    has_root_cause,
};

/// Observed properties of a single GitHub issue and its parent repo.
/// Populated by the fetch layer from REST + GraphQL; consumed by `score`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Factors {
    /// Issue body names a file:line or file+symbol.
    pub has_root_cause: bool,
    /// No open PR crosslinks the issue via `CROSS_REFERENCED_EVENT`.
    pub no_crosslinked_pr: bool,
    /// Days since the issue was last updated.
    pub updated_days_ago: f64,
    /// CONTRIBUTING.md lacks a CLA gate or "contact maintainers first"
    /// block.
    pub contributing_ok: bool,
    /// Issue body has a fenced code block, stack trace, or minimal-repro
    /// link.
    pub has_reproducer: bool,
    /// Issue is not labeled `enhancement`, `question`, `design`, `rfc`,
    /// `discussion`.
    pub effort_ok: bool,
    /// A top-5 committer has commented on the thread.
    pub maintainer_touched: bool,
    /// Days since the repo was last pushed.
    pub pushed_days_ago: f64,
}

/// Per-heuristic weights. Loaded from the user's TOML config.
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub root_cause: f64,
    pub no_pr: f64,
    pub recent: f64,
    pub contributing_ok: f64,
    pub reproducer: f64,
    pub effort_ok: f64,
    pub maintainer_touched: f64,
    pub active_repo: f64,
}

impl Default for Weights {
    /// Matches the defaults in the arch doc and in `scout init`.
    fn default() -> Self {
        Self {
            root_cause: 0.30,
            no_pr: 0.20,
            recent: 0.15,
            contributing_ok: 0.15,
            reproducer: 0.10,
            effort_ok: 0.10,
            maintainer_touched: 0.05,
            active_repo: 0.00,
        }
    }
}

/// Explainable scoring output. `total` is the clamped sum; `parts` lists
/// each heuristic's contribution so `--explain` can show the breakdown.
#[derive(Debug, Clone)]
pub struct Breakdown {
    pub total: f64,
    pub parts: Vec<(&'static str, f64)>,
}

/// Pure weighted sum, clamped at 1.0 for display. Six binary heuristics,
/// two linear-decay (`recent` over 14 days, `active_repo` over 30).
pub fn score(factors: &Factors, w: &Weights) -> Breakdown {
    let recent_score = decay(factors.updated_days_ago, 14.0);
    let active_score = decay(factors.pushed_days_ago, 30.0);

    let parts: Vec<(&'static str, f64)> = vec![
        ("root_cause", w.root_cause * b(factors.has_root_cause)),
        ("no_pr", w.no_pr * b(factors.no_crosslinked_pr)),
        ("recent", w.recent * recent_score),
        (
            "contributing_ok",
            w.contributing_ok * b(factors.contributing_ok),
        ),
        ("reproducer", w.reproducer * b(factors.has_reproducer)),
        ("effort_ok", w.effort_ok * b(factors.effort_ok)),
        (
            "maintainer_touched",
            w.maintainer_touched * b(factors.maintainer_touched),
        ),
        ("active_repo", w.active_repo * active_score),
    ];

    let total = parts.iter().map(|(_, v)| v).sum::<f64>().min(1.0);
    Breakdown { total, parts }
}

fn b(v: bool) -> f64 {
    if v { 1.0 } else { 0.0 }
}

fn decay(days: f64, horizon: f64) -> f64 {
    (1.0 - (days / horizon)).clamp(0.0, 1.0)
}

/// Build a `Factors` from the metadata the fetch layer can produce
/// today. `now_unix` is wall-clock seconds from the caller so scans
/// use a single consistent "now" across all issues. `contributing`
/// is the raw CONTRIBUTING body from [`crate::fetch::contributing_md`];
/// `None` means the repo has none, which the classifier treats as
/// contribution-friendly.
///
/// `effort_ok` is true when a positive low-effort label is present
/// OR when no explicit non-effort label blocks it; the positive label
/// wins when both coexist. Days that fail to parse decay to `0.0`
/// score via a large sentinel, which treats unparseable timestamps
/// as "effectively never updated" rather than "just now."
///
/// Fields left at their `false` defaults: `no_crosslinked_pr` (needs
/// cross-reference search), `maintainer_touched` (needs comments +
/// top-committers). A score computed from these defaults
/// systematically under-rates issues until the fetch layer fills
/// them in.
pub fn factors_from(
    issue: &IssueMeta,
    repo: &RepoMeta,
    contributing: Option<&str>,
    now_unix: i64,
) -> Factors {
    let updated_days_ago = days_to_f64(days_since(&issue.updated_at, now_unix));
    let pushed_days_ago = days_to_f64(days_since(&repo.pushed_at, now_unix));
    Factors {
        has_root_cause: has_root_cause(issue.body.as_deref()),
        has_reproducer: has_reproducer(issue.body.as_deref()),
        effort_ok: has_effort_label(&issue.labels) || !has_non_effort_label(&issue.labels),
        updated_days_ago,
        pushed_days_ago,
        no_crosslinked_pr: false,
        contributing_ok: contributing_looks_ok(contributing),
        maintainer_touched: false,
    }
}

/// Map a signed days-delta to the `f64` the `decay` function expects.
/// `None` (parse failure) becomes a large positive so the recency
/// decay clamps to 0. Negative deltas (future timestamps from clock
/// skew) become `0.0` so they score as maximally recent.
fn days_to_f64(days: Option<i64>) -> f64 {
    match days {
        Some(n) if n < 0 => 0.0,
        Some(n) => n as f64,
        None => 1_000_000.0,
    }
}
