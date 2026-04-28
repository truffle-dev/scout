//! Disk-loading tests for the `scan` orchestrator. The parser layers are
//! exercised in `tests/watchlist.rs` and `tests/config.rs`; this file
//! only covers the pieces the orchestrator adds: file IO, path-tagged
//! errors, and the contract that empty / comments-only files round-trip
//! through to the right defaults.

use std::fs;
use std::io::ErrorKind;

use scout::scan::{
    FetchedIssue, FetchedRepo, LedgerError, ScanError, load_config, load_ledger, load_watchlist,
    plan,
};
use scout::took::{IssueRef, append_entry};
use scout::watchlist::{WatchEntry, WatchlistError};
use scout::{Filters, IssueMeta, Label, PullRequestRef, RepoMeta, UserRef, Weights};
use tempfile::TempDir;

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

/// A real on-disk file with two entries parses into the same shape the
/// pure parser would produce. The orchestrator must not transform the
/// payload between read and parse.
#[test]
fn loads_two_entries_from_disk() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("watchlist.yaml");
    fs::write(&path, "repos:\n  - truffle-dev/scout\n  - tokio-rs/tokio\n").unwrap();

    let wl = load_watchlist(&path).unwrap();
    assert_eq!(
        wl.repos,
        vec![
            WatchEntry {
                owner: "truffle-dev".into(),
                repo: "scout".into(),
            },
            WatchEntry {
                owner: "tokio-rs".into(),
                repo: "tokio".into(),
            },
        ]
    );
}

/// An empty file is a valid empty watchlist on disk too, matching the
/// parser-layer contract. A user who runs `scout init` then `scan`
/// without editing should get an empty result, not an error.
#[test]
fn empty_file_loads_as_empty_watchlist() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("watchlist.yaml");
    fs::write(&path, "").unwrap();

    let wl = load_watchlist(&path).unwrap();
    assert!(wl.repos.is_empty());
}

/// The starter template `scout init` writes is comments-only; loading
/// it from disk must not error. This is the lock that keeps `init` and
/// the orchestrator in step the way `tests/watchlist.rs` already locks
/// `init` against the pure parser.
#[test]
fn starter_template_loads_as_empty_watchlist() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("watchlist.yaml");
    let starter = include_str!("../templates/watchlist.yaml");
    fs::write(&path, starter).unwrap();

    let wl = load_watchlist(&path).unwrap();
    assert!(wl.repos.is_empty());
}

/// A path that does not exist surfaces `ScanError::Io` with the
/// offending path attached and the underlying NotFound preserved.
/// Errors must say which file was missing; "no such file or directory"
/// without a path is a bug.
#[test]
fn missing_file_returns_io_error_with_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("does-not-exist.yaml");

    let err = load_watchlist(&path).expect_err("missing file");
    match err {
        ScanError::Io {
            path: p,
            source: io_err,
        } => {
            assert_eq!(p, path);
            assert_eq!(io_err.kind(), ErrorKind::NotFound);
        }
        other => panic!("expected ScanError::Io, got {other:?}"),
    }
}

/// Malformed YAML surfaces `ScanError::Watchlist`, with the inner
/// `WatchlistError` preserved verbatim so the parser's line-number
/// detail survives the disk-IO wrapper.
#[test]
fn malformed_file_returns_watchlist_error_with_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("watchlist.yaml");
    fs::write(&path, "repos:\n  - a/b/c\n").unwrap();

    let err = load_watchlist(&path).expect_err("malformed entry");
    match err {
        ScanError::Watchlist {
            path: p,
            source: WatchlistError::MalformedEntry { line, .. },
        } => {
            assert_eq!(p, path);
            assert_eq!(line, 2);
        }
        other => panic!("expected ScanError::Watchlist with MalformedEntry, got {other:?}"),
    }
}

/// Unknown top-level keys are reported with the source line, again
/// preserving the parser's detail under the orchestrator wrapper.
#[test]
fn unknown_top_level_key_surfaces_through_orchestrator() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("watchlist.yaml");
    fs::write(&path, "weights:\n  root_cause: 0.3\n").unwrap();

    let err = load_watchlist(&path).expect_err("unknown key");
    match err {
        ScanError::Watchlist {
            source: WatchlistError::UnexpectedTopLevel { line: 1, content },
            ..
        } => {
            assert_eq!(content, "weights:");
        }
        other => panic!("expected ScanError::Watchlist with UnexpectedTopLevel, got {other:?}"),
    }
}

