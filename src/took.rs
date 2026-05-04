//! `scout took` records a contribution in the local JSONL ledger so the
//! cooldown filter skips the issue on subsequent scans.
//!
//! Shape decisions:
//!
//! - The ledger is line-delimited JSON, one entry per line, append-only.
//!   This is the simplest shape that is both human-readable and trivially
//!   parsed back into a `HashMap<(owner, repo, number), timestamp>` for
//!   the cooldown filter; tail-merging two ledgers is `cat a b > c`.
//! - Default path follows the XDG spec, same as `scout init`:
//!   `$XDG_CONFIG_HOME/scout/ledger.jsonl` or `$HOME/.config/scout/ledger.jsonl`.
//!   The `--ledger PATH` global flag overrides.
//! - Timestamps are ISO-8601 UTC, second precision, trailing `Z`. This is
//!   the shape `infer::parse_iso8601_z` already accepts, so the cooldown
//!   read path needs no second parser. The reverse formula is also pure
//!   stdlib so scout stays chrono-free.
//!
//! The function set mirrors `init`: a parser, a path resolver, a writer,
//! and a CLI runner. Errors flow through one `TookError` so the runner
//! can match-and-print without per-call error variants leaking out.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use serde::Serialize;

const LEDGER_FILENAME: &str = "ledger.jsonl";

/// A parsed `OWNER/REPO#N` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u32,
}

/// Issue-reference parse failures. The argument carries the offending
/// text so error messages can quote it back to the user.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("missing '#': expected OWNER/REPO#N, got {0:?}")]
    MissingHash(String),
    #[error("missing '/': expected OWNER/REPO#N, got {0:?}")]
    MissingSlash(String),
    #[error("issue number must be a positive integer, got {0:?}")]
    BadNumber(String),
    #[error("owner or repo segment is empty in {0:?}")]
    EmptySegment(String),
}

/// Errors surfaced by the took module.
#[derive(Debug, thiserror::Error)]
pub enum TookError {
    /// Neither `$XDG_CONFIG_HOME` nor `$HOME` was set, so there is no
    /// canonical default path to write the ledger to.
    #[error("cannot resolve default ledger dir: neither $XDG_CONFIG_HOME nor $HOME is set")]
    NoConfigDir,

    /// The user-supplied issue reference did not parse.
    #[error("invalid issue reference: {0}")]
    Parse(#[from] ParseError),

    /// `fs::create_dir_all` or a write failed. The wrapped path is the
    /// one being written when the error occurred.
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// `SystemTime::now()` returned a value before the unix epoch. This
    /// is essentially never reachable on a sane host but the type system
    /// makes us say it.
    #[error("system clock before unix epoch: {0}")]
    Clock(#[from] SystemTimeError),
}

/// Parse a reference of the form `OWNER/REPO#N`. Owner and repo segments
/// must be non-empty; the number must be a `u32`. The slash and hash are
/// fixed delimiters, matching the shape `gh issue view` and the GitHub
/// web UI both use.
pub fn parse_issue_ref(s: &str) -> Result<IssueRef, ParseError> {
    let (slug, num_str) = s
        .split_once('#')
        .ok_or_else(|| ParseError::MissingHash(s.to_string()))?;
    let (owner, repo) = slug
        .split_once('/')
        .ok_or_else(|| ParseError::MissingSlash(s.to_string()))?;
    if owner.is_empty() || repo.is_empty() {
        return Err(ParseError::EmptySegment(s.to_string()));
    }
    let number: u32 = num_str
        .parse()
        .map_err(|_| ParseError::BadNumber(num_str.to_string()))?;
    Ok(IssueRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

/// Default path for the JSONL ledger:
/// `$XDG_CONFIG_HOME/scout/ledger.jsonl`, falling back to
/// `$HOME/.config/scout/ledger.jsonl`.
pub fn default_ledger_path() -> Result<PathBuf, TookError> {
    Ok(default_ledger_dir()?.join(LEDGER_FILENAME))
}

fn default_ledger_dir() -> Result<PathBuf, TookError> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg);
        if xdg.is_absolute() {
            return Ok(xdg.join("scout"));
        }
    }
    let home = std::env::var_os("HOME").ok_or(TookError::NoConfigDir)?;
    Ok(PathBuf::from(home).join(".config").join("scout"))
}

/// Default `event` value for legacy ledger lines that pre-date the
/// field. The first ledger format wrote only `{repo, number, timestamp}`;
/// reading those entries today should treat them as `took`, since that
/// was the only writer at the time.
pub const DEFAULT_EVENT: &str = "took";

#[derive(Debug, Serialize)]
struct LedgerEntry<'a> {
    repo: String,
    number: u32,
    timestamp: &'a str,
    event: &'a str,
}

