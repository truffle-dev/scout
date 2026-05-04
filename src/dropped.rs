//! `scout dropped` records an engagement that was investigated and
//! abandoned, in the same JSONL ledger that `took` writes to. The
//! cooldown filter treats both events identically: any prior record
//! suppresses the issue from future scan output. Use this for issues
//! you commented on, scoped, or attempted but decided not to PR.
//!
//! Shape decisions:
//!
//! - One ledger, two event tags. Splitting the file would force the
//!   cooldown reader to merge two sources at scan time and would
//!   double the file-format surface for what is, semantically, the
//!   same operation: "I touched this issue, do not surface it again
//!   for a while." The `event` field on each line is enough.
//! - This module is a thin CLI shim; the path resolver, parser, and
//!   writer all live in [`crate::took`]. No copy-paste, no parallel
//!   error type. A future event tag (e.g. `"merged"`) only needs to
//!   add another shim like this one.
//! - The success line says "recorded drop of …" rather than "recorded
//!   …" so the operator can grep their shell history and tell the
//!   two writers apart without reopening the ledger.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::took::{append_entry_with_event, default_ledger_path, now_iso8601_z, parse_issue_ref};

/// `event` value written for `scout dropped` lines.
pub const EVENT: &str = "dropped";

/// CLI entry point for `scout dropped`. Same plumbing as
/// [`crate::took::run`]; only the event tag and confirmation line
/// differ. `ExitCode::SUCCESS` on a clean run, `ExitCode::from(1)`
/// on any error.
pub fn run(ledger_override: Option<&str>, issue_ref: &str) -> ExitCode {
    let issue = match parse_issue_ref(issue_ref) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("scout dropped: {e}");
            return ExitCode::from(1);
        }
    };
    let path = match ledger_override {
        Some(p) => PathBuf::from(p),
        None => match default_ledger_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("scout dropped: {e}");
                return ExitCode::from(1);
            }
        },
    };
    let timestamp = match now_iso8601_z() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("scout dropped: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = append_entry_with_event(&path, &issue, &timestamp, EVENT) {
        eprintln!("scout dropped: {e}");
        return ExitCode::from(1);
    }
    println!(
        "recorded drop of {}/{}#{} at {} -> {}",
        issue.owner,
        issue.repo,
        issue.number,
        timestamp,
        path.display()
    );
    ExitCode::SUCCESS
}