/// Pointing the loader at a directory rather than a file surfaces an
/// IO error tagged with the directory path. The exact `ErrorKind` is
/// platform-dependent (Linux: IsADirectory; older stdlibs: Other), so
/// we assert on the variant only.
#[test]
fn directory_path_returns_io_error() {
    let dir = TempDir::new().unwrap();

    let err = load_watchlist(dir.path()).expect_err("dir not file");
    match err {
        ScanError::Io { path: p, .. } => {
            assert_eq!(p, dir.path());
        }
        other => panic!("expected ScanError::Io, got {other:?}"),
    }
}

/// `Display` on `ScanError::Io` includes both the path and the
/// underlying message so a single-line render in the CLI is enough.
#[test]
fn io_error_display_includes_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nope.yaml");

    let err = load_watchlist(&path).expect_err("missing");
    let msg = format!("{err}");
    assert!(msg.contains(path.to_str().unwrap()), "msg = {msg:?}");
    assert!(msg.contains("filesystem error"), "msg = {msg:?}");
}

/// `Display` on `ScanError::Watchlist` includes both the path and the
/// parser's own message, again so the runner can print a single line.
#[test]
fn watchlist_error_display_includes_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.yaml");
    fs::write(&path, "repos:\n  - just-an-owner\n").unwrap();

    let err = load_watchlist(&path).expect_err("malformed");
    let msg = format!("{err}");
    assert!(msg.contains(path.to_str().unwrap()), "msg = {msg:?}");
    assert!(msg.contains("watchlist"), "msg = {msg:?}");
}

// --- load_config ---

/// A real on-disk config with one weight overridden parses correctly,
/// preserving the override and falling back to defaults for the rest.
/// This is the common path for a user who tunes one knob.
#[test]
fn load_config_partial_weights_preserves_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[weights]\nroot_cause = 0.50\n").unwrap();

    let cfg = load_config(&path).unwrap();
    let w: Weights = cfg.weights.into();
    assert!(approx(w.root_cause, 0.50));
    assert!(approx(w.no_pr, 0.20), "no_pr should keep default");
    assert!(approx(w.recent, 0.15), "recent should keep default");
    assert_eq!(cfg.filters.max_age_days, 30);
}

/// An empty file is a fully-defaulted Config. A user who runs
/// `scout init` and `touch` (or the templated `init` writes the file
/// with comments only) should get the reference weighting.
#[test]
fn load_config_empty_file_yields_defaults() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "").unwrap();

    let cfg = load_config(&path).unwrap();
    let w: Weights = cfg.weights.into();
    assert!(approx(w.root_cause, 0.30));
    assert!(approx(w.active_repo, 0.00));
    assert_eq!(cfg.output.color, "auto");
    assert_eq!(cfg.output.limit, 20);
    assert_eq!(
        cfg.filters.exclude_labels,
        vec!["wontfix", "invalid", "duplicate"]
    );
}

/// The starter template `scout init` writes loads cleanly. This is the
/// lock that keeps `init` and the orchestrator in step the way the
/// watchlist case already does.
#[test]
fn load_config_starter_template_loads_with_explicit_weights() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let starter = include_str!("../templates/config.toml");
    fs::write(&path, starter).unwrap();

    let cfg = load_config(&path).unwrap();
    let w: Weights = cfg.weights.into();
    assert!(approx(w.root_cause, 0.30));
    assert!(approx(w.no_pr, 0.20));
    assert!(approx(w.maintainer_touched, 0.05));
    assert_eq!(cfg.filters.max_age_days, 30);
    assert_eq!(cfg.output.limit, 20);
}

/// A path that does not exist surfaces `ScanError::Io`, same shape as
/// the watchlist loader. Keeps "no such file" diagnostics consistent
/// across loaders.
#[test]
fn load_config_missing_file_returns_io_error_with_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("absent.toml");

    let err = load_config(&path).expect_err("missing");
    match err {
        ScanError::Io {
            path: p,
            source: io_err,
        } => {
            assert_eq!(p, path);
            assert_eq!(io_err.kind(), ErrorKind::NotFound);
        }
        other => panic!("expected ScanError::Io, got {other:?}"),
    }
}

