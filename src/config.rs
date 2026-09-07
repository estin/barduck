use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SOURCE_TYPES: &[&str] = &["http", "script"];
pub const VALUE_FORMATS: &[&str] = &["text", "markdown", "json"];
pub const LEVELS: &[&str] = &["green", "yellow", "red"];
pub const VIEWS: &[&str] = &["all", "tui", "web"];

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
    /// Humantime string (e.g. `"10s"`). How soon an interval-scheduled source
    /// retries after a failed fetch, instead of waiting the full `interval`;
    /// defaults to `default_retry_interval()` when not declared. Has no
    /// effect on a cron-scheduled source (mutually exclusive with `cron`) or
    /// on setup-command retries.
    #[serde(default, with = "humantime_serde::option")]
    pub retry_interval: Option<Duration>,
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
    /// Which UI(s) may display this source: `"all"` (default), `"tui"`, or
    /// `"web"`. Has no effect on data collection — the source is fetched on
    /// its schedule regardless (spec: source-configuration — per-source view
    /// visibility).
    pub show_in: Option<String>,
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

    /// How soon to retry after a failed fetch, when scheduled on `interval`
    /// rather than `cron`: its declared `retry_interval`, or the default when
    /// not set (spec: source-configuration — per-source fetch retry interval).
    #[must_use]
    pub fn effective_retry_interval(&self) -> Duration {
        self.retry_interval.unwrap_or_else(default_retry_interval)
    }

    /// Whether this source may display in `view` (`"tui"` or `"web"`), per
    /// `show_in`: `None`/`"all"` (default) means both views, otherwise only
    /// the named one (spec: source-configuration — per-source view
    /// visibility).
    #[must_use]
    pub fn visible_in(&self, view: &str) -> bool {
        match self.show_in.as_deref() {
            None | Some("all") => true,
            Some(v) => v == view,
        }
    }
}

/// Whether the source named `name` may display in `view`, or `true` when no
/// such source exists (config validation already guarantees every layout
/// reference resolves, so this only matters for callers not yet holding the
/// `SourceCfg` itself). Shared by the TUI and web renderers (spec:
/// source-configuration — per-source view visibility).
#[must_use]
pub fn source_visible_in(cfg: &Config, name: &str, view: &str) -> bool {
    cfg.sources.iter().find(|s| s.name == name).is_none_or(|s| s.visible_in(view))
}

/// Filters a generalized pane's `secondary`/`table` members down to those
/// visible in `view`, so a hidden member is simply omitted from that view's
/// rendering rather than failing startup (spec: tui / web-ui — hidden
/// sources render as space/empty).
#[must_use]
pub fn visible_items<'a>(cfg: &Config, items: &'a [GroupItem], view: &str) -> Vec<&'a GroupItem> {
    items.iter().filter(|item| source_visible_in(cfg, item.id(), view)).collect()
}

/// One member of a [`Cell::Group`]'s `main`/`secondary`/`table` sections.
/// Untagged so a bare source-id string is accepted (label defaults to the
/// id) alongside `{ id, label }` for an overridden label.
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
/// `{ title = "Pane", main = "a", secondary = ["b"], table = [{ id = "c", label = "C" }] }`.
/// A `Group` cell needs at least one of `main`/`secondary`/`table`; `title`
/// is itself optional too — a title-less pane falls back to `main`'s own
/// label when `main` is set, else renders with no header text (spec:
/// source-configuration — UI layouts are config-declared like sources).
///
/// Variant order matters for an untagged enum: serde tries each variant
/// top-to-bottom and stops at the first structural match. `Group`'s fields
/// are *all* optional, so it will happily match almost any table-shaped cell
/// that isn't `Pane`/`Space` (unknown fields are simply ignored — no variant
/// here uses `deny_unknown_fields`). Any variant added after `Group` needs a
/// required field of its own and must be placed *before* `Group` in this
/// enum, or a cell meant for that variant will silently become an empty,
/// then-rejected `Group` instead. `Text` is placed here for exactly that
/// reason: its `text` field is required, so a cell without one falls through
/// to `Group` exactly as before.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Source(String),
    Pane { id: String, title: Option<String> },
    Space { kind: String, colspan: Option<usize> },
    Text {
        #[serde(default)]
        title: Option<String>,
        format: Option<String>,
        text: String,
    },
    Group {
        #[serde(default)]
        title: Option<String>,
        main: Option<GroupItem>,
        #[serde(default)]
        secondary: Vec<GroupItem>,
        #[serde(default)]
        table: Vec<GroupItem>,
    },
}

