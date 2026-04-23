//! Integration coverage for the HTTP side of `list_issues_at`. Asserts
//! request shaping (URL path + query string, headers, bearer auth) and
//! response handling (decode, 404, 403, empty list). Decoder unit
//! tests in `tests/issues.rs` cover serde shape edge cases; this file
//! stays focused on the HTTP glue.

use scout::{FetchError, list_issues_at};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TWO_ITEM_LIST_JSON: &str = r#"[
    {
        "number": 1,
        "title": "a",
        "body": null,
        "html_url": "https://github.com/a/b/issues/1",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "user": {"login": "u"}
    },
    {
        "number": 2,
        "title": "b",
        "body": null,
        "html_url": "https://github.com/a/b/issues/2",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "user": {"login": "u"},
        "pull_request": {
            "html_url": "https://github.com/a/b/pull/2"
        }
    }
]"#;

#[tokio::test]
async fn happy_path_decodes_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues"))
        .and(query_param("state", "open"))
        .and(query_param("per_page", "100"))
        .and(header("Accept", "application/vnd.github+json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TWO_ITEM_LIST_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issues_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap();

    assert_eq!(list.len(), 2);
    assert_eq!(list[0].number, 1);
    assert!(!list[0].is_pull_request());
    assert!(list[1].is_pull_request());
}

#[tokio::test]
async fn bearer_token_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues"))
        .and(header("Authorization", "Bearer ghp_test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issues_at(
        &server.uri(),
        &client,
        "rust-lang",
        "cargo",
        Some("ghp_test"),
    )
    .await
    .unwrap();

    assert!(list.is_empty());
}

#[tokio::test]
async fn not_found_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/issues"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = list_issues_at(&server.uri(), &client, "foo", "bar", None)
        .await
        .unwrap_err();

    match err {
        FetchError::Status { status, url } => {
            assert_eq!(status, 404);
            assert!(url.contains("/repos/foo/bar/issues"));
        }
        other => panic!("expected Status(404), got {other:?}"),
    }
}

#[tokio::test]
async fn rate_limited_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = list_issues_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Status { status: 403, .. }));
}

#[tokio::test]
async fn empty_list_is_ok() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issues_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap();

    assert!(list.is_empty());
}

#[tokio::test]
async fn garbage_body_returns_decode_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = list_issues_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Decode(_)));
}
