//! Parse-layer tests for `scout::watchlist`. The parser is pure and
//! string-only, so these tests are all in-memory.

use scout::watchlist::{WatchEntry, Watchlist, WatchlistError, parse};

/// Empty input is a valid empty watchlist. A user who runs `scout init`
/// and then `scout scan` without editing the starter watchlist should
/// get an empty result, not a parse error.
#[test]
fn empty_input_parses_to_empty() {
    assert_eq!(parse("").unwrap(), Watchlist::default());
}

/// Comments and blank lines are ignored. A file containing only
/// commented-out examples (the shape of the starter template) parses
/// to an empty watchlist.
#[test]
fn comments_only_parses_to_empty() {
    let body = "# scout watchlist\n\n# - rust-lang/rust\n# - tokio-rs/tokio\n";
    assert_eq!(parse(body).unwrap(), Watchlist::default());
}

/// `repos:` with no entries below it is a valid empty watchlist.
#[test]
fn repos_key_with_no_entries_parses_empty() {
    assert_eq!(parse("repos:\n").unwrap(), Watchlist::default());
}

/// The starter template embedded by `scout init` parses to an empty
/// watchlist. This is the lock that keeps `init` and `parse_watchlist`
/// in step.
#[test]
fn starter_template_parses_to_empty() {
    let starter = include_str!("../templates/watchlist.yaml");
    assert_eq!(parse(starter).unwrap(), Watchlist::default());
}

/// One entry produces a one-item watchlist with the slug split on `/`.
#[test]
fn single_entry_parses() {
    let body = "repos:\n  - truffle-dev/scout\n";
    let wl = parse(body).unwrap();
    assert_eq!(
        wl.repos,
        vec![WatchEntry {
            owner: "truffle-dev".into(),
            repo: "scout".into(),
        }]
    );
}

/// Multiple entries preserve source order.
#[test]
fn multiple_entries_preserve_order() {
    let body = "repos:\n  - a/b\n  - c/d\n  - e/f\n";
    let wl = parse(body).unwrap();
    assert_eq!(
        wl.repos,
        vec![
            WatchEntry {
                owner: "a".into(),
                repo: "b".into(),
            },
            WatchEntry {
                owner: "c".into(),
                repo: "d".into(),
            },
            WatchEntry {
                owner: "e".into(),
                repo: "f".into(),
            },
        ]
    );
}

/// Inline `# ...` comments after an entry are stripped. The remaining
/// `- owner/repo` parses normally.
#[test]
fn inline_comment_is_stripped() {
    let body = "repos:\n  - truffle-dev/scout  # the swing-big repo\n";
    let wl = parse(body).unwrap();
    assert_eq!(
        wl.repos,
        vec![WatchEntry {
            owner: "truffle-dev".into(),
            repo: "scout".into(),
        }]
    );
}

/// A top-level key other than `repos:` is rejected with the offending
/// line number, surfacing typos instead of silently ignoring them.
#[test]
fn unknown_top_level_key_rejected() {
    let body = "weights:\n  root_cause: 0.3\n";
    let err = parse(body).expect_err("unknown key");
    assert_eq!(
        err,
        WatchlistError::UnexpectedTopLevel {
            line: 1,
            content: "weights:".into(),
        }
    );
}

/// An entry-shaped line before `repos:` is the same kind of typo and
/// gets the same treatment: report-with-line, do not silently consume.
#[test]
fn entry_before_repos_key_rejected() {
    let body = "  - a/b\n";
    let err = parse(body).expect_err("entry before repos");
    assert!(matches!(
        err,
        WatchlistError::UnexpectedTopLevel { line: 1, .. }
    ));
}

/// After `repos:`, a line without a leading `-` is rejected. This catches
/// the case where the user forgot the YAML list marker.
#[test]
fn missing_leading_dash_rejected() {
    let body = "repos:\n  truffle-dev/scout\n";
    let err = parse(body).expect_err("missing dash");
    assert_eq!(
        err,
        WatchlistError::ExpectedDash {
            line: 2,
            content: "truffle-dev/scout".into(),
        }
    );
}

/// `- foo` (no slash) is malformed. We don't accept owner-only entries
/// because the GitHub URL needs both segments.
#[test]
fn entry_without_slash_rejected() {
    let body = "repos:\n  - just-an-owner\n";
    let err = parse(body).expect_err("no slash");
    assert!(matches!(
        err,
        WatchlistError::MalformedEntry { line: 2, .. }
    ));
}

/// Multi-slash entries (`a/b/c`) don't match any GitHub repo and are
/// rejected as malformed rather than silently splitting.
#[test]
fn multi_slash_rejected() {
    let body = "repos:\n  - a/b/c\n";
    let err = parse(body).expect_err("multi slash");
    assert!(matches!(
        err,
        WatchlistError::MalformedEntry { line: 2, .. }
    ));
}

/// Empty owner segment (`/repo`) is rejected.
#[test]
fn empty_owner_rejected() {
    let body = "repos:\n  - /repo\n";
    let err = parse(body).expect_err("empty owner");
    assert!(matches!(err, WatchlistError::EmptySegment { line: 2, .. }));
}

/// Empty repo segment (`owner/`) is rejected.
#[test]
fn empty_repo_rejected() {
    let body = "repos:\n  - owner/\n";
    let err = parse(body).expect_err("empty repo");
    assert!(matches!(err, WatchlistError::EmptySegment { line: 2, .. }));
}

/// Whitespace inside an entry (`a / b`, `a /b`, `a/ b`) is rejected.
/// Quoted YAML strings would also fall in here, which is fine because
/// the parser doesn't support quoting yet.
#[test]
fn whitespace_inside_entry_rejected() {
    for body in [
        "repos:\n  - owner / repo\n",
        "repos:\n  - owner /repo\n",
        "repos:\n  - owner/ repo\n",
    ] {
        let err = parse(body).expect_err("whitespace");
        assert!(
            matches!(err, WatchlistError::InvalidChar { line: 2, .. }),
            "got {err:?} for {body:?}",
        );
    }
}

/// Duplicate entries are rejected so the user sees the typo instead
/// of scout scanning the same repo twice.
#[test]
fn duplicate_entry_rejected() {
    let body = "repos:\n  - a/b\n  - c/d\n  - a/b\n";
    let err = parse(body).expect_err("duplicate");
    assert_eq!(
        err,
        WatchlistError::Duplicate {
            line: 4,
            slug: "a/b".into(),
        }
    );
}

/// Comments and blank lines between entries are tolerated and don't
/// shift line numbers reported in errors.
#[test]
fn blank_and_comment_lines_between_entries() {
    let body = "repos:\n\n  # core\n  - a/b\n\n  # tooling\n  - c/d\n";
    let wl = parse(body).unwrap();
    assert_eq!(wl.repos.len(), 2);
}

/// The error line number tracks the source line, not a logical entry
/// counter. Off-by-one in the parser would surface here.
#[test]
fn error_line_tracks_source_line() {
    let body = "# header\n\nrepos:\n  - a/b\n  malformed\n";
    let err = parse(body).expect_err("malformed");
    assert!(matches!(err, WatchlistError::ExpectedDash { line: 5, .. }));
}
