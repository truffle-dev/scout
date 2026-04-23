//! Unit coverage for the pattern-match signal inference. Each signal
//! has a true-case, a false-case, and the edge cases that matter:
//! `None` body, empty string, case-insensitive matching, and the
//! kinds of false positives the scoring layer is willing to tolerate.

use scout::{Label, has_effort_label, has_reproducer, has_root_cause};

fn label(name: &str) -> Label {
    Label {
        name: name.to_string(),
    }
}

// --- has_reproducer --------------------------------------------------

#[test]
fn reproducer_true_on_fenced_code_block() {
    let body = "Here's what I did:\n\n```rust\nfoo();\n```\n";
    assert!(has_reproducer(Some(body)));
}

#[test]
fn reproducer_true_on_reproduce_keyword() {
    assert!(has_reproducer(Some("Steps to reproduce: open the file.")));
}

#[test]
fn reproducer_true_on_reproduce_case_insensitive() {
    assert!(has_reproducer(Some("REPRODUCE: open terminal")));
}

#[test]
fn reproducer_true_on_minimal_example() {
    assert!(has_reproducer(Some("Here's a minimal example that fails.")));
}

#[test]
fn reproducer_false_on_plain_prose() {
    assert!(!has_reproducer(Some("Something is wrong with the CLI.")));
}

#[test]
fn reproducer_false_on_none_body() {
    assert!(!has_reproducer(None));
}

#[test]
fn reproducer_false_on_empty_body() {
    assert!(!has_reproducer(Some("")));
}

// --- has_root_cause --------------------------------------------------

#[test]
fn root_cause_true_on_rust_file_line() {
    assert!(has_root_cause(Some(
        "Fails at src/resolver.rs:432 when the feature graph loops."
    )));
}

#[test]
fn root_cause_true_on_python_file_line() {
    assert!(has_root_cause(Some("traceback points at app/main.py:87.")));
}

#[test]
fn root_cause_true_on_typescript_file_line() {
    assert!(has_root_cause(Some(
        "see index.ts:10 for the offending line"
    )));
}

#[test]
fn root_cause_true_on_root_cause_phrase() {
    assert!(has_root_cause(Some(
        "The root cause appears to be an unset env var."
    )));
}

#[test]
fn root_cause_true_on_caused_by() {
    assert!(has_root_cause(Some(
        "This is caused by the default timeout being 0."
    )));
}

#[test]
fn root_cause_true_case_insensitive_phrase() {
    assert!(has_root_cause(Some(
        "Root Cause: off-by-one in the tokenizer."
    )));
}

#[test]
fn root_cause_false_on_file_without_line() {
    // `src/foo.rs` alone does not count; we want a concrete pointer.
    assert!(!has_root_cause(Some("The bug is somewhere in src/foo.rs")));
}

#[test]
fn root_cause_false_on_colon_without_digit() {
    assert!(!has_root_cause(Some("Check index.ts: it's a mess.")));
}

#[test]
fn root_cause_false_on_plain_prose() {
    assert!(!has_root_cause(Some("Something is broken.")));
}

#[test]
fn root_cause_false_on_none_body() {
    assert!(!has_root_cause(None));
}

// --- has_effort_label ------------------------------------------------

#[test]
fn effort_label_true_on_good_first_issue() {
    assert!(has_effort_label(&[label("good first issue")]));
}

#[test]
fn effort_label_true_on_help_wanted() {
    assert!(has_effort_label(&[label("help wanted")]));
}

#[test]
fn effort_label_true_on_effort_low() {
    assert!(has_effort_label(&[label("effort/low")]));
}

#[test]
fn effort_label_true_on_effort_medium_mixed_case() {
    assert!(has_effort_label(&[label("Effort/Medium")]));
}

#[test]
fn effort_label_true_when_mixed_with_other_labels() {
    assert!(has_effort_label(&[
        label("C-bug"),
        label("A-resolver"),
        label("good first issue"),
    ]));
}

#[test]
fn effort_label_false_on_enhancement_only() {
    assert!(!has_effort_label(&[
        label("enhancement"),
        label("discussion")
    ]));
}

#[test]
fn effort_label_false_on_empty_labels() {
    assert!(!has_effort_label(&[]));
}

#[test]
fn effort_label_false_on_near_miss() {
    // Partial match is intentionally not enough; scout favors false
    // negatives over false positives on the effort signal.
    assert!(!has_effort_label(&[label("effort/high")]));
    assert!(!has_effort_label(&[label("needs-effort-estimate")]));
}
