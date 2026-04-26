//! Integration coverage for `list_issue_timeline_at`. Mirrors
//! `comments_http.rs`: request shaping (URL path + query string,
//! Accept header, bearer-auth forwarding) and response handling
//! (happy decode, 404, 403, empty list). Decoder unit tests in
//! `tests/timeline.rs` cover the serde edge cases.

use scout::{FetchError, list_issue_timeline_at};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TWO_EVENT_TIMELINE_JSON: &str = r#"[
    {
        "event": "labeled"
    },
    {
        "event": "cross-referenced",
        "source": {
            "issue": {
                "state": "open",
                "pull_request": {
                    "html_url": "https://github.com/rust-lang/cargo/pull/777"
                }
            }
        }
    }
]"#;

#[tokio::test]
async fn happy_path_decodes_timeline_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/42/timeline"))
        .and(query_param("per_page", "100"))
        .and(header("Accept", "application/vnd.github+json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TWO_EVENT_TIMELINE_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issue_timeline_at(&server.uri(), &client, "rust-lang", "cargo", 42, None)
        .await
        .unwrap();

    assert_eq!(list.len(), 2);
    assert_eq!(list[0].event, "labeled");
    assert!(list[0].source.is_none());
    assert_eq!(list[1].event, "cross-referenced");
    let source = list[1].source.as_ref().unwrap();
    let issue = source.issue.as_ref().unwrap();
    assert_eq!(issue.state, "open");
    assert!(issue.pull_request.is_some());
}

#[tokio::test]
async fn bearer_token_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/1/timeline"))
        .and(header("Authorization", "Bearer ghp_test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issue_timeline_at(
        &server.uri(),
        &client,
        "rust-lang",
        "cargo",
        1,
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
        .and(path("/repos/foo/bar/issues/9999/timeline"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = list_issue_timeline_at(&server.uri(), &client, "foo", "bar", 9999, None)
        .await
        .unwrap_err();

    match err {
        FetchError::Status { status, url } => {
            assert_eq!(status, 404);
            assert!(url.contains("/repos/foo/bar/issues/9999/timeline"));
        }
        other => panic!("expected Status(404), got {other:?}"),
    }
}

#[tokio::test]
async fn rate_limited_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/1/timeline"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = list_issue_timeline_at(&server.uri(), &client, "rust-lang", "cargo", 1, None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Status { status: 403, .. }));
}

#[tokio::test]
async fn empty_list_is_ok() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/1/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let list = list_issue_timeline_at(&server.uri(), &client, "rust-lang", "cargo", 1, None)
        .await
        .unwrap();
    assert!(list.is_empty());
}
