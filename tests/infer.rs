//! Unit coverage for the pattern-match signal inference. Each signal
//! has a true-case, a false-case, and the edge cases that matter:
//! `None` body, empty string, case-insensitive matching, and the
//! kinds of false positives the scoring layer is willing to tolerate.

use scout::{
    CommentMeta, Label, PullRequestRef, TimelineEvent, TimelineSource, TimelineSourceIssue,
    UserRef, assigned_to_copilot_in_timeline, contributing_looks_ok,
    crosslinked_open_pr_in_timeline, days_since, has_effort_label, has_non_effort_label,
    has_reproducer, has_root_cause, maintainer_in_comments,
    pending_discussion_in_maintainer_comments,
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
        body: None,
    }
}

fn comment_with_body(association: &str, body: &str) -> CommentMeta {
    CommentMeta {
        user: UserRef { login: "u".into() },
        author_association: association.into(),
        body: Some(body.into()),
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
        assignee: None,
    }
}

fn other_event(name: &str) -> TimelineEvent {
    TimelineEvent {
        event: name.into(),
        source: None,
        assignee: None,
    }
}

fn assigned_to(login: &str) -> TimelineEvent {
    TimelineEvent {
        event: "assigned".into(),
        source: None,
        assignee: Some(UserRef {
            login: login.into(),
        }),
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

#[test]
fn contributing_not_ok_on_typst_no_vibecode_clause() {
    // Verbatim from typst/typst CONTRIBUTING.md (2026-05-12).
    let body = "\
        Implement your change. Do not vibecode the change!\n\
        Contributions that were implemented by an AI model will not be accepted.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_astral_autonomous_agents_clause() {
    // Verbatim from astral-sh AI_POLICY.md (uv/ruff/ty share this policy).
    let body = "\
        We do not allow autonomous agents to be used for contributing\n\
        to our projects. We will close any pull requests that we\n\
        believe were created autonomously.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_generated_by_ai_phrasing() {
    let body = "\
        Pull requests that are generated by AI will be closed without\n\
        review.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_no_ai_generated_phrasing() {
    let body = "\
        # AI policy\n\
        No AI-generated contributions are accepted in this repo.\n";
    // The "no ai-generated" prefix triggers; the long-form "ai-generated
    // contributions will not" pattern is also present as backup.
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_no_false_positive_on_friendly_ai_disclosure() {
    // Friendly disclosure prose ("if you used AI tools, please
    // disclose") must NOT trigger the ban. The patterns are long
    // enough to require restrictive phrasing.
    let body = "\
        # AI tools\n\
        If you used AI tools (Copilot, Claude, Codex) to draft your\n\
        change, please disclose it in the PR description. We welcome\n\
        AI-assisted contributions when the human contributor stands\n\
        behind the code.";
    assert!(contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_no_false_positive_on_ai_used_for_review() {
    // Mentioning AI in a positive review context is fine.
    let body = "\
        Our reviewers may use AI assistants to help spot issues. If\n\
        you submit a generated patch, mark it clearly.";
    assert!(contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_cli_cli_label_gate_positive_clause() {
    // Verbatim from cli/cli .github/CONTRIBUTING.md (2026-05-12).
    let body = "\
        We accept pull requests for issues labelled `help wanted`.\n\
        We encourage issues and discussion posts for all other\n\
        contributions.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_cli_cli_label_gate_negative_clause() {
    // Verbatim from cli/cli .github/CONTRIBUTING.md "Please do NOT" list.
    let body = "\
        Please do NOT:\n\
        * Open a pull request for issues without the `help wanted`\n\
        label or explicit Acceptance Criteria";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_cli_cli_external_pr_rejection_clause() {
    // Verbatim from cli/cli `core`-label section.
    let body = "\
        Open pull requests for any issue marked `core`. These issues\n\
        require additional context from the core CLI team at GitHub\n\
        and any external pull requests will not be accepted";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_not_ok_on_we_only_accept_phrasing() {
    // Stricter variant of cli/cli's positive declaration. Some repos
    // use "only" to underscore the gate.
    let body = "\
        We only accept pull requests for issues labeled `accepted`.\n\
        All other contributions should start as an issue.";
    assert!(!contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_no_false_positive_on_friendly_label_recommendation() {
    // A friendly recommendation pointing newcomers at a label is NOT a
    // gate. The patterns require the restrictive shape ("accept PRs for
    // issues labelled X" / "without the X label") which a recommendation
    // doesn't carry.
    let body = "\
        Looking for a place to start? Browse our `good first issue`\n\
        and `help wanted` labels for issues ready to pick up. PRs are\n\
        welcome for any open issue.";
    assert!(contributing_looks_ok(Some(body)));
}

#[test]
fn contributing_ok_no_false_positive_on_generic_label_mention() {
    // Mentioning labels in passing (here, on the issue side) is fine.
    let body = "\
        File a new issue if you hit a regression. Tag it with the\n\
        appropriate area label. Pull requests are welcome at any\n\
        stage.";
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

// --- pending_discussion_in_maintainer_comments -----------------------

#[test]
fn pending_discussion_false_on_empty_slice() {
    assert!(!pending_discussion_in_maintainer_comments(&[]));
}

#[test]
fn pending_discussion_false_when_no_body() {
    // `comment` helper sets body: None — same shape as a deleted
    // comment GitHub returned as null. No body, no signal.
    assert!(!pending_discussion_in_maintainer_comments(&[
        comment("OWNER"),
        comment("MEMBER"),
    ]));
}

#[test]
fn pending_discussion_true_on_owner_should_be_issue() {
    let comments = [comment_with_body(
        "OWNER",
        "Thanks for the PR, but this should be an issue first to discuss the design.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_true_on_member_needs_rfc() {
    let comments = [comment_with_body(
        "MEMBER",
        "This needs an RFC before we accept any implementation work.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_true_on_collaborator_needs_proposal_first() {
    let comments = [comment_with_body(
        "COLLABORATOR",
        "Could you open a proposal first? We need to settle the API shape.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_true_on_we_need_to_decide() {
    let comments = [comment_with_body(
        "OWNER",
        "We need to decide whether this lands as a flag or a separate command.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_true_on_havent_decided_yet() {
    let comments = [comment_with_body(
        "MEMBER",
        "We haven't decided yet how to handle the multi-tenant case.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_true_on_before_implementing() {
    let comments = [comment_with_body(
        "OWNER",
        "Before implementing, please run the proposal by us in an issue.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_case_insensitive() {
    let comments = [comment_with_body(
        "OWNER",
        "THIS SHOULD BE AN ISSUE, not a PR until the design is settled.",
    )];
    assert!(pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_false_on_contributor_who_says_should_be_issue() {
    // Drive-by suggestion from a CONTRIBUTOR (or anyone non-maintainer)
    // doesn't carry gate weight. Project owners decide what gates PRs,
    // not random commenters.
    let comments = [comment_with_body(
        "CONTRIBUTOR",
        "I think this should be an issue first, not a PR.",
    )];
    assert!(!pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_false_on_lets_discuss_alone() {
    // "Let's discuss" by itself is too broad — maintainers commonly say
    // it on PRs that get merged. Only the gate-shaped phrasings match.
    let comments = [comment_with_body(
        "OWNER",
        "Looks promising. Let's discuss the edge cases inline.",
    )];
    assert!(!pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_false_on_unrelated_friendly_comment() {
    let comments = [comment_with_body(
        "OWNER",
        "Thanks for the report! Will take a look this week.",
    )];
    assert!(!pending_discussion_in_maintainer_comments(&comments));
}

#[test]
fn pending_discussion_true_when_any_maintainer_comment_matches() {
    // Earlier maintainer comment carries the gate; later "ok go for it"
    // doesn't override it. Override path is the config knob, not the
    // predicate.
    let comments = [
        comment_with_body("OWNER", "Thanks for filing!"),
        comment_with_body("OWNER", "Actually, this should be an RFC first."),
        comment_with_body("OWNER", "OK, this is settled, please PR."),
    ];
    assert!(pending_discussion_in_maintainer_comments(&comments));
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
        assignee: None,
    };
    let no_issue = TimelineEvent {
        event: "cross-referenced".into(),
        source: Some(TimelineSource { issue: None }),
        assignee: None,
    };
    assert!(!crosslinked_open_pr_in_timeline(&[no_source, no_issue]));
}

// --- assigned_to_copilot_in_timeline ---------------------------------

#[test]
fn assigned_to_copilot_false_on_empty_slice() {
    assert!(!assigned_to_copilot_in_timeline(&[]));
}

#[test]
fn assigned_to_copilot_false_on_only_non_assigned_events() {
    let events = [
        other_event("commented"),
        other_event("cross-referenced"),
        other_event("labeled"),
        other_event("closed"),
    ];
    assert!(!assigned_to_copilot_in_timeline(&events));
}

#[test]
fn assigned_to_copilot_false_on_human_assignee() {
    let events = [
        assigned_to("hi-ogawa"),
        assigned_to("epage"),
        assigned_to("someone-else"),
    ];
    assert!(!assigned_to_copilot_in_timeline(&events));
}

#[test]
fn assigned_to_copilot_true_on_display_login() {
    // GitHub's API returns `"Copilot"` as the assignee.login for the
    // swe-agent. Verified against vitest-dev/vitest#10307 timeline.
    assert!(assigned_to_copilot_in_timeline(&[assigned_to("Copilot")]));
}

#[test]
fn assigned_to_copilot_true_on_alias_login() {
    assert!(assigned_to_copilot_in_timeline(&[assigned_to(
        "copilot-swe-agent"
    )]));
}

#[test]
fn assigned_to_copilot_true_case_insensitive() {
    assert!(assigned_to_copilot_in_timeline(&[assigned_to("COPILOT")]));
    assert!(assigned_to_copilot_in_timeline(&[assigned_to(
        "Copilot-SWE-Agent"
    )]));
}

#[test]
fn assigned_to_copilot_true_when_any_one_event_matches() {
    let events = [
        other_event("commented"),
        assigned_to("hi-ogawa"),
        assigned_to("Copilot"),
        other_event("labeled"),
    ];
    assert!(assigned_to_copilot_in_timeline(&events));
}

#[test]
fn assigned_to_copilot_false_when_event_is_assigned_but_no_assignee() {
    // Defensive: an event tagged `assigned` but with `assignee`
    // missing must not panic and must not count as a match.
    let no_assignee = TimelineEvent {
        event: "assigned".into(),
        source: None,
        assignee: None,
    };
    assert!(!assigned_to_copilot_in_timeline(&[no_assignee]));
}

#[test]
fn assigned_to_copilot_false_on_non_assigned_event_with_copilot_assignee() {
    // Defensive: only `event == "assigned"` counts. A future GitHub
    // API change that puts an `assignee` field on a different event
    // type must not bleed into this signal.
    let weird = TimelineEvent {
        event: "labeled".into(),
        source: None,
        assignee: Some(UserRef {
            login: "Copilot".into(),
        }),
    };
    assert!(!assigned_to_copilot_in_timeline(&[weird]));
}
