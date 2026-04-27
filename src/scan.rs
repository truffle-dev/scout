//! Orchestrator for `scout scan`. This is the layer that ties the
//! watchlist, config, ledger, and fetch modules together into the
//! ranked-issue listing the CLI prints. It owns disk IO and (later)
//! the per-issue async fetch budget; the modules it consumes stay
//! pure so they remain unit-testable without it.
//!
//! The first slice is `load_watchlist`: read the YAML file from disk
//! and parse it. Disk-IO errors and parse errors are folded into a
//! single `ScanError` so the runner above only matches one type.
//! Future slices will add config and ledger loaders, the per-repo
//! planner, the rate-limit-aware fetcher, and the renderer.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
