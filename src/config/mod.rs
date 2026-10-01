//! Config file schema, loading, and validation.
//!
//! Split by concern: [`source`] (source declarations + threshold/health
//! color model), [`layout`] (grid cells and value-rendering format),
//! [`defaults`] (built-in defaults + `BARDUCK_*` env overrides), and
//! [`validation`] (the actual field-by-field checks `validate` runs).
//! Everything is re-exported here so callers keep using `config::Whatever`
//! regardless of which submodule it actually lives in.

mod defaults;
mod layout;
mod source;
mod validation;

pub use layout::{Cell, LayoutCfg, TuiWidth, VALUE_FORMATS, ValueFormat};
pub use source::{
    ChildDecl, GroupItem, JsonlRow, JsonlTs, Level, SourceCfg, SourceType, Threshold, ValueType,
    View, accent_color, composite_children, expand_composites, level_for, parse_jsonl_row,
    source_visible_in, status_color, validate_thresholds, visible_items, worst_color,
};

use defaults::{
    apply_env_overrides_from, default_config_dir, default_db_path, default_history_points,
    default_interval, default_listen, default_logs_per_page, default_threshold, default_tui_width,
    default_user_js, default_web_content_width, default_web_refresh_interval,
};
use validation::{validate_cell, validate_source};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// App version shown in web UI, TUI, and CLI output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_db_path")]
    pub database_path: PathBuf,
    #[serde(default = "default_listen")]
    pub listen: String,
    /// Humantime string (e.g. `"5m"`); currently unused — see `SourceCfg::interval`.
    #[serde(default = "default_interval", with = "humantime_serde")]
    pub interval: Duration,
    #[serde(default = "default_threshold")]
    pub failure_threshold: u32,
    /// Default number of recent readings shown in a banded source's web UI
    /// history bar; overridable per source via `SourceCfg::history_points`.
    #[serde(default = "default_history_points")]
    pub history_points: u32,
    /// Default number of fetch-log entries shown per page in the web UI log
    /// view (`/logs/<source>`); overridable via `BARDUCK_LOGS_PER_PAGE`.
    #[serde(default = "default_logs_per_page")]
    pub logs_per_page: u32,
    /// Fallback web UI refresh interval (humantime string, e.g. `"5s"`):
    /// how often the browser re-renders panels when no refresh event
    /// arrived over the SSE stream (spec: web-ui — immediate panel
    /// refresh). Overridable via `BARDUCK_WEB_REFRESH_INTERVAL`.
    #[serde(default = "default_web_refresh_interval", with = "humantime_serde")]
    pub web_refresh_interval: Duration,
    /// Default web UI content width: `"narrow"` (centered capped column)
    /// or `"wide"` (panels span the full viewport). A per-browser cookie
    /// override wins over this default (spec: web-ui — per-browser width
    /// override).
    #[serde(default = "default_web_content_width")]
    pub web_content_width: String,
    /// How long to keep collected data before the daemon prunes it: a
    /// humantime string (e.g. `"30d"`). Unset (the default) keeps everything
    /// forever, matching prior behavior (spec: data-storage — retention).
    #[serde(default, with = "humantime_serde::option")]
    pub retention: Option<Duration>,
    /// User JavaScript files injected at the end of the web UI `<body>`,
    /// loaded in resolution order (spec: source-configuration — user-defined
    /// web UI scripts configuration). Each entry is a `.js` file or a
    /// directory of `.js` files. Like `sources`/`layouts`, never overridden
    /// from the environment. Resolution (relative-to-`config_dir` join,
    /// directory expansion, dedupe) happens in [`resolve_user_js`], called
    /// by [`load_from`] after `config_dir` is set.
    #[serde(default = "default_user_js")]
    pub web_user_js: Vec<PathBuf>,
    #[serde(default)]
    pub sources: Vec<SourceCfg>,
    #[serde(default)]
    pub layouts: Vec<LayoutCfg>,
    /// TUI content width — `"auto"` or a fixed column count.
    #[serde(default = "default_tui_width")]
    pub tui_width: TuiWidth,
    /// The config file's own directory: `database_path` and a relative
    /// `query`/`stream`/`setup` command resolve against this, not the process's
    /// launch directory (spec: source-configuration — config-relative
    /// working directory). Never set from TOML — populated by [`load`];
    /// defaults to `.` for a `Config` built directly (e.g. in tests).
    #[serde(skip, default = "default_config_dir")]
    pub config_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_path: default_db_path(),
            listen: default_listen(),
            interval: default_interval(),
            failure_threshold: default_threshold(),
            history_points: default_history_points(),
            logs_per_page: default_logs_per_page(),
            web_refresh_interval: default_web_refresh_interval(),
            web_content_width: default_web_content_width(),
            retention: None,
            web_user_js: default_user_js(),
            sources: Vec::new(),
            layouts: Vec::new(),
            tui_width: default_tui_width(),
            config_dir: default_config_dir(),
        }
    }
}

pub fn load(path: &Path) -> Result<Config> {
    load_from(path, |name| std::env::var(name).ok())
}

