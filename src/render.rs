//! Rendering layer for `scout scan` output. Two modes: a markdown
//! table for the default human-readable terminal output, and a JSON
//! shape for `--json` so callers can pipe into jq or anything else.
//!
//! Pure: no IO, no async. Takes `&[RankedRow]` and a `limit`, returns
//! a `String`. The orchestrator above sorts and decides which mode
//! based on the CLI flag; the renderer preserves input order and
//! truncates from the top when `limit` is set.

use crate::rank::RankedRow;

/// Render a slice of ranked rows as a GFM markdown table with three
/// columns: `score | issue | title`. The `issue` cell is a markdown
/// link to the GitHub browser URL labeled `OWNER/REPO#N`. Pipes and
/// newlines in titles are sanitized so a single row never spans
/// multiple table lines. `limit` truncates from the top; `None` keeps
/// all rows.
pub fn table_markdown(rows: &[RankedRow], limit: Option<usize>) -> String {
    let mut out = String::new();
    out.push_str("| score | issue | title |\n");
    out.push_str("| ----: | :---- | :---- |\n");
    let take = limit.map(|n| n.min(rows.len())).unwrap_or(rows.len());
    for row in rows.iter().take(take) {
        let issue_link = format!("[{}#{}]({})", row.full_name, row.number, row.html_url);
        let title = sanitize_cell(&row.title);
        out.push_str(&format!(
            "| {:.2} | {} | {} |\n",
            row.breakdown.total, issue_link, title,
        ));
    }
    out
}

/// Render a slice of ranked rows as a JSON array. Each element has
/// `full_name`, `number`, `title`, `html_url`, `score` (the clamped
/// total), and `parts` (the per-heuristic breakdown as
/// `[name, contribution]` pairs). Output is a single line; callers
/// that want pretty-printing pipe through `jq`. `limit` truncates
/// from the top.
pub fn json(rows: &[RankedRow], limit: Option<usize>) -> Result<String, serde_json::Error> {
    let take = limit.map(|n| n.min(rows.len())).unwrap_or(rows.len());
    let view: Vec<JsonRow<'_>> = rows.iter().take(take).map(JsonRow::from).collect();
    serde_json::to_string(&view)
}

#[derive(serde::Serialize)]
struct JsonRow<'a> {
    full_name: &'a str,
    number: u64,
    title: &'a str,
    html_url: &'a str,
    score: f64,
    parts: &'a [(&'static str, f64)],
}

impl<'a> From<&'a RankedRow> for JsonRow<'a> {
    fn from(row: &'a RankedRow) -> Self {
        Self {
            full_name: &row.full_name,
            number: row.number,
            title: &row.title,
            html_url: &row.html_url,
            score: row.breakdown.total,
            parts: &row.breakdown.parts,
        }
    }
}

/// Replace pipe and newline characters with safe substitutes so a
/// single title cell never breaks a markdown row. Pipes become
/// `\|` (still rendered as a literal pipe by GFM); CR and LF
/// collapse to a single space.
fn sanitize_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\r' | '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}
