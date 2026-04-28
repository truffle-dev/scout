//! Integration tests for the `render` module: markdown table and JSON
//! shape against synthetic `RankedRow` slices.

use scout::rank::RankedRow;
use scout::render::{json, table_markdown};
use scout::score::Breakdown;

fn ranked(full_name: &str, number: u64, title: &str, total: f64) -> RankedRow {
    RankedRow {
        full_name: full_name.to_string(),
        number,
        title: title.to_string(),
        html_url: format!("https://github.com/{full_name}/issues/{number}"),
        breakdown: Breakdown {
            total,
            parts: vec![("root_cause", 0.30), ("no_pr", 0.20)],
        },
    }
}

#[test]
fn table_markdown_emits_only_header_for_empty_rows() {
    let out = table_markdown(&[], None);
    assert_eq!(
        out,
        "\
| score | issue | title |
| ----: | :---- | :---- |
"
    );
}

#[test]
fn table_markdown_renders_single_row() {
    let rows = vec![ranked("foo/bar", 42, "Fix the thing", 0.85)];
    let out = table_markdown(&rows, None);
    assert!(
        out.contains(
            "| 0.85 | [foo/bar#42](https://github.com/foo/bar/issues/42) | Fix the thing |"
        )
    );
}

#[test]
fn table_markdown_preserves_input_order() {
    let rows = vec![
        ranked("a/a", 1, "first", 0.30),
        ranked("b/b", 2, "second", 0.90),
    ];
    let out = table_markdown(&rows, None);
    let first_pos = out.find("first").unwrap();
    let second_pos = out.find("second").unwrap();
    assert!(first_pos < second_pos);
}

#[test]
fn table_markdown_escapes_pipe_in_title() {
    let rows = vec![ranked("foo/bar", 1, "a | b", 0.50)];
    let out = table_markdown(&rows, None);
    assert!(out.contains("a \\| b"));
}

#[test]
fn table_markdown_collapses_newlines_in_title() {
    let rows = vec![ranked("foo/bar", 1, "line1\nline2\rline3", 0.50)];
    let out = table_markdown(&rows, None);
    assert!(out.contains("line1 line2 line3"));
    assert_eq!(out.lines().count(), 3, "row must be a single table line");
}

#[test]
fn table_markdown_truncates_at_limit() {
    let rows = vec![
        ranked("a/a", 1, "first", 0.90),
        ranked("b/b", 2, "second", 0.80),
        ranked("c/c", 3, "third", 0.70),
    ];
    let out = table_markdown(&rows, Some(2));
    assert!(out.contains("first"));
    assert!(out.contains("second"));
    assert!(!out.contains("third"));
}

#[test]
fn table_markdown_limit_larger_than_rows_returns_all() {
    let rows = vec![ranked("a/a", 1, "only", 0.50)];
    let out = table_markdown(&rows, Some(99));
    assert!(out.contains("only"));
}

#[test]
fn table_markdown_limit_zero_emits_only_header() {
    let rows = vec![ranked("a/a", 1, "skipped", 0.50)];
    let out = table_markdown(&rows, Some(0));
    assert!(!out.contains("skipped"));
    assert_eq!(out.lines().count(), 2);
}

#[test]
fn json_emits_empty_array_for_empty_rows() {
    let out = json(&[], None).expect("json must serialize");
    assert_eq!(out, "[]");
}

#[test]
fn json_round_trips_score_and_parts() {
    let rows = vec![ranked("foo/bar", 42, "Fix", 0.85)];
    let out = json(&rows, None).expect("json must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = &parsed[0];
    assert_eq!(row["full_name"], "foo/bar");
    assert_eq!(row["number"], 42);
    assert_eq!(row["title"], "Fix");
    assert_eq!(row["html_url"], "https://github.com/foo/bar/issues/42");
    assert_eq!(row["score"], 0.85);
    assert_eq!(row["parts"][0][0], "root_cause");
    assert_eq!(row["parts"][0][1], 0.30);
    assert_eq!(row["parts"][1][0], "no_pr");
    assert_eq!(row["parts"][1][1], 0.20);
}

#[test]
fn json_truncates_at_limit() {
    let rows = vec![
        ranked("a/a", 1, "first", 0.90),
        ranked("b/b", 2, "second", 0.80),
        ranked("c/c", 3, "third", 0.70),
    ];
    let out = json(&rows, Some(2)).expect("json must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["title"], "first");
    assert_eq!(arr[1]["title"], "second");
}

#[test]
fn json_does_not_escape_special_chars_in_title_beyond_json_default() {
    let rows = vec![ranked("foo/bar", 1, "a | b\nc", 0.50)];
    let out = json(&rows, None).expect("json must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed[0]["title"], "a | b\nc");
}
