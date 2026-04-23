//! scout ranks open-source issues by how likely they are to be worth
//! contributing to. The ranking is a weighted sum of eight heuristics;
//! weights live in the user's TOML config so the scoring is auditable
//! and tunable per-user.

pub mod config;
pub mod fetch;
pub mod infer;
pub mod score;

pub use config::{Auth, Config, Filters, Output, WeightsConfig, parse as parse_config};
pub use fetch::{
    FetchError, IssueMeta, Label, PullRequestRef, RepoMeta, UserRef, decode_issue_list,
    decode_repo_meta, list_issues, list_issues_at, repo_meta, repo_meta_at,
};
pub use infer::{has_effort_label, has_reproducer, has_root_cause};
pub use score::{Breakdown, Factors, Weights, score};
