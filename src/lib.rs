//! scout ranks open-source issues by how likely they are to be worth
//! contributing to. The ranking is a weighted sum of eight heuristics;
//! weights live in the user's TOML config so the scoring is auditable
//! and tunable per-user.

pub mod config;
pub mod fetch;
pub mod infer;
pub mod init;
pub mod score;
pub mod took;
pub mod watchlist;

pub use config::{Auth, Config, Filters, Output, WeightsConfig, parse as parse_config};
pub use fetch::{
    CONTRIBUTING_PATHS, DEFAULT_PAGE_CAP, FetchError, IssueMeta, Label, PullRequestRef, RepoMeta,
    UserRef, contributing_md, contributing_md_at, decode_issue_list, decode_repo_meta, list_issues,
    list_issues_at, list_issues_paginated, list_issues_paginated_at, parse_next_link, repo_meta,
    repo_meta_at,
};
pub use infer::{
    contributing_looks_ok, days_since, has_effort_label, has_non_effort_label, has_reproducer,
    has_root_cause,
};
pub use init::{
    InitError, InitSummary, WriteOutcome, default_config_path, default_watchlist_path,
    write_starter_files,
};
pub use score::{Breakdown, Factors, Weights, factors_from, score};
pub use took::{
    IssueRef, ParseError, TookError, append_entry, default_ledger_path, format_iso8601_z,
    now_iso8601_z, parse_issue_ref,
};
pub use watchlist::{WatchEntry, Watchlist, WatchlistError, parse as parse_watchlist};
