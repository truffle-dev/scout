//! Integration coverage for the async `fetch_repos_at` orchestrator.
//! Wiremock stands in for `api.github.com`; tests verify the repo-then-
//! issues-then-comments-then-timeline composition, PR pre-filtering
//! (so we don't burn requests on items the planner is going to drop),
//! order preservation across multiple repos, and error propagation
//! at every endpoint family.

use scout::watchlist::{WatchEntry, Watchlist};
use scout::{FetchError, fetch_repos_at};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPO_META_JSON: &str = r#"{
    "full_name": "owner/repo",
    "stargazers_count": 100,
    "open_issues_count": 5,
    "pushed_at": "2026-04-20T12:00:00Z",
    "archived": false
}"#;

const ONE_ISSUE_JSON: &str = r#"[
    {
        "number": 42,
        "title": "real issue",
        "body": "body",
        "html_url": "https://github.com/owner/repo/issues/42",
        "state": "open",
        "labels": [],
        "comments": 1,
        "created_at": "2026-04-15T00:00:00Z",
        "updated_at": "2026-04-20T00:00:00Z",
        "user": {"login": "reporter"}
    }
]"#;

const ISSUE_PLUS_PR_JSON: &str = r#"[
    {
        "number": 1,
        "title": "real issue",
        "body": null,
        "html_url": "https://github.com/owner/repo/issues/1",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-04-15T00:00:00Z",
        "updated_at": "2026-04-20T00:00:00Z",
        "user": {"login": "u"}
    },
    {
        "number": 2,
        "title": "PR shaped",
        "body": null,
        "html_url": "https://github.com/owner/repo/issues/2",
        "state": "open",
        "labels": [],
        "comments": 0,
        "created_at": "2026-04-15T00:00:00Z",
        "updated_at": "2026-04-20T00:00:00Z",
        "user": {"login": "u"},
        "pull_request": {"html_url": "https://github.com/owner/repo/pull/2"}
    }
]"#;

const COMMENTS_JSON: &str = r#"[
    {"user": {"login": "maintainer"}, "author_association": "OWNER"}
]"#;

const TIMELINE_JSON: &str = r#"[
    {"event": "labeled"}
]"#;

fn watchlist_with(entries: &[(&str, &str)]) -> Watchlist {
    Watchlist {
        repos: entries
            .iter()
            .map(|(o, r)| WatchEntry {
                owner: (*o).to_string(),
                repo: (*r).to_string(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn empty_watchlist_makes_no_requests_and_returns_empty() {
    let server = MockServer::start().await;
    let client = reqwest::Client::new();
    let out = fetch_repos_at(&server.uri(), &client, &Watchlist::default(), None, 10)
        .await
        .unwrap();
    assert!(out.is_empty());
}

#[tokio::test]
async fn single_repo_with_one_issue_returns_full_bundle() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_META_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string("contributing body"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ONE_ISSUE_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_string(COMMENTS_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/42/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TIMELINE_JSON))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10)
        .await
        .unwrap();

    assert_eq!(out.len(), 1);
    let fr = &out[0];
    assert_eq!(fr.repo.full_name, "owner/repo");
    assert_eq!(fr.contributing.as_deref(), Some("contributing body"));
    assert_eq!(fr.issues.len(), 1);
    let fi = &fr.issues[0];
    assert_eq!(fi.issue.number, 42);
    assert_eq!(fi.issue.title, "real issue");
    assert_eq!(fi.comments.len(), 1);
    assert_eq!(fi.comments[0].user.login, "maintainer");
    assert_eq!(fi.timeline.len(), 1);
    assert_eq!(fi.timeline[0].event, "labeled");
}

#[tokio::test]
async fn preserves_repo_order_across_multiple_repos() {
    let server = MockServer::start().await;
    for slug in ["first/one", "second/two", "third/three"] {
        let (owner, repo) = slug.split_once('/').unwrap();
        let body = format!(
            r#"{{
                "full_name": "{owner}/{repo}",
                "stargazers_count": 1,
                "open_issues_count": 0,
                "pushed_at": "2026-04-20T12:00:00Z",
                "archived": false
            }}"#
        );
        Mock::given(method("GET"))
            .and(path(format!("/repos/{owner}/{repo}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        for cpath in [
            "CONTRIBUTING.md",
            ".github/CONTRIBUTING.md",
            "docs/CONTRIBUTING.md",
        ] {
            Mock::given(method("GET"))
                .and(path(format!("/repos/{owner}/{repo}/contents/{cpath}")))
                .respond_with(ResponseTemplate::new(404))
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path(format!("/repos/{owner}/{repo}/issues")))
            .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
            .mount(&server)
            .await;
    }

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("first", "one"), ("second", "two"), ("third", "three")]);
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10)
        .await
        .unwrap();

    assert_eq!(out.len(), 3);
    assert_eq!(out[0].repo.full_name, "first/one");
    assert_eq!(out[1].repo.full_name, "second/two");
    assert_eq!(out[2].repo.full_name, "third/three");
}

#[tokio::test]
async fn pull_requests_skip_per_issue_fetch_calls() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_META_JSON))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/contents/.github/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/contents/docs/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ISSUE_PLUS_PR_JSON))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/1/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/1/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/2/comments"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/2/timeline"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10)
        .await
        .unwrap();
    assert_eq!(out[0].issues.len(), 1);
    assert_eq!(out[0].issues[0].issue.number, 1);
}

#[tokio::test]
async fn missing_contributing_returns_none_and_continues() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_META_JSON))
        .mount(&server)
        .await;
    for cpath in [
        "CONTRIBUTING.md",
        ".github/CONTRIBUTING.md",
        "docs/CONTRIBUTING.md",
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/owner/repo/contents/{cpath}")))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10)
        .await
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].contributing.is_none());
    assert!(out[0].issues.is_empty());
}

#[tokio::test]
async fn repo_meta_failure_propagates_without_calling_downstream_endpoints() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(0)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let err = fetch_repos_at(&server.uri(), &client, &wl, None, 10)
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::Status { status: 500, .. }));
}

#[tokio::test]
async fn issue_comments_failure_propagates_without_calling_timeline() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_META_JSON))
        .mount(&server)
        .await;
    for cpath in [
        "CONTRIBUTING.md",
        ".github/CONTRIBUTING.md",
        "docs/CONTRIBUTING.md",
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/owner/repo/contents/{cpath}")))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ONE_ISSUE_JSON))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/42/comments"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/42/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(0)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let err = fetch_repos_at(&server.uri(), &client, &wl, None, 10)
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::Status { status: 503, .. }));
}
