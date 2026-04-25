//! Filesystem-touching cases on `scout::took`. Each test owns its own
//! `TempDir` and points the writer at paths inside it, so tests do not
//! collide with each other or with the real `~/.config/scout/ledger.jsonl`
//! of whoever runs `cargo test`.
//!
//! Path-resolution tests use a serial mutex around `XDG_CONFIG_HOME` and
//! `HOME` because `std::env::set_var` mutates process-global state and
//! other tests in this binary read the same variables.

use std::sync::Mutex;

use scout::took::{
    IssueRef, ParseError, TookError, append_entry, default_ledger_path, format_iso8601_z,
    parse_issue_ref,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Happy-path parse: `OWNER/REPO#N` round-trips into the struct fields
/// without trimming or mutation.
#[test]
fn parses_well_formed_issue_ref() {
    let parsed = parse_issue_ref("truffle-dev/scout#42").expect("valid ref");
    assert_eq!(
        parsed,
        IssueRef {
            owner: "truffle-dev".to_string(),
            repo: "scout".to_string(),
            number: 42,
        }
    );
}

/// `OWNER/REPO` (no `#N`) fails with `MissingHash`. This is the most
/// likely typo: forgetting the issue number entirely.
#[test]
fn missing_hash_rejected() {
    let err = parse_issue_ref("truffle-dev/scout").expect_err("missing hash");
    assert!(matches!(err, ParseError::MissingHash(_)), "got {err:?}");
}

/// `REPO#N` (no `OWNER/`) fails with `MissingSlash`. The owner segment
/// is required for the GitHub API path.
#[test]
fn missing_slash_rejected() {
    let err = parse_issue_ref("scout#42").expect_err("missing slash");
    assert!(matches!(err, ParseError::MissingSlash(_)), "got {err:?}");
}

/// Empty `OWNER` segment (`/repo#1`) is rejected with `EmptySegment`,
/// not silently accepted as a zero-length owner.
#[test]
fn empty_owner_rejected() {
    let err = parse_issue_ref("/scout#1").expect_err("empty owner");
    assert!(matches!(err, ParseError::EmptySegment(_)), "got {err:?}");
}

/// Empty `REPO` segment (`owner/#1`) is rejected with `EmptySegment`.
#[test]
fn empty_repo_rejected() {
    let err = parse_issue_ref("truffle-dev/#1").expect_err("empty repo");
    assert!(matches!(err, ParseError::EmptySegment(_)), "got {err:?}");
}

/// Non-numeric issue number fails with `BadNumber`. A negative number
/// also fails because the parser uses `u32::from_str`.
#[test]
fn non_numeric_issue_rejected() {
    assert!(matches!(
        parse_issue_ref("o/r#abc").expect_err("bad number"),
        ParseError::BadNumber(_),
    ));
    assert!(matches!(
        parse_issue_ref("o/r#-1").expect_err("negative number"),
        ParseError::BadNumber(_),
    ));
    assert!(matches!(
        parse_issue_ref("o/r#").expect_err("empty number"),
        ParseError::BadNumber(_),
    ));
}

/// Append into a fresh empty directory creates the parent dir, the
/// file, and writes a single JSONL line ending in `\n`. The line
/// contains the canonical fields in the documented order.
#[test]
fn append_creates_file_and_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("nested").join("ledger.jsonl");

    let issue = IssueRef {
        owner: "truffle-dev".into(),
        repo: "scout".into(),
        number: 7,
    };
    append_entry(&ledger, &issue, "2026-04-25T07:30:00Z").expect("append");

    assert!(ledger.exists(), "ledger file should exist");
    let body = std::fs::read_to_string(&ledger).unwrap();
    assert_eq!(
        body,
        "{\"repo\":\"truffle-dev/scout\",\"number\":7,\"timestamp\":\"2026-04-25T07:30:00Z\"}\n"
    );
}

