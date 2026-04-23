//! Decoder coverage for the fetch layer. Captured-shape snippets of a
//! `/repos/{owner}/{repo}` response exercise the serde derive; the
//! oversized-payload case pins down the "extra fields are ignored"
//! contract that keeps scout stable against GitHub adding new fields.

use scout::decode_repo_meta;

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
