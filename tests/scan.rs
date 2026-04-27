//! Disk-loading tests for the `scan` orchestrator. The parser layers are
//! exercised in `tests/watchlist.rs` and `tests/config.rs`; this file
//! only covers the pieces the orchestrator adds: file IO, path-tagged
//! errors, and the contract that empty / comments-only files round-trip
//! through to the right defaults.

use std::fs;
use std::io::ErrorKind;

use scout::Weights;
use scout::scan::{ScanError, load_config, load_watchlist};
use scout::watchlist::{WatchEntry, WatchlistError};
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