/// Append twice writes two distinct JSONL lines. Each line is separately
/// JSON-decodable; the file as a whole is not (that's the point of JSONL).
#[test]
fn second_append_appends_new_line() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.jsonl");

    let issue1 = IssueRef {
        owner: "a".into(),
        repo: "b".into(),
        number: 1,
    };
    let issue2 = IssueRef {
        owner: "c".into(),
        repo: "d".into(),
        number: 2,
    };

    append_entry(&ledger, &issue1, "2026-04-25T07:00:00Z").unwrap();
    append_entry(&ledger, &issue2, "2026-04-25T08:00:00Z").unwrap();

    let body = std::fs::read_to_string(&ledger).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"repo\":\"a/b\""));
    assert!(lines[0].contains("\"number\":1"));
    assert!(lines[1].contains("\"repo\":\"c/d\""));
    assert!(lines[1].contains("\"number\":2"));
}

/// Pre-existing user content in the ledger is not clobbered. We open
/// with append mode, never truncate.
#[test]
fn append_does_not_truncate_existing_content() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.jsonl");
    std::fs::write(&ledger, "# user wrote this\n").unwrap();

    let issue = IssueRef {
        owner: "o".into(),
        repo: "r".into(),
        number: 99,
    };
    append_entry(&ledger, &issue, "2026-04-25T09:00:00Z").unwrap();

    let body = std::fs::read_to_string(&ledger).unwrap();
    assert!(
        body.starts_with("# user wrote this\n"),
        "first line preserved"
    );
    assert!(
        body.contains("\"number\":99"),
        "appended entry present, body was: {body}"
    );
}

/// `format_iso8601_z` produces the documented narrow shape for the unix
/// epoch and a few hand-picked timestamps. These values are also legal
/// inputs to `infer::parse_iso8601_z`, so the formats line up.
#[test]
fn format_iso8601_z_known_values() {
    assert_eq!(format_iso8601_z(0), "1970-01-01T00:00:00Z");
    // 2009-02-13 23:31:30 UTC, the famous billion-second tick.
    assert_eq!(format_iso8601_z(1_234_567_890), "2009-02-13T23:31:30Z");
    // 2026-04-25 12:00:00 UTC, hand-derived: 20568 days since epoch.
    assert_eq!(format_iso8601_z(1_777_118_400), "2026-04-25T12:00:00Z");
    // Leap day check: 2024-02-29 00:00:00 UTC.
    assert_eq!(format_iso8601_z(1_709_164_800), "2024-02-29T00:00:00Z");
}

/// `XDG_CONFIG_HOME` wins for the ledger path when set to an absolute
/// path. Default lands under `$XDG_CONFIG_HOME/scout/ledger.jsonl`.
#[test]
fn xdg_config_home_overrides_home_for_ledger() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: env mutation is serialized via ENV_LOCK above; no other
    // thread reads these variables while the guard is held.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("HOME", "/should/not/be/used");
    }

    let ledger = default_ledger_path().expect("xdg path");
    assert_eq!(ledger, dir.path().join("scout").join("ledger.jsonl"));
}

/// With `XDG_CONFIG_HOME` unset, default ledger path falls back to
/// `$HOME/.config/scout/ledger.jsonl`.
#[test]
fn home_fallback_when_xdg_unset_for_ledger() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("HOME", dir.path());
    }

    let ledger = default_ledger_path().expect("home path");
    assert_eq!(
        ledger,
        dir.path()
            .join(".config")
            .join("scout")
            .join("ledger.jsonl"),
    );
}

/// Neither variable set produces `TookError::NoConfigDir`. Same failure
/// mode as the init module's `default_config_path`.
#[test]
fn no_env_yields_no_config_dir_error_for_ledger() {
    let _g = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("HOME");
    }

    let err = default_ledger_path().expect_err("no env should error");
    assert!(matches!(err, TookError::NoConfigDir));
}

/// A relative `XDG_CONFIG_HOME` is rejected and we fall through to the
/// `HOME` branch, mirroring `init::default_config_path`.
#[test]
fn relative_xdg_falls_through_to_home_for_ledger() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", "relative/path");
        std::env::set_var("HOME", dir.path());
    }

    let ledger = default_ledger_path().expect("home fallback");
    assert_eq!(
        ledger,
        dir.path()
            .join(".config")
            .join("scout")
            .join("ledger.jsonl"),
    );
}
