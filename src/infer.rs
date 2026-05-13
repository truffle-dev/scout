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
//! contributing_ok) classify their fetched payload here too; only
//! the network call lives in `fetch`.

use crate::fetch::CommentMeta;
use crate::fetch::Label;
use crate::fetch::TimelineEvent;

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

/// CONTRIBUTING body looks contribution-friendly: no Contributor
/// License Agreement gate, no "please discuss / email / contact
/// before" gate, no categorical ban on AI-implemented contributions,
/// no label-gate that auto-closes PRs whose linked issue is missing
/// a specific label.
/// `None` (repo has no CONTRIBUTING) is treated as "ok" — most small
/// repos don't ship one and they're conventionally open to drive-by
/// PRs; the repos that do gate contributions write the gate into
/// CONTRIBUTING explicitly.
///
/// Pattern set errs on the side of "not ok": over-awarding a gated
/// repo would cost a wasted PR, under-awarding a friendly one just
/// trims the recency bonus. The CLA vocabulary covers the common
/// enterprise shapes (Google / Meta / Microsoft / CNCF EasyCLA /
/// Eclipse ECA / Apache ICLA); the gate vocabulary covers the
/// "open an issue / discuss / email / reach out first" idioms; the
/// AI-ban vocabulary covers projects that categorically refuse
/// AI-implemented or autonomous-agent contributions (typst, astral
/// org, sprocket are the canonical instances at the time of writing);
/// the label-gate vocabulary covers projects where a bot auto-closes
/// PRs whose linked issue lacks a specific label (cli/cli is the
/// canonical instance, with `github-actions[bot]` enforcing a 4-day
/// timer on missing `help wanted`).
pub fn contributing_looks_ok(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return true;
    };
    let lower = body.to_ascii_lowercase();
    const CLA_MARKERS: &[&str] = &[
        "contributor license agreement",
        "contributor licence agreement",
        "cla-assistant",
        "cla assistant",
        "easycla",
        "eclipse contributor agreement",
        "individual contributor license",
        "sign a cla",
        "sign our cla",
        "sign the cla",
        "accept our cla",
    ];
    const GATE_MARKERS: &[&str] = &[
        "please open an issue first",
        "please open an issue before",
        "please discuss first",
        "please discuss before",
        "please contact us first",
        "please contact us before",
        "please email us first",
        "please email us before",
        "please reach out first",
        "please reach out before",
    ];
    // AI-ban markers: phrases that categorically reject AI-implemented
    // or autonomous-agent contributions. Patterns are intentionally
    // long enough to avoid catching friendly disclosure prose ("if you
    // used AI tools, please disclose"). Compare against the lowercased
    // body since CONTRIBUTING files use varied casing.
    const AI_BAN_MARKERS: &[&str] = &[
        "implemented by an ai",
        "implemented by ai",
        "generated by an ai",
        "generated by ai",
        "written by an ai",
        "written by ai",
        "do not vibecode",
        "do not allow autonomous agents",
        "no autonomous agents",
        "no ai-generated",
        "no ai generated",
        "ai-generated content will not",
        "ai generated content will not",
        "ai-generated contributions will not",
        "ai generated contributions will not",
        "will close any pull requests that we believe were created autonomously",
    ];
    // Label-gate markers: phrases declaring that PRs are only accepted
    // for issues carrying a specific label, paired with an enforcement
    // bot that auto-closes non-conforming PRs. Patterns lean on the
    // restrictive shape ("for issues labelled X", "without the X
    // label", "external pull requests will not be accepted") because
    // friendly recommendations ("look for issues labelled `good first
    // issue` if you want a starting point") don't carry the gate-shape.
    const LABEL_GATE_MARKERS: &[&str] = &[
        "we accept pull requests for issues labelled",
        "we accept pull requests for issues labeled",
        "we only accept pull requests for issues labelled",
        "we only accept pull requests for issues labeled",
        "pull request for issues without the",
        "pull requests for issues without the",
        "open a pull request for any issue without",
        "open pull requests for any issue without",
        "external pull requests will not be accepted",
    ];
    !CLA_MARKERS
        .iter()
        .chain(GATE_MARKERS.iter())
        .chain(AI_BAN_MARKERS.iter())
        .chain(LABEL_GATE_MARKERS.iter())
        .any(|m| lower.contains(m))
}

/// Any comment in the slice was authored by someone the repo treats
/// as a maintainer. Drives the `maintainer_touched` heuristic: if a
/// maintainer has already engaged with the issue, the issue is more
/// likely to land a PR than one that has only seen drive-by reporters.
///
/// "Maintainer" here is the GitHub `author_association` set
/// `OWNER` ∪ `MEMBER` ∪ `COLLABORATOR`. `CONTRIBUTOR` is excluded
/// deliberately: anyone with one merged PR carries that association
/// for the rest of time, so it overcounts. The other documented
/// values (`FIRST_TIMER`, `FIRST_TIME_CONTRIBUTOR`, `MANNEQUIN`,
/// `NONE`) are clearly not maintainer touches.
///
/// Empty slice returns `false`. Unknown association strings (GitHub
/// adds new ones occasionally) also return `false` rather than
/// matching loosely.
pub fn maintainer_in_comments(comments: &[CommentMeta]) -> bool {
    comments.iter().any(is_maintainer_comment)
}

/// Whether a single comment counts as a maintainer touch. Public via
/// `maintainer_in_comments`; the per-comment predicate stays private
/// because callers only need the slice-level answer.
fn is_maintainer_comment(c: &CommentMeta) -> bool {
    matches!(
        c.author_association.as_str(),
        "OWNER" | "MEMBER" | "COLLABORATOR"
    )
}

