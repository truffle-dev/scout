//! Signal inference from issue content. Pure pattern-match functions
//! that take an `IssueMeta`'s body text or labels and return the
//! boolean signals the scoring layer consumes.
//!
//! The functions here deliberately stay simple. Regex would catch a
//! few more cases but adds a dependency and hides the rules from the
//! reader; hand-rolled checks make the heuristic transparent and keep
//! the build tight. False positives on a single signal don't poison
//! the final score because each signal is weighted individually.
//!
//! Time-decay signals (`updated_days_ago`, `pushed_days_ago`) and
//! signals that need additional API calls (maintainer_touched, no_pr,
//! contributing_ok) live elsewhere; this module only handles signals
//! derivable from a single `IssueMeta` without extra network.

use crate::fetch::Label;

/// Body contains something that looks like a reproducer: a fenced
/// code block, or an explicit "reproduce" cue.
///
/// A fenced code block is a strong signal because most repro sections
/// include one even when the prose doesn't use the word. The word cue
/// catches issues where the reporter wrote out steps in plain prose.
/// `None` body (issue with no description) is always false.
pub fn has_reproducer(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return false;
    };
    if body.contains("```") {
        return true;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("reproduce") || lower.contains("minimal example") || lower.contains("to repro")
}

/// Body contains a pinpoint that suggests the reporter has narrowed
/// the cause: a `path.ext:lineno` reference, a "root cause" phrase,
/// or a "caused by" phrase.
///
/// The file-extension set covers the languages scout's target repos
/// are written in. Adding a new language is one line. We look for
/// common source extensions followed by `:` and a digit; this is
/// what compilers, stack traces, and careful bug reports produce.
pub fn has_root_cause(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return false;
    };
    let lower = body.to_ascii_lowercase();
    if lower.contains("root cause") || lower.contains("caused by") {
        return true;
    }
    has_file_line_pointer(body)
}

/// Label set contains a low-effort marker.
///
/// Matched against the curated list below. Exact match on the label
/// name (GitHub labels are case-preserved but conventionally
/// lowercase with hyphens/slashes). A repo that uses a different
/// convention will fail to match here; that's a weight-tuning issue,
/// not a bug in the signal.
pub fn has_effort_label(labels: &[Label]) -> bool {
    const LOW_EFFORT: &[&str] = &[
        "good first issue",
        "help wanted",
        "effort/low",
        "effort/medium",
        "easy",
        "beginner",
        "beginner-friendly",
        "low-hanging-fruit",
    ];
    labels.iter().any(|l| {
        LOW_EFFORT
            .iter()
            .any(|&marker| l.name.eq_ignore_ascii_case(marker))
    })
}

/// Scan for a `path.ext:lineno` pointer in the body. Walks through a
/// small set of common source extensions; the match is substring +
/// digit-after-colon, which is lightweight and correct for the usual
/// shapes (e.g. `src/foo.rs:123`, `lib/bar.py:45`, `index.ts:10`).
fn has_file_line_pointer(body: &str) -> bool {
    const EXTENSIONS: &[&str] = &[
        ".rs:", ".py:", ".ts:", ".tsx:", ".js:", ".jsx:", ".go:", ".java:", ".c:", ".cc:", ".cpp:",
        ".cxx:", ".h:", ".hpp:", ".rb:", ".kt:", ".swift:", ".mjs:", ".cjs:",
    ];
    EXTENSIONS.iter().any(|ext| {
        body.match_indices(ext).any(|(idx, _)| {
            let rest = &body[idx + ext.len()..];
            rest.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
    })
}
