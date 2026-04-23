//! Integration coverage for `contributing_md_at`. Asserts the probe
//! order through the three candidate paths (root, `.github/`, `docs/`),
//! the `Accept: application/vnd.github.raw` media type on every
//! request, bearer-auth forwarding, and the abort-on-non-404 policy.
//! Decoder coverage is not needed here — `contributing_md_at` returns
//! the body verbatim, so the "decode" is identity.

use scout::{FetchError, contributing_md_at};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FRIENDLY_BODY: &str = "# Contributing\n\nThanks! Fork and PR.\n";
const CLA_BODY: &str = "Sign our Contributor License Agreement first.\n";

#[tokio::test]
async fn root_path_hit_short_circuits_remaining_paths() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/CONTRIBUTING.md"))
        .and(header("Accept", "application/vnd.github.raw"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FRIENDLY_BODY))
        .expect(1)
        .mount(&server)
        .await;

    // Fallback paths are mounted to panic if hit; the first-path 200
    // should short-circuit the walk and never reach them.
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let body = contributing_md_at(&server.uri(), &client, "o", "r", None)
        .await
        .unwrap();

    assert_eq!(body.as_deref(), Some(FRIENDLY_BODY));
}

#[tokio::test]
async fn falls_through_to_dotgithub_on_root_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CLA_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let body = contributing_md_at(&server.uri(), &client, "o", "r", None)
        .await
        .unwrap();

    assert_eq!(body.as_deref(), Some(CLA_BODY));
}

#[tokio::test]
async fn falls_through_to_docs_when_first_two_404() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/docs/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FRIENDLY_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let body = contributing_md_at(&server.uri(), &client, "o", "r", None)
        .await
        .unwrap();

    assert_eq!(body.as_deref(), Some(FRIENDLY_BODY));
}

#[tokio::test]
async fn all_paths_404_returns_none() {
    let server = MockServer::start().await;
    for p in [
        "/repos/o/r/contents/CONTRIBUTING.md",
        "/repos/o/r/contents/.github/CONTRIBUTING.md",
        "/repos/o/r/contents/docs/CONTRIBUTING.md",
    ] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
    }

    let client = reqwest::Client::new();
    let body = contributing_md_at(&server.uri(), &client, "o", "r", None)
        .await
        .unwrap();

    assert!(body.is_none());
}

#[tokio::test]
async fn bearer_token_is_forwarded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/CONTRIBUTING.md"))
        .and(header("Authorization", "Bearer ghp_test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(FRIENDLY_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let body = contributing_md_at(&server.uri(), &client, "o", "r", Some("ghp_test"))
        .await
        .unwrap();

    assert_eq!(body.as_deref(), Some(FRIENDLY_BODY));
}

#[tokio::test]
async fn rate_limited_aborts_walk() {
    // First path returns 403. The walk must surface the error rather
    // than silently moving on to the fallback paths; otherwise a
    // rate-limited caller would see spurious "no CONTRIBUTING" results
    // and every repo they scan would inherit the ok-default.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = contributing_md_at(&server.uri(), &client, "o", "r", None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Status { status: 403, .. }));
}

#[tokio::test]
async fn server_error_mid_walk_aborts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/o/r/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = contributing_md_at(&server.uri(), &client, "o", "r", None)
        .await
        .unwrap_err();

    assert!(matches!(err, FetchError::Status { status: 500, .. }));
}
