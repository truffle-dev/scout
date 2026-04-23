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
//! Time-delta signals (`days_since`) also live here because they're
//! derivable from a single `IssueMeta` field without extra network.
//! Signals that need additional API calls (maintainer_touched, no_pr,
//! contributing_ok) live elsewhere.

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

/// Label set contains a marker that says "this isn't an actionable
/// bug." Paired with `has_effort_label` by the aggregator to derive
/// the `effort_ok` factor; the positive low-effort label wins when
/// both are present.
///
/// Same exact-match policy as `has_effort_label`.
pub fn has_non_effort_label(labels: &[Label]) -> bool {
    const NON_EFFORT: &[&str] = &["enhancement", "question", "design", "rfc", "discussion"];
    labels.iter().any(|l| {
        NON_EFFORT
            .iter()
            .any(|&marker| l.name.eq_ignore_ascii_case(marker))
    })
}

/// Days between an ISO-8601 timestamp and a reference unix-seconds
/// `now`. Negative values are possible if the timestamp is in the
/// future (clock skew between GitHub and the caller). Returns `None`
/// if the timestamp fails to parse.
///
/// Taking `now` as a parameter keeps this deterministic and
/// test-friendly; the caller grabs wall-clock seconds once per scan.
pub fn days_since(iso: &str, now_unix: i64) -> Option<i64> {
    let then = parse_iso8601_z(iso)?;
    Some((now_unix - then).div_euclid(86_400))
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

/// Parse an ISO-8601 timestamp in the narrow shape GitHub returns:
/// `YYYY-MM-DDTHH:MM:SSZ` (20 bytes, trailing `Z`, no fractional
/// seconds, no offset). Returns unix-seconds or `None` on any shape
/// mismatch. We intentionally do not try to handle the wider
/// ISO-8601 surface, because the GitHub API is consistent and a
/// tolerant parser would just hide upstream format changes.
///
/// Date-to-days uses the Fliegel & Van Flandern Julian Day Number
/// formula; the algorithm is public domain and correct for all
/// dates in the Gregorian calendar. Unix epoch is JDN 2440588.
fn parse_iso8601_z(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() != 20 || bytes[19] != b'Z' {
        return None;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let y: i32 = s.get(0..4)?.parse().ok()?;
    let mo: i32 = s.get(5..7)?.parse().ok()?;
    let d: i32 = s.get(8..10)?.parse().ok()?;
    let h: i64 = s.get(11..13)?.parse().ok()?;
    let mi: i64 = s.get(14..16)?.parse().ok()?;
    let se: i64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    if !(0..=23).contains(&h) || !(0..=59).contains(&mi) || !(0..=60).contains(&se) {
        return None;
    }
    let a = (14 - mo) / 12;
    let yy = y + 4800 - a;
    let mm = mo + 12 * a - 3;
    let jdn = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32_045;
    let days_since_epoch = (jdn - 2_440_588) as i64;
    Some(days_since_epoch * 86_400 + h * 3_600 + mi * 60 + se)
}
