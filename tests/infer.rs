//! Unit coverage for the pattern-match signal inference. Each signal
//! has a true-case, a false-case, and the edge cases that matter:
//! `None` body, empty string, case-insensitive matching, and the
//! kinds of false positives the scoring layer is willing to tolerate.

use scout::{
    CommentMeta, Label, PullRequestRef, TimelineEvent, TimelineSource, TimelineSourceIssue,
    UserRef, contributing_looks_ok, crosslinked_open_pr_in_timeline, days_since, has_effort_label,
    has_non_effort_label, has_reproducer, has_root_cause, maintainer_in_comments,
};

fn label(name: &str) -> Label {
    Label {
        name: name.to_string(),
    }
}

fn comment(association: &str) -> CommentMeta {
    CommentMeta {
        user: UserRef { login: "u".into() },
        author_association: association.into(),
    }
}

fn cross_ref(state: &str, is_pr: bool) -> TimelineEvent {
    TimelineEvent {
        event: "cross-referenced".into(),
        source: Some(TimelineSource {
            issue: Some(TimelineSourceIssue {
                state: state.into(),
                pull_request: is_pr.then(|| PullRequestRef {
                    html_url: "https://example.invalid/pr".into(),
                }),
            }),
        }),
    }
}

fn other_event(name: &str) -> TimelineEvent {
    TimelineEvent {
        event: name.into(),
        source: None,
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

// --- contributing_looks_ok -------------------------------------------

#[test]
fn contributing_ok_on_missing_body() {
    // No CONTRIBUTING in the repo: default to ok. Most small repos
    // don't ship one and are contribution-friendly by convention.
    assert!(contributing_looks_ok(None));
}

#[test]
fn contributing_ok_on_plain_friendly_body() {
    let body = "\
        # Contributing\n\
        Thanks for your interest! Fork the repo, open a PR, and we'll\n\
        take a look. Please add a test for any bug fix.\n";
    assert!(contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_cla_full_phrase() {
    let body = "\
        All contributors must sign our Contributor License Agreement\n\
        before we can merge any changes.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_cla_uk_spelling() {
    let body = "You must sign our Contributor Licence Agreement.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_cla_assistant_bot() {
    let body = "Our cla-assistant bot will prompt you for a signature.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_easycla() {
    let body = "See the EasyCLA docs for the signing workflow.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_eclipse_contributor_agreement() {
    let body = "Please sign the Eclipse Contributor Agreement first.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_apache_icla() {
    let body = "Sign the Individual Contributor License (ICLA).";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_sign_a_cla_phrasing() {
    let body = "You will need to sign a CLA. We'll guide you through it.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_discuss_first_gate() {
    let body = "\
        For non-trivial changes, please discuss first in an issue.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_open_issue_first_gate() {
    let body = "\
        Please open an issue first before sending a pull request.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_email_first_gate() {
    let body = "Please email us first to coordinate larger changes.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_case_insensitive_match() {
    // Uppercase should still trigger — the classifier lowers before
    // matching.
    let body = "SIGN OUR CONTRIBUTOR LICENSE AGREEMENT";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_no_false_positive_on_cla_substring() {
    // The narrow classifier won't match a bare "cla" or "classroom"
    // substring; it only triggers on explicit CLA-vocabulary phrases.
    let body = "\
        We welcome contributions! Check out our class diagrams in\n\
        docs/architecture.md before proposing big changes.";
    assert!(contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_no_false_positive_on_discuss_without_gate() {
    // "We discuss things on Discord" is fine; "please discuss first"
    // is not. The classifier wants the explicit gate phrase.
    let body = "Join our Discord if you want to discuss ideas.";
    assert!(contributing_looks_ok(Some(body)));
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

// --- has_non_effort_label --------------------------------------------

#[test]
fn non_effort_label_true_on_enhancement() {
    assert!(has_non_effort_label(&[label("enhancement")]));
}

#[test]
fn non_effort_label_true_on_question() {
    assert!(has_non_effort_label(&[label("question")]));
}

#[test]
fn non_effort_label_true_on_rfc_mixed_case() {
    assert!(has_non_effort_label(&[label("RFC")]));
}

#[test]
fn non_effort_label_true_on_design() {
    assert!(has_non_effort_label(&[label("Design")]));
}

#[test]
fn non_effort_label_true_when_mixed_with_other_labels() {
    assert!(has_non_effort_label(&[
        label("C-bug"),
        label("discussion"),
        label("A-resolver"),
    ]));
}

#[test]
fn non_effort_label_false_on_plain_bug() {
    assert!(!has_non_effort_label(&[label("bug")]));
    assert!(!has_non_effort_label(&[
        label("C-bug"),
        label("A-resolver")
    ]));
}

#[test]
fn non_effort_label_false_on_empty_labels() {
    assert!(!has_non_effort_label(&[]));
}

#[test]
fn non_effort_label_false_on_near_miss() {
    // "enhancement-request" is not the same label as "enhancement".
    assert!(!has_non_effort_label(&[label("enhancement-request")]));
    assert!(!has_non_effort_label(&[label("discuss")]));
}

// --- days_since ------------------------------------------------------
//
// Anchor values used below come from arithmetic on the Gregorian
// calendar; each "now" constant is the unix-seconds value of a
// specific UTC wall-clock moment, computed independently of the
// parser being tested. Matching the parser against these anchors
// keeps the tests honest: a bug in the JDN math would produce a
// different second count and the assertion would fire.

// 1970-01-01T00:00:00Z by definition.
const EPOCH: i64 = 0;

// 2000-01-01T00:00:00Z. 30 years after epoch; 7 leap years (1972,
// 1976, 1980, 1984, 1988, 1992, 1996) add 7 days. 10957 * 86400.
const Y2K: i64 = 946_684_800;

// 2026-01-01T00:00:00Z. 26 years after Y2K; 7 leap years (2000,
// 2004, 2008, 2012, 2016, 2020, 2024) add 7 days. 9497 * 86400
// added to Y2K.
const Y2026: i64 = 1_767_225_600;

#[test]
fn days_since_epoch_at_epoch_is_zero() {
    assert_eq!(days_since("1970-01-01T00:00:00Z", EPOCH), Some(0));
}

#[test]
fn days_since_epoch_one_day_later_is_one() {
    assert_eq!(days_since("1970-01-01T00:00:00Z", EPOCH + 86_400), Some(1));
}

#[test]
fn days_since_epoch_one_second_short_of_day_is_zero() {
    // Whole days only; 23h59m59s ago still counts as same-day.
    assert_eq!(days_since("1970-01-01T00:00:00Z", EPOCH + 86_399), Some(0));
}

#[test]
fn days_since_y2k_is_correct() {
    assert_eq!(days_since("2000-01-01T00:00:00Z", Y2K), Some(0));
    assert_eq!(
        days_since("2000-01-01T00:00:00Z", Y2K + 30 * 86_400),
        Some(30)
    );
}

#[test]
fn days_since_2026_anchor_is_correct() {
    assert_eq!(days_since("2026-01-01T00:00:00Z", Y2026), Some(0));
    assert_eq!(
        days_since("2026-01-01T00:00:00Z", Y2026 + 365 * 86_400),
        Some(365)
    );
}

#[test]
fn days_since_future_timestamp_is_negative() {
    // Clock skew: timestamp is one hour after `now`.
    // div_euclid floors toward negative infinity, so even a
    // small negative delta crosses the zero-day boundary.
    assert_eq!(days_since("1970-01-01T01:00:00Z", EPOCH), Some(-1));
}

#[test]
fn days_since_invalid_returns_none() {
    // Wrong length.
    assert_eq!(days_since("2026-01-01", Y2026), None);
    // Missing Z.
    assert_eq!(days_since("2026-01-01T00:00:00", Y2026), None);
    // Offset instead of Z.
    assert_eq!(days_since("2026-01-01T00:00:00+00:00", Y2026), None);
    // Non-numeric field.
    assert_eq!(days_since("2026-XX-01T00:00:00Z", Y2026), None);
    // Empty.
    assert_eq!(days_since("", Y2026), None);
}

#[test]
fn days_since_out_of_range_fields_return_none() {
    assert_eq!(days_since("2026-13-01T00:00:00Z", Y2026), None);
    assert_eq!(days_since("2026-00-01T00:00:00Z", Y2026), None);
    assert_eq!(days_since("2026-02-32T00:00:00Z", Y2026), None);
    assert_eq!(days_since("2026-01-01T24:00:00Z", Y2026), None);
    assert_eq!(days_since("2026-01-01T00:60:00Z", Y2026), None);
}

// --- maintainer_in_comments -----------------------------------------

#[test]
fn maintainer_in_comments_false_on_empty_slice() {
    assert!(!maintainer_in_comments(&[]));
}

#[test]
fn maintainer_in_comments_true_on_owner() {
    assert!(maintainer_in_comments(&[comment("OWNER")]));
}

#[test]
fn maintainer_in_comments_true_on_member() {
    assert!(maintainer_in_comments(&[comment("MEMBER")]));
}

#[test]
fn maintainer_in_comments_true_on_collaborator() {
    assert!(maintainer_in_comments(&[comment("COLLABORATOR")]));
}

#[test]
fn maintainer_in_comments_false_on_contributor() {
    // CONTRIBUTOR is anyone with one prior merged PR. It overcounts;
    // we deliberately exclude it from the maintainer set.
    assert!(!maintainer_in_comments(&[comment("CONTRIBUTOR")]));
}

#[test]
fn maintainer_in_comments_false_on_first_timer_and_none() {
    assert!(!maintainer_in_comments(&[
        comment("FIRST_TIMER"),
        comment("FIRST_TIME_CONTRIBUTOR"),
        comment("MANNEQUIN"),
        comment("NONE"),
    ]));
}

#[test]
fn maintainer_in_comments_false_on_unknown_association() {
    // GitHub adds new association values occasionally. We refuse to
    // match loosely; an unknown string returns false rather than
    // accidentally counting as a maintainer touch.
    assert!(!maintainer_in_comments(&[comment("FUTURE_VALUE")]));
}

#[test]
fn maintainer_in_comments_true_when_any_one_matches() {
    let comments = [
        comment("NONE"),
        comment("CONTRIBUTOR"),
        comment("MEMBER"),
        comment("NONE"),
    ];
    assert!(maintainer_in_comments(&comments));
}

// --- crosslinked_open_pr_in_timeline ---------------------------------

#[test]
fn crosslinked_open_pr_false_on_empty_slice() {
    assert!(!crosslinked_open_pr_in_timeline(&[]));
}

#[test]
fn crosslinked_open_pr_false_on_only_non_cross_reference_events() {
    let events = [
        other_event("commented"),
        other_event("labeled"),
        other_event("assigned"),
        other_event("closed"),
        other_event("renamed"),
    ];
    assert!(!crosslinked_open_pr_in_timeline(&events));
}

#[test]
fn crosslinked_open_pr_false_on_issue_to_issue_cross_reference() {
    // Source has no `pull_request`; it's an issue, not a PR.
    assert!(!crosslinked_open_pr_in_timeline(&[cross_ref(
        "open", false
    )]));
}

#[test]
fn crosslinked_open_pr_false_on_only_closed_pr_cross_references() {
    let events = [cross_ref("closed", true), cross_ref("closed", true)];
    assert!(!crosslinked_open_pr_in_timeline(&events));
}

#[test]
fn crosslinked_open_pr_true_on_open_pr_cross_reference() {
    assert!(crosslinked_open_pr_in_timeline(&[cross_ref("open", true)]));
}

#[test]
fn crosslinked_open_pr_true_when_any_one_event_matches() {
    let events = [
        other_event("commented"),
        cross_ref("closed", true),
        cross_ref("open", false),
        cross_ref("open", true),
        other_event("labeled"),
    ];
    assert!(crosslinked_open_pr_in_timeline(&events));
}

#[test]
fn crosslinked_open_pr_false_when_source_or_issue_is_missing() {
    // Defensive: an event tagged `cross-referenced` but with `source`
    // or `source.issue` missing must not panic and must not count as
    // a maintainer-blocking signal.
    let no_source = TimelineEvent {
        event: "cross-referenced".into(),
        source: None,
    };
    let no_issue = TimelineEvent {
        event: "cross-referenced".into(),
        source: Some(TimelineSource { issue: None }),
    };
    assert!(!crosslinked_open_pr_in_timeline(&[no_source, no_issue]));
}
