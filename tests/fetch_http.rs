//! Integration coverage for the HTTP side of the fetch layer. A local
//! wiremock server stands in for api.github.com so these tests exercise
//! real reqwest + serde + error mapping without depending on network or
//! secrets. The decoder itself has its own unit tests in `fetch.rs`.

use scout::{FetchError, repo_meta_at};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Captured-shape fixture for a single repo. Same field set the decoder
/// tests use; kept here verbatim so the HTTP-layer tests don't depend
/// on the decoder-layer fixture module.
const CARGO_REPO_JSON: &str = r#"{
    "full_name": "rust-lang/cargo",
    "stargazers_count": 12567,
    "open_issues_count": 1783,
    "pushed_at": "2026-04-22T18:30:00Z",
    "archived": false
}"#;

#[tokio::test]
async fn happy_path_decodes_repo_meta() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .and(header(
            "User-Agent",
            concat!("scout/", env!("CARGO_PKG_VERSION")),
        ))
        .and(header("Accept", "application/vnd.github+json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CARGO_REPO_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let meta = repo_meta_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap();

    assert_eq!(meta.full_name, "rust-lang/cargo");
    assert_eq!(meta.stargazers_count, 12567);
    assert!(!meta.archived);
}

#[tokio::test]
async fn bearer_token_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .and(header("Authorization", "Bearer ghp_test123"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CARGO_REPO_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let meta = repo_meta_at(
        &server.uri(),
        &client,
        "rust-lang",
        "cargo",
        Some("ghp_test123"),
    )
    .await
    .unwrap();

    assert_eq!(meta.full_name, "rust-lang/cargo");
}

#[tokio::test]
async fn no_token_sends_no_authorization_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .and(header_exists("Authorization"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CARGO_REPO_JSON))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let meta = repo_meta_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap();

    assert_eq!(meta.full_name, "rust-lang/cargo");
}

#[tokio::test]
async fn not_found_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_string(r#"{"message":"Not Found","documentation_url":"..."}"#),
        )
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = repo_meta_at(&server.uri(), &client, "foo", "bar", None)
        .await
        .unwrap_err();

    match err {
        FetchError::Status { status, url } => {
            assert_eq!(status, 404);
            assert!(url.ends_with("/repos/foo/bar"));
        }
        other => panic!("expected Status(404), got {other:?}"),
    }
}

#[tokio::test]
async fn rate_limited_returns_status_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = repo_meta_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Status { status: 403, .. }));
}

#[tokio::test]
async fn success_body_with_garbage_returns_decode_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = repo_meta_at(&server.uri(), &client, "rust-lang", "cargo", None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Decode(_)));
}