/// Unknown top-level keys are rejected with the source location
/// preserved in the toml-de message; the orchestrator wrapper adds
/// the file path. This is the typo-protection lock.
#[test]
fn load_config_unknown_key_returns_config_error_with_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "[mystery]\nfoo = 1\n").unwrap();

    let err = load_config(&path).expect_err("unknown key");
    match err {
        ScanError::Config { path: p, .. } => {
            assert_eq!(p, path);
        }
        other => panic!("expected ScanError::Config, got {other:?}"),
    }
}

/// Syntactically invalid TOML surfaces `ScanError::Config`, again
/// with the path tagged. Distinguishing parse vs IO matters because
/// the runner can hint at "edit the file" for parse errors and
/// "check the path" for IO.
#[test]
fn load_config_syntax_error_returns_config_error_with_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "= not valid toml\n").unwrap();

    let err = load_config(&path).expect_err("bad toml");
    match err {
        ScanError::Config { path: p, .. } => {
            assert_eq!(p, path);
        }
        other => panic!("expected ScanError::Config, got {other:?}"),
    }
}

/// Pointing the loader at a directory rather than a file surfaces an
/// IO error tagged with the directory path, mirroring the watchlist
/// loader's behavior. Keeps the two loaders' surface symmetrical.
#[test]
fn load_config_directory_path_returns_io_error() {
    let dir = TempDir::new().unwrap();

    let err = load_config(dir.path()).expect_err("dir not file");
    match err {
        ScanError::Io { path: p, .. } => {
            assert_eq!(p, dir.path());
        }
        other => panic!("expected ScanError::Io, got {other:?}"),
    }
}

/// `Display` on `ScanError::Config` includes both the path and the
/// underlying toml-de message so a single-line render in the CLI is
/// enough.
#[test]
fn config_error_display_includes_path() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "= not valid toml\n").unwrap();

    let err = load_config(&path).expect_err("bad toml");
    let msg = format!("{err}");
    assert!(msg.contains(path.to_str().unwrap()), "msg = {msg:?}");
    assert!(msg.contains("config"), "msg = {msg:?}");
}

// --- load_ledger ---

/// A path that does not exist round-trips as an empty ledger. The
/// cooldown semantic is "no ledger means nothing in cooldown," which
/// is what a fresh user sees before their first `scout took`. Other
/// IO errors (permission, EISDIR) still surface.
#[test]
fn load_ledger_missing_file_returns_empty_index() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let idx = load_ledger(&path).unwrap();
    assert!(idx.is_empty());
    assert_eq!(idx.len(), 0);
}

/// An empty file is a valid empty ledger. Same shape as missing-file
/// so the cooldown layer doesn't have to special-case the two.
#[test]
fn load_ledger_empty_file_yields_empty_index() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(&path, "").unwrap();

    let idx = load_ledger(&path).unwrap();
    assert!(idx.is_empty());
}

/// A file of only blank lines yields an empty ledger; the parser
/// tolerates blank lines so a hand-edited file with separator
/// whitespace doesn't break the read path.
#[test]
fn load_ledger_blank_lines_only_yields_empty_index() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(&path, "\n\n   \n\n").unwrap();

    let idx = load_ledger(&path).unwrap();
    assert!(idx.is_empty());
}

/// A single valid entry is keyed by `(owner, repo, number)` and
/// looked up by `last_taken`. Timestamp must round-trip through the
/// same `infer::parse_iso8601_z` parser that `days_since` uses.
#[test]
fn load_ledger_single_entry_roundtrips() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"truffle-dev/scout\",\"number\":42,\"timestamp\":\"2026-04-28T00:00:00Z\"}\n",
    )
    .unwrap();

    let idx = load_ledger(&path).unwrap();
    assert_eq!(idx.len(), 1);
    let secs = idx
        .last_taken("truffle-dev", "scout", 42)
        .expect("entry present");
    // 2026-04-28T00:00:00Z = 20571 days × 86400 sec since the unix
    // epoch (56 × 365 days + 14 leap days from 1970-2025 + 117 days
    // from 2026-01-01 to 2026-04-28).
    assert_eq!(secs, 1_777_334_400);
}

/// Two distinct issues produce two entries; both are independently
/// lookupable. Order in the file does not matter.
#[test]
fn load_ledger_two_distinct_entries_both_present() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"truffle-dev/scout\",\"number\":1,\"timestamp\":\"2026-04-27T12:00:00Z\"}\n\
         {\"repo\":\"pnpm/pnpm\",\"number\":11358,\"timestamp\":\"2026-04-28T00:14:00Z\"}\n",
    )
    .unwrap();

    let idx = load_ledger(&path).unwrap();
    assert_eq!(idx.len(), 2);
    assert!(idx.last_taken("truffle-dev", "scout", 1).is_some());
    assert!(idx.last_taken("pnpm", "pnpm", 11358).is_some());
}

