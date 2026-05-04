//! Filesystem-touching cases on `scout::dropped`. The module is a thin
//! shim over `scout::took::append_entry_with_event`; the asserts here
//! pin only the parts that differ from the `took` writer:
//!
//! - The serialized line carries `"event":"dropped"`.
//! - `EVENT` is the exact tag the writer emits.
//! - A `dropped` line and a `took` line for the same issue coexist
//!   in the file with no truncation, in append order.
//!
//! Path resolution and parsing are exercised by `tests/took.rs`; the
//! shim reuses the same helpers, so re-testing them here would be
//! duplicate coverage that future renames would have to chase twice.

use scout::dropped::EVENT;
use scout::took::{IssueRef, append_entry, append_entry_with_event};

/// Calling `append_entry_with_event` with `EVENT` ("dropped") writes a
/// line whose `event` field is `"dropped"` and whose other fields
/// match the took writer's shape.
#[test]
fn dropped_line_carries_dropped_event_tag() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.jsonl");
    let issue = IssueRef {
        owner: "truffle-dev".into(),
        repo: "scout".into(),
        number: 7,
    };

    append_entry_with_event(&ledger, &issue, "2026-05-04T03:00:00Z", EVENT).unwrap();

    let body = std::fs::read_to_string(&ledger).unwrap();
    assert_eq!(
        body,
        "{\"repo\":\"truffle-dev/scout\",\"number\":7,\"timestamp\":\"2026-05-04T03:00:00Z\",\"event\":\"dropped\"}\n",
    );
}

/// `EVENT` is the exact string the writer emits. A future rename here
/// would silently desync the ledger; pin it.
#[test]
fn dropped_event_constant_is_dropped() {
    assert_eq!(EVENT, "dropped");
}

/// A dropped line and a took line for the same issue both land in the
/// file in append order. The cooldown filter doesn't distinguish, but
/// the file does, so a future reader can replay the engagement story.
#[test]
fn took_and_dropped_lines_coexist_in_append_order() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = dir.path().join("ledger.jsonl");
    let issue = IssueRef {
        owner: "starship".into(),
        repo: "starship".into(),
        number: 7433,
    };

    append_entry_with_event(&ledger, &issue, "2026-05-03T10:00:00Z", EVENT).unwrap();
    append_entry(&ledger, &issue, "2026-05-04T11:00:00Z").unwrap();

    let body = std::fs::read_to_string(&ledger).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].contains("\"event\":\"dropped\""),
        "first line should be the drop, was {:?}",
        lines[0],
    );
    assert!(
        lines[1].contains("\"event\":\"took\""),
        "second line should be the take, was {:?}",
        lines[1],
    );
}
