//! Decoder coverage for the fetch layer. Captured-shape snippets of a
//! `/repos/{owner}/{repo}` response exercise the serde derive; the
//! oversized-payload case pins down the "extra fields are ignored"
//! contract that keeps scout stable against GitHub adding new fields.
//!
//! `parse_next_link` is covered at the bottom of the file. It's a pure
//! string parser for GitHub's Link header; shape coverage goes here
//! alongside the other pure decoders.

use scout::{decode_repo_meta, parse_next_link};

#[test]
fn decodes_minimal_repo_payload() {
    let body = r#"{
        "full_name": "rust-lang/cargo",
        "stargazers_count": 12567,
        "open_issues_count": 1783,
        "pushed_at": "2026-04-22T18:30:00Z",
        "archived": false
    }"#;

    let meta = decode_repo_meta(body).unwrap();

    assert_eq!(meta.full_name, "rust-lang/cargo");
    assert_eq!(meta.stargazers_count, 12567);
    assert_eq!(meta.open_issues_count, 1783);
    assert_eq!(meta.pushed_at, "2026-04-22T18:30:00Z");
    assert!(!meta.archived);
}

#[test]
fn ignores_unknown_upstream_fields() {
    let body = r#"{
        "id": 12345,
        "node_id": "R_kgDOAbc",
        "name": "cargo",
        "full_name": "rust-lang/cargo",
        "owner": {"login": "rust-lang", "id": 5430905},
        "description": "The Rust package manager",
        "html_url": "https://github.com/rust-lang/cargo",
        "stargazers_count": 12567,
        "watchers_count": 12567,
        "forks_count": 2456,
        "open_issues_count": 1783,
        "pushed_at": "2026-04-22T18:30:00Z",
        "created_at": "2014-03-12T23:42:55Z",
        "updated_at": "2026-04-23T00:00:00Z",
        "archived": false,
        "disabled": false,
        "license": {"key": "apache-2.0"}
    }"#;

    let meta = decode_repo_meta(body).unwrap();

    assert_eq!(meta.full_name, "rust-lang/cargo");
    assert!(!meta.archived);
}

#[test]
fn archived_true_round_trips() {
    let body = r#"{
        "full_name": "mozilla/rust-old",
        "stargazers_count": 42,
        "open_issues_count": 0,
        "pushed_at": "2018-06-01T00:00:00Z",
        "archived": true
    }"#;

    let meta = decode_repo_meta(body).unwrap();

    assert!(meta.archived);
}

#[test]
fn missing_required_field_errors() {
    let body = r#"{
        "full_name": "foo/bar",
        "stargazers_count": 1,
        "open_issues_count": 0,
        "archived": false
    }"#;

    let err = decode_repo_meta(body).unwrap_err();

    assert!(err.to_string().contains("pushed_at"));
}

#[test]
fn wrong_field_type_errors() {
    let body = r#"{
        "full_name": "foo/bar",
        "stargazers_count": "not a number",
        "open_issues_count": 0,
        "pushed_at": "2026-01-01T00:00:00Z",
        "archived": false
    }"#;

    assert!(decode_repo_meta(body).is_err());
}

#[test]
fn github_error_object_fails_decode() {
    let body = r#"{
        "message": "Not Found",
        "documentation_url": "https://docs.github.com/rest/repos/repos#get-a-repository"
    }"#;

    assert!(decode_repo_meta(body).is_err());
}

// --- parse_next_link -------------------------------------------------
//
// Shape reference: GitHub emits
//   <URL>; rel="prev", <URL>; rel="next", <URL>; rel="last", <URL>; rel="first"
// with no whitespace-canonicalization promises. The parser only needs
// to return the `next` URL; missing-next and empty cases must return
// None, and malformed input must not panic.

#[test]
fn next_link_single_next_entry() {
    let header = r#"<https://api.github.com/repos/x/y/issues?page=2>; rel="next""#;
    assert_eq!(
        parse_next_link(header),
        Some("https://api.github.com/repos/x/y/issues?page=2".to_string())
    );
}

#[test]
fn next_link_present_with_other_rels() {
    let header = concat!(
        r#"<https://api.github.com/repos/x/y/issues?page=2>; rel="next", "#,
        r#"<https://api.github.com/repos/x/y/issues?page=5>; rel="last""#
    );
    assert_eq!(
        parse_next_link(header),
        Some("https://api.github.com/repos/x/y/issues?page=2".to_string())
    );
}

#[test]
fn next_link_in_middle_of_entries() {
    let header = concat!(
        r#"<https://api.github.com/repos/x/y/issues?page=1>; rel="first", "#,
        r#"<https://api.github.com/repos/x/y/issues?page=1>; rel="prev", "#,
        r#"<https://api.github.com/repos/x/y/issues?page=3>; rel="next", "#,
        r#"<https://api.github.com/repos/x/y/issues?page=5>; rel="last""#
    );
    assert_eq!(
        parse_next_link(header),
        Some("https://api.github.com/repos/x/y/issues?page=3".to_string())
    );
}

#[test]
fn next_link_absent_only_prev_and_last() {
    let header = concat!(
        r#"<https://api.github.com/repos/x/y/issues?page=1>; rel="first", "#,
        r#"<https://api.github.com/repos/x/y/issues?page=4>; rel="prev""#
    );
    assert_eq!(parse_next_link(header), None);
}

#[test]
fn next_link_empty_header_returns_none() {
    assert_eq!(parse_next_link(""), None);
}

#[test]
fn next_link_tolerates_unquoted_rel() {
    let header = r#"<https://api.github.com/repos/x/y/issues?page=2>; rel=next"#;
    assert_eq!(
        parse_next_link(header),
        Some("https://api.github.com/repos/x/y/issues?page=2".to_string())
    );
}

#[test]
fn next_link_ignores_extra_params() {
    let header = concat!(
        r#"<https://api.github.com/repos/x/y/issues?page=2>; "#,
        r#"title="Issues page 2"; rel="next""#
    );
    assert_eq!(
        parse_next_link(header),
        Some("https://api.github.com/repos/x/y/issues?page=2".to_string())
    );
}

#[test]
fn next_link_malformed_missing_angle_bracket_returns_none() {
    assert_eq!(
        parse_next_link(r#"https://example.invalid; rel="next""#),
        None
    );
}

#[test]
fn next_link_malformed_missing_close_bracket_returns_none() {
    assert_eq!(
        parse_next_link(r#"<https://example.invalid rel="next""#),
        None
    );
}

#[test]
fn next_link_ignores_rel_substring() {
    // rel="nextpage" is not a next relation; substring matching would
    // be a false positive.
    let header = r#"<https://api.github.com/repos/x/y/issues?page=2>; rel="nextpage""#;
    assert_eq!(parse_next_link(header), None);
}

#[test]
fn next_link_entry_separators_tolerate_whitespace() {
    let header = concat!(
        "<https://example.invalid/a>; rel=\"prev\"  ,\n   ",
        "<https://example.invalid/b>; rel=\"next\""
    );
    assert_eq!(
        parse_next_link(header),
        Some("https://example.invalid/b".to_string())
    );
}
