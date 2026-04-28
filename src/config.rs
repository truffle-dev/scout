//! TOML-backed user config. Shape follows the arch doc: `[auth]`,
//! `[weights]`, `[filters]`, `[output]`. Every field has a default so a
//! freshly-parsed config from an empty string still matches the reference
//! weighting. `scout init` writes a fully-populated copy; `scout --config`
//! accepts a partial file and fills the gaps from `Default`.
//!
//! This module owns only the parse + convert layer. No IO. Callers read
//! the file (or compose one in tests) and hand the string to `parse`.

use serde::Deserialize;

use crate::score::Weights;

/// Parsed user config. Each section has a `Default` impl, so a missing
/// `[auth]` or `[output]` block parses as the reference shape.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub auth: Auth,
    pub weights: WeightsConfig,
    pub filters: Filters,
    pub output: Output,
}

/// Authentication config. `token_path` is a filesystem path (tilde allowed,
/// expansion happens in the fetch layer). Unset means fall back to
/// `$GITHUB_TOKEN`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Auth {
    pub token_path: Option<String>,
}

/// Weight block. Mirrors `score::Weights` one-for-one, but owned here so
/// serde can hydrate it with partial TOML input. Convertible into the
/// scoring-layer type via `From`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeightsConfig {
    pub root_cause: f64,
    pub no_pr: f64,
    pub recent: f64,
    pub contributing_ok: f64,
    pub reproducer: f64,
    pub effort_ok: f64,
    pub maintainer_touched: f64,
    pub active_repo: f64,
}

impl Default for WeightsConfig {
    fn default() -> Self {
        let w = Weights::default();
        Self {
            root_cause: w.root_cause,
            no_pr: w.no_pr,
            recent: w.recent,
            contributing_ok: w.contributing_ok,
            reproducer: w.reproducer,
            effort_ok: w.effort_ok,
            maintainer_touched: w.maintainer_touched,
            active_repo: w.active_repo,
        }
    }
}

impl From<WeightsConfig> for Weights {
    fn from(c: WeightsConfig) -> Self {
        Self {
            root_cause: c.root_cause,
            no_pr: c.no_pr,
            recent: c.recent,
            contributing_ok: c.contributing_ok,
            reproducer: c.reproducer,
            effort_ok: c.effort_ok,
            maintainer_touched: c.maintainer_touched,
            active_repo: c.active_repo,
        }
    }
}

/// Filter block. Controls which issues are considered at all.
/// `exclude_labels` is case-sensitive; GitHub label strings are already
/// normalized by the fetch layer before comparison. `cooldown_days`
/// pairs with the JSONL ledger `scout took` writes: an issue taken
/// less than that many days ago is filtered out so a user does not
/// re-pick the same issue during a contribution attempt. `0` disables
/// the filter (every issue is always available).
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Filters {
    pub max_age_days: u32,
    pub min_score: f64,
    pub cooldown_days: u32,
    pub exclude_labels: Vec<String>,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            min_score: 0.50,
            cooldown_days: 14,
            exclude_labels: vec![
                "wontfix".to_string(),
                "invalid".to_string(),
                "duplicate".to_string(),
            ],
        }
    }
}

/// Output block. `color` is free-form for now; the CLI layer validates it
/// against `auto`, `always`, `never`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Output {
    pub color: String,
    pub limit: u32,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            color: "auto".to_string(),
            limit: 20,
        }
    }
}

/// Parse a TOML string into a `Config`. Missing sections fall through to
/// their `Default` impls; unknown keys fail the parse so typos surface
/// instead of silently turning into default values.
pub fn parse(s: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(s)
}
