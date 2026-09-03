use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SOURCE_TYPES: &[&str] = &["http", "script"];
pub const VALUE_FORMATS: &[&str] = &["text", "markdown", "json"];
pub const LEVELS: &[&str] = &["green", "yellow", "red"];

/// App version shown in web UI, TUI, and CLI output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One coloring band for a source's numeric value.
#[derive(Debug, Clone, Deserialize)]
pub struct Threshold {
    /// Upper bound of this band (inclusive); the last band covers everything above.
    pub bound: f64,
    /// `green`, `yellow`, or `red`.
    pub level: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueFormat {
    #[default]
    Text,
    Markdown,
    Json,
}

impl ValueFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "text" => Ok(Self::Text),
            "markdown" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            other => bail!(
                "unknown value format `{other}` (known formats: {})",
                VALUE_FORMATS.join(", ")
            ),
        }
    }
}

/// Resolves the color level for a reading against the source's thresholds.
/// Bands are interpreted ascending by bound ("value ≤ bound"); encoding the
/// levels green→red or red→green gives either direction.
/// Returns None for non-numeric readings.
#[must_use]
pub fn level_for(thresholds: &[Threshold], value: &str) -> Option<String> {
    let v: f64 = value.trim().parse().ok()?;
    let mut bands: Vec<&Threshold> = thresholds.iter().collect();
    bands.sort_by(|a, b| a.bound.total_cmp(&b.bound));
    bands
        .iter()
        .find(|t| v <= t.bound)
        .or_else(|| bands.last())
        .map(|t| t.level.clone())
}

/// Resolves `"red"`/`"yellow"`/`"green"` for a source's aggregate/alerting
/// signal (a group's own border, the summary-strip chip) and for a healthy,
/// banded source's own value: health status takes priority whenever the
/// source isn't currently healthy (`failing`→red, `stale`→yellow) — even
/// overriding a threshold-band reading, since a stale or failing fetch means
/// that reading is no longer trustworthy. Only when healthy does the
/// threshold band level apply, falling back to `"green"` when there's no
/// band either; shared by the web UI and TUI (spec: web-ui — threshold band
/// coloring; tui — threshold band coloring).
#[must_use]
pub fn status_color(level: Option<&str>, status: &str) -> &'static str {
    accent_color(level, status).unwrap_or("green")
}

/// Same priority as [`status_color`], but `None` when the source is
/// currently healthy and has no threshold bands: there's nothing meaningful
/// to accent, so a panel/row should render with no color at all rather than
/// a default green (spec: web-ui — health visible at a glance; tui —
/// threshold band coloring).
#[must_use]
pub fn accent_color(level: Option<&str>, status: &str) -> Option<&'static str> {
    match status {
        "failing" => Some("red"),
        "stale" => Some("yellow"),
        _ => match level {
            Some("red") => Some("red"),
            Some("yellow") => Some("yellow"),
            Some("green") => Some("green"),
            _ => None,
        },
    }
}

/// Worst of a set of `"red"`/`"yellow"`/`"green"` colors (red > yellow >
/// green), used to color a group pane/panel by its worst member. `"green"`
/// for an empty set — unreachable for a real group, since validation
/// rejects an empty `ids` list.
#[must_use]
pub fn worst_color<'a>(colors: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut worst = "green";
    for color in colors {
        if color == "red" {
            return "red";
        }
        if color == "yellow" {
            worst = "yellow";
        }
    }
    worst
}