/// Any maintainer comment on the issue carries a "discuss-first /
/// needs-proposal / not-yet-decided" signal, meaning the issue is not
/// PR-ready yet. Drives the optional `drop_if_pending_discussion`
/// planner filter: when this returns `true`, opening a PR against
/// the issue would invite a "this should be an issue / proposal /
/// RFC first" close from the maintainer.
///
/// Only matches maintainer-authored comments (`OWNER` ∪ `MEMBER` ∪
/// `COLLABORATOR`); drive-by suggestions from random commenters don't
/// move the needle on whether the project wants a PR. Patterns are
/// intentionally high-specificity: `"let's discuss"` alone is too
/// broad (maintainers commonly say it on PRs that get merged), but
/// `"let's discuss this first"` or `"we need to decide"` carries the
/// gate-shape. Compares against the lowercased body so casing varies
/// don't matter; comments with no body (deleted, GitHub omission)
/// don't match.
///
/// Caveat: this fires on the FIRST maintainer comment that matches;
/// a later resolution comment like "OK, this is settled, please PR"
/// won't override it. Mitigation is on the config side
/// (`drop_if_pending_discussion = false` to bypass) rather than
/// inside this predicate, which keeps the pattern surface narrow.
pub fn pending_discussion_in_maintainer_comments(comments: &[CommentMeta]) -> bool {
    comments
        .iter()
        .filter(|c| is_maintainer_comment(c))
        .any(|c| {
            c.body
                .as_deref()
                .map(comment_signals_pending_discussion)
                .unwrap_or(false)
        })
}

/// Whether a single comment body carries a pending-discussion signal.
/// Public via `pending_discussion_in_maintainer_comments`; the
/// per-body predicate stays private because callers only need the
/// slice-level answer.
fn comment_signals_pending_discussion(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    const PENDING_DISCUSSION_MARKERS: &[&str] = &[
        "this should be an issue",
        "this should be a proposal",
        "this should be an rfc",
        "let's discuss this first",
        "let's discuss this before",
        "let us discuss this first",
        "needs an rfc",
        "needs a proposal first",
        "needs design discussion",
        "needs a design discussion",
        "open a proposal first",
        "open a proposal before",
        "before opening a pr",
        "before opening a pull request",
        "before implementing",
        "we need to decide",
        "we haven't decided",
        "we have not decided",
        "we haven't figured out",
        "we have not figured out",
        "haven't decided yet",
        "have not decided yet",
        "i'd want a proposal",
        "i would want a proposal",
        "i'd like a proposal",
        "i would like a proposal",
        "i'd want an rfc",
        "i would want an rfc",
    ];
    PENDING_DISCUSSION_MARKERS.iter().any(|m| lower.contains(m))
}

/// Whether any timeline event cross-references an open pull request.
/// Drives the `no_crosslinked_pr` factor: when this returns `true`,
/// someone else's PR already touches the issue and scout should
/// down-rank the candidate. The factor on the score side is the
/// negation so the field name reads as "good when true".
///
/// A cross-reference event surfaces as `event == "cross-referenced"`.
/// The source can be either an issue or a PR; only PR sources
/// (the source's `pull_request` field is present) count for this
/// signal. Closed-PR cross-references don't matter because a closed
/// PR doesn't block someone else from picking the issue up.
///
/// Empty slice returns `false`. Non-cross-reference events return
/// `false` regardless of their other fields.
pub fn crosslinked_open_pr_in_timeline(events: &[TimelineEvent]) -> bool {
    events.iter().any(is_open_pr_cross_reference)
}

/// Whether a single timeline event is a cross-reference from an open
/// PR. Public via `crosslinked_open_pr_in_timeline`; the per-event
/// predicate stays private because callers only need the slice-level
/// answer.
fn is_open_pr_cross_reference(e: &TimelineEvent) -> bool {
    if e.event != "cross-referenced" {
        return false;
    }
    let Some(source) = &e.source else {
        return false;
    };
    let Some(issue) = &source.issue else {
        return false;
    };
    issue.pull_request.is_some() && issue.state == "open"
}

/// Whether any timeline event records an assignment to GitHub's
/// Copilot agent. Drives the optional `drop_if_copilot_assigned`
/// planner filter: when a maintainer assigns an issue to Copilot,
/// the auto-PR opens within minutes-to-days, and humans should not
/// race that PR. `drop_if_open_pr` catches the same outcome once
/// the cross-reference event fires, but there is a real window
/// (observed at vitest#10307: ~2 days between assignment and
/// cross-reference) where only the assignment event is visible.
///
/// Matches assignee login `"Copilot"` (the display login GitHub uses
/// in the API for the swe-agent) and the alias `"copilot-swe-agent"`,
/// case-insensitive. Empty slice returns `false`. Non-assigned events
/// return `false` regardless of their other fields.
pub fn assigned_to_copilot_in_timeline(events: &[TimelineEvent]) -> bool {
    events.iter().any(is_copilot_assignment)
}

/// Whether a single timeline event is an assignment to the Copilot
/// agent. Public via `assigned_to_copilot_in_timeline`; the
/// per-event predicate stays private.
fn is_copilot_assignment(e: &TimelineEvent) -> bool {
    if e.event != "assigned" {
        return false;
    }
    let Some(assignee) = &e.assignee else {
        return false;
    };
    let login = assignee.login.to_lowercase();
    login == "copilot" || login == "copilot-swe-agent"
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
pub(crate) fn parse_iso8601_z(s: &str) -> Option<i64> {
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
