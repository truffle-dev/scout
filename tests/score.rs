//! 12 boundary cases on the pure scoring function. No network, no fixtures
//! beyond hand-built `Factors`. One case per heuristic at boundary, plus
//! a zero-factors case, a max-factors clamp case, and two decay-curve cases.

use scout::{Factors, Weights, score};

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

/// All factors false / days_ago far past horizon; total score is 0.0.
#[test]
fn all_factors_off_scores_zero() {
    let factors = Factors {
        has_root_cause: false,
        no_crosslinked_pr: false,
        updated_days_ago: 365.0,
        contributing_ok: false,
        has_reproducer: false,
        effort_ok: false,
        maintainer_touched: false,
        pushed_days_ago: 365.0,
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.0), "expected 0.0, got {}", b.total);
}

/// All factors true, day 0 on both decay axes; total clamps at 1.0
/// (raw sum would be 1.05 since active_repo carries weight 0 by default
/// but decay returns 1.0; re-testing the clamp requires a non-default
/// weight set).
#[test]
fn all_factors_on_clamps_at_one() {
    let factors = Factors {
        has_root_cause: true,
        no_crosslinked_pr: true,
        updated_days_ago: 0.0,
        contributing_ok: true,
        has_reproducer: true,
        effort_ok: true,
        maintainer_touched: true,
        pushed_days_ago: 0.0,
    };
    // Push active_repo weight up so the raw sum exceeds 1.0 and we
    // actually exercise the clamp.
    let w = Weights {
        active_repo: 0.20,
        ..Weights::default()
    };
    let b = score(&factors, &w);
    let raw: f64 = b.parts.iter().map(|(_, v)| v).sum();
    assert!(
        raw > 1.0,
        "raw sum {} should exceed 1.0 to exercise clamp",
        raw
    );
    assert!(
        approx(b.total, 1.0),
        "expected clamp to 1.0, got {}",
        b.total
    );
}

/// root_cause = true alone contributes exactly the weight.
#[test]
fn root_cause_alone_equals_weight() {
    let factors = Factors {
        has_root_cause: true,
        updated_days_ago: 365.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.30), "expected 0.30, got {}", b.total);
}

/// no_pr = true alone contributes exactly the weight.
#[test]
fn no_pr_alone_equals_weight() {
    let factors = Factors {
        no_crosslinked_pr: true,
        updated_days_ago: 365.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.20), "expected 0.20, got {}", b.total);
}

/// updated_days_ago = 0 with recent weight = 0.15 contributes 0.15.
#[test]
fn recent_at_day_zero_full_weight() {
    let factors = Factors {
        updated_days_ago: 0.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.15), "expected 0.15, got {}", b.total);
}

/// updated_days_ago = 14 (horizon boundary) contributes 0 to recent.
#[test]
fn recent_at_horizon_zero_contribution() {
    let factors = Factors {
        updated_days_ago: 14.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(
        approx(b.total, 0.0),
        "expected 0.0 at horizon, got {}",
        b.total
    );
}

/// updated_days_ago = 7 (half horizon) contributes half the weight.
#[test]
fn recent_at_half_horizon_half_weight() {
    let factors = Factors {
        updated_days_ago: 7.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(
        approx(b.total, 0.075),
        "expected 0.075 (half of 0.15), got {}",
        b.total
    );
}

/// contributing_ok = true alone contributes exactly the weight.
#[test]
fn contributing_ok_alone_equals_weight() {
    let factors = Factors {
        contributing_ok: true,
        updated_days_ago: 365.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.15), "expected 0.15, got {}", b.total);
}

/// reproducer = true alone contributes exactly the weight.
#[test]
fn reproducer_alone_equals_weight() {
    let factors = Factors {
        has_reproducer: true,
        updated_days_ago: 365.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.10), "expected 0.10, got {}", b.total);
}

/// effort_ok = true alone contributes exactly the weight.
#[test]
fn effort_ok_alone_equals_weight() {
    let factors = Factors {
        effort_ok: true,
        updated_days_ago: 365.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.10), "expected 0.10, got {}", b.total);
}

/// maintainer_touched = true alone contributes exactly the weight.
#[test]
fn maintainer_touched_alone_equals_weight() {
    let factors = Factors {
        maintainer_touched: true,
        updated_days_ago: 365.0,
        pushed_days_ago: 365.0,
        ..Default::default()
    };
    let b = score(&factors, &Weights::default());
    assert!(approx(b.total, 0.05), "expected 0.05, got {}", b.total);
}

/// breakdown.parts lists all eight heuristics in declared order, so
/// --explain can render them without reordering.
#[test]
fn breakdown_parts_preserve_heuristic_order() {
    let factors = Factors::default();
    let b = score(&factors, &Weights::default());
    let names: Vec<&str> = b.parts.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        names,
        vec![
            "root_cause",
            "no_pr",
            "recent",
            "contributing_ok",
            "reproducer",
            "effort_ok",
            "maintainer_touched",
            "active_repo",
        ]
    );
}