/// Same as [`load`], but reads `BARDUCK_*` overrides through `lookup`
/// instead of the real process environment — lets tests exercise the
/// override-then-resolve interaction without mutating global process state
/// (`std::env::set_var` is `unsafe` as of the 2024 edition, and this
/// project denies `unsafe_code`).
fn load_from(path: &Path, lookup: impl Fn(&str) -> Option<String>) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let mut cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
    // `Path::parent` yields `Some("")` for a bare relative path like
    // `config.toml` — treat that as `.` so `config_dir.join(...)` and
    // `Command::current_dir(config_dir)` resolve against the launch
    // directory instead of failing with ENOENT.
    cfg.config_dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    // Env overrides apply before the relative-path resolution below, so a
    // `BARDUCK_DATABASE_PATH` override is resolved against the config file's
    // own directory exactly like a TOML-declared one — not the process's
    // current directory (spec: source-configuration — config-relative
    // working directory) — `main()` never has to `chdir` the whole process.
    apply_env_overrides_from(&mut cfg, lookup)?;
    if cfg.database_path.is_relative() {
        cfg.database_path = cfg.config_dir.join(&cfg.database_path);
    }
    source::expand_composites(&mut cfg)?;
    validate(&cfg)?;
    Ok(cfg)
}

/// One user JavaScript file resolved from [`Config::web_user_js`]: `name`
/// is the opaque per-startup URL key served under `/assets/user-js/<name>`
/// (never derived from the request — traversal impossible by construction);
/// `path` is the resolved file the handler reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUserScript {
    pub name: String,
    pub path: PathBuf,
}

/// Resolves [`Config::web_user_js`] into the ordered, deduped file list the
/// web UI injects and serves (spec: source-configuration — user-defined web
/// UI scripts configuration): config-list order, then byte-wise filename
/// order within each directory. Entries join `config_dir` when relative,
/// exactly like `database_path`. Unresolvable entries (missing, unreadable,
/// or neither file nor directory) warn naming the entry and are skipped, so
/// a typo'd script path never takes the dashboard down. Dedupe is by
/// canonical path, keeping the first occurrence.
#[must_use]
pub fn resolve_user_js(entries: &[PathBuf], config_dir: &Path) -> Vec<ResolvedUserScript> {
    let mut out: Vec<ResolvedUserScript> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for entry in entries {
        let joined = if entry.is_relative() {
            config_dir.join(entry)
        } else {
            entry.clone()
        };
        match std::fs::metadata(&joined) {
            Ok(md) if md.is_dir() => match std::fs::read_dir(&joined) {
                Ok(dir) => {
                    let mut files: Vec<PathBuf> = dir
                        .filter_map(std::result::Result::ok)
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|ext| ext == "js") && p.is_file())
                        .collect();
                    files.sort();
                    for file in &files {
                        push_user_js_file(file, &mut seen, &mut out);
                    }
                }
                Err(e) => {
                    tracing::warn!("web_user_js: skipping {}: {e:#}", joined.display());
                }
            },
            Ok(_) => push_user_js_file(&joined, &mut seen, &mut out),
            Err(e) => {
                tracing::warn!("web_user_js: skipping {}: {e:#}", joined.display());
            }
        }
    }
    out
}

