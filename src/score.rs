//! Scoring function for scout. Eight heuristics, six binary and two
//! linear-decay. Pure: no IO, no async, just `Factors + Weights ->
//! Breakdown`. Lives separate from the fetch/parse layers so the test
//! suite can exercise it without touching the network.

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