/// When the same `(owner, repo, number)` appears on multiple lines,
/// the most recent timestamp wins. This preserves the tail-merge
/// invariant: concatenating two ledgers with `cat a b > c` and
/// reading `c` gives the same view as the latest record per issue.
#[test]
fn load_ledger_duplicate_issue_keeps_most_recent_timestamp() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"a/b\",\"number\":7,\"timestamp\":\"2026-04-01T00:00:00Z\"}\n\
         {\"repo\":\"a/b\",\"number\":7,\"timestamp\":\"2026-04-15T00:00:00Z\"}\n\
         {\"repo\":\"a/b\",\"number\":7,\"timestamp\":\"2026-04-08T00:00:00Z\"}\n",
    )
    .unwrap();

    let idx = load_ledger(&path).unwrap();
    assert_eq!(idx.len(), 1);
    let latest = idx.last_taken("a", "b", 7).expect("entry present");
    // 2026-04-15T00:00:00Z is the middle line's timestamp; the
    // earlier and later-written-but-older lines both lose to it.
    // 20558 days × 86400 sec since the unix epoch.
    assert_eq!(latest, 1_776_211_200);
}

/// `last_taken` returns `None` for an issue not in the ledger. The
/// cooldown filter relies on this to distinguish "never taken" from
/// "taken long ago"; both should pass the filter, but for opposite
/// reasons.
#[test]
fn load_ledger_unknown_issue_returns_none() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"a/b\",\"number\":1,\"timestamp\":\"2026-04-28T00:00:00Z\"}\n",
    )
    .unwrap();

    let idx = load_ledger(&path).unwrap();
    assert!(idx.last_taken("c", "d", 1).is_none());
    assert!(idx.last_taken("a", "b", 2).is_none());
    assert!(idx.last_taken("a", "x", 1).is_none());
}

/// Malformed JSON on a single line surfaces `LedgerError::Json` with
/// the 1-based line number. We fail fast rather than silently skip;
/// hidden corruption is the failure mode this guards against.
#[test]
fn load_ledger_malformed_json_returns_json_error_with_line() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"a/b\",\"number\":1,\"timestamp\":\"2026-04-28T00:00:00Z\"}\n\
         not valid json\n",
    )
    .unwrap();

    let err = load_ledger(&path).expect_err("bad json");
    match err {
        ScanError::Ledger {
            path: p,
            source: LedgerError::Json { line, .. },
        } => {
            assert_eq!(p, path);
            assert_eq!(line, 2, "second line is the offender");
        }
        other => panic!("expected ScanError::Ledger with Json, got {other:?}"),
    }
}

/// A `repo` field without a `/` surfaces `LedgerError::MalformedRepo`.
/// Empty owner or repo segments are also caught by the same variant.
#[test]
fn load_ledger_repo_without_slash_returns_malformed_repo() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"justname\",\"number\":1,\"timestamp\":\"2026-04-28T00:00:00Z\"}\n",
    )
    .unwrap();

    let err = load_ledger(&path).expect_err("malformed repo");
    match err {
        ScanError::Ledger {
            source: LedgerError::MalformedRepo { line, repo },
            ..
        } => {
            assert_eq!(line, 1);
            assert_eq!(repo, "justname");
        }
        other => panic!("expected ScanError::Ledger with MalformedRepo, got {other:?}"),
    }
}

/// A `repo` with an empty owner segment is malformed too. The
/// asymmetric case of owner empty / repo present should not be
/// silently coerced; the user should see and fix it.
#[test]
fn load_ledger_repo_with_empty_segment_returns_malformed_repo() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"/scout\",\"number\":1,\"timestamp\":\"2026-04-28T00:00:00Z\"}\n",
    )
    .unwrap();

    let err = load_ledger(&path).expect_err("empty owner");
    match err {
        ScanError::Ledger {
            source: LedgerError::MalformedRepo { line, repo },
            ..
        } => {
            assert_eq!(line, 1);
            assert_eq!(repo, "/scout");
        }
        other => panic!("expected ScanError::Ledger with MalformedRepo, got {other:?}"),
    }
}

