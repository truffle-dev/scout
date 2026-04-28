//! Orchestrator for `scout scan`. This is the layer that ties the
//! watchlist, config, ledger, and fetch modules together into the
//! ranked-issue listing the CLI prints. It owns disk IO and (later)
//! the per-issue async fetch budget; the modules it consumes stay
//! pure so they remain unit-testable without it.
//!
//! Three loaders ship today: `load_watchlist` (YAML, hand-rolled
//! parser), `load_config` (TOML, serde), and `load_ledger` (JSONL,
//! serde + a small post-pass to dedupe by most-recent timestamp).
//! Disk-IO errors and parse errors are folded into a single
//! `ScanError` so the runner above only matches one type. Future
//! slices will add the per-repo planner, the rate-limit-aware
//! fetcher, and the renderer on the same `ScanError` stack.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::{Config, parse as parse_config};
use crate::infer::parse_iso8601_z;
use crate::watchlist::{Watchlist, WatchlistError, parse as parse_watchlist};

/// Errors surfaced by the scan orchestrator. Each variant tags the
/// path that produced the error so messages are actionable without
/// the caller threading paths around.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// `fs::read_to_string` failed. The wrapped path is the file we
    /// were trying to read; the inner `io::Error` carries the kind
    /// (NotFound, PermissionDenied, IsADirectory, etc.).
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// The file read but the contents did not parse as a watchlist.
    /// The wrapped path lets the renderer print
    /// `<path>: line 5: malformed entry "a/b/c"` without a second
    /// pass through the message.
    #[error("watchlist {path}: {source}")]
    Watchlist {
        path: PathBuf,
        #[source]
        source: WatchlistError,
    },

    /// The file read but the contents did not parse as TOML config.
    /// The wrapped path lets the renderer prefix the toml-de error
    /// (which already carries its own line/column span) with the
    /// file location in one render pass.
    #[error("config {path}: {source}")]
    Config {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    /// The file read but a line did not parse as a ledger entry. The
    /// wrapped path lets the renderer print
    /// `<path>: line 5: malformed timestamp ...` without a second
    /// pass through the message.
    #[error("ledger {path}: {source}")]
    Ledger {
        path: PathBuf,
        #[source]
        source: LedgerError,
    },
}

/// Errors surfaced by the JSONL ledger parser. Each variant carries
/// the line number so users can fix the offending entry by editing
/// the file at the reported line; the line numbers are 1-based to
/// match what an editor's gutter shows.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// `serde_json::from_str` failed on a line. The wrapped error
    /// already carries column-level detail; the line number we add
    /// is the one in the ledger file as a whole.
    #[error("line {line}: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    /// The line parsed as JSON but the `repo` field was not in the
    /// `OWNER/REPO` shape. This happens if a user hand-edits the file
    /// or merges a corrupt ledger; we surface rather than silently
    /// skip so the user can fix the source.
    #[error("line {line}: malformed repo {repo:?}: expected OWNER/REPO")]
    MalformedRepo { line: usize, repo: String },

    /// The line parsed as JSON but the `timestamp` field was not in
    /// the narrow `YYYY-MM-DDTHH:MM:SSZ` shape `scout took` writes.
    /// This is the same parser the days-since signal uses, so the
    /// shape is stable across the codebase.
    #[error("line {line}: malformed timestamp {timestamp:?}: expected YYYY-MM-DDTHH:MM:SSZ")]
    Timestamp { line: usize, timestamp: String },
}

/// A read-only view of the JSONL ledger, keyed by `(owner, repo,
/// number)` to the unix-seconds timestamp of the most recent take.
/// Tail-merging two ledgers (`cat a b > c`) is preserved: when the
/// same issue appears multiple times, the latest timestamp wins.
/// The cooldown filter consumes this; the loader does not enforce
/// `cooldown_days` itself.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LedgerIndex {
    entries: HashMap<(String, String, u32), i64>,
}

impl LedgerIndex {
    /// Number of distinct issues recorded. Multiple ledger lines for
    /// the same issue collapse to one entry, so this is at most the
    /// line count of the source file.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no issues are recorded. A missing ledger file and a
    /// ledger of only blank lines both produce an empty index.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Unix-seconds timestamp of the most recent take for the issue
    /// identified by `owner`, `repo`, and `number`. `None` if the
    /// issue is not in the ledger.
    pub fn last_taken(&self, owner: &str, repo: &str, number: u32) -> Option<i64> {
        self.entries
            .get(&(owner.to_string(), repo.to_string(), number))
            .copied()
    }
}