/// TUI content width: `"auto"` (scale to content, capped at the terminal) or
/// a fixed number of columns (also capped at the terminal) (spec: tui —
/// configurable TUI content width).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TuiWidth {
    Named(String),
    Fixed(u16),
}

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
    /// Humantime string (e.g. `"30m"`).
    #[serde(default = "default_stale", with = "humantime_serde")]
    pub stale_after: Duration,
    /// Default number of recent readings shown in a banded source's web UI
    /// history bar; overridable per source via `SourceCfg::history_points`.
    #[serde(default = "default_history_points")]
    pub history_points: u32,
    #[serde(default)]
    pub sources: Vec<SourceCfg>,
    #[serde(default)]
    pub layouts: Vec<LayoutCfg>,
    /// TUI content width — `"auto"` or a fixed column count.
    #[serde(default = "default_tui_width")]
    pub tui_width: TuiWidth,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCfg {
    pub name: String,
    /// Friendlier display label shown in place of `name` as a panel title or
    /// group row label, unless a cell/group entry overrides it explicitly.
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    /// Humantime string (e.g. `"5m"`, `"30s"`). Mutually exclusive with `cron`;
    /// defaults to `default_interval()` when neither is declared.
    #[serde(default, with = "humantime_serde::option")]
    pub interval: Option<Duration>,
    /// Cron expression (parsed with the `croner` crate; standard cron syntax
    /// with an optional leading seconds field, e.g. `"0 0 3 * * *"`).
    /// Mutually exclusive with `interval`.
    pub cron: Option<String>,
    /// Humantime string (e.g. `"30s"`).
    #[serde(default = "default_timeout", with = "humantime_serde")]
    pub timeout: Duration,
    pub unit: Option<String>,
    /// Optional shell command run once before this source's first fetch
    /// (start a service, open a tunnel). Failure defers fetching; retried on schedule.
    pub setup: Option<String>,
    /// How to render the value in the UI: `text` (default), `markdown`, or `json`.
    pub format: Option<String>,
    /// Optional coloring bands, e.g. `[{bound=60.0, level="green"}, {bound=85.0, level="yellow"}, {bound=100.0, level="red"}]`.
    #[serde(default)]
    pub thresholds: Vec<Threshold>,
    /// Overrides `Config::history_points` for this source's web UI history bar.
    pub history_points: Option<u32>,
    /// Whether this source's web UI history bar renders at all; defaults to
    /// `true`. Has no effect on a source with no threshold bands, which
    /// never renders a bar regardless (spec: source-configuration — per-source
    /// history bar visibility).
    pub show_history: Option<bool>,
    // http
    pub url: Option<String>,
    pub selector: Option<String>,
    // script
    pub command: Option<String>,
}

impl SourceCfg {
    #[must_use]
    pub fn display_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The interval to schedule this source on when it has no `cron`
    /// expression: its declared `interval`, or the default when neither is set.
    #[must_use]
    pub fn effective_interval(&self) -> Duration {
        self.interval.unwrap_or_else(default_interval)
    }
}

/// One member of a [`Cell::Group`]'s `ids` list. Untagged so a bare source-id
/// string is accepted (label defaults to the id) alongside `{ id, label }`
/// for an overridden label.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum GroupItem {
    Id(String),
    Labeled { id: String, label: String },
}

impl GroupItem {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            GroupItem::Id(id) | GroupItem::Labeled { id, .. } => id,
        }
    }

    /// The configured label override, if any — `None` for a bare id, whose
    /// caller falls back to the source's `title` or its own id.
    #[must_use]
    pub fn explicit_label(&self) -> Option<&str> {
        match self {
            GroupItem::Id(_) => None,
            GroupItem::Labeled { label, .. } => Some(label),
        }
    }
}

/// One grid cell in a layout row. Untagged so the TOML stays terse:
/// `"src"`, `{ id = "src", title = "Pane" }`, `{ kind = "space", colspan = 2 }`,
/// `{ title = "Pane", ids = ["a", { id = "b", label = "Balance" }] }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Source(String),
    Pane { id: String, title: Option<String> },
    Space { kind: String, colspan: Option<usize> },
    Group { title: String, ids: Vec<GroupItem> },
}

impl Cell {
    /// Every source this cell references: one for `Source`/`Pane`, one per
    /// member for `Group`, none for `Space`.
    #[must_use]
    pub fn source_names(&self) -> Vec<&str> {
        match self {
            Cell::Source(name) | Cell::Pane { id: name, .. } => vec![name],
            Cell::Group { ids, .. } => ids.iter().map(GroupItem::id).collect(),
            Cell::Space { .. } => vec![],
        }
    }

    #[must_use]
    pub fn span(&self) -> usize {
        match self {
            Cell::Space { colspan, .. } => colspan.unwrap_or(1),
            _ => 1,
        }
    }

