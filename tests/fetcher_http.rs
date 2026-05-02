//! Integration coverage for the async `fetch_repos_at` orchestrator.
//! Wiremock stands in for `api.github.com`; tests verify the repo-then-
//! issues-then-comments-then-timeline composition, PR pre-filtering
//! (so we don't burn requests on items the planner is going to drop),
//! order preservation across multiple repos and across issues within
//! a repo (under bounded-concurrency fan-out), the explicit
//! concurrency knob, and error propagation at every endpoint family.

use scout::watchlist::{WatchEntry, Watchlist};
use scout::{AgeFilter, FetchError, fetch_repos_at, fetch_repos_at_with_concurrency};
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
    let out = fetch_repos_at(
        &server.uri(),
        &client,
        &Watchlist::default(),
        None,
        10,
        AgeFilter::disabled(),
    )
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
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
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
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
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
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
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
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
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
    let err = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
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
    let err = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
        .await
        .unwrap_err();
    assert!(matches!(err, FetchError::Status { status: 503, .. }));
}

#[tokio::test]
async fn preserves_issue_order_within_repo_under_concurrent_fetch() {
    // Three non-PR issues fetched concurrently; the orchestrator
    // collects per-issue tasks in the order issues were returned by
    // GitHub, so the output preserves that order even when the inner
    // tasks complete out of order.
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
    let three_issues = r#"[
        {"number": 100, "title": "first", "body": null, "html_url": "x", "state": "open",
         "labels": [], "comments": 0, "created_at": "2026-04-15T00:00:00Z",
         "updated_at": "2026-04-20T00:00:00Z", "user": {"login": "u"}},
        {"number": 200, "title": "second", "body": null, "html_url": "x", "state": "open",
         "labels": [], "comments": 0, "created_at": "2026-04-15T00:00:00Z",
         "updated_at": "2026-04-20T00:00:00Z", "user": {"login": "u"}},
        {"number": 300, "title": "third", "body": null, "html_url": "x", "state": "open",
         "labels": [], "comments": 0, "created_at": "2026-04-15T00:00:00Z",
         "updated_at": "2026-04-20T00:00:00Z", "user": {"login": "u"}}
    ]"#;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(three_issues))
        .mount(&server)
        .await;
    // Stagger response delays so the natural completion order would
    // be 300 -> 200 -> 100 (reverse of input). The orchestrator must
    // still collect in input order.
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/100/comments"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("[]")
                .set_delay(std::time::Duration::from_millis(150)),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/100/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/200/comments"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("[]")
                .set_delay(std::time::Duration::from_millis(75)),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/200/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/300/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/300/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at(&server.uri(), &client, &wl, None, 10, AgeFilter::disabled())
        .await
        .unwrap();
    assert_eq!(out[0].issues.len(), 3);
    assert_eq!(out[0].issues[0].issue.number, 100);
    assert_eq!(out[0].issues[1].issue.number, 200);
    assert_eq!(out[0].issues[2].issue.number, 300);
}

#[tokio::test]
async fn concurrency_one_serializes_request_pattern() {
    // With concurrency=1 the single semaphore permit forces every
    // request to wait for the previous one. Behavior of the public
    // surface is identical to the default; this test wires the
    // explicit knob path so it can't bit-rot.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(REPO_META_JSON))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/contents/CONTRIBUTING.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string("body"))
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
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/42/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;

    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at_with_concurrency(
        &server.uri(),
        &client,
        &wl,
        None,
        10,
        1,
        AgeFilter::disabled(),
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].issues.len(), 1);
    assert_eq!(out[0].issues[0].issue.number, 42);
}

#[tokio::test]
async fn concurrency_zero_is_clamped_to_one() {
    // Sanity: a 0 cap would deadlock the semaphore. The orchestrator
    // clamps it up to 1 so callers don't have to special-case the
    // edge of the configurable range.
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
    let out = fetch_repos_at_with_concurrency(
        &server.uri(),
        &client,
        &wl,
        None,
        10,
        0,
        AgeFilter::disabled(),
    )
    .await
    .unwrap();
    assert_eq!(out.len(), 1);
}

#[tokio::test]
async fn stale_issues_skip_per_issue_fetch_calls() {
    // Two issues: #1 created within the age window, #2 created
    // outside it. The orchestrator must fetch the #1 bundle (comments
    // + timeline) and skip the #2 bundle entirely, so the planner
    // doesn't pay for payloads it would drop anyway.
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
    let two_issues = r#"[
        {"number": 1, "title": "fresh", "body": null, "html_url": "x", "state": "open",
         "labels": [], "comments": 0, "created_at": "2026-04-15T00:00:00Z",
         "updated_at": "2026-04-15T00:00:00Z", "user": {"login": "u"}},
        {"number": 2, "title": "stale", "body": null, "html_url": "x", "state": "open",
         "labels": [], "comments": 0, "created_at": "2025-01-01T00:00:00Z",
         "updated_at": "2025-01-01T00:00:00Z", "user": {"login": "u"}}
    ]"#;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(two_issues))
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

    // 2026-05-01T00:00:00Z = 1777593600. Age window = 30 days, so
    // 2026-04-15 (16 days old) passes and 2025-01-01 (485 days old)
    // is dropped.
    let now_unix = 1_777_593_600;
    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at(
        &server.uri(),
        &client,
        &wl,
        None,
        10,
        AgeFilter {
            max_age_days: 30,
            now_unix,
        },
    )
    .await
    .unwrap();
    assert_eq!(out[0].issues.len(), 1);
    assert_eq!(out[0].issues[0].issue.number, 1);
}

#[tokio::test]
async fn max_age_zero_disables_age_filter() {
    // `0` matches the planner's "no filter" convention: every non-PR
    // issue still gets its bundle fetched even when the issue is
    // ancient.
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
    let ancient = r#"[
        {"number": 99, "title": "ancient", "body": null, "html_url": "x", "state": "open",
         "labels": [], "comments": 0, "created_at": "2020-01-01T00:00:00Z",
         "updated_at": "2020-01-01T00:00:00Z", "user": {"login": "u"}}
    ]"#;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ancient))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/99/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/99/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .expect(1)
        .mount(&server)
        .await;

    let now_unix = 1_777_593_600;
    let client = reqwest::Client::new();
    let wl = watchlist_with(&[("owner", "repo")]);
    let out = fetch_repos_at(
        &server.uri(),
        &client,
        &wl,
        None,
        10,
        AgeFilter {
            max_age_days: 0,
            now_unix,
        },
    )
    .await
    .unwrap();
    assert_eq!(out[0].issues.len(), 1);
    assert_eq!(out[0].issues[0].issue.number, 99);
}
