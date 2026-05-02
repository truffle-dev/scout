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
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::config::{Config, Filters, parse as parse_config};
use crate::fetch::{CommentMeta, IssueMeta, RepoMeta, TimelineEvent};
use crate::fetcher::{AgeFilter, fetch_repos};
use crate::infer::{days_since, parse_iso8601_z};
use crate::init;
use crate::rank::{RankInput, rank};
use crate::render;
use crate::took;
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

    /// True if the issue is in the cooldown window: it appears in the
    /// ledger and the most recent take is within `cooldown_days` of
    /// `now_unix`. The boundary is exclusive on the late side, so an
    /// issue taken exactly `cooldown_days` ago is no longer in
    /// cooldown — that matches the natural reading of "wait N days
    /// before taking again."
    ///
    /// Issues not in the ledger return `false`: a fresh user with no
    /// `scout took` history has nothing in cooldown. `cooldown_days`
    /// of `0` returns `false` for every issue, which matches "no
    /// cooldown configured" and lets the planner short-circuit
    /// without a separate code path.
    ///
    /// Future-dated takes (clock skew between the ledger writer and
    /// the scan caller) are treated as in cooldown rather than
    /// erroring. This is a permissive call site by design: a wrong
    /// clock should not crash a scan, only delay re-listing the
    /// affected issue until the skew is corrected.
    pub fn in_cooldown(
        &self,
        owner: &str,
        repo: &str,
        number: u32,
        cooldown_days: u32,
        now_unix: i64,
    ) -> bool {
        if cooldown_days == 0 {
            return false;
        }
        let Some(taken_at) = self.last_taken(owner, repo, number) else {
            return false;
        };
        let elapsed = now_unix - taken_at;
        let cooldown_secs = (cooldown_days as i64) * 86_400;
        elapsed < cooldown_secs
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

/// One repo's worth of fetched data, owned by the orchestrator and
/// borrowed into [`crate::rank::RankInput`] values by [`plan`]. The
/// fields mirror the payloads `fetch::repo_meta`,
/// `fetch::contributing_md`, and `fetch::list_issues_paginated`
/// produce; bundling them per-repo keeps the per-issue references
/// in `RankInput` from straddling multiple owning collections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRepo {
    pub repo: RepoMeta,
    pub contributing: Option<String>,
    pub issues: Vec<FetchedIssue>,
}

/// One issue's worth of fetched data inside a [`FetchedRepo`].
/// Bundles the payloads `fetch::list_issue_comments` and
/// `fetch::list_issue_timeline` produce alongside the issue's own
/// metadata so [`plan`] can build a single
/// [`crate::rank::RankInput`] from one `FetchedIssue` borrow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedIssue {
    pub issue: IssueMeta,
    pub comments: Vec<CommentMeta>,
    pub timeline: Vec<TimelineEvent>,
}

/// Filter pre-fetched payloads against the user's filters and the
/// cooldown ledger, returning the per-issue inputs ready for the
/// ranking layer. The returned `RankInput`s borrow from `repos`, so
/// the caller keeps ownership of the fetch buffers across the call.
///
/// Filters applied, in order:
///
///   1. PR-shaped items (the issues endpoint returns both): dropped.
///   2. Excluded labels (`filters.exclude_labels`, case-sensitive
///      direct match per the config-layer contract): dropped if any
///      label name matches.
///   3. Issue age (`filters.max_age_days`, computed from
///      `issue.created_at` against `now_unix`): dropped if older.
///      `0` disables the check, matching the cooldown convention.
///      An unparseable timestamp passes through, since the scoring
///      layer already treats unparseable timestamps as "very old"
///      via the recency decay.
///   4. Cooldown ([`LedgerIndex::in_cooldown`]): dropped if recently
///      taken. Issue numbers that exceed `u32` are passed through
///      untouched (the ledger keys on `u32`); the asymmetry is
///      vanishingly rare in practice and the safe permissive
///      default is to surface the issue rather than drop it.
///
/// `min_score` is not applied here. Score is computed by the ranker
/// downstream of this function, so filtering on score happens
/// against the ranker's output, not the planner's input.
pub fn plan<'a>(
    repos: &'a [FetchedRepo],
    filters: &Filters,
    ledger: &LedgerIndex,
    now_unix: i64,
) -> Vec<RankInput<'a>> {
    let mut out = Vec::new();
    for fr in repos {
        let owner_repo = fr.repo.full_name.split_once('/');
        for fi in &fr.issues {
            if fi.issue.is_pull_request() {
                continue;
            }
            if fi
                .issue
                .labels
                .iter()
                .any(|l| filters.exclude_labels.iter().any(|ex| ex == &l.name))
            {
                continue;
            }
            if filters.max_age_days != 0
                && let Some(days) = days_since(&fi.issue.created_at, now_unix)
                && days > filters.max_age_days as i64
            {
                continue;
            }
            if let Some((owner, repo_name)) = owner_repo
                && let Ok(num) = u32::try_from(fi.issue.number)
                && ledger.in_cooldown(owner, repo_name, num, filters.cooldown_days, now_unix)
            {
                continue;
            }
            out.push(RankInput {
                issue: &fi.issue,
                repo: &fr.repo,
                contributing: fr.contributing.as_deref(),
                comments: &fi.comments,
                timeline: &fi.timeline,
            });
        }
    }
    out
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