fn push_user_js_file(
    file: &Path,
    seen: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<ResolvedUserScript>,
) {
    match file.canonicalize() {
        Ok(canonical) => {
            if seen.insert(canonical.clone()) {
                let name = format!("u{}.js", out.len());
                out.push(ResolvedUserScript {
                    name,
                    path: canonical,
                });
            }
        }
        Err(e) => {
            tracing::warn!("web_user_js: skipping {}: {e:#}", file.display());
        }
    }
}
pub fn validate(cfg: &Config) -> Result<()> {
    // `history_points` sizes a per-panel allocation in the web renderer,
    // while the DB read behind it is capped at `MAX_HISTORY_LIMIT` — a
    // larger value can only pad with empty entries, so bound it to the cap.
    if cfg.history_points == 0 || i64::from(cfg.history_points) > crate::db::MAX_HISTORY_LIMIT {
        bail!(
            "history_points must be in 1..={}",
            crate::db::MAX_HISTORY_LIMIT
        );
    }
    if cfg.logs_per_page == 0 {
        bail!("logs_per_page must be > 0");
    }
    if cfg.web_refresh_interval.is_zero() {
        bail!("web_refresh_interval must be > 0");
    }
    if cfg.web_content_width != "wide" && cfg.web_content_width != "narrow" {
        bail!(
            "web_content_width must be \"wide\" or \"narrow\", got `{}`",
            cfg.web_content_width
        );
    }
    // `failure_threshold` counts consecutive failures from the newest
    // `failure_threshold` log rows, but `db.logs`/`db.health_inputs` clamp
    // that row count to `MAX_LOGS_LIMIT` — a larger threshold could never be
    // reached, so the source would degrade to `stale` instead of `failing`.
    // A 0 threshold would likewise mark every source permanently failing
    // from its very first health check (spec: data-collection — consecutive
    // failures flip to failing).
    if cfg.failure_threshold == 0 || i64::from(cfg.failure_threshold) > crate::db::MAX_LOGS_LIMIT {
        bail!(
            "failure_threshold must be in 1..={}",
            crate::db::MAX_LOGS_LIMIT
        );
    }
    if let Some(r) = cfg.retention
        && r.is_zero()
    {
        bail!("retention must be > 0");
    }
    match &cfg.tui_width {
        TuiWidth::Named(s) if s != "auto" => {
            bail!("tui_width must be \"auto\" or a positive integer, got `{s}`");
        }
        TuiWidth::Fixed(0) => bail!("tui_width must be > 0"),
        TuiWidth::Named(_) | TuiWidth::Fixed(_) => {}
    }
    let mut seen = std::collections::HashSet::new();
    for s in &cfg.sources {
        if !seen.insert(s.name().to_string()) {
            bail!("duplicate source name `{}`", s.name());
        }
        validate_source(s)?;
    }
    for l in &cfg.layouts {
        if l.rows.is_empty() {
            bail!("layout `{}` has no rows", l.title);
        }
        if l.columns() == 0 {
            bail!("layout `{}` column count is 0", l.title);
        }
        for (ri, row) in l.rows.iter().enumerate() {
            for cell in row {
                validate_cell(cfg, &l.title, ri, cell)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::health::Health;
    use defaults::{apply_env_overrides_from, default_interval, default_retry_interval};

    fn source_toml(extra: &str) -> String {
        format!("[[sources]]\nname = \"cpu\"\ntype = \"query\"\ncommand = \"echo 0\"\n{extra}\n")
    }

    #[test]
    fn invalid_tui_width_string_rejected() {
        let cfg: Config = toml::from_str("tui_width = \"wide\"").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("wide"),
            "error should name the invalid value: {err}"
        );
    }

    #[test]
    fn zero_tui_width_rejected() {
        let cfg: Config = toml::from_str("tui_width = 0").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("tui_width"),
            "error should name the field: {err}"
        );
    }

    #[test]
    fn show_history_false_parses() {
        let cfg: Config = toml::from_str(&source_toml("show_history = false")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].show_history(), Some(false));
    }

    #[test]
    fn humantime_duration_parses_and_applies() {
        let cfg: Config =
            toml::from_str(&source_toml("interval = \"5m\"\ntimeout = \"30s\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].interval(), Some(Duration::from_mins(5)));
        assert_eq!(cfg.sources[0].timeout(), Duration::from_secs(30));
    }

    #[test]
    fn invalid_duration_string_rejected() {
        let err = toml::from_str::<Config>(&source_toml("timeout = \"banana\"")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("banana"),
            "error should name the invalid value: {msg}"
        );
        assert!(
            msg.contains("timeout"),
            "error should name the offending field: {msg}"
        );
    }

    #[test]
    fn cron_schedule_accepted() {
        let cfg: Config = toml::from_str(&source_toml("cron = \"0 0 3 * * *\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].cron(), Some("0 0 3 * * *"));
        assert_eq!(cfg.sources[0].interval(), None);
    }

    #[test]
    fn effective_interval_falls_back_to_default_when_unset() {
        let cfg: Config = toml::from_str(&source_toml("cron = \"0 0 3 * * *\"")).unwrap();
        assert_eq!(cfg.sources[0].effective_interval(), default_interval());
    }

    #[test]
    fn effective_retry_interval_falls_back_to_default_when_unset() {
        let cfg: Config = toml::from_str(&source_toml("")).unwrap();
        assert_eq!(
            cfg.sources[0].effective_retry_interval(),
            default_retry_interval()
        );
    }

    #[test]
    fn effective_retry_interval_uses_declared_value() {
        let cfg: Config = toml::from_str(&source_toml("retry_interval = \"10s\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(
            cfg.sources[0].effective_retry_interval(),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn both_interval_and_cron_rejected() {
        let cfg: Config =
            toml::from_str(&source_toml("interval = \"5m\"\ncron = \"0 */5 * * * *\"")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("interval") && err.to_string().contains("cron"),
            "error should name both fields: {err}"
        );
    }

    #[test]
    fn zero_retry_interval_rejected() {
        let cfg: Config = toml::from_str(&source_toml("retry_interval = \"0s\"")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("retry_interval"),
            "error should name the field: {err}"
        );
    }

    #[test]
    fn retry_interval_with_cron_rejected() {
        let cfg: Config = toml::from_str(&source_toml(
            "cron = \"0 */5 * * * *\"\nretry_interval = \"10s\"",
        ))
        .unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("retry_interval") && err.to_string().contains("cron"),
            "error should name both fields: {err}"
        );
    }

    #[test]
    fn interval_and_retry_interval_together_accepted() {
        let cfg: Config =
            toml::from_str(&source_toml("interval = \"5m\"\nretry_interval = \"10s\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(
            cfg.sources[0].effective_retry_interval(),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn invalid_cron_expression_rejected() {
        let cfg: Config = toml::from_str(&source_toml("cron = \"not a cron\"")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("cron"),
            "error should mention the invalid cron expression: {err}"
        );
    }

    #[test]
    fn leftover_pre_rename_field_rejected() {
        let err = toml::from_str::<Config>(&source_toml("interval_secs = 300")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("interval_secs"),
            "error should name the unrecognized field: {msg}"
        );
    }

    #[test]
    fn group_cell_span_and_title() {
        let cell = Cell::Group {
            title: Some("ihor".into()),
            main: None,
            secondary: Vec::new(),
            table: vec![GroupItem::Id("a".into())],
            style: None,
        };
        assert_eq!(cell.span(), 1);
        assert_eq!(cell.pane_title(), Some("ihor"));
    }
    #[test]
    fn text_cell_span_title_and_source_names() {
        let cell = Cell::Text {
            title: Some("Links".into()),
            format: Some("markdown".into()),
            text: "- [GitHub](https://github.com)".into(),
            style: None,
        };
        assert_eq!(cell.span(), 1);
        assert_eq!(cell.pane_title(), Some("Links"));
        assert!(
            cell.source_names().is_empty(),
            "a text cell has no backing source"
        );
    }

    #[test]
    fn text_cell_without_title_has_no_pane_title() {
        let cell = Cell::Text {
            title: None,
            format: None,
            text: "note".into(),
            style: None,
        };
        assert_eq!(cell.pane_title(), None);
    }

    #[test]
    fn text_cell_toml_parses_before_group() {
        // Proves a `{ text, format }` table deserializes as `Cell::Text`, not
        // an empty `Cell::Group` — `Group`'s fields are all optional, so
        // `Text` must be tried first (design.md — Cell variant ordering).
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"Links\", format = \"markdown\", text = \"hi\" }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let cell = &cfg.layouts[0].rows[0][0];
        assert!(
            matches!(cell, Cell::Text { .. }),
            "expected Cell::Text, got a different variant: {cell:?}"
        );
        assert_eq!(cell.pane_title(), Some("Links"));
    }

    #[test]
    fn valid_text_cell_accepted() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ text = \"hi\" }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn text_cell_empty_text_rejected() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"Links\", text = \"\" }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("empty text"),
            "error should mention the empty text panel: {err}"
        );
    }

    #[test]
    fn text_cell_invalid_format_rejected() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ text = \"hi\", format = \"yaml\" }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("yaml"),
            "error should name the invalid format: {err}"
        );
    }

    /// (spec: source-configuration — Value formats)
    #[test]
    fn removed_json_format_rejected() {
        let err = toml::from_str::<Config>(&source_toml("format = \"json\"")).unwrap_err();
        assert!(
            err.to_string().contains("json"),
            "error should name the removed format: {err}"
        );
    }

    #[test]
    fn accent_color_prioritizes_health_over_a_stale_band() {
        // Unhealthy overrides any band reading, banded or not.
        assert_eq!(
            accent_color(Some(Level::Green), Health::Failing),
            Some(Level::Red)
        );
        assert_eq!(
            accent_color(Some(Level::Green), Health::Stale),
            Some(Level::Yellow)
        );
        assert_eq!(accent_color(None, Health::Failing), Some(Level::Red));
        assert_eq!(accent_color(None, Health::Stale), Some(Level::Yellow));
        // Healthy: band color when present, else nothing to accent.
        assert_eq!(
            accent_color(Some(Level::Red), Health::Healthy),
            Some(Level::Red)
        );
        assert_eq!(accent_color(None, Health::Healthy), None);
    }

    #[test]
    fn status_color_falls_back_to_green_when_accent_color_is_none() {
        assert_eq!(status_color(None, Health::Healthy), Level::Green);
        assert_eq!(status_color(None, Health::Failing), Level::Red);
    }

    #[test]
    fn worst_color_ranks_red_over_yellow_over_green() {
        assert_eq!(
            worst_color([Level::Green, Level::Yellow, Level::Red]),
            Level::Red
        );
        assert_eq!(worst_color([Level::Green, Level::Yellow]), Level::Yellow);
        assert_eq!(worst_color([Level::Green, Level::Green]), Level::Green);
        assert_eq!(worst_color([]), Level::Green);
    }

    #[test]
    fn group_item_explicit_label_none_for_bare_id() {
        assert_eq!(GroupItem::Id("vds-base1".into()).explicit_label(), None);
        assert_eq!(
            GroupItem::Labeled {
                id: "vds-base1".into(),
                label: "days left".into()
            }
            .explicit_label(),
            Some("days left")
        );
    }

    #[test]
    fn group_cell_source_names_lists_every_member() {
        let cell = Cell::Group {
            title: Some("ihor".into()),
            main: Some(GroupItem::Id("a".into())),
            secondary: vec![GroupItem::Labeled {
                id: "b".into(),
                label: "B".into(),
            }],
            table: vec![GroupItem::Id("c".into())],
            style: None,
        };
        assert_eq!(cell.source_names(), vec!["a", "b", "c"]);
    }

    fn group_layout_toml(group_extra: &str) -> String {
        format!(
            "{}\n[[layouts]]\ntitle = \"L\"\nrows = [[{{ title = \"ihor\", {group_extra} }}]]\n",
            source_toml("")
        )
    }

    #[test]
    fn valid_group_cell_accepted() {
        let cfg: Config = toml::from_str(&group_layout_toml("table = [\"cpu\"]")).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn valid_group_cell_with_only_main_accepted() {
        let cfg: Config = toml::from_str(&group_layout_toml("main = \"cpu\"")).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn group_cell_combining_main_secondary_table_accepted() {
        let cfg: Config = toml::from_str(&group_layout_toml(
            "main = \"cpu\", secondary = [\"cpu\"], table = [\"cpu\"]",
        ))
        .unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn group_cell_empty_title_rejected() {
        let toml = "[[sources]]\nname = \"cpu\"\ntype = \"query\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"\", table = [\"cpu\"] }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("empty title"),
            "error should mention empty title: {err}"
        );
    }

    #[test]
    fn group_cell_without_title_accepted() {
        let toml = "[[sources]]\nname = \"cpu\"\ntype = \"query\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ secondary = [\"cpu\"] }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        validate(&cfg).unwrap();
    }

    /// `kind = "space"` is the only kind's schema allows; a typo (e.g.
    /// `kind = "spacer"`) has to surface as a named mistake instead of
    /// silently rendering as a working space cell.
    #[test]
    fn space_cell_unknown_kind_rejected() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ kind = \"spacer\", colspan = 2 }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("spacer"),
            "error should name the bad kind: {err}"
        );
    }

    #[test]
    fn space_cell_colspan_zero_rejected() {
        // A second, normal-width cell in the row keeps the layout's total
        // column count above 0, so it's the colspan-0 space cell itself
        // that trips validation, not the layout-wide column-count check.
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ kind = \"space\", colspan = 0 }, { kind = \"space\", colspan = 1 }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("colspan 0"),
            "error should name the problem: {err}"
        );
    }

    /// A `colspan` wider than any real column is a mistake, and it also
    /// sizes the layout's column count (and the renderer's grid) directly.
    #[test]
    fn space_cell_colspan_above_maximum_rejected() {
        let toml = format!(
            "[[layouts]]\ntitle = \"L\"\nrows = [[{{ kind = \"space\", colspan = {} }}]]\n",
            super::layout::MAX_COLSPAN + 1
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("maximum"),
            "error should name the maximum: {err}"
        );
    }

    #[test]
    fn space_cell_colspan_at_maximum_accepted() {
        let toml = format!(
            "[[layouts]]\ntitle = \"L\"\nrows = [[{{ kind = \"space\", colspan = {} }}]]\n",
            super::layout::MAX_COLSPAN
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn pane_cell_empty_title_rejected() {
        let toml = format!(
            "{}\n[[layouts]]\ntitle = \"L\"\nrows = [[{{ id = \"cpu\", title = \"\" }}]]\n",
            source_toml("")
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("empty title"),
            "error should mention the empty title: {err}"
        );
    }

    #[test]
    fn text_cell_empty_title_rejected() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"\", text = \"hi\" }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("empty title"),
            "error should mention the empty title: {err}"
        );
    }

    #[test]
    fn pane_and_text_cells_without_title_accepted() {
        let toml = format!(
            "{}\n[[layouts]]\ntitle = \"L\"\nrows = [[{{ id = \"cpu\" }}, {{ text = \"hi\" }}]]\n",
            source_toml("")
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn space_cell_accepted() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ kind = \"space\", colspan = 2 }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn group_cell_with_no_sections_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("table = []")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("main/secondary/table"),
            "error should mention the empty group: {err}"
        );
    }

    #[test]
    fn group_cell_unknown_source_in_main_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("main = \"nope\"")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("nope"),
            "error should name the unknown source: {err}"
        );
    }

    #[test]
    fn group_cell_unknown_source_in_secondary_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("secondary = [\"nope\"]")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("nope"),
            "error should name the unknown source: {err}"
        );
    }

    #[test]
    fn group_cell_unknown_source_in_table_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("table = [\"nope\"]")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("nope"),
            "error should name the unknown source: {err}"
        );
    }

    fn lookup_from(
        pairs: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn env_var_overrides_config_file_value() {
        let mut cfg: Config = toml::from_str("listen = \"127.0.0.1:8420\"").unwrap();
        apply_env_overrides_from(&mut cfg, lookup_from(&[("BARDUCK_LISTEN", "0.0.0.0:9000")]))
            .unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:9000");
    }

    #[test]
    fn env_var_overrides_default() {
        let mut cfg = Config::default();
        apply_env_overrides_from(&mut cfg, lookup_from(&[("BARDUCK_HISTORY_POINTS", "100")]))
            .unwrap();
        assert_eq!(cfg.history_points, 100);
    }

    /// `BARDUCK_TUI_WIDTH` is trimmed like every other env value before it's
    /// compared, so `" auto "` is accepted rather than rejected with a
    /// confusing parse error (spec: source-configuration — environment
    /// variables override top-level settings).
    #[test]
    fn env_tui_width_accepts_surrounding_whitespace() {
        for (raw, expect_named) in [(" auto ", true), ("auto", true), (" 12 ", false)] {
            let mut cfg = Config::default();
            apply_env_overrides_from(&mut cfg, |name| {
                (name == "BARDUCK_TUI_WIDTH").then(|| raw.to_string())
            })
            .unwrap();
            match cfg.tui_width {
                TuiWidth::Named(s) => assert!(expect_named, "`{raw}` parsed as a name: {s}"),
                TuiWidth::Fixed(n) => assert!(!expect_named, "`{raw}` parsed as fixed: {n}"),
            }
        }
    }

    #[test]
    fn logs_per_page_defaults_to_50_and_env_can_override_it() {
        assert_eq!(Config::default().logs_per_page, 50);
        let mut cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.logs_per_page, 50);
        apply_env_overrides_from(&mut cfg, lookup_from(&[("BARDUCK_LOGS_PER_PAGE", "20")]))
            .unwrap();
        assert_eq!(cfg.logs_per_page, 20);
        validate(&cfg).unwrap();
    }

    #[test]
    fn zero_logs_per_page_is_rejected() {
        let cfg: Config = toml::from_str("logs_per_page = 0").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("logs_per_page"));
    }
    #[test]
    fn web_content_width_defaults_to_narrow() {
        assert_eq!(Config::default().web_content_width, "narrow");
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.web_content_width, "narrow");
        validate(&cfg).unwrap();
    }

    #[test]
    fn web_content_width_wide_is_accepted() {
        let cfg: Config = toml::from_str("web_content_width = \"wide\"").unwrap();
        assert_eq!(cfg.web_content_width, "wide");
        validate(&cfg).unwrap();
    }

    #[test]
    fn invalid_web_content_width_is_rejected() {
        let cfg: Config = toml::from_str("web_content_width = \"medium\"").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("web_content_width"),
            "error should name the key: {err}"
        );
    }

    #[test]
    fn no_env_var_leaves_config_value_unchanged() {
        let mut cfg: Config = toml::from_str("failure_threshold = 5").unwrap();
        apply_env_overrides_from(&mut cfg, lookup_from(&[])).unwrap();
        assert_eq!(cfg.failure_threshold, 5);
    }

    #[test]
    fn unparseable_env_override_rejected() {
        let mut cfg = Config::default();
        let err = apply_env_overrides_from(
            &mut cfg,
            lookup_from(&[("BARDUCK_INTERVAL", "not-a-duration")]),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("BARDUCK_INTERVAL"),
            "error should name the variable: {err}"
        );
    }

    #[test]
    fn value_type_defaults_to_string_when_unset() {
        let cfg: Config = toml::from_str(&source_toml("")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].value_type(), None);
        assert_eq!(cfg.sources[0].effective_value_type(), ValueType::String);
    }

    #[test]
    fn value_type_bigint_double_json_parse() {
        for (toml_val, expected) in [
            ("bigint", ValueType::Bigint),
            ("double", ValueType::Double),
            ("json", ValueType::Json),
        ] {
            let cfg: Config =
                toml::from_str(&source_toml(&format!("value_type = \"{toml_val}\""))).unwrap();
            validate(&cfg).unwrap();
            assert_eq!(cfg.sources[0].effective_value_type(), expected);
        }
    }

    #[test]
    fn value_type_invalid_value_rejected() {
        let err = toml::from_str::<Config>(&source_toml("value_type = \"decimal\"")).unwrap_err();
        assert!(
            err.to_string().contains("decimal"),
            "error should name the invalid value: {err}"
        );
    }

    #[test]
    fn show_in_defaults_to_visible_everywhere() {
        let cfg: Config = toml::from_str(&source_toml("")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].show_in(), None);
        assert!(cfg.sources[0].visible_in(View::Tui));
        assert!(cfg.sources[0].visible_in(View::Web));
    }

    #[test]
    fn show_in_restricts_to_one_view() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"tui\"")).unwrap();
        validate(&cfg).unwrap();
        assert!(cfg.sources[0].visible_in(View::Tui));
        assert!(!cfg.sources[0].visible_in(View::Web));
    }

    #[test]
    fn show_in_invalid_value_rejected() {
        // `show_in` is a real enum now, so an invalid value is rejected at
        // deserialization rather than by a separate runtime check.
        let err = toml::from_str::<Config>(&source_toml("show_in = \"cli\"")).unwrap_err();
        assert!(
            err.to_string().contains("cli"),
            "error should name the invalid value: {err}"
        );
    }

    #[test]
    fn source_visible_in_true_for_unknown_source() {
        // Validation already guarantees layout references resolve; a caller
        // without the SourceCfg in hand still gets a sensible default.
        let cfg = Config::default();
        assert!(source_visible_in(&cfg, "nope", View::Tui));
    }

    #[test]
    fn source_visible_in_matches_source_show_in() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"web\"")).unwrap();
        assert!(!source_visible_in(&cfg, "cpu", View::Tui));
        assert!(source_visible_in(&cfg, "cpu", View::Web));
    }

    #[test]
    fn visible_items_omits_hidden_members() {
        let toml = "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[sources]]\nname = \"b\"\ntype = \"query\"\ncommand = \"echo 0\"\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let items = vec![GroupItem::Id("a".into()), GroupItem::Id("b".into())];
        let visible = visible_items(&cfg, &items, View::Tui);
        assert_eq!(
            visible.iter().map(|i| i.id()).collect::<Vec<_>>(),
            vec!["b"]
        );
    }

    #[test]
    fn visible_items_empty_when_all_members_hidden() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"web\"")).unwrap();
        let items = vec![GroupItem::Id("cpu".into())];
        assert!(visible_items(&cfg, &items, View::Tui).is_empty());
    }

    fn stream_toml(extra: &str) -> String {
        format!(
            "[[sources]]\nname = \"ticks\"\ntype = \"stream\"\ncommand = \"tail -f /dev/null\"\nexpected_interval = \"20s\"\n{extra}\n"
        )
    }

    /// (spec: source-configuration — Stream source type)
    #[test]
    fn stream_source_accepted_with_expected_interval() {
        let cfg: Config = toml::from_str(&stream_toml("")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(
            cfg.sources[0].expected_interval(),
            Some(Duration::from_secs(20))
        );
        assert!(cfg.sources[0].is_stream());
        assert_eq!(cfg.sources[0].cron(), None);
        assert_eq!(cfg.sources[0].interval(), None);
    }

    /// (spec: source-configuration — Stream source type)
    #[test]
    fn stream_source_missing_expected_interval_rejected() {
        let toml =
            "[[sources]]\nname = \"ticks\"\ntype = \"stream\"\ncommand = \"tail -f /dev/null\"\n";
        let err = toml::from_str::<Config>(toml).unwrap_err();
        assert!(
            err.to_string().contains("expected_interval"),
            "error should name the missing field: {err}"
        );
    }

    /// (spec: source-configuration — Stream source type)
    #[test]
    fn stream_source_with_interval_rejected() {
        let err = toml::from_str::<Config>(&stream_toml("interval = \"5m\"")).unwrap_err();
        assert!(
            err.to_string().contains("interval"),
            "error should name the cross-type field: {err}"
        );
    }

    /// (spec: source-configuration — Per-type source fields)
    #[test]
    fn removed_http_type_rejected() {
        let toml = "[[sources]]\nname = \"bank\"\ntype = \"http\"\nurl = \"http://x/\"\n";
        let err = toml::from_str::<Config>(toml).unwrap_err();
        assert!(
            err.to_string().contains("http"),
            "error should name the unknown type: {err}"
        );
    }

    /// (spec: source-configuration — Per-type source fields)
    #[test]
    fn renamed_script_type_rejected() {
        let toml = "[[sources]]\nname = \"cpu\"\ntype = \"script\"\ncommand = \"echo 0\"\n";
        let err = toml::from_str::<Config>(toml).unwrap_err();
        assert!(
            err.to_string().contains("script"),
            "error should name the unknown type: {err}"
        );
    }

    /// (spec: source-configuration — Per-type source fields)
    #[test]
    fn query_source_with_expected_interval_rejected() {
        let err = toml::from_str::<Config>(&source_toml("expected_interval = \"1m\"")).unwrap_err();
        assert!(
            err.to_string().contains("expected_interval"),
            "error should name the cross-type field: {err}"
        );
    }

    #[test]
    fn layout_style_override_parses() {
        let toml = "[[layouts]]\ntitle = \"L\"\nstyle = { font_family = \"monospace\", font_size = \"14px\" }\nrows = [[{ main = \"s\", style = { font_family = \"sans-serif\" } }]]\n[[sources]]\nname = \"s\"\ntype = \"query\"\ncommand = \"echo 1\"\ninterval = \"10s\"\ntimeout = \"5s\"\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.layouts[0].style.as_ref().unwrap().get("font_family"),
            Some(&"monospace".to_string())
        );
        assert_eq!(
            cfg.layouts[0].style.as_ref().unwrap().get("font_size"),
            Some(&"14px".to_string())
        );
        if let Cell::Group {
            style: Some(cell_style),
            ..
        } = &cfg.layouts[0].rows[0][0]
        {
            assert_eq!(
                cell_style.get("font_family"),
                Some(&"sans-serif".to_string())
            );
        } else {
            unreachable!("expected Group cell");
        }
    }

    #[test]
    fn layout_without_style_defaults_to_none() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ main = \"s\" }]]\n[[sources]]\nname = \"s\"\ntype = \"query\"\ncommand = \"echo 1\"\ninterval = \"10s\"\ntimeout = \"5s\"\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.layouts[0].style.is_none());
    }

    /// A `0` threshold would make `health::compute` treat "0 consecutive
    /// failures >= threshold" as failing, marking every source permanently
    /// failing from its first health check (spec: data-collection —
    /// consecutive failures flip to failing).
    #[test]
    fn failure_threshold_zero_rejected() {
        let cfg: Config = toml::from_str("failure_threshold = 0").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("failure_threshold"),
            "error should name the field: {err}"
        );
    }

    /// A `failure_threshold` above `MAX_LOGS_LIMIT` counts more log rows than
    /// the health read ever returns, so the source could never flip to
    /// `failing` — it would silently degrade to `stale` forever.
    #[test]
    fn failure_threshold_above_max_logs_limit_rejected() {
        let cap = crate::db::MAX_LOGS_LIMIT.to_string();
        let cfg: Config = toml::from_str(&format!(
            "failure_threshold = {}",
            crate::db::MAX_LOGS_LIMIT + 1
        ))
        .unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains(&cap),
            "error should name the cap: {err}"
        );
    }

    #[test]
    fn failure_threshold_at_max_logs_limit_accepted() {
        let cfg: Config = toml::from_str(&format!(
            "failure_threshold = {}",
            crate::db::MAX_LOGS_LIMIT
        ))
        .unwrap();
        validate(&cfg).unwrap();
    }

    /// `history_points` sizes the web renderer's per-panel allocation while
    /// the DB read behind it is capped at `MAX_HISTORY_LIMIT`.
    #[test]
    fn history_points_above_max_history_limit_rejected() {
        let cap = crate::db::MAX_HISTORY_LIMIT.to_string();
        let cfg: Config = toml::from_str(&format!(
            "history_points = {}",
            crate::db::MAX_HISTORY_LIMIT + 1
        ))
        .unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains(&cap),
            "error should name the cap: {err}"
        );
    }

    #[test]
    fn history_points_at_max_history_limit_accepted() {
        let cfg: Config = toml::from_str(&format!(
            "history_points = {}",
            crate::db::MAX_HISTORY_LIMIT
        ))
        .unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn per_source_history_points_above_max_rejected() {
        let cap = crate::db::MAX_HISTORY_LIMIT.to_string();
        let cfg: Config = toml::from_str(&source_toml(&format!(
            "history_points = {}",
            crate::db::MAX_HISTORY_LIMIT + 1
        )))
        .unwrap();
        let err = validate(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("cpu") && msg.contains(&cap),
            "error should name the source and the cap: {err}"
        );
    }

    #[test]
    fn per_source_history_points_at_max_accepted() {
        let cfg: Config = toml::from_str(&source_toml(&format!(
            "history_points = {}",
            crate::db::MAX_HISTORY_LIMIT
        )))
        .unwrap();
        validate(&cfg).unwrap();
    }

    /// A zero `timeout` loads fine but makes every fetch of the source time
    /// out instantly, forever (spec: source-configuration — Human-readable
    /// duration configuration).
    #[test]
    fn zero_timeout_rejected() {
        for toml in [
            source_toml("timeout = \"0s\""),
            "[[sources]]\nname = \"s\"\ntype = \"stream\"\ncommand = \"cat\"\nexpected_interval = \"1m\"\ntimeout = \"0s\"\n".to_string(),
        ] {
            let cfg: Config = toml::from_str(&toml).unwrap();
            let err = validate(&cfg).unwrap_err();
            assert!(
                err.to_string().contains("timeout"),
                "error should name the field: {err}"
            );
        }
    }

    /// A duplicate band bound makes one of the two bands sharing it
    /// unreachable with no diagnostic.
    #[test]
    fn duplicate_threshold_bound_rejected() {
        let cfg: Config = toml::from_str(&source_toml(
            "thresholds = [{bound = 60.0, level = \"green\"}, {bound = 60.0, level = \"red\"}]",
        ))
        .unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("cpu") && err.to_string().contains("60"),
            "error should name the source and the repeated bound: {err}"
        );
    }

    #[test]
    fn distinct_threshold_bounds_accepted() {
        let cfg: Config = toml::from_str(&source_toml(
            "thresholds = [{bound = 60.0, level = \"green\"}, {bound = 85.0, level = \"yellow\"}, {bound = 100.0, level = \"red\"}]",
        ))
        .unwrap();
        validate(&cfg).unwrap();
    }

    /// A bare relative config path (`config.toml`) has an *empty* parent,
    /// which is not a usable working directory — it must resolve to `.` so
    /// `config_dir.join(database_path)` and every source command's
    /// `current_dir` keep working (spec: source-configuration —
    /// config-relative working directory).
    #[test]
    fn bare_relative_config_path_yields_usable_config_dir() {
        // A uniquely named file in the process's own working directory,
        // addressed by its single path component — the shape whose
        // `parent()` is `Some("")` rather than a real directory. Auto-deleted
        // on drop, so no process-wide `set_current_dir` race.
        let file = tempfile::Builder::new()
            .prefix("barduck-cfg-test-")
            .suffix(".toml")
            .tempfile_in(".")
            .unwrap();
        std::fs::write(file.path(), "listen = \"127.0.0.1:0\"\n").unwrap();
        let name = file.path().file_name().unwrap().to_str().unwrap();

        let cfg = load_from(Path::new(name), lookup_from(&[])).unwrap();

        assert_eq!(cfg.config_dir, Path::new("."));
        assert_eq!(cfg.database_path, Path::new("./dashboard.duckdb"));
    }

    /// A relative `BARDUCK_DATABASE_PATH` override must resolve against the
    /// config file's own directory, exactly like a TOML-declared relative
    /// `database_path` — not the process's current directory (spec:
    /// source-configuration — config-relative working directory). Exercises
    /// `load_from` directly (real file, injectable env lookup) rather than
    /// spawning a daemon, so it's neither flaky nor timing-dependent.
    #[test]
    fn env_override_relative_database_path_resolves_against_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "listen = \"127.0.0.1:0\"\n").unwrap();

        let cfg = load_from(
            &config_path,
            lookup_from(&[("BARDUCK_DATABASE_PATH", "override.duckdb")]),
        )
        .unwrap();

        assert_eq!(cfg.database_path, dir.path().join("override.duckdb"));
    }

    /// `web_user_js` resolves file-or-dir entries in config-list order with
    /// alphabetic directory expansion, dedupes by canonical path keeping the
    /// first occurrence, and skips missing paths without failing (spec:
    /// source-configuration — user-defined web UI scripts configuration).
    #[test]
    fn user_js_resolution_orders_expands_and_dedupes() {
        let dir = tempfile::tempdir().unwrap();
        let extra = dir.path().join("extra");
        std::fs::create_dir(&extra).unwrap();
        // `b.js` sorts before `a.js` on disk creation order but after it
        // alphabetically; `note.txt` must not be picked up.
        std::fs::write(extra.join("b.js"), "b").unwrap();
        std::fs::write(extra.join("a.js"), "a").unwrap();
        std::fs::write(extra.join("note.txt"), "x").unwrap();
        let lone = dir.path().join("lone.js");
        std::fs::write(&lone, "lone").unwrap();

        let resolved = resolve_user_js(
            &[
                lone.clone(),
                extra.clone(),
                extra.join("a.js"),
                dir.path().join("missing.js"),
            ],
            dir.path(),
        );

        let names: Vec<&str> = resolved
            .iter()
            .map(|s| s.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["lone.js", "a.js", "b.js"]);
        let urls: Vec<&str> = resolved.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(urls, ["u0.js", "u1.js", "u2.js"]);
    }

    /// Relative `web_user_js` entries resolve against the config file's own
    /// directory, exactly like `database_path` (spec: source-configuration —
    /// user-defined web UI scripts configuration).
    #[test]
    fn user_js_relative_entries_resolve_against_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(
            &config_path,
            "listen = \"127.0.0.1:0\"\nweb_user_js = [\"scripts/x.js\"]\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("scripts")).unwrap();
        std::fs::write(dir.path().join("scripts/x.js"), "x").unwrap();

        let cfg = load_from(&config_path, lookup_from(&[])).unwrap();

        assert_eq!(cfg.web_user_js, vec![PathBuf::from("scripts/x.js")]);
        let resolved = resolve_user_js(&cfg.web_user_js, &cfg.config_dir);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].path, dir.path().join("scripts/x.js"));
    }

    /// Unknown top-level keys still fail even next to a valid `web_user_js`
    /// (spec: source-configuration — user-defined web UI scripts
    /// configuration).
    #[test]
    fn unknown_key_next_to_user_js_still_rejected() {
        let err = toml::from_str::<Config>("web_user_js = [\"x.js\"]\nweb_user_jss = [\"y.js\"]\n")
            .unwrap_err();
        assert!(
            err.to_string().contains("web_user_jss"),
            "error should name the unknown key: {err}"
        );
    }
}
