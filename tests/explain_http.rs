//! End-to-end coverage for `explain::explain_at`. Stands up a wiremock
//! server, registers all five GitHub endpoints `scout explain` calls,
//! invokes the async pipeline, and asserts the rendered markdown.
//! The score itself is computed by the pure `score::factors_from` +
//! `score` pair already covered in `tests/score.rs`; this file only
//! pins the HTTP fan-out, the borrow-shape into `factors_from`, and
//! the rendered output.

use scout::Weights;
use scout::explain::explain_at;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPO_JSON: &str = r#"{
    "full_name": "rust-lang/cargo",
    "stargazers_count": 12000,
    "open_issues_count": 200,
    "pushed_at": "2026-04-28T00:00:00Z",
    "archived": false
}"#;

const CONTRIBUTING_BODY: &str = "# Contributing\n\nThanks! Fork and send a PR.\n";

const ISSUE_JSON: &str = r#"{
    "number": 42,
    "title": "panic on empty input",
    "body": "Repro at src/lib.rs:120.\n\n```rust\nlet x = parse(\"\");\n```",
    "html_url": "https://github.com/rust-lang/cargo/issues/42",
    "state": "open",
    "labels": [{"name": "bug"}],
    "comments": 0,
    "created_at": "2026-04-25T00:00:00Z",
    "updated_at": "2026-04-28T00:00:00Z",
    "user": {"login": "alice"}
}"#;

const COMMENTS_JSON: &str = "[]";
const TIMELINE_JSON: &str = "[]";

/// 2026-04-28T00:00:00Z. Same anchor scan/plan tests use.
const APR_28_2026: i64 = 1_777_334_400;

/// Run the full pipeline against a wiremock server: all five endpoints
/// resolve, the score function consumes the merged payload, and the
/// rendered breakdown carries the title, the URL, the clamped total,
/// and one row per heuristic. Anchors the borrow shape between the
/// fetched payloads and `factors_from`.
#[tokio::test]
async fn explain_at_renders_breakdown_for_friendly_issue() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CONTRIBUTING_BODY))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/42"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ISSUE_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_string(COMMENTS_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/rust-lang/cargo/issues/42/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TIMELINE_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let weights = Weights::default();
    let rendered = explain_at(
        &server.uri(),
        &client,
        "rust-lang",
        "cargo",
        42,
        &weights,
        None,
        APR_28_2026,
    )
    .await
    .expect("explain_at returns Ok");

    assert!(
        rendered.starts_with("# panic on empty input"),
        "title heading missing in:\n{rendered}"
    );
    assert!(
        rendered.contains("https://github.com/rust-lang/cargo/issues/42"),
        "browser url missing in:\n{rendered}"
    );
    assert!(
        rendered.contains("**Score**:"),
        "score line missing in:\n{rendered}"
    );
    assert!(
        rendered.contains("| factor | weighted |"),
        "table header missing in:\n{rendered}"
    );

    for factor in [
        "root_cause",
        "no_pr",
        "recent",
        "contributing_ok",
        "reproducer",
        "effort_ok",
        "maintainer_touched",
        "active_repo",
    ] {
        assert!(
            rendered.contains(factor),
            "factor row {factor} missing in:\n{rendered}"
        );
    }

    // The fixture is a friendly bug report on a recently-pushed repo:
    // root_cause hits (file:line), reproducer hits (fenced block),
    // no PR crosslinks, contributing is friendly, no excluded labels,
    // updated today. Score should clear the default min_score (0.50).
    let score_line = rendered
        .lines()
        .find(|l| l.starts_with("**Score**:"))
        .expect("score line present");
    let score: f64 = score_line
        .trim_start_matches("**Score**:")
        .trim()
        .parse()
        .expect("score parses as f64");
    assert!(
        score >= 0.50,
        "friendly fixture should clear default min_score: {score}"
    );
}

/// A 404 on any of the five endpoints surfaces as an error from
/// `explain_at`. We don't pin which fetch failed because `try_join!`
/// races the futures; the one we care about is that the error
/// surfaces rather than producing a partial breakdown with garbage.
#[tokio::test]
async fn explain_at_propagates_fetch_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/foo/bar"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_JSON))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/contents/docs/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    // The issue endpoint 404s — that's the failure we drive.
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/issues/9999"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/issues/9999/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/foo/bar/issues/9999/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let err = explain_at(
        &server.uri(),
        &client,
        "foo",
        "bar",
        9999,
        &Weights::default(),
        None,
        APR_28_2026,
    )
    .await
    .expect_err("404 on issue endpoint should error");

    let msg = format!("{err}");
    assert!(
        msg.contains("404") || msg.to_lowercase().contains("status"),
        "error message should mention HTTP status: {msg}"
    );
}
