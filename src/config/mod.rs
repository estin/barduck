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
    GroupItem, Level, SourceCfg, SourceType, Threshold, ValueType, View, accent_color, level_for,
    source_visible_in, status_color, visible_items, worst_color,
};

use defaults::{
    apply_env_overrides, default_config_dir, default_db_path, default_history_points,
    default_interval, default_listen, default_threshold, default_tui_width,
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
    /// How long to keep collected data before the daemon prunes it: a
    /// humantime string (e.g. `"30d"`). Unset (the default) keeps everything
    /// forever, matching prior behavior (spec: data-storage — retention).
    #[serde(default, with = "humantime_serde::option")]
    pub retention: Option<Duration>,
    #[serde(default)]
    pub sources: Vec<SourceCfg>,
    #[serde(default)]
    pub layouts: Vec<LayoutCfg>,
    /// TUI content width — `"auto"` or a fixed column count.
    #[serde(default = "default_tui_width")]
    pub tui_width: TuiWidth,
    /// The config file's own directory: `database_path` and a relative
    /// `script`/`setup` command resolve against this, not the process's
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
            retention: None,
            sources: Vec::new(),
            layouts: Vec::new(),
            tui_width: default_tui_width(),
            config_dir: default_config_dir(),
        }
    }
}

pub fn load(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let mut cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
    // `database_path` (and any future relative, config-declared path) is
    // resolved against the config file's own directory here, up front, so
    // nothing downstream needs the process's current directory to behave
    // correctly (spec: source-configuration — config-relative working
    // directory) — `main()` never has to `chdir` the whole process.
    cfg.config_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    if cfg.database_path.is_relative() {
        cfg.database_path = cfg.config_dir.join(&cfg.database_path);
    }
    apply_env_overrides(&mut cfg)?;
    validate(&cfg)?;
    Ok(cfg)
}

pub fn validate(cfg: &Config) -> Result<()> {
    if cfg.history_points == 0 {
        bail!("history_points must be > 0");
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
        if !seen.insert(s.name.clone()) {
            bail!("duplicate source name `{}`", s.name);
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
        format!("[[sources]]\nname = \"cpu\"\ntype = \"script\"\ncommand = \"echo 0\"\n{extra}\n")
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
        assert_eq!(cfg.sources[0].show_history, Some(false));
    }

    #[test]
    fn humantime_duration_parses_and_applies() {
        let cfg: Config =
            toml::from_str(&source_toml("interval = \"5m\"\ntimeout = \"30s\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].interval, Some(Duration::from_mins(5)));
        assert_eq!(cfg.sources[0].timeout, Duration::from_secs(30));
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
        assert_eq!(cfg.sources[0].cron.as_deref(), Some("0 0 3 * * *"));
        assert_eq!(cfg.sources[0].interval, None);
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
        let toml = "[[sources]]\nname = \"cpu\"\ntype = \"script\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"\", table = [\"cpu\"] }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("empty title"),
            "error should mention empty title: {err}"
        );
    }

    #[test]
    fn group_cell_without_title_accepted() {
        let toml = "[[sources]]\nname = \"cpu\"\ntype = \"script\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ secondary = [\"cpu\"] }]]\n";
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
        assert_eq!(cfg.sources[0].value_type, None);
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
        let err =
            toml::from_str::<Config>(&source_toml("value_type = \"decimal\"")).unwrap_err();
        assert!(
            err.to_string().contains("decimal"),
            "error should name the invalid value: {err}"
        );
    }

    #[test]
    fn show_in_defaults_to_visible_everywhere() {
        let cfg: Config = toml::from_str(&source_toml("")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].show_in, None);
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
        let toml = "[[sources]]\nname = \"a\"\ntype = \"script\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[sources]]\nname = \"b\"\ntype = \"script\"\ncommand = \"echo 0\"\n";
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
}
