//! Integration coverage for the HTTP side of `list_issues_at`. Asserts
//! request shaping (URL path + query string, headers, bearer auth) and
//! response handling (decode, 404, 403, empty list). Decoder unit
//! tests in `tests/issues.rs` cover serde shape edge cases; this file
//! stays focused on the HTTP glue.
//!
//! Pagination tests at the bottom exercise the `Link: rel="next"`
//! cursor walk. GitHub emits absolute URLs in Link headers, so each
//! test server stitches its own `uri()` into the next-page URL to keep
//! the walker pointed at the mock.

use scout::{FetchError, issue_meta_at, list_issues_at, list_issues_paginated_at};
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

// --- pagination -----------------------------------------------------

/// One-item page-1 body. Used by the pagination tests to keep the
/// issue numbers distinct across pages so the caller can verify the
/// concatenation order.
fn page_body(number: u64) -> String {
    format!(
        r#"[
            {{
                "number": {number},
                "title": "p{number}",
                "body": null,
                "html_url": "https://github.com/a/b/issues/{number}",
                "state": "open",
                "labels": [],
                "comments": 0,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "user": {{"login": "u"}}
            }}
        ]"#
    )
}

#[tokio::test]
async fn pagination_single_page_no_link_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/a/b/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page_body(1)))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issues_paginated_at(&server.uri(), &client, "a", "b", None, 10)
        .await
        .unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].number, 1);
}

#[tokio::test]
async fn pagination_walks_two_pages_via_link_next() {
    let server = MockServer::start().await;
    let next_url = format!(
        "{}/repos/a/b/issues?state=open&per_page=100&page=2",
        server.uri()
    );

    // Page 1: one item + Link header pointing at page 2. The first
    // request scout emits doesn't carry `page=`; scout only learns
    // about the `page=` query once it follows the `rel="next"` URL.
    Mock::given(method("GET"))
        .and(path("/repos/a/b/issues"))
        .and(query_param_missing("page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(page_body(1))
                .insert_header("link", format!(r#"<{next_url}>; rel="next""#).as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;

    // Page 2: one item, no Link header (terminal page).
    Mock::given(method("GET"))
        .and(path("/repos/a/b/issues"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(page_body(2)))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issues_paginated_at(&server.uri(), &client, "a", "b", None, 10)
        .await
        .unwrap();

    assert_eq!(list.len(), 2);
    assert_eq!(list[0].number, 1);
    assert_eq!(list[1].number, 2);
}

#[tokio::test]
async fn pagination_stops_at_max_pages_cap() {
    let server = MockServer::start().await;
    let next_url = format!(
        "{}/repos/a/b/issues?state=open&per_page=100&page=2",
        server.uri()
    );

    // Every page returns a Link header with rel="next" pointing at
    // page=2. Without a cap the walker would loop forever; cap=2 means
    // the walker makes exactly two requests then returns.
    Mock::given(method("GET"))
        .and(path("/repos/a/b/issues"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(page_body(1))
                .insert_header("link", format!(r#"<{next_url}>; rel="next""#).as_str()),
        )
        .expect(2)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issues_paginated_at(&server.uri(), &client, "a", "b", None, 2)
        .await
        .unwrap();

    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn pagination_aborts_and_discards_on_mid_walk_error() {
    let server = MockServer::start().await;
    let next_url = format!(
        "{}/repos/a/b/issues?state=open&per_page=100&page=2",
        server.uri()
    );

    Mock::given(method("GET"))
        .and(path("/repos/a/b/issues"))
        .and(query_param_missing("page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(page_body(1))
                .insert_header("link", format!(r#"<{next_url}>; rel="next""#).as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/a/b/issues"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = list_issues_paginated_at(&server.uri(), &client, "a", "b", None, 10)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Status { status: 500, .. }));
}

// Helper: wiremock doesn't ship a negative query_param matcher, so
// this synthesizes one by asserting the request's query string does
// not contain the named key. Scoped to this file to keep the shape
// visible alongside the tests that need it.
fn query_param_missing(key: &'static str) -> QueryParamMissing {
    QueryParamMissing { key }
}

struct QueryParamMissing {
    key: &'static str,
}

impl wiremock::Match for QueryParamMissing {
    fn matches(&self, request: &wiremock::Request) -> bool {
        !request.url.query_pairs().any(|(k, _)| k == self.key)
    }
}

// --- single-issue meta -------------------------------------------------

const SINGLE_ISSUE_JSON: &str = r#"{
    "number": 42,
    "title": "panic on empty input",
    "body": "Reproducer at src/lib.rs:120.",
    "html_url": "https://github.com/rust-lang/cargo/issues/42",
    "state": "open",
    "labels": [{"name": "bug"}],
    "comments": 3,
    "created_at": "2026-04-25T00:00:00Z",
    "updated_at": "2026-04-27T00:00:00Z",
    "user": {"login": "alice"}
}"#;

#[tokio::test]
async fn issue_meta_happy_path_decodes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/42"))
        .and(header("Accept", "application/vnd.github+json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SINGLE_ISSUE_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let issue = issue_meta_at(&server.uri(), &client, "rust-lang", "cargo", 42, None)
        .await
        .unwrap();

    assert_eq!(issue.number, 42);
    assert_eq!(issue.title, "panic on empty input");
    assert!(!issue.is_pull_request());
    assert_eq!(issue.labels.len(), 1);
    assert_eq!(issue.labels[0].name, "bug");
}

#[tokio::test]
async fn issue_meta_bearer_token_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/42"))
        .and(header("Authorization", "Bearer ghp_test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SINGLE_ISSUE_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let issue = issue_meta_at(
        &server.uri(),
        &client,
        "rust-lang",
        "cargo",
        42,
        Some("ghp_test"),
    )
    .await
    .unwrap();

    assert_eq!(issue.number, 42);
}

#[tokio::test]
async fn issue_meta_not_found_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/issues/9999"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = issue_meta_at(&server.uri(), &client, "foo", "bar", 9999, None)
        .await
        .unwrap_err();

    match err {
        FetchError::Status { status, url } => {
            assert_eq!(status, 404);
            assert!(url.contains("/repos/foo/bar/issues/9999"));
        }
        other => panic!("expected Status(404), got {other:?}"),
    }
}