/// Append a JSONL line tagged `event = "took"` for `issue` at
/// `timestamp_iso` to `ledger_path`. Thin wrapper around
/// [`append_entry_with_event`] for the most common writer.
pub fn append_entry(
    ledger_path: &Path,
    issue: &IssueRef,
    timestamp_iso: &str,
) -> Result<(), TookError> {
    append_entry_with_event(ledger_path, issue, timestamp_iso, DEFAULT_EVENT)
}

/// Append a JSONL line for `issue` at `timestamp_iso` to `ledger_path`,
/// tagging it with `event` (e.g. `"took"`, `"dropped"`). Creates parent
/// directories and the file itself as needed; never truncates. The line
/// shape is `{"repo":"o/r","number":N,"timestamp":"…Z","event":"…"}\n`.
pub fn append_entry_with_event(
    ledger_path: &Path,
    issue: &IssueRef,
    timestamp_iso: &str,
    event: &str,
) -> Result<(), TookError> {
    if let Some(parent) = ledger_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|source| TookError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let entry = LedgerEntry {
        repo: format!("{}/{}", issue.owner, issue.repo),
        number: issue.number,
        timestamp: timestamp_iso,
        event,
    };
    let mut line = serde_json::to_string(&entry).expect("LedgerEntry serializes");
    line.push('\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger_path)
        .map_err(|source| TookError::Io {
            path: ledger_path.to_path_buf(),
            source,
        })?;
    file.write_all(line.as_bytes())
        .map_err(|source| TookError::Io {
            path: ledger_path.to_path_buf(),
            source,
        })?;
    Ok(())
}

/// Format unix-seconds as `YYYY-MM-DDTHH:MM:SSZ`. Inverse of
/// `infer::parse_iso8601_z`. Uses the public-domain inverse-Julian-Day
/// formula for the date part and integer arithmetic on the
/// seconds-of-day for the time part. No external dep, no chrono.
pub fn format_iso8601_z(unix_secs: i64) -> String {
    let day = unix_secs.div_euclid(86_400);
    let secs_in_day = unix_secs.rem_euclid(86_400);
    let h = secs_in_day / 3_600;
    let mi = (secs_in_day % 3_600) / 60;
    let se = secs_in_day % 60;

    let jdn = day + 2_440_588;
    let mut l = jdn + 68_569;
    let n = (4 * l) / 146_097;
    l -= (146_097 * n + 3) / 4;
    let i = (4_000 * (l + 1)) / 1_461_001;
    l = l - (1_461 * i) / 4 + 31;
    let j = (80 * l) / 2_447;
    let d = l - (2_447 * j) / 80;
    let l2 = j / 11;
    let mo = j + 2 - 12 * l2;
    let y = 100 * (n - 49) + i + l2;

    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z")
}

/// Wall-clock now formatted as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso8601_z() -> Result<String, TookError> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    Ok(format_iso8601_z(secs))
}

/// CLI entry point for `scout took`. Parses `issue_ref`, resolves the
/// ledger path (override > default), appends a fresh entry, and prints
/// a one-line confirmation. `ExitCode::SUCCESS` on a clean run,
/// `ExitCode::from(1)` on any error.
pub fn run(ledger_override: Option<&str>, issue_ref: &str) -> ExitCode {
    let issue = match parse_issue_ref(issue_ref) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("scout took: {e}");
            return ExitCode::from(1);
        }
    };
    let path = match ledger_override {
        Some(p) => PathBuf::from(p),
        None => match default_ledger_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("scout took: {e}");
                return ExitCode::from(1);
            }
        },
    };
    let timestamp = match now_iso8601_z() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("scout took: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = append_entry(&path, &issue, &timestamp) {
        eprintln!("scout took: {e}");
        return ExitCode::from(1);
    }
    println!(
        "recorded {}/{}#{} at {} -> {}",
        issue.owner,
        issue.repo,
        issue.number,
        timestamp,
        path.display()
    );
    ExitCode::SUCCESS
}
