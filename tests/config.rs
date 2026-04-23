//! Parse-layer cases on `scout::parse_config`. Exercises default hydration
//! from an empty string, section-by-section overrides, unknown-key rejection,
//! and the `WeightsConfig -> Weights` conversion that the scoring layer
//! consumes. No filesystem IO; every case builds the TOML as a string
//! inline.

use scout::{Weights, parse_config};

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

/// Empty string parses to a fully-defaulted Config. This is the shape a
/// user gets when they ship `scout init` and never touch the file.
#[test]
fn empty_input_yields_defaults() {
    let cfg = parse_config("").expect("empty TOML should parse");
    assert_eq!(cfg.auth.token_path, None);
    assert_eq!(cfg.filters.max_age_days, 30);
    assert!(approx(cfg.filters.min_score, 0.50));
    assert_eq!(
        cfg.filters.exclude_labels,
        vec!["wontfix", "invalid", "duplicate"]
    );
    assert_eq!(cfg.output.color, "auto");
    assert_eq!(cfg.output.limit, 20);

    let w: Weights = cfg.weights.into();
    assert!(approx(w.root_cause, 0.30));
    assert!(approx(w.no_pr, 0.20));
    assert!(approx(w.recent, 0.15));
    assert!(approx(w.contributing_ok, 0.15));
    assert!(approx(w.reproducer, 0.10));
    assert!(approx(w.effort_ok, 0.10));
    assert!(approx(w.maintainer_touched, 0.05));
    assert!(approx(w.active_repo, 0.00));
}

/// Overriding one weight field leaves the others at their default. Serde's
/// `#[serde(default)]` on the struct means partial input is the common path.
#[test]
fn partial_weights_preserves_other_defaults() {
    let cfg = parse_config("[weights]\nroot_cause = 0.50\n").expect("parse");
    let w: Weights = cfg.weights.into();
    assert!(approx(w.root_cause, 0.50));
    assert!(approx(w.no_pr, 0.20), "no_pr should keep default");
    assert!(approx(w.recent, 0.15), "recent should keep default");
}

/// Full schema from the arch doc round-trips into the expected struct
/// values. Locks in the fact that `scout init`'s starter file parses
/// without error.
#[test]
fn full_schema_round_trip() {
    let src = r#"
[auth]
token_path = "~/.config/scout/token"

[weights]
root_cause         = 0.30
no_pr              = 0.20
recent             = 0.15
contributing_ok    = 0.15
reproducer         = 0.10
effort_ok          = 0.10
maintainer_touched = 0.05
active_repo        = 0.00

[filters]
max_age_days = 30
min_score    = 0.50
exclude_labels = ["wontfix", "invalid", "duplicate"]

[output]
color = "auto"
limit = 20
"#;
    let cfg = parse_config(src).expect("starter schema should parse");
    assert_eq!(
        cfg.auth.token_path.as_deref(),
        Some("~/.config/scout/token")
    );
    assert_eq!(cfg.filters.max_age_days, 30);
    assert_eq!(cfg.output.limit, 20);
    let w: Weights = cfg.weights.into();
    assert!(approx(w.root_cause, 0.30));
    assert!(approx(w.active_repo, 0.00));
}

/// Unknown keys fail the parse so a typo like `root_cuase` surfaces
/// instead of silently dropping back to the default weight.
#[test]
fn unknown_key_is_rejected() {
    let src = "[weights]\nroot_cuase = 0.50\n";
    let err = parse_config(src).expect_err("typo should reject");
    let msg = err.to_string();
    assert!(
        msg.contains("root_cuase") || msg.contains("unknown field"),
        "error should mention the bad key, got: {msg}"
    );
}

/// Filters section accepts an empty exclude_labels list without falling
/// back to the default three-label set. Needed so a user who deliberately
/// wants no label filtering can say so.
#[test]
fn empty_exclude_labels_is_honored() {
    let src = r#"
[filters]
max_age_days = 7
min_score = 0.25
exclude_labels = []
"#;
    let cfg = parse_config(src).expect("parse");
    assert_eq!(cfg.filters.max_age_days, 7);
    assert!(approx(cfg.filters.min_score, 0.25));
    assert!(cfg.filters.exclude_labels.is_empty());
}
