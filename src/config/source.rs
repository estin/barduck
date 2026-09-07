//! Source declarations: what to fetch, how often, and where it may display
//! — plus the threshold-band/health color model shared by the web UI and TUI.

use super::{
    Config, ValueFormat,
    defaults::{default_interval, default_retry_interval},
};
use serde::Deserialize;
use std::time::Duration;

/// A source's collection mechanism (spec: source-configuration — `http`/
/// `script` source types). An unknown `type` value is rejected at
/// deserialization time, not by a separate runtime membership check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Http,
    Script,
}

impl SourceType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::Http => "http",
            SourceType::Script => "script",
        }
    }
}

/// The three threshold-band/accent-color levels, shared by config
/// validation, the web UI, and the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Green,
    Yellow,
    Red,
}

impl Level {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Green => "green",
            Level::Yellow => "yellow",
            Level::Red => "red",
        }
    }
}

/// Which UI(s) a source may display in (spec: source-configuration —
/// per-source view visibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    All,
    Tui,
    Web,
}

/// One coloring band for a source's numeric value.
#[derive(Debug, Clone, Deserialize)]
pub struct Threshold {
    /// Upper bound of this band (inclusive); the last band covers everything above.
    pub bound: f64,
    pub level: Level,
}

/// Resolves the color level for a reading against the source's thresholds.
/// Bands are interpreted ascending by bound ("value ≤ bound"); encoding the
/// levels green→red or red→green gives either direction.
/// Returns None for non-numeric readings.
#[must_use]
pub fn level_for(thresholds: &[Threshold], value: &str) -> Option<Level> {
    let v: f64 = value.trim().parse().ok()?;
    let mut bands: Vec<&Threshold> = thresholds.iter().collect();
    bands.sort_by(|a, b| a.bound.total_cmp(&b.bound));
    bands.iter().find(|t| v <= t.bound).or_else(|| bands.last()).map(|t| t.level)
}

/// Resolves the accent color for a source's aggregate/alerting signal (a
/// group's own border, the summary-strip chip) and for a healthy, banded
/// source's own value: health status takes priority whenever the source
/// isn't currently healthy (`failing`→red, `stale`→yellow) — even overriding
/// a threshold-band reading, since a stale or failing fetch means that
/// reading is no longer trustworthy. Only when healthy does the threshold
/// band level apply, falling back to green when there's no band either;
/// shared by the web UI and TUI (spec: web-ui — threshold band coloring;
/// tui — threshold band coloring).
#[must_use]
pub fn status_color(level: Option<Level>, status: crate::health::Health) -> Level {
    accent_color(level, status).unwrap_or(Level::Green)
}

/// Same priority as [`status_color`], but `None` when the source is
/// currently healthy and has no threshold bands: there's nothing meaningful
/// to accent, so a panel/row should render with no color at all rather than
/// a default green (spec: web-ui — health visible at a glance; tui —
/// threshold band coloring).
#[must_use]
pub fn accent_color(level: Option<Level>, status: crate::health::Health) -> Option<Level> {
    use crate::health::Health;
    match status {
        Health::Failing => Some(Level::Red),
        Health::Stale => Some(Level::Yellow),
        Health::Healthy => level,
    }
}

/// Worst of a set of colors (red > yellow > green), used to color a group
/// pane/panel by its worst member. Green for an empty set — unreachable for
/// a real group, since validation rejects an empty `ids` list.
#[must_use]
pub fn worst_color(colors: impl IntoIterator<Item = Level>) -> Level {
    let mut worst = Level::Green;
    for color in colors {
        if color == Level::Red {
            return Level::Red;
        }
        if color == Level::Yellow {
            worst = Level::Yellow;
        }
    }
    worst
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCfg {
    pub name: String,
    /// Friendlier display label shown in place of `name` as a panel title or
    /// group row label, unless a cell/group entry overrides it explicitly.
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub kind: SourceType,
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
    #[serde(default = "super::defaults::default_timeout", with = "humantime_serde")]
    pub timeout: Duration,
    pub unit: Option<String>,
    /// Optional shell command run once before this source's first fetch
    /// (start a service, open a tunnel). Failure defers fetching; retried on schedule.
    pub setup: Option<String>,
    /// How to render the value in the UI: `text` (default), `markdown`, or `json`.
    pub format: Option<ValueFormat>,
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
    /// Which UI(s) may display this source: `all` (default), `tui`, or
    /// `web`. Has no effect on data collection — the source is fetched on
    /// its schedule regardless (spec: source-configuration — per-source view
    /// visibility).
    pub show_in: Option<View>,
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

    /// Whether this source may display in `view` (`Tui` or `Web`), per
    /// `show_in`: `None`/`All` (default) means both views, otherwise only
    /// the named one (spec: source-configuration — per-source view
    /// visibility).
    #[must_use]
    pub fn visible_in(&self, view: View) -> bool {
        match self.show_in {
            None | Some(View::All) => true,
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
pub fn source_visible_in(cfg: &Config, name: &str, view: View) -> bool {
    cfg.sources.iter().find(|s| s.name == name).is_none_or(|s| s.visible_in(view))
}

/// One member of a [`super::Cell::Group`]'s `main`/`secondary`/`table`
/// sections. Untagged so a bare source-id string is accepted (label
/// defaults to the id) alongside `{ id, label }` for an overridden label.
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

/// Filters a generalized pane's `secondary`/`table` members down to those
/// visible in `view`, so a hidden member is simply omitted from that view's
/// rendering rather than failing startup (spec: tui / web-ui — hidden
/// sources render as space/empty).
#[must_use]
pub fn visible_items<'a>(cfg: &Config, items: &'a [GroupItem], view: View) -> Vec<&'a GroupItem> {
    items.iter().filter(|item| source_visible_in(cfg, item.id(), view)).collect()
}
