//! Aggregator coverage. Wires `IssueMeta + RepoMeta + now` through
//! the inference functions into a `Factors` value and checks that
//! each field ends up in the expected column.

use scout::{CommentMeta, IssueMeta, Label, RepoMeta, UserRef, Weights, factors_from, score};

fn comment(login: &str, association: &str) -> CommentMeta {
    CommentMeta {
        user: UserRef {
            login: login.into(),
        },
        author_association: association.into(),
    }
}

// Anchor: 2026-04-23T00:00:00Z in unix-seconds. Computed from Y2026
// (1_767_225_600) + 112 days (31 Jan + 28 Feb + 31 Mar + 22 Apr) *
// 86400. Independent of the parser under test.
const NOW_UNIX: i64 = 1_776_902_400;

fn issue(body: Option<&str>, updated_at: &str, labels: &[&str]) -> IssueMeta {
    IssueMeta {
        number: 1,
        title: "t".into(),
        body: body.map(|s| s.to_string()),
        html_url: "https://example.invalid/1".into(),
        state: "open".into(),
        labels: labels
            .iter()
            .map(|n| Label {
                name: (*n).to_string(),
            })
            .collect(),
        comments: 0,
        created_at: "2026-04-22T00:00:00Z".into(),
        updated_at: updated_at.into(),
        user: UserRef {
            login: "rep".into(),
        },
        pull_request: None,
    }
}

fn repo(pushed_at: &str) -> RepoMeta {
    RepoMeta {
        full_name: "o/r".into(),
        stargazers_count: 0,
        open_issues_count: 0,
        pushed_at: pushed_at.into(),
        archived: false,
    }
}

// --- happy path ------------------------------------------------------

#[test]
fn all_positive_signals_propagate() {
    let i = issue(
        Some("Repro:\n\n```\nfoo()\n```\n\nRoot cause: src/lib.rs:42."),
        "2026-04-22T00:00:00Z", // 1 day before NOW
        &["good first issue", "C-bug"],
    );
    let r = repo("2026-04-20T00:00:00Z"); // 3 days before NOW
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);

    assert!(f.has_reproducer);
    assert!(f.has_root_cause);
    assert!(f.effort_ok);
    assert_eq!(f.updated_days_ago, 1.0);
    assert_eq!(f.pushed_days_ago, 3.0);
}

#[test]
fn no_signals_propagates_empty_factors() {
    let i = issue(Some("It broke, please fix."), "2026-01-01T00:00:00Z", &[]);
    let r = repo("2026-01-01T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);

    assert!(!f.has_reproducer);
    assert!(!f.has_root_cause);
    // No labels at all; no non-effort labels means effort_ok = true.
    assert!(f.effort_ok);
    assert_eq!(f.updated_days_ago, 112.0);
    assert_eq!(f.pushed_days_ago, 112.0);
}

#[test]
fn still_unwired_factors_default_to_false() {
    let i = issue(Some("repro"), "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);

    // maintainer_touched is wired now; an empty comment slice means
    // no maintainer touch, which still resolves to false. The
    // remaining unwired field is no_crosslinked_pr.
    assert!(!f.no_crosslinked_pr);
    assert!(!f.maintainer_touched);
}

// --- maintainer_touched propagation ----------------------------------

#[test]
fn maintainer_touched_false_on_no_comments() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);
    assert!(!f.maintainer_touched);
}

#[test]
fn maintainer_touched_false_on_drive_by_only() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let comments = [
        comment("nobody", "NONE"),
        comment("first-timer", "FIRST_TIMER"),
        comment("prior-pr", "CONTRIBUTOR"),
    ];
    let f = factors_from(&i, &r, None, &comments, NOW_UNIX);
    assert!(!f.maintainer_touched);
}

#[test]
fn maintainer_touched_true_on_owner_comment() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let comments = [comment("nobody", "NONE"), comment("the-owner", "OWNER")];
    let f = factors_from(&i, &r, None, &comments, NOW_UNIX);
    assert!(f.maintainer_touched);
}

#[test]
fn maintainer_touched_true_on_member_comment() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let comments = [comment("a-member", "MEMBER")];
    let f = factors_from(&i, &r, None, &comments, NOW_UNIX);
    assert!(f.maintainer_touched);
}

#[test]
fn maintainer_touched_true_on_collaborator_comment() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let comments = [comment("a-collab", "COLLABORATOR")];
    let f = factors_from(&i, &r, None, &comments, NOW_UNIX);
    assert!(f.maintainer_touched);
}