/// A timestamp outside the narrow `YYYY-MM-DDTHH:MM:SSZ` shape
/// surfaces `LedgerError::Timestamp`. The parser is intentionally
/// strict so upstream format drift surfaces rather than hides.
#[test]
fn load_ledger_malformed_timestamp_returns_timestamp_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(
        &path,
        "{\"repo\":\"a/b\",\"number\":1,\"timestamp\":\"yesterday\"}\n",
    )
    .unwrap();

    let err = load_ledger(&path).expect_err("bad ts");
    match err {
        ScanError::Ledger {
            source: LedgerError::Timestamp { line, timestamp },
            ..
        } => {
            assert_eq!(line, 1);
            assert_eq!(timestamp, "yesterday");
        }
        other => panic!("expected ScanError::Ledger with Timestamp, got {other:?}"),
    }
}

/// Pointing the loader at a directory rather than a file surfaces
/// `ScanError::Io`, mirroring the watchlist and config loaders.
#[test]
fn load_ledger_directory_path_returns_io_error() {
    let dir = TempDir::new().unwrap();

    let err = load_ledger(dir.path()).expect_err("dir not file");
    match err {
        ScanError::Io { path: p, .. } => {
            assert_eq!(p, dir.path());
        }
        other => panic!("expected ScanError::Io, got {other:?}"),
    }
}

/// Round-trip: the writer in `took::append_entry` produces lines the
/// reader in `load_ledger` consumes without a second parser. This is
/// the lock that keeps writer and reader in step the way the init
/// templates lock against their loaders.
#[test]
fn load_ledger_roundtrip_with_took_writer() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let issue_a = IssueRef {
        owner: "truffle-dev".into(),
        repo: "scout".into(),
        number: 1,
    };
    let issue_b = IssueRef {
        owner: "pnpm".into(),
        repo: "pnpm".into(),
        number: 11358,
    };
    append_entry(&path, &issue_a, "2026-04-27T18:00:00Z").unwrap();
    append_entry(&path, &issue_b, "2026-04-28T00:14:00Z").unwrap();
    // Re-record issue_a later: the duplicate-dedupe path must keep
    // the most recent timestamp the writer produced.
    append_entry(&path, &issue_a, "2026-04-28T01:00:00Z").unwrap();

    let idx = load_ledger(&path).unwrap();
    assert_eq!(idx.len(), 2, "issue_a's two records collapse");
    let a_secs = idx.last_taken("truffle-dev", "scout", 1).unwrap();
    let b_secs = idx.last_taken("pnpm", "pnpm", 11358).unwrap();
    assert!(
        a_secs > b_secs,
        "issue_a's later record beats issue_b's record"
    );
}

/// `Display` on `ScanError::Ledger` includes the path and the inner
/// LedgerError detail so a single-line render in the CLI suffices.
#[test]
fn ledger_error_display_includes_path_and_line() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    fs::write(&path, "not valid json\n").unwrap();

    let err = load_ledger(&path).expect_err("bad json");
    let msg = format!("{err}");
    assert!(msg.contains(path.to_str().unwrap()), "msg = {msg:?}");
    assert!(msg.contains("ledger"), "msg = {msg:?}");
    assert!(msg.contains("line 1"), "msg = {msg:?}");
}

// --- LedgerIndex::in_cooldown ---
//
// The cooldown filter is the cooldown semantic the scoring pipeline
// consumes. Tests below pin the boundary, the unknown-issue case, the
// `cooldown_days = 0` short-circuit, and the future-dated permissive
// behavior so a future change has to fight the docs to land.

const DAY_SECS: i64 = 86_400;
const APR_28_2026: i64 = 1_777_334_400; // 2026-04-28T00:00:00Z

fn ledger_with_one_entry(taken_iso: &str) -> scout::scan::LedgerIndex {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    let line =
        format!("{{\"repo\":\"truffle-dev/scout\",\"number\":42,\"timestamp\":\"{taken_iso}\"}}\n");
    fs::write(&path, line).unwrap();
    load_ledger(&path).unwrap()
}

/// Issue not in the ledger is never in cooldown. A fresh user with no
/// `scout took` history has nothing to filter out, regardless of the
/// configured `cooldown_days`.
#[test]
fn in_cooldown_unknown_issue_returns_false() {
    let idx = scout::scan::LedgerIndex::default();
    assert!(!idx.in_cooldown("truffle-dev", "scout", 42, 14, APR_28_2026));
    assert!(!idx.in_cooldown("truffle-dev", "scout", 42, 365, APR_28_2026));
}

