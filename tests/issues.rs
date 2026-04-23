//! Decoder coverage for the issue-list layer. Captured-shape fixtures
//! exercise the serde derive against real GitHub payload fragments;
//! the PR-vs-issue and labels/body-null edge cases pin down the
//! contract the scoring layer depends on.

use scout::{IssueMeta, decode_issue_list};

/// Two items: a real issue (no `pull_request`, two labels, non-empty
/// body) and a PR-shaped item (has `pull_request` sub-object, empty
/// labels, body null).
const MIXED_LIST_JSON: &str = r#"[
    {
        "number": 1234,
        "title": "panic in resolver when feature graph is cyclic",
        "body": "Repro steps:\n1. ...",
        "html_url": "https://github.com/rust-lang/cargo/issues/1234",
        "state": "open",
        "labels": [
            {"id": 1, "name": "A-features", "color": "ffaaaa", "description": "features", "url": "...", "default": false},
            {"id": 2, "name": "C-bug", "color": "dddddd", "description": "bug", "url": "...", "default": false}
        ],
        "comments": 5,
        "created_at": "2026-04-20T12:00:00Z",
        "updated_at": "2026-04-22T10:00:00Z",
        "user": {"login": "alice", "id": 42, "avatar_url": "..."},
        "pull_request": null
    },
    {
        "number": 16935,
        "title": "fix(compile): Ignore unused deps if also transitive",
        "body": null,
        "html_url": "https://github.com/rust-lang/cargo/issues/16935",
        "state": "open",
        "labels": [],
        "comments": 1,
        "created_at": "2026-04-23T14:00:00Z",
        "updated_at": "2026-04-23T15:32:55Z",
        "user": {"login": "epage", "id": 99, "avatar_url": "..."},
        "pull_request": {
            "url": "https://api.github.com/repos/rust-lang/cargo/pulls/16935",
            "html_url": "https://github.com/rust-lang/cargo/pull/16935",
            "diff_url": "https://github.com/rust-lang/cargo/pull/16935.diff",
            "patch_url": "https://github.com/rust-lang/cargo/pull/16935.patch",
            "merged_at": null
        }
    }
]"#;

#[test]
fn decodes_mixed_issue_and_pr() {
    let list = decode_issue_list(MIXED_LIST_JSON).unwrap();

    assert_eq!(list.len(), 2);

    let issue = &list[0];
    assert_eq!(issue.number, 1234);
    assert_eq!(
        issue.title,
        "panic in resolver when feature graph is cyclic"
    );
    assert_eq!(issue.state, "open");
    assert_eq!(issue.labels.len(), 2);
    assert_eq!(issue.labels[0].name, "A-features");
    assert_eq!(issue.labels[1].name, "C-bug");
    assert_eq!(issue.comments, 5);
    assert_eq!(issue.user.login, "alice");
    assert!(!issue.is_pull_request());
    assert_eq!(issue.body.as_deref(), Some("Repro steps:\n1. ..."));

    let pr = &list[1];
    assert_eq!(pr.number, 16935);
    assert!(pr.is_pull_request());
    assert_eq!(
        pr.pull_request.as_ref().unwrap().html_url,
        "https://github.com/rust-lang/cargo/pull/16935"
    );
    assert!(pr.labels.is_empty());
    assert!(pr.body.is_none());
}

#[test]
fn empty_list_decodes_to_empty_vec() {
    let list = decode_issue_list("[]").unwrap();
    assert!(list.is_empty());
}

#[test]
fn missing_pull_request_field_decodes_as_issue() {
    // Older GitHub responses sometimes omit the field entirely for
    // plain issues rather than including `"pull_request": null`.
    // `#[serde(default)]` must make both shapes equivalent.
    let body = r#"[{
        "number": 7,
        "title": "t",
        "html_url": "https://github.com/a/b/issues/7",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "user": {"login": "u"}
    }]"#;
    let list = decode_issue_list(body).unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].is_pull_request());
}

#[test]
fn null_body_decodes_as_none() {
    let body = r#"[{
        "number": 1,
        "title": "t",
        "body": null,
        "html_url": "https://github.com/a/b/issues/1",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "user": {"login": "u"}
    }]"#;
    let list = decode_issue_list(body).unwrap();
    assert!(list[0].body.is_none());
}

#[test]
fn unknown_top_level_fields_are_ignored() {
    // GitHub adds fields over time (reactions, state_reason, type,
    // active_lock_reason, ...). The decoder must not reject them.
    let body = r#"[{
        "number": 1,
        "title": "t",
        "html_url": "https://github.com/a/b/issues/1",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "user": {"login": "u"},
        "reactions": {"total_count": 3},
        "state_reason": null,
        "active_lock_reason": null,
        "type": null,
        "issue_field_values": []
    }]"#;
    let list = decode_issue_list(body).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn filter_out_prs_via_iter() {
    // Downstream consumers will often want issues only. Spot-check the
    // expected idiom works against the fixture.
    let list = decode_issue_list(MIXED_LIST_JSON).unwrap();
    let issues_only: Vec<&IssueMeta> = list.iter().filter(|i| !i.is_pull_request()).collect();
    assert_eq!(issues_only.len(), 1);
    assert_eq!(issues_only[0].number, 1234);
}

#[test]
fn github_error_object_fails_decode() {
    let body = r#"{"message": "Not Found", "documentation_url": "..."}"#;
    assert!(decode_issue_list(body).is_err());
}