/// Read and parse the watchlist at `path`. Returns the parsed
/// `Watchlist` on success; on failure the path is folded into the
/// error so the caller can print a single line with both the file
/// and the underlying cause.
///
/// An empty file and a comments-only file both produce an empty
/// watchlist; that is the parser's contract and the orchestrator
/// preserves it. The starter template `scout init` writes is in
/// the comments-only shape, so `init` followed immediately by
/// `scan` returns an empty watchlist rather than an error.
pub fn load_watchlist(path: &Path) -> Result<Watchlist, ScanError> {
    let body = fs::read_to_string(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_watchlist(&body).map_err(|source| ScanError::Watchlist {
        path: path.to_path_buf(),
        source,
    })
}

/// Read and parse the TOML config at `path`. Returns the parsed
/// `Config` on success; on failure the path is folded into the
/// error so the caller can print a single line with both the file
/// and the underlying cause.
///
/// An empty file parses to a fully-defaulted `Config`, matching
/// the parser-layer contract: every section has a `Default` impl,
/// so a user who runs `scout init` and never edits the file still
/// gets the reference weighting. Unknown TOML keys are rejected
/// at parse time (`#[serde(deny_unknown_fields)]`) so typos
/// surface instead of silently turning into default values.
pub fn load_config(path: &Path) -> Result<Config, ScanError> {
    let body = fs::read_to_string(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_config(&body).map_err(|source| ScanError::Config {
        path: path.to_path_buf(),
        source,
    })
}

/// Read and parse the JSONL ledger at `path`. Returns a `LedgerIndex`
/// keyed by `(owner, repo, number)` on success; on failure the path
/// is folded into the error so the caller can print a single line.
///
/// Missing files round-trip as an empty index. The cooldown semantic
/// is "no ledger means nothing in cooldown," which matches what a
/// fresh user sees before their first `scout took`. Other IO errors
/// (permission denied, EISDIR, etc.) still surface as
/// `ScanError::Io` with the path attached.
///
/// Within the file, blank lines are tolerated and skipped; every
/// non-blank line must parse as `{"repo":"o/r","number":N,"timestamp":"…Z"}`
/// or the load fails fast with the offending line number. Silently
/// skipping malformed lines would hide ledger corruption.
///
/// When the same `(owner, repo, number)` appears on multiple lines,
/// the most recent timestamp wins. This preserves the tail-merge
/// invariant: concatenating two ledgers (`cat a b > c`) and reading
/// `c` gives the same view as reading either alone and taking the
/// later record per issue.
pub fn load_ledger(path: &Path) -> Result<LedgerIndex, ScanError> {
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(LedgerIndex::default()),
        Err(source) => {
            return Err(ScanError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    parse_ledger(&body).map_err(|source| ScanError::Ledger {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Deserialize)]
struct LedgerLine {
    repo: String,
    number: u32,
    timestamp: String,
}

fn parse_ledger(body: &str) -> Result<LedgerIndex, LedgerError> {
    let mut entries: HashMap<(String, String, u32), i64> = HashMap::new();

    for (idx, line) in body.lines().enumerate() {
        let line_num = idx + 1;
        if line.trim().is_empty() {
            continue;
        }

        let parsed: LedgerLine =
            serde_json::from_str(line).map_err(|source| LedgerError::Json {
                line: line_num,
                source,
            })?;

        let (owner, repo) =
            parsed
                .repo
                .split_once('/')
                .ok_or_else(|| LedgerError::MalformedRepo {
                    line: line_num,
                    repo: parsed.repo.clone(),
                })?;
        if owner.is_empty() || repo.is_empty() {
            return Err(LedgerError::MalformedRepo {
                line: line_num,
                repo: parsed.repo.clone(),
            });
        }

        let secs = parse_iso8601_z(&parsed.timestamp).ok_or_else(|| LedgerError::Timestamp {
            line: line_num,
            timestamp: parsed.timestamp.clone(),
        })?;

        let key = (owner.to_string(), repo.to_string(), parsed.number);
        entries
            .entry(key)
            .and_modify(|existing| {
                if secs > *existing {
                    *existing = secs;
                }
            })
            .or_insert(secs);
    }

    Ok(LedgerIndex { entries })
}
