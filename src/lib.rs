//! scout ranks open-source issues by how likely they are to be worth
//! contributing to. The ranking is a weighted sum of eight heuristics;
//! weights live in the user's TOML config so the scoring is auditable
//! and tunable per-user.

pub mod config;
pub mod fetch;
pub mod fetcher;
pub mod infer;
pub mod init;
pub mod rank;
pub mod render;
pub mod scan;
pub mod score;
pub mod took;
pub mod watchlist;

pub use config::{Auth, Config, Filters, Output, WeightsConfig, parse as parse_config};
pub use fetch::{
    CONTRIBUTING_PATHS, CommentMeta, DEFAULT_PAGE_CAP, FetchError, IssueMeta, Label,
    PullRequestRef, RepoMeta, TimelineEvent, TimelineSource, TimelineSourceIssue, UserRef,
    contributing_md, contributing_md_at, decode_comment_list, decode_issue_list, decode_repo_meta,
    decode_timeline_list, list_issue_comments, list_issue_comments_at, list_issue_timeline,
    list_issue_timeline_at, list_issues, list_issues_at, list_issues_paginated,
    list_issues_paginated_at, parse_next_link, repo_meta, repo_meta_at,
};
pub use fetcher::{fetch_repos, fetch_repos_at};
pub use infer::{
    contributing_looks_ok, crosslinked_open_pr_in_timeline, days_since, has_effort_label,
    has_non_effort_label, has_reproducer, has_root_cause, maintainer_in_comments,
};
pub use init::{
    InitError, InitSummary, WriteOutcome, default_config_path, default_watchlist_path,
    write_starter_files,
};
pub use rank::{RankInput, RankedRow, rank};
pub use render::{json as render_json, table_markdown};
pub use scan::{
    FetchedIssue, FetchedRepo, LedgerError, LedgerIndex, ScanError, load_config, load_ledger,
    load_watchlist, plan,
};
pub use score::{Breakdown, Factors, Weights, factors_from, score};
pub use took::{
    IssueRef, ParseError, TookError, append_entry, default_ledger_path, format_iso8601_z,
    now_iso8601_z, parse_issue_ref,
};
pub use watchlist::{WatchEntry, Watchlist, WatchlistError, parse as parse_watchlist};