impl Cell {
    /// Every source this cell references: one for `Source`/`Pane`, one per
    /// member for `Group` (across `main`, `secondary`, and `table`), none for
    /// `Space` or `Text` (a `Text` cell has no backing source at all).
    #[must_use]
    pub fn source_names(&self) -> Vec<&str> {
        match self {
            Cell::Source(name) | Cell::Pane { id: name, .. } => vec![name],
            Cell::Group { main, secondary, table, .. } => main
                .iter()
                .chain(secondary)
                .chain(table)
                .map(GroupItem::id)
                .collect(),
            Cell::Space { .. } | Cell::Text { .. } => vec![],
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
            Cell::Pane { title, .. } | Cell::Group { title, .. } | Cell::Text { title, .. } => {
                title.as_deref()
            }
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
fn default_retry_interval() -> Duration {
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
    let mut cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
    apply_env_overrides(&mut cfg)?;
    validate(&cfg)?;
    Ok(cfg)
}

/// Overrides top-level scalar settings from `BARDUCK_<FIELD>` environment
/// variables, taking precedence over both the config file's value and the
/// field's built-in default (spec: source-configuration — environment
/// variables override top-level settings). Structural fields (`sources`,
/// `layouts`) are not covered.
fn apply_env_overrides(cfg: &mut Config) -> Result<()> {
    apply_env_overrides_from(cfg, |name| std::env::var(name).ok())
}

/// Same as [`apply_env_overrides`], but reads variables through `lookup`
/// instead of the real process environment — lets tests exercise the
/// override logic without mutating global process state.
fn apply_env_overrides_from(cfg: &mut Config, lookup: impl Fn(&str) -> Option<String>) -> Result<()> {
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
    if let Some(iv) = s.retry_interval
        && iv.is_zero()
    {
        bail!("source `{}` retry_interval must be > 0", s.name);
    }
    if s.retry_interval.is_some() && s.cron.is_some() {
        bail!(
            "source `{}` cannot declare `retry_interval` with `cron` — retry_interval has no effect on a cron-scheduled source",
            s.name
        );
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
    if let Some(v) = &s.show_in
        && !VIEWS.contains(&v.as_str())
    {
        bail!(
            "source `{}` has invalid show_in `{}` (known values: {})",
            s.name,
            v,
            VIEWS.join(", ")
        );
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
        Cell::Text { text, format, .. } => {
            if text.is_empty() {
                bail!("layout `{}` row {} has a text panel with empty text", layout_title, row_idx + 1);
            }
            if let Some(fmt) = format {
                ValueFormat::parse(fmt)?;
            }
        }
        Cell::Group { title, main, secondary, table } => {
            if title.as_deref() == Some("") {
                bail!("layout `{}` row {} has a group with an empty title", layout_title, row_idx + 1);
            }
            let group_label = title.as_deref().unwrap_or("<untitled>");
            if main.is_none() && secondary.is_empty() && table.is_empty() {
                bail!(
                    "layout `{}` row {} has group `{}` with none of main/secondary/table",
                    layout_title,
                    row_idx + 1,
                    group_label
                );
            }
            for item in main.iter().chain(secondary).chain(table) {
                if !cfg.sources.iter().any(|s| s.name == item.id()) {
                    bail!(
                        "layout `{}` row {} group `{}` references unknown source `{}`",
                        layout_title,
                        row_idx + 1,
                        group_label,
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
    fn effective_retry_interval_falls_back_to_default_when_unset() {
        let cfg: Config = toml::from_str(&source_toml("")).unwrap();
        assert_eq!(cfg.sources[0].effective_retry_interval(), default_retry_interval());
    }

    #[test]
    fn effective_retry_interval_uses_declared_value() {
        let cfg: Config = toml::from_str(&source_toml("retry_interval = \"10s\"")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].effective_retry_interval(), Duration::from_secs(10));
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
        assert!(err.to_string().contains("retry_interval"), "error should name the field: {err}");
    }

    #[test]
    fn retry_interval_with_cron_rejected() {
        let cfg: Config =
            toml::from_str(&source_toml("cron = \"0 */5 * * * *\"\nretry_interval = \"10s\"")).unwrap();
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
        assert_eq!(cfg.sources[0].effective_retry_interval(), Duration::from_secs(10));
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
        assert!(cell.source_names().is_empty(), "a text cell has no backing source");
    }

    #[test]
    fn text_cell_without_title_has_no_pane_title() {
        let cell = Cell::Text { title: None, format: None, text: "note".into() };
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
        assert!(matches!(cell, Cell::Text { .. }), "expected Cell::Text, got a different variant: {cell:?}");
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
        assert!(err.to_string().contains("empty text"), "error should mention the empty text panel: {err}");
    }

    #[test]
    fn text_cell_invalid_format_rejected() {
        let toml = "[[layouts]]\ntitle = \"L\"\nrows = [[{ text = \"hi\", format = \"yaml\" }]]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("yaml"), "error should name the invalid format: {err}");
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
            title: Some("ihor".into()),
            main: Some(GroupItem::Id("a".into())),
            secondary: vec![GroupItem::Labeled { id: "b".into(), label: "B".into() }],
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
        assert!(err.to_string().contains("empty title"), "error should mention empty title: {err}");
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
        assert!(err.to_string().contains("nope"), "error should name the unknown source: {err}");
    }

    #[test]
    fn group_cell_unknown_source_in_secondary_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("secondary = [\"nope\"]")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("nope"), "error should name the unknown source: {err}");
    }

    #[test]
    fn group_cell_unknown_source_in_table_rejected() {
        let cfg: Config = toml::from_str(&group_layout_toml("table = [\"nope\"]")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("nope"), "error should name the unknown source: {err}");
    }

    fn lookup_from(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_string())
    }

    #[test]
    fn env_var_overrides_config_file_value() {
        let mut cfg: Config = toml::from_str("listen = \"127.0.0.1:8420\"").unwrap();
        apply_env_overrides_from(&mut cfg, lookup_from(&[("BARDUCK_LISTEN", "0.0.0.0:9000")])).unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:9000");
    }

    #[test]
    fn env_var_overrides_default() {
        let mut cfg = Config::default();
        apply_env_overrides_from(&mut cfg, lookup_from(&[("BARDUCK_HISTORY_POINTS", "100")])).unwrap();
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
        let err = apply_env_overrides_from(&mut cfg, lookup_from(&[("BARDUCK_STALE_AFTER", "not-a-duration")]))
            .unwrap_err();
        assert!(err.to_string().contains("BARDUCK_STALE_AFTER"), "error should name the variable: {err}");
    }

    #[test]
    fn show_in_defaults_to_visible_everywhere() {
        let cfg: Config = toml::from_str(&source_toml("")).unwrap();
        validate(&cfg).unwrap();
        assert_eq!(cfg.sources[0].show_in, None);
        assert!(cfg.sources[0].visible_in("tui"));
        assert!(cfg.sources[0].visible_in("web"));
    }

    #[test]
    fn show_in_restricts_to_one_view() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"tui\"")).unwrap();
        validate(&cfg).unwrap();
        assert!(cfg.sources[0].visible_in("tui"));
        assert!(!cfg.sources[0].visible_in("web"));
    }

    #[test]
    fn show_in_invalid_value_rejected() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"cli\"")).unwrap();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("cpu"), "error should name the source: {err}");
        assert!(err.to_string().contains("cli"), "error should name the invalid value: {err}");
    }

    #[test]
    fn source_visible_in_true_for_unknown_source() {
        // Validation already guarantees layout references resolve; a caller
        // without the SourceCfg in hand still gets a sensible default.
        let cfg = Config::default();
        assert!(source_visible_in(&cfg, "nope", "tui"));
    }

    #[test]
    fn source_visible_in_matches_source_show_in() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"web\"")).unwrap();
        assert!(!source_visible_in(&cfg, "cpu", "tui"));
        assert!(source_visible_in(&cfg, "cpu", "web"));
    }

    #[test]
    fn visible_items_omits_hidden_members() {
        let toml = "[[sources]]\nname = \"a\"\ntype = \"script\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[sources]]\nname = \"b\"\ntype = \"script\"\ncommand = \"echo 0\"\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let items = vec![GroupItem::Id("a".into()), GroupItem::Id("b".into())];
        let visible = visible_items(&cfg, &items, "tui");
        assert_eq!(visible.iter().map(|i| i.id()).collect::<Vec<_>>(), vec!["b"]);
    }

    #[test]
    fn visible_items_empty_when_all_members_hidden() {
        let cfg: Config = toml::from_str(&source_toml("show_in = \"web\"")).unwrap();
        let items = vec![GroupItem::Id("cpu".into())];
        assert!(visible_items(&cfg, &items, "tui").is_empty());
    }
}