/// `cooldown_days = 0` short-circuits to `false` for every issue,
/// even one taken five seconds ago. This matches the "no cooldown
/// configured" reading; without the short-circuit the planner would
/// have to special-case the zero before calling.
#[test]
fn in_cooldown_zero_days_returns_false_even_for_recent_take() {
    let idx = ledger_with_one_entry("2026-04-27T23:59:55Z");
    assert!(!idx.in_cooldown("truffle-dev", "scout", 42, 0, APR_28_2026));
}

/// An issue taken less than `cooldown_days` ago is in cooldown. One
/// day before the boundary at 14 days is the canonical "yes" case.
#[test]
fn in_cooldown_recent_take_under_window_returns_true() {
    let idx = ledger_with_one_entry("2026-04-27T00:00:00Z");
    assert!(idx.in_cooldown("truffle-dev", "scout", 42, 14, APR_28_2026));
}

/// An issue taken exactly `cooldown_days` ago is no longer in
/// cooldown — the boundary is exclusive on the late side. This
/// matches the natural reading of "wait N days before taking again":
/// once N days have elapsed, you can take it.
#[test]
fn in_cooldown_exact_boundary_returns_false() {
    let idx = ledger_with_one_entry("2026-04-14T00:00:00Z");
    let now = APR_28_2026;
    let taken = idx.last_taken("truffle-dev", "scout", 42).unwrap();
    assert_eq!(now - taken, 14 * DAY_SECS, "boundary fixture is exact");
    assert!(!idx.in_cooldown("truffle-dev", "scout", 42, 14, now));
}

/// One second before the exact boundary is still in cooldown. Pairs
/// with the boundary test to anchor the comparison as `<` not `<=`.
#[test]
fn in_cooldown_one_second_before_boundary_returns_true() {
    let idx = ledger_with_one_entry("2026-04-14T00:00:01Z");
    assert!(idx.in_cooldown("truffle-dev", "scout", 42, 14, APR_28_2026));
}

/// An issue taken well beyond the cooldown window is not in
/// cooldown. The complementary "yes" anchor for the recent-take case.
#[test]
fn in_cooldown_old_take_past_window_returns_false() {
    let idx = ledger_with_one_entry("2026-01-01T00:00:00Z");
    assert!(!idx.in_cooldown("truffle-dev", "scout", 42, 14, APR_28_2026));
}

/// A future-dated take (clock skew between writer and reader) is
/// treated as in cooldown rather than erroring. A wrong clock should
/// only delay re-listing the affected issue, not crash the scan.
#[test]
fn in_cooldown_future_dated_take_returns_true() {
    let idx = ledger_with_one_entry("2026-04-29T00:00:00Z");
    assert!(idx.in_cooldown("truffle-dev", "scout", 42, 14, APR_28_2026));
}

/// Cooldown checks key on `(owner, repo, number)`. A different repo
/// with the same issue number is independent — the `truffle-dev/scout#42`
/// ledger entry does not put `pnpm/pnpm#42` in cooldown.
#[test]
fn in_cooldown_keyed_on_owner_repo_number_tuple() {
    let idx = ledger_with_one_entry("2026-04-27T00:00:00Z");
    assert!(idx.in_cooldown("truffle-dev", "scout", 42, 14, APR_28_2026));
    assert!(!idx.in_cooldown("pnpm", "pnpm", 42, 14, APR_28_2026));
    assert!(!idx.in_cooldown("truffle-dev", "scout", 99, 14, APR_28_2026));
}

/// After `append_entry` writes a take, `load_ledger` + `in_cooldown`
/// agree that the same issue is in cooldown when probed against a
/// `now` close to the take time. Anchors the read/write contract.
#[test]
fn in_cooldown_roundtrips_with_took_writer() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("ledger.jsonl");
    let issue = IssueRef {
        owner: "truffle-dev".into(),
        repo: "scout".into(),
        number: 7,
    };
    append_entry(&path, &issue, "2026-04-27T12:00:00Z").unwrap();

    let idx = load_ledger(&path).unwrap();
    assert!(idx.in_cooldown("truffle-dev", "scout", 7, 7, APR_28_2026));
    assert!(!idx.in_cooldown("truffle-dev", "scout", 7, 0, APR_28_2026));
}

