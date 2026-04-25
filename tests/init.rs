//! Filesystem-touching cases on `scout::init`. Each test owns its own
//! `TempDir` and points the writer at paths inside it, so tests do not
//! collide with each other or with the real `~/.config/scout` of whoever
//! runs `cargo test`.
//!
//! Path-resolution tests use a serial mutex around `XDG_CONFIG_HOME` and
//! `HOME` because `std::env::set_var` mutates process-global state and
//! other tests in this binary read the same variables.

use std::sync::Mutex;

use scout::init::{
    InitError, WriteOutcome, default_config_path, default_watchlist_path, write_starter_files,
};
use scout::parse_config;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Both files written into a fresh empty directory; outcomes are `Created`
/// and the on-disk content matches the embedded templates.
#[test]
fn writes_both_templates_into_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let watchlist = dir.path().join("watchlist.yaml");

    let summary = write_starter_files(&config, &watchlist, false).expect("write");
    assert_eq!(summary.config, WriteOutcome::Created);
    assert_eq!(summary.watchlist, WriteOutcome::Created);

    let config_body = std::fs::read_to_string(&config).unwrap();
    let watchlist_body = std::fs::read_to_string(&watchlist).unwrap();
    assert!(config_body.contains("[weights]"));
    assert!(config_body.contains("root_cause"));
    assert!(watchlist_body.contains("repos:"));
}

/// The starter config parses cleanly via the public `parse_config` entry
/// and round-trips into the same defaults the parser produces from an
/// empty string. This is the lock that keeps `init` and `config` in step.
#[test]
fn starter_config_parses_into_reference_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let watchlist = dir.path().join("watchlist.yaml");
    write_starter_files(&config, &watchlist, false).unwrap();

    let body = std::fs::read_to_string(&config).unwrap();
    let cfg = parse_config(&body).expect("starter config should parse");
    assert_eq!(cfg.filters.max_age_days, 30);
    assert_eq!(cfg.output.limit, 20);
    assert_eq!(cfg.output.color, "auto");
    assert_eq!(
        cfg.filters.exclude_labels,
        vec!["wontfix", "invalid", "duplicate"]
    );
}

/// An existing config file is preserved on the second `init` run when
/// `force` is false. The user's edits are not clobbered.
#[test]
fn second_run_without_force_preserves_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let watchlist = dir.path().join("watchlist.yaml");

    write_starter_files(&config, &watchlist, false).unwrap();
    std::fs::write(&config, "# user edited").unwrap();
    std::fs::write(&watchlist, "# user edited").unwrap();

    let summary = write_starter_files(&config, &watchlist, false).expect("write");
    assert_eq!(summary.config, WriteOutcome::Preserved);
    assert_eq!(summary.watchlist, WriteOutcome::Preserved);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "# user edited");
    assert_eq!(
        std::fs::read_to_string(&watchlist).unwrap(),
        "# user edited"
    );
}

/// `force = true` overwrites both files and reports `Overwritten`.
#[test]
fn force_overwrites_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let watchlist = dir.path().join("watchlist.yaml");

    std::fs::write(&config, "# stale").unwrap();
    std::fs::write(&watchlist, "# stale").unwrap();

    let summary = write_starter_files(&config, &watchlist, true).expect("write");
    assert_eq!(summary.config, WriteOutcome::Overwritten);
    assert_eq!(summary.watchlist, WriteOutcome::Overwritten);
    let body = std::fs::read_to_string(&config).unwrap();
    assert!(body.contains("[weights]"), "force should rewrite contents");
}

/// Parent directories are created on first run so the user does not need
/// to `mkdir -p` before invoking `scout init`.
#[test]
fn missing_parent_dirs_are_created() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("nested").join("a").join("config.toml");
    let watchlist = dir.path().join("nested").join("b").join("watchlist.yaml");

    let summary = write_starter_files(&config, &watchlist, false).expect("write");
    assert_eq!(summary.config, WriteOutcome::Created);
    assert_eq!(summary.watchlist, WriteOutcome::Created);
    assert!(config.exists());
    assert!(watchlist.exists());
}

/// `XDG_CONFIG_HOME` wins when set to an absolute path. The default paths
/// land under `$XDG_CONFIG_HOME/scout/`.
#[test]
fn xdg_config_home_overrides_home() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: env mutation is serialized via ENV_LOCK above; no other
    // thread reads these variables while the guard is held.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", dir.path());
        std::env::set_var("HOME", "/should/not/be/used");
    }

    let config = default_config_path().expect("xdg path");
    let watchlist = default_watchlist_path().expect("xdg path");
    assert_eq!(config, dir.path().join("scout").join("config.toml"));
    assert_eq!(watchlist, dir.path().join("scout").join("watchlist.yaml"));
}

/// With `XDG_CONFIG_HOME` unset, default paths fall back to
/// `$HOME/.config/scout/`.
#[test]
fn home_fallback_when_xdg_unset() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("HOME", dir.path());
    }

    let config = default_config_path().expect("home path");
    assert_eq!(
        config,
        dir.path().join(".config").join("scout").join("config.toml"),
    );
}

/// Neither variable set produces `InitError::NoConfigDir`. This is the
/// failure mode for environments like minimal containers that strip both.
#[test]
fn no_env_yields_no_config_dir_error() {
    let _g = ENV_LOCK.lock().unwrap();
    unsafe {
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("HOME");
    }

    let err = default_config_path().expect_err("no env should error");
    assert!(matches!(err, InitError::NoConfigDir));
}

/// A relative `XDG_CONFIG_HOME` is rejected and we fall through to the
/// `HOME` branch. The XDG spec defines the variable as required-absolute.
#[test]
fn relative_xdg_falls_through_to_home() {
    let _g = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", "relative/path");
        std::env::set_var("HOME", dir.path());
    }

    let config = default_config_path().expect("home fallback");
    assert_eq!(
        config,
        dir.path().join(".config").join("scout").join("config.toml"),
    );
}