    #[must_use]
    pub fn pane_title(&self) -> Option<&str> {
        match self {
            Cell::Pane { title, .. } => title.as_deref(),
            Cell::Group { title, .. } => Some(title),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutCfg {
    pub title: String,
    pub rows: Vec<Vec<Cell>>,
}

impl LayoutCfg {
    /// Column count = widest row (cells weighted by colspan).
    #[must_use]
    pub fn columns(&self) -> usize {
        self.rows
            .iter()
            .map(|row| row.iter().map(Cell::span).sum())
            .max()
            .unwrap_or(0)
    }

    #[must_use]
    pub fn source_names(&self) -> Vec<&str> {
        self.rows
            .iter()
            .flatten()
            .flat_map(Cell::source_names)
            .collect()
    }
}

fn default_db_path() -> PathBuf {
    PathBuf::from("dashboard.duckdb")
}
fn default_listen() -> String {
    "127.0.0.1:8420".into()
}
fn default_interval() -> Duration {
    Duration::from_mins(5)
}
fn default_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_threshold() -> u32 {
    3
}
fn default_stale() -> Duration {
    Duration::from_mins(30)
}
fn default_history_points() -> u32 {
    30
}
fn default_tui_width() -> TuiWidth {
    TuiWidth::Named("auto".into())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_path: default_db_path(),
            listen: default_listen(),
            interval: default_interval(),
            failure_threshold: default_threshold(),
            stale_after: default_stale(),
            history_points: default_history_points(),
            sources: Vec::new(),
            layouts: Vec::new(),
            tui_width: default_tui_width(),
        }
    }
}

pub fn load(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading config {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
    validate(&cfg)?;
    Ok(cfg)
}

pub fn validate(cfg: &Config) -> Result<()> {
    if cfg.history_points == 0 {
        bail!("history_points must be > 0");
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

fn validate_source(s: &SourceCfg) -> Result<()> {
    if s.name.is_empty() {
        bail!("source with empty name");
    }
    if !SOURCE_TYPES.contains(&s.kind.as_str()) {
        bail!(
            "source `{}` has unknown type `{}` (known types: {})",
            s.name,
            s.kind,
            SOURCE_TYPES.join(", ")
        );
    }
    if s.kind == "http" && s.url.as_deref().unwrap_or("").is_empty() {
        bail!("http source `{}` requires `url`", s.name);
    }
    if s.kind == "script" && s.command.as_deref().unwrap_or("").is_empty() {
        bail!("script source `{}` requires `command`", s.name);
    }
    if let Some(iv) = s.interval
        && iv.is_zero()
    {
        bail!("source `{}` interval must be > 0", s.name);
    }
    if s.interval.is_some() && s.cron.is_some() {
        bail!("source `{}` cannot declare both `interval` and `cron`", s.name);
    }
    if let Some(expr) = &s.cron {
        let cron: croner::Cron = expr
            .parse()
            .map_err(|e| anyhow::anyhow!("source `{}` has invalid cron expression `{expr}`: {e}", s.name))?;
        if cron.find_next_occurrence(&chrono::Utc::now(), true).is_err() {
            bail!("source `{}` cron expression `{expr}` has no future occurrence", s.name);
        }
    }
    if let Some(fmt) = &s.format {
        ValueFormat::parse(fmt)?;
    }
    for t in &s.thresholds {
        if !LEVELS.contains(&t.level.as_str()) {
            bail!(
                "source `{}` threshold level `{}` invalid (known levels: {})",
                s.name,
                t.level,
                LEVELS.join(", ")
            );
        }
        if !t.bound.is_finite() {
            bail!("source `{}` threshold bound must be finite", s.name);
        }
    }
    if s.thresholds.len() == 1 {
        bail!("source `{}` needs at least 2 thresholds to form bands", s.name);
    }
    if s.history_points == Some(0) {
        bail!("source `{}` history_points must be > 0", s.name);
    }
    Ok(())
}

fn validate_cell(cfg: &Config, layout_title: &str, row_idx: usize, cell: &Cell) -> Result<()> {
    match cell {
        Cell::Source(name) | Cell::Pane { id: name, .. } => {
            if !cfg.sources.iter().any(|s| &s.name == name) {
                bail!(
                    "layout `{}` row {} references unknown source `{}`",
                    layout_title,
                    row_idx + 1,
                    name
                );
            }
        }
        Cell::Space {
            colspan: Some(0), ..
        } => {
            bail!("layout `{}` row {} has a space with colspan 0", layout_title, row_idx + 1);
        }
        Cell::Space { .. } => {}
        Cell::Group { title, ids } => {
            if title.is_empty() {
                bail!("layout `{}` row {} has a group with an empty title", layout_title, row_idx + 1);
            }
            if ids.is_empty() {
                bail!("layout `{}` row {} has group `{}` with no ids", layout_title, row_idx + 1, title);
            }
            for item in ids {
                if !cfg.sources.iter().any(|s| s.name == item.id()) {
                    bail!(
                        "layout `{}` row {} group `{}` references unknown source `{}`",
                        layout_title,
                        row_idx + 1,
                        title,
                        item.id()
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn source_toml(extra: &str) -> String {
        format!(
            "[[sources]]\nname = \"cpu\"\ntype = \"script\"\ncommand = \"echo 0\"\n{extra}\n"
        )
    }

    #[test]
    fn invalid_tui_width_string_rejected() {
        let cfg: Config = toml::from_str("tui_width = \"wide\"").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("wide"), "error should name the invalid value: {err}");
    }

    #[test]
    fn zero_tui_width_rejected() {
        let cfg: Config = toml::from_str("tui_width = 0").unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("tui_width"), "error should name the field: {err}");
    }

    #[test]
    fn show_history_false_parses() {
        let cfg: Config = toml::from_str(&source_toml("show_history = false")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].show_history, Some(false));
    }

    #[test]
    fn humantime_duration_parses_and_applies() {
        let cfg: Config = toml::from_str(&source_toml("interval = \"5m\"\ntimeout = \"30s\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].interval, Some(Duration::from_mins(5)));
        assert_eq!(cfg.sources[0].timeout, Duration::from_secs(30));
    }

    #[test]
    fn invalid_duration_string_rejected() {
        let err = toml::from_str::<Config>(&source_toml("timeout = \"banana\"")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("banana"), "error should name the invalid value: {msg}");
        assert!(msg.contains("timeout"), "error should name the offending field: {msg}");
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
    fn invalid_cron_expression_rejected() {
        let cfg: Config = toml::from_str(&source_toml("cron = \"not a cron\"")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("cron"), "error should mention the invalid cron expression: {err}");
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
            title: "ihor".into(),
            ids: vec![GroupItem::Id("a".into())],
        };
        assert_eq!(cell.span(), 1);
        assert_eq!(cell.pane_title(), Some("ihor"));
    }

    #[test]
    fn accent_color_prioritizes_health_over_a_stale_band() {
        // Unhealthy overrides any band reading, banded or not.
        assert_eq!(accent_color(Some("green"), "failing"), Some("red"));
        assert_eq!(accent_color(Some("green"), "stale"), Some("yellow"));
        assert_eq!(accent_color(None, "failing"), Some("red"));
        assert_eq!(accent_color(None, "stale"), Some("yellow"));
        // Healthy: band color when present, else nothing to accent.
        assert_eq!(accent_color(Some("red"), "healthy"), Some("red"));
        assert_eq!(accent_color(None, "healthy"), None);
    }

    #[test]
    fn status_color_falls_back_to_green_when_accent_color_is_none() {
        assert_eq!(status_color(None, "healthy"), "green");
        assert_eq!(status_color(None, "failing"), "red");
    }

    #[test]
    fn worst_color_ranks_red_over_yellow_over_green() {
        assert_eq!(worst_color(["green", "yellow", "red"]), "red");
        assert_eq!(worst_color(["green", "yellow"]), "yellow");
        assert_eq!(worst_color(["green", "green"]), "green");
        assert_eq!(worst_color([]), "green");
    }

    #[test]
    fn group_item_explicit_label_none_for_bare_id() {
        assert_eq!(GroupItem::Id("vds-base1".into()).explicit_label(), None);
        assert_eq!(
            GroupItem::Labeled { id: "vds-base1".into(), label: "days left".into() }.explicit_label(),
            Some("days left")
        );
    }

    #[test]
    fn group_cell_source_names_lists_every_member() {
        let cell = Cell::Group {
            title: "ihor".into(),
            ids: vec![
                GroupItem::Id("a".into()),
                GroupItem::Labeled { id: "b".into(), label: "B".into() },
            ],
        };
        assert_eq!(cell.source_names(), vec!["a", "b"]);
    }

    fn group_layout_toml(group_extra: &str) -> String {
        format!(
            "{}\n[[layouts]]\ntitle = \"L\"\nrows = [[{{ title = \"ihor\", {group_extra} }}]]\n",
            source_toml("")
        )
    }

    #[test]
    fn valid_group_cell_accepted() {
        let cfg: Config = toml::from_str(&group_layout_toml("ids = [\"cpu\"]")).unwrap();
        validate(&cfg).unwrap();
    }

    #[test]
    fn group_cell_empty_title_rejected() {
        let toml = "[[sources]]\nname = \"cpu\"\ntype = \"script\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"\", ids = [\"cpu\"] }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("empty title"), "error should mention empty title: {err}");
    }

    #[test]
    fn group_cell_empty_ids_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("ids = []")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("no ids"), "error should mention the empty group: {err}");
    }

    #[test]
    fn group_cell_unknown_source_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("ids = [\"nope\"]")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("nope"), "error should name the unknown source: {err}");
    }
}