// --- contributing_ok propagation -------------------------------------

#[test]
fn contributing_ok_true_when_no_contributing_fetched() {
    // None (repo has no CONTRIBUTING) defaults to ok in the classifier
    // and must propagate through the aggregator.
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);
    assert!(f.contributing_ok);
}

#[test]
fn contributing_ok_false_when_cla_present() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let body = "All contributors must sign a CLA before we can merge.";
    let f = factors_from(&i, &r, Some(body), &[], NOW_UNIX);
    assert!(!f.contributing_ok);
}

#[test]
fn contributing_ok_true_when_body_is_friendly() {
    let i = issue(None, "2026-04-23T00:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let body = "# Contributing\n\nThanks! Open a PR.";
    let f = factors_from(&i, &r, Some(body), &[], NOW_UNIX);
    assert!(f.contributing_ok);
}

// --- effort_ok branches ----------------------------------------------

#[test]
fn effort_ok_false_on_non_effort_label_alone() {
    let i = issue(None, "2026-04-23T00:00:00Z", &["enhancement"]);
    let r = repo("2026-04-23T00:00:00Z");
    assert!(!factors_from(&i, &r, None, &[], NOW_UNIX).effort_ok);
}

#[test]
fn effort_ok_true_when_both_effort_and_non_effort_coexist() {
    // Positive low-effort label wins: an issue tagged `good first issue`
    // AND `enhancement` is still effort_ok. The maintainer's explicit
    // low-effort vote overrides the category signal.
    let i = issue(
        None,
        "2026-04-23T00:00:00Z",
        &["enhancement", "good first issue"],
    );
    let r = repo("2026-04-23T00:00:00Z");
    assert!(factors_from(&i, &r, None, &[], NOW_UNIX).effort_ok);
}

#[test]
fn effort_ok_true_on_plain_bug_label() {
    let i = issue(None, "2026-04-23T00:00:00Z", &["bug", "C-bug"]);
    let r = repo("2026-04-23T00:00:00Z");
    assert!(factors_from(&i, &r, None, &[], NOW_UNIX).effort_ok);
}

// --- days_since edge cases -------------------------------------------

#[test]
fn future_timestamp_clamps_to_zero_days() {
    // Clock skew: issue updated_at is 1 hour ahead of NOW_UNIX.
    let i = issue(None, "2026-04-23T01:00:00Z", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);
    assert_eq!(f.updated_days_ago, 0.0);
}

#[test]
fn unparseable_timestamp_becomes_large_sentinel() {
    let i = issue(None, "not-a-timestamp", &[]);
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);
    // Sentinel is large enough to decay recency to 0 in the scoring
    // function; the exact number is an implementation detail. We
    // assert only the shape.
    assert!(f.updated_days_ago >= 365.0 * 1000.0);
}

// --- end-to-end through score ----------------------------------------

#[test]
fn aggregator_plumbs_through_score() {
    let i = issue(
        Some("Repro:\n\n```\nfoo()\n```\n\nRoot cause: at src/lib.rs:42"),
        "2026-04-23T00:00:00Z",
        &["good first issue"],
    );
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(&i, &r, None, &[], NOW_UNIX);
    let b = score(&f, &Weights::default());

    // At minimum: root_cause (0.30) + reproducer (0.10) + effort_ok
    // (0.10) + recent (0.15) + contributing_ok (0.15, via None ->
    // ok) + active_repo decay at 0 days (0.00, weight defaults to 0)
    // = 0.80. no_pr/maintainer_touched are still unwired, so their
    // contributions are 0.
    assert!(
        b.total >= 0.79 && b.total <= 0.81,
        "expected ~0.80, got {}",
        b.total
    );
}

#[test]
fn aggregator_cla_body_drops_contributing_bonus() {
    // Same issue shape as `aggregator_plumbs_through_score`, but with
    // a CLA-gated CONTRIBUTING body; the total should drop by exactly
    // the contributing_ok weight (0.15).
    let i = issue(
        Some("Repro:\n\n```\nfoo()\n```\n\nRoot cause: at src/lib.rs:42"),
        "2026-04-23T00:00:00Z",
        &["good first issue"],
    );
    let r = repo("2026-04-23T00:00:00Z");
    let f = factors_from(
        &i,
        &r,
        Some("Sign our Contributor License Agreement."),
        &[],
        NOW_UNIX,
    );
    let b = score(&f, &Weights::default());

    assert!(
        b.total >= 0.64 && b.total <= 0.66,
        "expected ~0.65, got {}",
        b.total
    );
}
