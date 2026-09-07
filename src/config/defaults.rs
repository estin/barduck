//! Built-in defaults and `BARDUCK_*` environment variable overrides.

use super::{Config, TuiWidth};
use anyhow::{Context as _, Result};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) fn default_db_path() -> PathBuf {
    PathBuf::from("dashboard.duckdb")
}
pub(crate) fn default_listen() -> String {
    "127.0.0.1:8420".into()
}
pub(crate) fn default_interval() -> Duration {
    Duration::from_mins(5)
}
pub(crate) fn default_timeout() -> Duration {
    Duration::from_secs(30)
}
pub(crate) fn default_retry_interval() -> Duration {
    Duration::from_secs(30)
}
pub(crate) fn default_threshold() -> u32 {
    3
}
pub(crate) fn default_stale() -> Duration {
    Duration::from_mins(30)
}
pub(crate) fn default_history_points() -> u32 {
    30
}
pub(crate) fn default_tui_width() -> TuiWidth {
    TuiWidth::Named("auto".into())
}
pub(crate) fn default_config_dir() -> PathBuf {
    PathBuf::from(".")
}

/// Overrides top-level scalar settings from `BARDUCK_<FIELD>` environment
/// variables, taking precedence over both the config file's value and the
/// field's built-in default (spec: source-configuration — environment
/// variables override top-level settings). Structural fields (`sources`,
/// `layouts`) are not covered.
pub(crate) fn apply_env_overrides(cfg: &mut Config) -> Result<()> {
    apply_env_overrides_from(cfg, |name| std::env::var(name).ok())
}

/// Same as [`apply_env_overrides`], but reads variables through `lookup`
/// instead of the real process environment — lets tests exercise the
/// override logic without mutating global process state.
pub(crate) fn apply_env_overrides_from(cfg: &mut Config, lookup: impl Fn(&str) -> Option<String>) -> Result<()> {
    if let Some(v) = lookup("BARDUCK_DATABASE_PATH") {
        cfg.database_path = PathBuf::from(v);
    }
    if let Some(v) = lookup("BARDUCK_LISTEN") {
        cfg.listen = v;
    }
    if let Some(v) = lookup("BARDUCK_INTERVAL") {
        cfg.interval = parse_env_duration("BARDUCK_INTERVAL", &v)?;
    }
    if let Some(v) = lookup("BARDUCK_FAILURE_THRESHOLD") {
        cfg.failure_threshold = parse_env_u32("BARDUCK_FAILURE_THRESHOLD", &v)?;
    }
    if let Some(v) = lookup("BARDUCK_STALE_AFTER") {
        cfg.stale_after = parse_env_duration("BARDUCK_STALE_AFTER", &v)?;
    }
    if let Some(v) = lookup("BARDUCK_HISTORY_POINTS") {
        cfg.history_points = parse_env_u32("BARDUCK_HISTORY_POINTS", &v)?;
    }
    if let Some(v) = lookup("BARDUCK_TUI_WIDTH") {
        cfg.tui_width = parse_env_tui_width(&v)?;
    }
    Ok(())
}

fn parse_env_duration(var: &str, v: &str) -> Result<Duration> {
    humantime::parse_duration(v)
        .with_context(|| format!("environment variable `{var}` value `{v}` is invalid"))
}

fn parse_env_u32(var: &str, v: &str) -> Result<u32> {
    v.trim()
        .parse()
        .with_context(|| format!("environment variable `{var}` value `{v}` is invalid"))
}

fn parse_env_tui_width(v: &str) -> Result<TuiWidth> {
    if v == "auto" {
        return Ok(TuiWidth::Named("auto".into()));
    }
    v.trim()
        .parse()
        .map(TuiWidth::Fixed)
        .with_context(|| format!("environment variable `BARDUCK_TUI_WIDTH` value `{v}` is invalid"))
}
