//! scout CLI entry. Subcommands follow the arch doc: `init` writes a
//! starter TOML config, `scan` runs a watchlist against the GitHub API
//! and prints ranked issues, `took` appends a contribution to the local
//! ledger for cooldown tracking, `explain` shows the score breakdown for
//! a single issue.
//!
//! Fetch + render are not implemented yet; each subcommand exits with a
//! "not yet implemented" message rather than a panic so the binary is
//! safe to hand to someone who wants to poke at it. The shape is fixed
//! here so the upcoming fetch-layer commits slot in without renaming
//! flags or arguments.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "scout",
    version,
    about = "Rank open-source issues worth contributing to",
    long_about = None,
)]
struct Cli {
    /// Path to the TOML config file. Defaults to ~/.config/scout/config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<String>,

    /// Path to the YAML watchlist file. Defaults to
    /// ~/.config/scout/watchlist.yaml.
    #[arg(long, global = true, value_name = "PATH")]
    watchlist: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Write starter config and watchlist files under ~/.config/scout/.
    /// Idempotent: existing files are left alone unless --force is set.
    Init {
        /// Overwrite existing config and watchlist files.
        #[arg(long)]
        force: bool,
    },

    /// Scan the watchlist and print a ranked table of open issues.
    Scan {
        /// Maximum number of issues to show across all repos.
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
        /// Emit JSON instead of the default markdown table.
        #[arg(long)]
        json: bool,
    },

    /// Record a contribution in the local ledger so the cooldown filter
    /// skips the issue on subsequent scans.
    Took {
        /// Issue reference in `OWNER/REPO#N` form.
        #[arg(value_name = "OWNER/REPO#N")]
        issue: String,
    },

    /// Show the per-heuristic score breakdown for a single issue.
    Explain {
        /// Issue reference in `OWNER/REPO#N` form.
        #[arg(value_name = "OWNER/REPO#N")]
        issue: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { .. }
        | Command::Scan { .. }
        | Command::Took { .. }
        | Command::Explain { .. } => {
            eprintln!("scout: fetch layer not implemented yet");
            ExitCode::from(2)
        }
    }
}