// --- plan ---
//
// The planner filters pre-fetched payloads against the user's filters
// and the cooldown ledger, returning the inputs the ranker consumes.
// Tests below pin the four filter behaviors (PR-shaped items,
// excluded labels, max-age, cooldown), the borrow shape that lets
// `RankInput` reach back into the owning `FetchedRepo`, and the
// multi-repo case.

fn issue_fixture(number: u64, created_at: &str) -> IssueMeta {
    IssueMeta {
        number,
        title: "test issue".into(),
        body: None,
        html_url: format!("https://example.com/issues/{number}"),
        state: "open".into(),
        labels: vec![],
        comments: 0,
        created_at: created_at.into(),
        updated_at: created_at.into(),
        user: UserRef {
            login: "alice".into(),
        },
        pull_request: None,
    }
}

fn repo_fixture(full_name: &str) -> RepoMeta {
    RepoMeta {
        full_name: full_name.into(),
        stargazers_count: 0,
        open_issues_count: 0,
        pushed_at: "2026-04-28T00:00:00Z".into(),
        archived: false,
    }
}

fn fetched_repo(full_name: &str, issues: Vec<FetchedIssue>) -> FetchedRepo {
    FetchedRepo {
        repo: repo_fixture(full_name),
        contributing: None,
        issues,
    }
}

fn fetched_issue(issue: IssueMeta) -> FetchedIssue {
    FetchedIssue {
        issue,
        comments: vec![],
        timeline: vec![],
    }
}

/// PR-shaped items in the issues endpoint response are dropped. The
/// ranker is for issues; PRs surface through other tooling.
#[test]
fn plan_skips_pull_requests() {
    let mut pr = issue_fixture(1, "2026-04-28T00:00:00Z");
    pr.pull_request = Some(PullRequestRef {
        html_url: "https://example.com/pull/1".into(),
    });
    let regular = issue_fixture(2, "2026-04-28T00:00:00Z");

    let repos = vec![fetched_repo(
        "truffle-dev/scout",
        vec![fetched_issue(pr), fetched_issue(regular)],
    )];
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(out.len(), 1, "PR is filtered out");
    assert_eq!(out[0].issue.number, 2, "regular issue passes");
}

/// An issue carrying any label in `filters.exclude_labels` is dropped.
/// Default `Filters` excludes `wontfix`, `invalid`, `duplicate`.
#[test]
fn plan_skips_issues_with_excluded_label() {
    let mut wontfix = issue_fixture(1, "2026-04-28T00:00:00Z");
    wontfix.labels = vec![Label {
        name: "wontfix".into(),
    }];
    let bug = issue_fixture(2, "2026-04-28T00:00:00Z");

    let repos = vec![fetched_repo(
        "truffle-dev/scout",
        vec![fetched_issue(wontfix), fetched_issue(bug)],
    )];
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(out.len(), 1, "wontfix-labeled issue is filtered out");
    assert_eq!(out[0].issue.number, 2);
}

/// The exclude-label match is case-sensitive, matching the
/// config-layer doc comment. A repo using `WontFix` instead of
/// `wontfix` does not get picked up by the default exclude list.
#[test]
fn plan_excluded_label_match_is_case_sensitive() {
    let mut upper = issue_fixture(1, "2026-04-28T00:00:00Z");
    upper.labels = vec![Label {
        name: "WontFix".into(),
    }];

    let repos = vec![fetched_repo("a/b", vec![fetched_issue(upper)])];
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(
        out.len(),
        1,
        "case-mismatched label does not match the case-sensitive exclude list"
    );
}

/// An issue created more than `filters.max_age_days` days before
/// `now_unix` is dropped. The boundary uses `created_at`, not
/// `updated_at`; recency-of-update is a scoring factor, not a filter.
#[test]
fn plan_skips_issues_older_than_max_age() {
    // 90 days before APR_28_2026
    let old = issue_fixture(1, "2026-01-28T00:00:00Z");
    let fresh = issue_fixture(2, "2026-04-20T00:00:00Z");

    let repos = vec![fetched_repo(
        "a/b",
        vec![fetched_issue(old), fetched_issue(fresh)],
    )];
    let filters = Filters {
        max_age_days: 30,
        ..Filters::default()
    };
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &filters, &ledger, APR_28_2026);
    assert_eq!(out.len(), 1, "old issue is filtered out");
    assert_eq!(out[0].issue.number, 2);
}

