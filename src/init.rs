//! `scout init` writes a starter config and watchlist into
//! `~/.config/scout/`. Shape decisions:
//!
//! - Default paths follow the XDG Base Directory spec: `$XDG_CONFIG_HOME`
//!   first, then `$HOME/.config`. The CLI's `--config` and `--watchlist`
//!   flags override these.
//! - Existing files are preserved unless `--force` is passed. The summary
//!   tells the caller which files were written and which were left alone.
//! - Templates are embedded at compile time (`include_str!`), so a built
//!   binary doesn't need the source tree at runtime.
//!
//! The starter config is a fully-populated copy of the reference shape so
//! `scout init` plus a manual edit produces a watchlist that scans without
//! further setup. The starter watchlist is a commented placeholder; a user
//! who runs `scout scan` straight after `init` should get an empty result,
//! not a scan of someone else's repos.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const CONFIG_TEMPLATE: &str = include_str!("../templates/config.toml");
const WATCHLIST_TEMPLATE: &str = include_str!("../templates/watchlist.yaml");

/// What `write_starter_files` did with each path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// File did not exist; created with the embedded template.
    Created,
    /// File already existed; left untouched (force was not set).
    Preserved,
    /// File already existed; overwritten because force was set.
    Overwritten,
}

/// Per-call summary returned from `write_starter_files` so the caller can
/// print human output and tests can assert behavior without re-reading the
/// files.
#[derive(Debug, Clone)]
pub struct InitSummary {
    pub config_path: PathBuf,
    pub config: WriteOutcome,
    pub watchlist_path: PathBuf,
    pub watchlist: WriteOutcome,
}

/// Errors surfaced by the init module. Filesystem failures are wrapped
/// once so callers can match on `io::ErrorKind` if they need to.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// Neither `$XDG_CONFIG_HOME` nor `$HOME` was set, so there is no
    /// canonical default path to write to.
    #[error("cannot resolve default config dir: neither $XDG_CONFIG_HOME nor $HOME is set")]
    NoConfigDir,

    /// `fs::create_dir_all` or a write failed. The wrapped path is the one
    /// being written when the error occurred.
    #[error("filesystem error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Default path for the TOML config: `$XDG_CONFIG_HOME/scout/config.toml`,
/// falling back to `$HOME/.config/scout/config.toml`.
pub fn default_config_path() -> Result<PathBuf, InitError> {
    Ok(default_config_dir()?.join("config.toml"))
}

/// Default path for the YAML watchlist: `$XDG_CONFIG_HOME/scout/watchlist.yaml`,
/// falling back to `$HOME/.config/scout/watchlist.yaml`.
pub fn default_watchlist_path() -> Result<PathBuf, InitError> {
    Ok(default_config_dir()?.join("watchlist.yaml"))
}

fn default_config_dir() -> Result<PathBuf, InitError> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg);
        if xdg.is_absolute() {
            return Ok(xdg.join("scout"));
        }
    }
    let home = std::env::var_os("HOME").ok_or(InitError::NoConfigDir)?;
    Ok(PathBuf::from(home).join(".config").join("scout"))
}

/// Write the embedded config and watchlist templates to the given paths.
/// Creates parent directories as needed. If `force` is false, an existing
/// file at either path is preserved and reported as such in the returned
/// summary.
pub fn write_starter_files(
    config_path: &Path,
    watchlist_path: &Path,
    force: bool,
) -> Result<InitSummary, InitError> {
    let config = write_one(config_path, CONFIG_TEMPLATE, force)?;
    let watchlist = write_one(watchlist_path, WATCHLIST_TEMPLATE, force)?;
    Ok(InitSummary {
        config_path: config_path.to_path_buf(),
        config,
        watchlist_path: watchlist_path.to_path_buf(),
        watchlist,
    })
}

fn write_one(path: &Path, contents: &str, force: bool) -> Result<WriteOutcome, InitError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| InitError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let exists = path.try_exists().map_err(|source| InitError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let outcome = match (exists, force) {
        (true, false) => return Ok(WriteOutcome::Preserved),
        (true, true) => WriteOutcome::Overwritten,
        (false, _) => WriteOutcome::Created,
    };

    fs::write(path, contents).map_err(|source| InitError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(outcome)
}

/// CLI entry point for `scout init`. Resolves the config and watchlist
/// paths (CLI overrides, then defaults), writes the templates, and prints
/// one human-readable line per file. Returns `ExitCode::SUCCESS` on a
/// clean run and `ExitCode::from(1)` on any error.
pub fn run(
    config_override: Option<&str>,
    watchlist_override: Option<&str>,
    force: bool,
) -> ExitCode {
    let config_path = match resolve(config_override, default_config_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("scout init: {e}");
            return ExitCode::from(1);
        }
    };
    let watchlist_path = match resolve(watchlist_override, default_watchlist_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("scout init: {e}");
            return ExitCode::from(1);
        }
    };

    match write_starter_files(&config_path, &watchlist_path, force) {
        Ok(summary) => {
            print_outcome(&summary.config_path, summary.config);
            print_outcome(&summary.watchlist_path, summary.watchlist);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("scout init: {e}");
            ExitCode::from(1)
        }
    }
}

fn resolve(
    override_path: Option<&str>,
    default: fn() -> Result<PathBuf, InitError>,
) -> Result<PathBuf, InitError> {
    match override_path {
        Some(p) => Ok(PathBuf::from(p)),
        None => default(),
    }
}

fn print_outcome(path: &Path, outcome: WriteOutcome) {
    let display = path.display();
    match outcome {
        WriteOutcome::Created => println!("created {display}"),
        WriteOutcome::Preserved => println!("kept    {display} (use --force to overwrite)"),
        WriteOutcome::Overwritten => println!("wrote   {display}"),
    }
}
