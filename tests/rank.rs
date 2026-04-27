//! Coverage for the pure ranking layer. Builds `RankInput` bundles
//! by hand and checks that the output rows propagate identifying
//! fields, sort by total descending, preserve input order on ties,
//! and forward the breakdown intact.

use scout::{Breakdown, IssueMeta, Label, RankInput, RankedRow, RepoMeta, UserRef, Weights, rank};

const NOW_UNIX: i64 = 1_776_902_400;

fn issue(number: u64, title: &str, body: Option<&str>, labels: &[&str]) -> IssueMeta {
    IssueMeta {
        number,
        title: title.into(),
        body: body.map(|s| s.to_string()),
        html_url: format!("https://example.invalid/{number}"),
        state: "open".into(),
        labels: labels
            .iter()
            .map(|n| Label {
                name: (*n).to_string(),
            })
            .collect(),
        comments: 0,
        created_at: "2026-04-22T00:00:00Z".into(),
        updated_at: "2026-04-22T00:00:00Z".into(),
        user: UserRef {
            login: "rep".into(),
        },
        pull_request: None,
    }
}

fn repo(full_name: &str) -> RepoMeta {
    RepoMeta {
        full_name: full_name.into(),
        stargazers_count: 0,
        open_issues_count: 0,
        pushed_at: "2026-04-20T00:00:00Z".into(),
        archived: false,
    }
}

#[test]
fn rank_empty_returns_empty() {
    let rows = rank(&[], &Weights::default(), NOW_UNIX);
    assert!(rows.is_empty());
}

#[test]
fn rank_single_input_propagates_identity_fields() {
    let i = issue(42, "broken thing", Some("Root cause: src/lib.rs:1."), &[]);
    let r = repo("acme/widget");
    let inputs = [RankInput {
        issue: &i,
        repo: &r,
        contributing: None,
        comments: &[],
        timeline: &[],
    }];

    let rows = rank(&inputs, &Weights::default(), NOW_UNIX);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].full_name, "acme/widget");
    assert_eq!(rows[0].number, 42);
    assert_eq!(rows[0].title, "broken thing");
    assert_eq!(rows[0].html_url, "https://example.invalid/42");
}

#[test]
fn rank_sorts_by_total_descending() {
    // Three issues with descending strength of signals.
    // Strong: root cause + reproducer + good-first-issue.
    let i_strong = issue(
        1,
        "strong",
        Some("Root cause: src/foo.rs:10\n\n```\nrepro\n```"),
        &["good first issue"],
    );
    // Medium: root cause only.
    let i_medium = issue(2, "medium", Some("Root cause: src/foo.rs:10"), &[]);
    // Weak: nothing the inference catches.
    let i_weak = issue(3, "weak", Some("It broke."), &[]);

    let r = repo("acme/widget");
    let inputs = [
        // Submit weak first to verify rank() doesn't just pass through
        // input order.
        RankInput {
            issue: &i_weak,
            repo: &r,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
        RankInput {
            issue: &i_strong,
            repo: &r,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
        RankInput {
            issue: &i_medium,
            repo: &r,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
    ];

    let rows = rank(&inputs, &Weights::default(), NOW_UNIX);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].number, 1, "strong should rank first");
    assert_eq!(rows[1].number, 2, "medium should rank second");
    assert_eq!(rows[2].number, 3, "weak should rank last");
    assert!(rows[0].breakdown.total >= rows[1].breakdown.total);
    assert!(rows[1].breakdown.total >= rows[2].breakdown.total);
}

#[test]
fn rank_ties_preserve_input_order() {
    // Three issues that all score identically: same body, same labels,
    // same repo. The stable sort guarantees the original input order
    // survives among the ties.
    let i_a = issue(10, "a", Some("It broke."), &[]);
    let i_b = issue(20, "b", Some("It broke."), &[]);
    let i_c = issue(30, "c", Some("It broke."), &[]);
    let r = repo("acme/widget");
    let inputs = [
        RankInput {
            issue: &i_a,
            repo: &r,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
        RankInput {
            issue: &i_b,
            repo: &r,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
        RankInput {
            issue: &i_c,
            repo: &r,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
    ];

    let rows = rank(&inputs, &Weights::default(), NOW_UNIX);
    assert_eq!(rows.len(), 3);
    let totals: Vec<f64> = rows.iter().map(|r| r.breakdown.total).collect();
    assert_eq!(totals[0], totals[1]);
    assert_eq!(totals[1], totals[2]);
    let numbers: Vec<u64> = rows.iter().map(|r| r.number).collect();
    assert_eq!(numbers, vec![10, 20, 30]);
}

#[test]
fn rank_propagates_breakdown_total_and_parts() {
    // Single all-positive input. The total should be > 0 and the parts
    // list should contain all eight heuristic names in the canonical
    // score-module order.
    let i = issue(
        1,
        "t",
        Some("Repro:\n\n```\nfoo()\n```\n\nRoot cause: src/lib.rs:42"),
        &["good first issue"],
    );
    let r = repo("acme/widget");
    let inputs = [RankInput {
        issue: &i,
        repo: &r,
        contributing: None,
        comments: &[],
        timeline: &[],
    }];

    let rows = rank(&inputs, &Weights::default(), NOW_UNIX);
    let row: &RankedRow = &rows[0];
    let bd: &Breakdown = &row.breakdown;
    assert!(bd.total > 0.0);
    let part_names: Vec<&str> = bd.parts.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        part_names,
        vec![
            "root_cause",
            "no_pr",
            "recent",
            "contributing_ok",
            "reproducer",
            "effort_ok",
            "maintainer_touched",
            "active_repo",
        ]
    );
}

#[test]
fn rank_multiple_repos_keeps_full_name_per_row() {
    let i_a = issue(1, "a", Some("Root cause: x.rs:1"), &[]);
    let i_b = issue(2, "b", Some("It broke."), &[]);
    let r_a = repo("alice/one");
    let r_b = repo("bob/two");
    let inputs = [
        RankInput {
            issue: &i_a,
            repo: &r_a,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
        RankInput {
            issue: &i_b,
            repo: &r_b,
            contributing: None,
            comments: &[],
            timeline: &[],
        },
    ];

    let rows = rank(&inputs, &Weights::default(), NOW_UNIX);
    let mut by_number: Vec<_> = rows
        .iter()
        .map(|r| (r.number, r.full_name.as_str()))
        .collect();
    by_number.sort_by_key(|(n, _)| *n);
    assert_eq!(by_number, vec![(1, "alice/one"), (2, "bob/two")]);
}