/// `max_age_days = 0` disables the age filter, matching the
/// `cooldown_days = 0` convention. Without the short-circuit a user
/// who set the knob to "any age" by writing 0 would silently filter
/// out every issue ever created.
#[test]
fn plan_max_age_zero_disables_age_filter() {
    let very_old = issue_fixture(1, "2024-01-01T00:00:00Z");

    let repos = vec![fetched_repo("a/b", vec![fetched_issue(very_old)])];
    let filters = Filters {
        max_age_days: 0,
        ..Filters::default()
    };
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &filters, &ledger, APR_28_2026);
    assert_eq!(out.len(), 1, "max_age_days=0 disables the age filter");
}

/// An unparseable `created_at` passes through the age filter rather
/// than dropping the issue. The scoring layer already treats
/// unparseable timestamps as "very old" via the recency decay, so
/// the ranker will down-rank the issue but not lose it entirely.
#[test]
fn plan_unparseable_created_at_passes_through_age_filter() {
    let unparseable = issue_fixture(1, "yesterday");

    let repos = vec![fetched_repo("a/b", vec![fetched_issue(unparseable)])];
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(
        out.len(),
        1,
        "unparseable created_at passes through; scoring will decay it"
    );
}

/// An issue listed in the cooldown ledger within the configured
/// window is dropped. Pairs with `LedgerIndex::in_cooldown` to keep
/// the planner and the predicate's `<` boundary consistent.
#[test]
fn plan_skips_issues_in_cooldown() {
    let in_cooldown = issue_fixture(42, "2026-04-28T00:00:00Z");
    let untouched = issue_fixture(99, "2026-04-28T00:00:00Z");

    let repos = vec![fetched_repo(
        "truffle-dev/scout",
        vec![fetched_issue(in_cooldown), fetched_issue(untouched)],
    )];
    // Default Filters has cooldown_days = 14; ledger entry is 1 day
    // ago, well inside the window.
    let ledger = ledger_with_one_entry("2026-04-27T00:00:00Z");

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(out.len(), 1, "issue in cooldown is filtered out");
    assert_eq!(out[0].issue.number, 99);
}

/// Multiple repos are walked independently. The same issue number
/// in two different repos has independent cooldown state, because
/// the ledger keys on `(owner, repo, number)`. This is the lock
/// that keeps the `full_name.split_once('/')` path correct.
#[test]
fn plan_handles_multiple_repos_with_independent_cooldown() {
    let scout_42 = issue_fixture(42, "2026-04-28T00:00:00Z");
    let pnpm_42 = issue_fixture(42, "2026-04-28T00:00:00Z");

    let repos = vec![
        fetched_repo("truffle-dev/scout", vec![fetched_issue(scout_42)]),
        fetched_repo("pnpm/pnpm", vec![fetched_issue(pnpm_42)]),
    ];
    // The ledger helper writes truffle-dev/scout#42 only.
    let ledger = ledger_with_one_entry("2026-04-27T00:00:00Z");

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(out.len(), 1, "only pnpm/pnpm#42 should pass");
    assert_eq!(out[0].repo.full_name, "pnpm/pnpm");
    assert_eq!(out[0].issue.number, 42);
}

/// An eligible issue produces a `RankInput` whose references reach
/// back into the owning `FetchedRepo`. This is the lock on the
/// borrow shape: every field on `RankInput` should reflect the
/// fetched payload byte-for-byte.
#[test]
fn plan_passes_eligible_issue_with_correct_borrow_shape() {
    let issue = issue_fixture(7, "2026-04-25T00:00:00Z");
    let repos = vec![FetchedRepo {
        repo: repo_fixture("truffle-dev/scout"),
        contributing: Some("# Contributing\n\nWelcome!\n".into()),
        issues: vec![FetchedIssue {
            issue,
            comments: vec![],
            timeline: vec![],
        }],
    }];
    let ledger = scout::scan::LedgerIndex::default();

    let out = plan(&repos, &Filters::default(), &ledger, APR_28_2026);
    assert_eq!(out.len(), 1);
    let row = &out[0];
    assert_eq!(row.issue.number, 7);
    assert_eq!(row.repo.full_name, "truffle-dev/scout");
    assert_eq!(row.contributing, Some("# Contributing\n\nWelcome!\n"));
    assert!(row.comments.is_empty());
    assert!(row.timeline.is_empty());
}
