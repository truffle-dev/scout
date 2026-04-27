//! Disk-loading tests for the `scan` orchestrator. The parser layer is
//! exercised in `tests/watchlist.rs`; this file only covers the pieces
//! the orchestrator adds: file IO, path-tagged errors, and the contract
//! that empty / comments-only files round-trip to an empty watchlist.

use std::fs;
use std::io::ErrorKind;

use scout::scan::{ScanError, load_watchlist};
use scout::watchlist::{WatchEntry, WatchlistError};
use tempfile::TempDir;

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