/// CLI entry point for `scout scan`. Loads config, watchlist, and
/// ledger; resolves the auth token (config `token_path` first, then
/// `$GITHUB_TOKEN`); runs the async fetcher inside a fresh tokio
/// runtime; plans + ranks the fetched payloads; prints the rendered
/// output. Returns `ExitCode::SUCCESS` on a clean run and
/// `ExitCode::from(1)` on any error.
pub fn run(
    config_override: Option<&str>,
    watchlist_override: Option<&str>,
    ledger_override: Option<&str>,
    limit_override: Option<u32>,
    json: bool,
) -> ExitCode {
    match run_inner(
        config_override,
        watchlist_override,
        ledger_override,
        limit_override,
        json,
    ) {
        Ok(rendered) => {
            print!("{rendered}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("scout scan: {e}");
            ExitCode::from(1)
        }
    }
}

pub(crate) type RunError = Box<dyn std::error::Error + Send + Sync>;

fn run_inner(
    config_override: Option<&str>,
    watchlist_override: Option<&str>,
    ledger_override: Option<&str>,
    limit_override: Option<u32>,
    json: bool,
) -> Result<String, RunError> {
    let config_path = match config_override {
        Some(p) => PathBuf::from(p),
        None => init::default_config_path()?,
    };
    let config = load_config(&config_path)?;

    let watchlist_path = match watchlist_override {
        Some(p) => PathBuf::from(p),
        None => init::default_watchlist_path()?,
    };
    let watchlist = load_watchlist(&watchlist_path)?;

    let ledger_path = match ledger_override {
        Some(p) => PathBuf::from(p),
        None => took::default_ledger_path()?,
    };
    let ledger = load_ledger(&ledger_path)?;

    let token = resolve_token(config.auth.token_path.as_deref())?;

    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let age_filter = AgeFilter {
        max_age_days: config.filters.max_age_days,
        now_unix,
    };
    let runtime = tokio::runtime::Runtime::new()?;
    let repos = runtime.block_on(fetch_repos(&watchlist, token.as_deref(), age_filter))?;

    let inputs = plan(&repos, &config.filters, &ledger, now_unix);
    let mut rows = rank(&inputs, &config.weights.into(), now_unix);
    rows.retain(|row| row.breakdown.total >= config.filters.min_score);

    let limit = limit_override
        .map(|n| n as usize)
        .unwrap_or(config.output.limit as usize);

    if json {
        let mut s = render::json(&rows, Some(limit))?;
        s.push('\n');
        Ok(s)
    } else {
        Ok(render::table_markdown(&rows, Some(limit)))
    }
}

/// Resolve the GitHub auth token. Config `token_path` wins if set
/// (with `~/` tilde expansion against `$HOME`); otherwise fall back
/// to the `$GITHUB_TOKEN` environment variable. Whitespace is
/// trimmed off whichever source supplies the value, so a token file
/// with a trailing newline works.
pub(crate) fn resolve_token(token_path: Option<&str>) -> Result<Option<String>, RunError> {
    if let Some(path) = token_path {
        let expanded = expand_tilde(path);
        let raw = fs::read_to_string(&expanded).map_err(|source| ScanError::Io {
            path: expanded,
            source,
        })?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        return Ok(Some(trimmed.to_string()));
    }
    if let Ok(env_token) = std::env::var("GITHUB_TOKEN") {
        let trimmed = env_token.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_string()));
        }
    }
    Ok(None)
}

/// Expand a leading `~/` against `$HOME`. Anything else passes
/// through verbatim. Mirrors the shell convention without pulling
/// in a dirs crate; the watchlist parser and the ledger writer both
/// already do their own `$HOME` reads in the same shape.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}
