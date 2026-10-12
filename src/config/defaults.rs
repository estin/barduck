//! Built-in defaults for every `Config` field.
use super::TuiWidth;
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
pub(crate) fn default_history_points() -> u32 {
    30
}
pub(crate) fn default_logs_per_page() -> u32 {
    50
}
pub(crate) fn default_user_js() -> Vec<PathBuf> {
    Vec::new()
}
pub(crate) fn default_web_refresh_interval() -> Duration {
    Duration::from_secs(5)
}
pub(crate) fn default_web_content_width() -> String {
    "narrow".into()
}
pub(crate) fn default_tui_width() -> TuiWidth {
    TuiWidth::Named("auto".into())
}
pub(crate) fn default_config_dir() -> PathBuf {
    PathBuf::from(".")
}
