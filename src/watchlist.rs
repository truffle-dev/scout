//! Watchlist parser. The watchlist is a YAML-shaped list of GitHub
//! repositories scout scans for issues. The format is intentionally
//! narrow:
//!
//! ```yaml
//! repos:
//!   - owner1/repo1
//!   - owner2/repo2
//! ```
//!
//! Plus `# comment` lines and blank lines, both ignored. Inline
//! comments (`- owner/repo  # main repo`) are also stripped.
//!
//! We hand-roll the parser instead of pulling a YAML crate. Adding
//! `serde_yaml` (deprecated) or `serde_yml` (~50 transitive deps) for
//! a list of strings is the wrong shape for a tool that prides itself
//! on a tight build. The format below is the entire surface; if it
//! grows, we revisit.
//!
//! Strict-by-default: unknown top-level keys, missing leading `-`,
//! whitespace inside an entry, and duplicates are all errors. Empty
//! files and comments-only files parse as an empty watchlist.

/// A parsed `OWNER/REPO` entry from the watchlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEntry {
    pub owner: String,
    pub repo: String,
}

/// A fully-parsed watchlist. Order matches the source order; duplicates
/// are rejected at parse time so this vector is unique.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watchlist {
    pub repos: Vec<WatchEntry>,
}

/// Errors surfaced by the watchlist parser. Each variant carries the
/// 1-indexed line number where the problem was detected so the user
/// can jump straight to the offending line.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WatchlistError {
    #[error("line {line}: unexpected top-level content {content:?}; expected `repos:`")]
    UnexpectedTopLevel { line: usize, content: String },
    #[error("line {line}: expected list entry starting with `-`, got {content:?}")]
    ExpectedDash { line: usize, content: String },
    #[error("line {line}: malformed entry {entry:?}; expected `owner/repo`")]
    MalformedEntry { line: usize, entry: String },
    #[error("line {line}: owner or repo segment is empty in {entry:?}")]
    EmptySegment { line: usize, entry: String },
    #[error("line {line}: whitespace not allowed inside entry {entry:?}")]
    InvalidChar { line: usize, entry: String },
    #[error("line {line}: duplicate entry {slug:?}")]
    Duplicate { line: usize, slug: String },
}

/// Parse a watchlist string into a `Watchlist`. Empty input and
/// comments-only input both produce an empty watchlist.
pub fn parse(s: &str) -> Result<Watchlist, WatchlistError> {
    let mut repos: Vec<WatchEntry> = Vec::new();
    let mut state = State::BeforeRepos;

    for (idx, raw_line) in s.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = strip_comment(raw_line).trim();
        if trimmed.is_empty() {
            continue;
        }
        match state {
            State::BeforeRepos => {
                if trimmed == "repos:" {
                    state = State::AfterRepos;
                } else {
                    return Err(WatchlistError::UnexpectedTopLevel {
                        line: line_no,
                        content: trimmed.to_string(),
                    });
                }
            }
            State::AfterRepos => {
                let entry = parse_entry(trimmed, line_no)?;
                if repos.iter().any(|e| e == &entry) {
                    return Err(WatchlistError::Duplicate {
                        line: line_no,
                        slug: format!("{}/{}", entry.owner, entry.repo),
                    });
                }
                repos.push(entry);
            }
        }
    }

    Ok(Watchlist { repos })
}

enum State {
    BeforeRepos,
    AfterRepos,
}

/// Strip an inline `# ...` comment. Anything from the first `#` to the
/// end of the line is dropped.
fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

/// Parse a single `- owner/repo` entry. The line is already trimmed
/// and known non-empty. The leading `-` is required; the body must be
/// exactly `owner/repo` with no whitespace and one slash.
fn parse_entry(trimmed: &str, line: usize) -> Result<WatchEntry, WatchlistError> {
    let body = trimmed
        .strip_prefix('-')
        .ok_or_else(|| WatchlistError::ExpectedDash {
            line,
            content: trimmed.to_string(),
        })?
        .trim();

    if body.is_empty() {
        return Err(WatchlistError::MalformedEntry {
            line,
            entry: body.to_string(),
        });
    }
    if body.chars().any(char::is_whitespace) {
        return Err(WatchlistError::InvalidChar {
            line,
            entry: body.to_string(),
        });
    }

    let parts: Vec<&str> = body.split('/').collect();
    if parts.len() != 2 {
        return Err(WatchlistError::MalformedEntry {
            line,
            entry: body.to_string(),
        });
    }
    let (owner, repo) = (parts[0], parts[1]);
    if owner.is_empty() || repo.is_empty() {
        return Err(WatchlistError::EmptySegment {
            line,
            entry: body.to_string(),
        });
    }

    Ok(WatchEntry {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}
