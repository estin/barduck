//! Source declarations: what to fetch, how often, and where it may display
//! — plus the threshold-band/health color model shared by the web UI and TUI.

use super::{
    Config, ValueFormat,
    defaults::{default_interval, default_retry_interval},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// `humantime_serde` wrappers that name the offending field: inside an
/// internally-tagged enum, serde buffers each source table before
/// dispatching on `type`, so the raw duration error would otherwise point
/// at the whole table instead of the field (spec: source-configuration —
/// Human-readable duration configuration).
fn de_interval<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    humantime_serde::option::deserialize(d)
        .map_err(|e| serde::de::Error::custom(format!("interval: {e}")))
}

fn de_retry_interval<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    humantime_serde::option::deserialize(d)
        .map_err(|e| serde::de::Error::custom(format!("retry_interval: {e}")))
}

fn de_timeout<'de, D>(d: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    humantime_serde::deserialize(d).map_err(|e| serde::de::Error::custom(format!("timeout: {e}")))
}

fn de_expected_interval<'de, D>(d: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    humantime_serde::deserialize(d)
        .map_err(|e| serde::de::Error::custom(format!("expected_interval: {e}")))
}

/// A source's collection mechanism (spec: source-configuration — `query`/
/// `stream` source types). An unknown `type` value (including the removed
/// `http` and `script`) is rejected at deserialization time, not by a
/// separate runtime membership check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Query,
    Stream,
    Ingest,
    /// A composite source's declared child (spec: source-configuration —
    /// Composite source children). Never written directly in config: it
    /// only ever exists as [`expand_composites`]'s own output.
    Child,
}

impl SourceType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::Query => "query",
            SourceType::Stream => "stream",
            SourceType::Ingest => "ingest",
            SourceType::Child => "child",
        }
    }
}

/// The three threshold-band/accent-color levels, shared by config
/// validation, the web UI, and the TUI. `Serialize` supports persisting
/// `jsonl`-supplied overrides (spec: source-configuration — JSONL row
/// schema).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
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

/// How a source's fetched value is stored (spec: source-configuration —
/// configurable stored value type): `string` (default) leaves storage
/// unchanged; `bigint`/`double`/`json` additionally populate the matching
/// typed column on `readings` alongside the existing string value.
/// Orthogonal to `format`, which only controls UI rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    #[default]
    String,
    Bigint,
    Double,
    Json,
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

/// One declared child of a composite `query` source (spec:
/// source-configuration — Composite source children): a bare name, unique
/// among its own siblings, plus the same per-value fields an ordinary source
/// declares. Deliberately excludes `command`, `interval`/`cron`, `timeout`,
/// `setup`, and `retry_interval` — those stay on the parent, so declaring one
/// here is a parse-time "unknown field" error via `deny_unknown_fields`
/// rather than a runtime check.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildDecl {
    pub name: String,
    pub title: Option<String>,
    pub unit: Option<String>,
    pub format: Option<ValueFormat>,
    #[serde(default)]
    pub thresholds: Vec<Threshold>,
    pub history_points: Option<u32>,
    pub show_history: Option<bool>,
    pub show_in: Option<View>,
    pub value_type: Option<ValueType>,
}

/// One coloring band for a source's numeric value. `Serialize` supports
/// persisting `jsonl`-supplied overrides.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Threshold {
    pub bound: f64,
    pub level: Level,
}

/// Validates one band list — declared or `jsonl`-supplied — with a single
/// rule set (spec: source-configuration — Threshold bands): a lone band
/// cannot color anything, so it is rejected naming the source.
pub fn validate_thresholds(source: &str, thresholds: &[Threshold]) -> Result<()> {
    if thresholds.len() == 1 {
        bail!("source `{source}` needs at least 2 thresholds to form bands");
    }
    Ok(())
}

/// Resolves the color level for a reading against the source's thresholds.
/// Bands are interpreted ascending by bound ("value ≤ bound"); encoding the
/// levels green→red or red→green gives either direction.
/// Returns None for non-numeric readings.
///
/// One allocation-free pass rather than sorting a borrowed copy: the answer
/// is just "the lowest bound at or above `v`, else the highest bound
/// overall", and this runs once per history-bar segment per panel on every
/// render (`history_points` × sources), where a per-call `Vec` + sort was
/// pure overhead.
#[must_use]
pub fn level_for(thresholds: &[Threshold], value: &str) -> Option<Level> {
    let v: f64 = value.trim().parse().ok()?;
    let mut covering: Option<&Threshold> = None;
    let mut highest: Option<&Threshold> = None;
    for t in thresholds {
        if v <= t.bound && covering.is_none_or(|c| t.bound < c.bound) {
            covering = Some(t);
        }
        // `>=`, so the *last*-declared band wins a tie for the highest
        // bound — matching what a stable sort + `last()` used to yield.
        if highest.is_none_or(|h| t.bound >= h.bound) {
            highest = Some(t);
        }
    }
    covering.or(highest).map(|t| t.level)
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

/// One source declaration, deserialized through an explicit per-type enum
/// (spec: source-configuration — Per-type source fields): each variant
/// carries only its own fields, so a cross-type field (`expected_interval`
/// on a query, `url` anywhere) fails at parse time naming the source and
/// field. An unknown `type` value (including the removed `http` and
/// `script`) is rejected the same way.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum SourceCfg {
    /// Oneshot shell command run per schedule tick (`interval` or `cron`).
    Query {
        name: String,
        /// Friendlier display label shown in place of `name` as a panel title or
        /// group row label, unless a cell/group entry overrides it explicitly.
        title: Option<String>,
        /// Humantime string (e.g. `"5m"`, `"30s"`). Mutually exclusive with `cron`;
        /// defaults to `default_interval()` when neither is declared.
        #[serde(default, deserialize_with = "de_interval")]
        interval: Option<Duration>,
        /// Cron expression (parsed with the `croner` crate; standard cron syntax
        /// with an optional leading seconds field, e.g. `"0 0 3 * * *"`).
        /// Mutually exclusive with `interval`.
        cron: Option<String>,
        /// Humantime string (e.g. `"10s"`). How soon an interval-scheduled query
        /// retries after a failed fetch, instead of waiting the full `interval`;
        /// defaults to `default_retry_interval()` when not declared. Has no
        /// effect on a cron-scheduled query (mutually exclusive with `cron`) or
        /// on setup-command retries.
        #[serde(default, deserialize_with = "de_retry_interval")]
        retry_interval: Option<Duration>,
        /// Humantime string (e.g. `"30s"`).
        #[serde(
            default = "super::defaults::default_timeout",
            deserialize_with = "de_timeout"
        )]
        timeout: Duration,
        unit: Option<String>,
        /// Optional shell command run once before this source's first fetch
        /// (start a service, open a tunnel). Failure defers fetching; retried on schedule.
        setup: Option<String>,
        /// How to render the value in the UI: `text` (default) or `markdown`.
        format: Option<ValueFormat>,
        /// Optional coloring bands, e.g. `[{bound=60.0, level="green"}, {bound=85.0, level="yellow"}, {bound=100.0, level="red"}]`.
        #[serde(default)]
        thresholds: Vec<Threshold>,
        /// Overrides `Config::history_points` for this source's web UI history bar.
        history_points: Option<u32>,
        /// Whether this source's web UI history bar renders at all; defaults to
        /// `true`. Has no effect on a source with no threshold bands, which
        /// never renders a bar regardless (spec: source-configuration — per-source
        /// history bar visibility).
        show_history: Option<bool>,
        /// Which UI(s) may display this source: `all` (default), `tui`, or
        /// `web`. Has no effect on data collection (spec: source-configuration —
        /// per-source view visibility).
        show_in: Option<View>,
        /// How to store the fetched value: `string` (default), `bigint`,
        /// `double`, or `json` (spec: source-configuration — configurable
        /// stored value type).
        value_type: Option<ValueType>,
        command: String,
        /// Declared children, forming a composite source (spec:
        /// source-configuration — Composite source children): when present
        /// and non-empty, this source's command output is a JSON array
        /// fanned out to these children instead of a single value.
        /// `Some(vec![])` (an explicitly empty list) is invalid — distinct
        /// from `None` (an ordinary, non-composite query source) precisely
        /// so an empty list can be rejected instead of silently behaving
        /// like no `children` field at all.
        #[serde(default)]
        children: Option<Vec<ChildDecl>>,
    },
    /// Long-running shell command whose stdout is a stream of `jsonl` rows,
    /// one JSON object per line, each producing one reading. No schedule:
    /// `expected_interval` bounds the silent window before the source
    /// reports stale, and `retry_interval` delays reopening after exit.
    Stream {
        name: String,
        title: Option<String>,
        /// Humantime string (e.g. `"30s"`); applies to the `setup` command
        /// only, never to the running stream itself.
        #[serde(
            default = "super::defaults::default_timeout",
            deserialize_with = "de_timeout"
        )]
        timeout: Duration,
        unit: Option<String>,
        setup: Option<String>,
        format: Option<ValueFormat>,
        #[serde(default)]
        thresholds: Vec<Threshold>,
        history_points: Option<u32>,
        show_history: Option<bool>,
        show_in: Option<View>,
        value_type: Option<ValueType>,
        command: String,
        /// Humantime string (e.g. `"1m"`). Required: the maximum silence
        /// between values before the source reports stale.
        #[serde(deserialize_with = "de_expected_interval")]
        expected_interval: Duration,
        /// Humantime string (e.g. `"10s"`). Delay before reopening the
        /// command after it ends; defaults to `default_retry_interval()`.
        #[serde(default, deserialize_with = "de_retry_interval")]
        retry_interval: Option<Duration>,
    },
    /// Push-based source that receives data via HTTP. No command;
    /// data arrives via `POST /api/ingest`. Staleness is governed
    /// by `expected_interval` (each push counts as a success).
    Ingest {
        name: String,
        title: Option<String>,
        /// Humantime string (e.g. `"1m"`). Required: the maximum
        /// silence between pushes before the source reports stale.
        #[serde(deserialize_with = "de_expected_interval")]
        expected_interval: Duration,
        unit: Option<String>,
        format: Option<ValueFormat>,
        #[serde(default)]
        thresholds: Vec<Threshold>,
        history_points: Option<u32>,
        show_history: Option<bool>,
        show_in: Option<View>,
        value_type: Option<ValueType>,
    },
    /// One composite source's expanded child (spec: source-configuration —
    /// Composite source children). Never deserialized from a real
    /// `[[sources]]` entry in practice — [`expand_composites`] is the only
    /// producer, and rejects any source that already arrived as this variant
    /// before it runs. `effective_interval`, `cron`, and `timeout` are
    /// denormalized copies of the parent's own schedule/timeout at expansion
    /// time, since a child declares none of its own — this keeps every
    /// accessor below a pure `match self`, needing no lookup into the rest
    /// of `Config::sources`.
    Child {
        name: String,
        parent: String,
        title: Option<String>,
        unit: Option<String>,
        format: Option<ValueFormat>,
        #[serde(default)]
        thresholds: Vec<Threshold>,
        history_points: Option<u32>,
        show_history: Option<bool>,
        show_in: Option<View>,
        value_type: Option<ValueType>,
        #[serde(skip, default = "super::defaults::default_interval")]
        effective_interval: Duration,
        #[serde(skip)]
        cron: Option<String>,
        #[serde(skip, default = "super::defaults::default_timeout")]
        timeout: Duration,
    },
}

impl SourceCfg {
    #[must_use]
    pub fn kind(&self) -> SourceType {
        match self {
            SourceCfg::Query { .. } => SourceType::Query,
            SourceCfg::Stream { .. } => SourceType::Stream,
            SourceCfg::Ingest { .. } => SourceType::Ingest,
            SourceCfg::Child { .. } => SourceType::Child,
        }
    }

    #[must_use]
    pub fn is_stream(&self) -> bool {
        matches!(self, SourceCfg::Stream { .. })
    }

    #[must_use]
    pub fn is_ingest(&self) -> bool {
        matches!(self, SourceCfg::Ingest { .. })
    }

    /// Whether this source is a composite root: a `query` source declaring
    /// one or more children (spec: source-configuration — Composite source
    /// children). Always `false` for a `Child` itself — nesting isn't
    /// supported.
    #[must_use]
    pub fn is_composite(&self) -> bool {
        matches!(self, SourceCfg::Query { children: Some(c), .. } if !c.is_empty())
    }

    /// Whether this source is one composite source's expanded child (spec:
    /// source-configuration — Composite source children).
    #[must_use]
    pub fn is_child(&self) -> bool {
        matches!(self, SourceCfg::Child { .. })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            SourceCfg::Query { name, .. }
            | SourceCfg::Stream { name, .. }
            | SourceCfg::Ingest { name, .. }
            | SourceCfg::Child { name, .. } => name,
        }
    }

    /// The full name of this child's composite root, or `None` for anything
    /// but a `Child` (spec: source-configuration — Composite source
    /// children).
    #[must_use]
    pub fn parent(&self) -> Option<&str> {
        match self {
            SourceCfg::Child { parent, .. } => Some(parent),
            _ => None,
        }
    }

    #[must_use]
    pub fn display_title(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { title, .. }
            | SourceCfg::Stream { title, .. }
            | SourceCfg::Ingest { title, .. }
            | SourceCfg::Child { title, .. } => title.as_deref(),
        }
    }

    /// A child's own `timeout` is denormalized from its parent at expansion
    /// time: a child has no command of its own, but this is still consulted
    /// to size a daemon-mode poll request's client timeout (spec: cli — Poll
    /// and fetch accept composite roots and children).
    #[must_use]
    pub fn timeout(&self) -> Duration {
        match self {
            SourceCfg::Query { timeout, .. }
            | SourceCfg::Stream { timeout, .. }
            | SourceCfg::Child { timeout, .. } => *timeout,
            SourceCfg::Ingest { .. } => super::defaults::default_timeout(),
        }
    }

    #[must_use]
    pub fn unit(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { unit, .. }
            | SourceCfg::Stream { unit, .. }
            | SourceCfg::Ingest { unit, .. }
            | SourceCfg::Child { unit, .. } => unit.as_deref(),
        }
    }

    #[must_use]
    pub fn setup(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { setup, .. } | SourceCfg::Stream { setup, .. } => setup.as_deref(),
            SourceCfg::Ingest { .. } | SourceCfg::Child { .. } => None,
        }
    }

    #[must_use]
    pub fn format(&self) -> Option<ValueFormat> {
        match self {
            SourceCfg::Query { format, .. }
            | SourceCfg::Stream { format, .. }
            | SourceCfg::Ingest { format, .. }
            | SourceCfg::Child { format, .. } => *format,
        }
    }

    #[must_use]
    pub fn thresholds(&self) -> &[Threshold] {
        match self {
            SourceCfg::Query { thresholds, .. }
            | SourceCfg::Stream { thresholds, .. }
            | SourceCfg::Ingest { thresholds, .. }
            | SourceCfg::Child { thresholds, .. } => thresholds,
        }
    }

    #[must_use]
    pub fn history_points(&self) -> Option<u32> {
        match self {
            SourceCfg::Query { history_points, .. }
            | SourceCfg::Stream { history_points, .. }
            | SourceCfg::Ingest { history_points, .. }
            | SourceCfg::Child { history_points, .. } => *history_points,
        }
    }

    #[must_use]
    pub fn show_history(&self) -> Option<bool> {
        match self {
            SourceCfg::Query { show_history, .. }
            | SourceCfg::Stream { show_history, .. }
            | SourceCfg::Ingest { show_history, .. }
            | SourceCfg::Child { show_history, .. } => *show_history,
        }
    }

    #[must_use]
    pub fn show_in(&self) -> Option<View> {
        match self {
            SourceCfg::Query { show_in, .. }
            | SourceCfg::Stream { show_in, .. }
            | SourceCfg::Ingest { show_in, .. }
            | SourceCfg::Child { show_in, .. } => *show_in,
        }
    }

    #[must_use]
    pub fn value_type(&self) -> Option<ValueType> {
        match self {
            SourceCfg::Query { value_type, .. }
            | SourceCfg::Stream { value_type, .. }
            | SourceCfg::Ingest { value_type, .. }
            | SourceCfg::Child { value_type, .. } => *value_type,
        }
    }

    #[must_use]
    pub fn command(&self) -> &str {
        match self {
            SourceCfg::Query { command, .. } | SourceCfg::Stream { command, .. } => command,
            SourceCfg::Ingest { .. } | SourceCfg::Child { .. } => "",
        }
    }

    /// A query source's `cron` expression, if scheduled by cron. Always
    /// `None` for streams, which are continuous rather than scheduled. A
    /// child returns its composite root's own `cron`, denormalized at
    /// expansion time (spec: source-configuration — Composite source
    /// children).
    #[must_use]
    pub fn cron(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { cron, .. } | SourceCfg::Child { cron, .. } => cron.as_deref(),
            SourceCfg::Stream { .. } | SourceCfg::Ingest { .. } => None,
        }
    }

    /// A query source's declared `interval`, if scheduled by interval.
    /// Always `None` for streams and children (a child has no schedule of
    /// its own to report — use [`SourceCfg::effective_interval`] instead).
    #[must_use]
    pub fn interval(&self) -> Option<Duration> {
        match self {
            SourceCfg::Query { interval, .. } => *interval,
            SourceCfg::Stream { .. } | SourceCfg::Ingest { .. } | SourceCfg::Child { .. } => None,
        }
    }

    #[must_use]
    pub fn retry_interval(&self) -> Option<Duration> {
        match self {
            SourceCfg::Query { retry_interval, .. } | SourceCfg::Stream { retry_interval, .. } => {
                *retry_interval
            }
            SourceCfg::Ingest { .. } | SourceCfg::Child { .. } => None,
        }
    }

    /// A stream source's maximum silence between values. Always `None` for
    /// queries and children, which are scheduled rather than continuous.
    #[must_use]
    pub fn expected_interval(&self) -> Option<Duration> {
        match self {
            SourceCfg::Query { .. } | SourceCfg::Child { .. } => None,
            SourceCfg::Stream {
                expected_interval, ..
            }
            | SourceCfg::Ingest { expected_interval, .. } => Some(*expected_interval),
        }
    }

    /// The interval to schedule a query source on when it has no `cron`
    /// expression: its declared `interval`, or the default when neither is set.
    /// For streams this is the staleness window, i.e. `expected_interval`. A
    /// child returns its composite root's own effective interval,
    /// denormalized at expansion time (spec: source-configuration —
    /// Composite source children).
    #[must_use]
    pub fn effective_interval(&self) -> Duration {
        match self {
            SourceCfg::Query { interval, .. } => interval.unwrap_or_else(default_interval),
            SourceCfg::Stream {
                expected_interval, ..
            }
            | SourceCfg::Ingest { expected_interval, .. } => *expected_interval,
            SourceCfg::Child {
                effective_interval, ..
            } => *effective_interval,
        }
    }

    /// How soon to retry after a failed fetch (query), or to reopen the
    /// command after it ends (stream): the declared `retry_interval`, or the
    /// default when not set (spec: source-configuration — per-source fetch
    /// retry interval).
    #[must_use]
    pub fn effective_retry_interval(&self) -> Duration {
        self.retry_interval().unwrap_or_else(default_retry_interval)
    }

    /// Whether this source may display in `view` (`Tui` or `Web`), per
    /// `show_in`: `None`/`All` (default) means both views, otherwise only
    /// the named one (spec: source-configuration — per-source view
    /// visibility).
    #[must_use]
    pub fn visible_in(&self, view: View) -> bool {
        match self.show_in() {
            None | Some(View::All) => true,
            Some(v) => v == view,
        }
    }

    /// The type to store this source's readings as: its declared
    /// `value_type`, or the default `string` when not set (spec:
    /// source-configuration — configurable stored value type).
    #[must_use]
    pub fn effective_value_type(&self) -> ValueType {
        self.value_type().unwrap_or_default()
    }
}

/// Every declared child of the composite source named `name`, in declaration
/// order, or empty when `name` isn't a composite source (spec: web-ui / tui —
/// a composite root renders as a table of its children; data-collection —
/// Force polling a composite source or its children).
#[must_use]
pub fn composite_children<'a>(cfg: &'a Config, name: &str) -> Vec<&'a SourceCfg> {
    cfg.sources
        .iter()
        .filter(|s| s.parent() == Some(name))
        .collect()
}

/// Expands every composite `query` source's declared `children` into
/// addressable [`SourceCfg::Child`] entries appended to `cfg.sources`, and
/// validates the composite shape (spec: source-configuration — Composite
/// source children; Composite root field restrictions). Must run before
/// [`super::validate`], whose per-source uniqueness and field checks then
/// cover the expanded entries for free — [`super::load_from`] does this;
/// tests exercising composite behavior without going through `load` must
/// call this explicitly first.
pub fn expand_composites(cfg: &mut Config) -> Result<()> {
    // `Child` only ever exists as this function's own output — a config
    // that already contains one arrived via a hand-authored `type = "child"`
    // entry, bypassing every check below.
    for s in &cfg.sources {
        if let SourceCfg::Child { name, .. } = s {
            bail!(
                "source `{name}`: `child` is not a valid source `type` (children are declared under a parent's `children` list)"
            );
        }
    }
    let mut expanded = Vec::new();
    for s in &cfg.sources {
        let SourceCfg::Query {
            name,
            children: Some(children),
            unit,
            thresholds,
            value_type,
            format,
            show_history,
            ..
        } = s
        else {
            continue;
        };
        if children.is_empty() {
            bail!("composite source `{name}` declares an empty `children` list");
        }
        if unit.is_some() {
            bail!("composite source `{name}` must not declare `unit`");
        }
        if !thresholds.is_empty() {
            bail!("composite source `{name}` must not declare `thresholds`");
        }
        if value_type.is_some() {
            bail!("composite source `{name}` must not declare `value_type`");
        }
        if format.is_some() {
            bail!("composite source `{name}` must not declare `format`");
        }
        if show_history.is_some() {
            bail!("composite source `{name}` must not declare `show_history`");
        }
        let mut seen = std::collections::HashSet::new();
        for c in children {
            if c.name.is_empty()
                || !c
                    .name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            {
                bail!(
                    "composite source `{name}` child `{}` name must contain only ASCII letters, digits, `_`, or `-`",
                    c.name
                );
            }
            if !seen.insert(c.name.clone()) {
                bail!("composite source `{name}` has duplicate child `{}`", c.name);
            }
            expanded.push(SourceCfg::Child {
                name: format!("{name}::{}", c.name),
                parent: name.clone(),
                title: c.title.clone(),
                unit: c.unit.clone(),
                format: c.format,
                thresholds: c.thresholds.clone(),
                history_points: c.history_points,
                show_history: c.show_history,
                show_in: c.show_in,
                value_type: c.value_type,
                effective_interval: s.effective_interval(),
                cron: s.cron().map(str::to_string),
                timeout: s.timeout(),
            });
        }
    }
    cfg.sources.extend(expanded);
    Ok(())
}

/// One structured output row: a single-line JSON object carrying a new
/// reading (spec: source-configuration — JSONL row schema). Built only by
/// [`parse_jsonl_row`], never deserialized directly: `threshold` is
/// deliberately lenient (an invalid band list is ignored with the reading
/// still recorded), which plain `Deserialize` cannot express.
#[derive(Debug, Clone)]
pub struct JsonlRow {
    /// The new reading.
    pub value: String,
    /// The row's timestamp when usable; `None` means store arrival time.
    pub ts: Option<JsonlTs>,
    /// Bands replacing the source's thresholds, when present and valid.
    pub threshold: Option<Vec<Threshold>>,
}

/// A row timestamp: epoch seconds as a number, or an RFC 3339 string.
#[derive(Debug, Clone)]
pub enum JsonlTs {
    Epoch(f64),
    Text(String),
}

impl JsonlTs {
    /// Resolves to `(ts_epoch, ts)`, falling back to `fallback` (the
    /// arrival time) for unparseable input. Epoch numbers always convert
    /// unless out of range; text must be RFC 3339.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    pub fn resolve(&self, fallback: (f64, String)) -> (f64, String) {
        match self {
            JsonlTs::Epoch(secs) => {
                // `floor`, not `trunc`: for a pre-epoch fractional value
                // (e.g. `-1.5`), `trunc` rounds toward zero (`-1.0`) and
                // `.abs()` on the remainder then flips it positive, landing
                // exactly one second late. Flooring keeps `secs - whole` in
                // `[0, 1)` for either sign, already the correct positive
                // nanosecond offset.
                let whole = secs.floor();
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let nanos = ((secs - whole) * 1_000_000_000.0).round() as u32;
                #[allow(clippy::cast_possible_truncation)]
                let dt = chrono::DateTime::from_timestamp(whole as i64, nanos);
                dt.map_or(fallback, |d| {
                    (d.timestamp_millis() as f64 / 1000.0, d.to_rfc3339())
                })
            }
            JsonlTs::Text(s) => chrono::DateTime::parse_from_rfc3339(s).map_or(fallback, |d| {
                (d.timestamp_millis() as f64 / 1000.0, d.to_rfc3339())
            }),
        }
    }
}

/// Parses one `jsonl` candidate object into a [`JsonlRow`]. Fails (no
/// reading recorded) on an unknown field or a missing/non-string `value`;
/// a present-but-invalid `threshold` is degraded to `None` instead — the
/// reading is still recorded with the source's previous bands (spec:
/// source-configuration — JSONL row schema). A present-but-unusable `ts`
/// degrades to `None` (arrival time stored) the same way.
pub fn parse_jsonl_row(
    source: &str,
    mut obj: serde_json::Map<String, serde_json::Value>,
) -> Result<JsonlRow> {
    for key in obj.keys() {
        if key != "value" && key != "ts" && key != "threshold" {
            bail!("source `{source}` jsonl row has unknown field `{key}`");
        }
    }
    let Some(serde_json::Value::String(value)) = obj.remove("value") else {
        bail!("source `{source}` jsonl row is missing a string `value`");
    };
    let ts = match obj.remove("ts") {
        Some(serde_json::Value::Number(n)) => n.as_f64().map(JsonlTs::Epoch),
        Some(serde_json::Value::String(s)) => Some(JsonlTs::Text(s)),
        _ => None,
    };
    let threshold = match obj.remove("threshold") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => convert_row_thresholds(source, v),
    };
    Ok(JsonlRow {
        value,
        ts,
        threshold,
    })
}

/// Converts a row's raw `threshold` JSON into validated bands, returning
/// `None` (bands unchanged) for anything unusable: wrong shape, unknown
/// level, non-finite bound, or a lone band.
fn convert_row_thresholds(source: &str, v: serde_json::Value) -> Option<Vec<Threshold>> {
    let bands: Vec<Threshold> = serde_json::from_value(v).ok()?;
    if bands.iter().any(|t| !t.bound.is_finite()) {
        return None;
    }
    validate_thresholds(source, &bands).ok()?;
    Some(bands)
}

/// Whether the source named `name` may display in `view`, or `true` when no
/// such source exists (config validation already guarantees every layout
/// reference resolves, so this only matters for callers not yet holding the
/// `SourceCfg` itself). Shared by the TUI and web renderers (spec:
/// source-configuration — per-source view visibility).
#[must_use]
pub fn source_visible_in(cfg: &Config, name: &str, view: View) -> bool {
    cfg.sources
        .iter()
        .find(|s| s.name() == name)
        .is_none_or(|s| s.visible_in(view))
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
    items
        .iter()
        .filter(|item| source_visible_in(cfg, item.id(), view))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn row(text: &str) -> serde_json::Map<String, serde_json::Value> {
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        v.as_object().cloned().unwrap()
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn full_row_parses_value_ts_and_thresholds() {
        let r = parse_jsonl_row(
            "s",
            row(r#"{"value":"ok","ts":"2026-09-09T12:00:00Z","threshold":[{"bound":1.0,"level":"green"},{"bound":2.0,"level":"red"}]}"#),
        )
        .unwrap();
        assert_eq!(r.value, "ok");
        let (epoch, ts) = r.ts.unwrap().resolve((0.0, "fallback".into()));
        assert!(
            epoch > 1_780_000_000.0,
            "epoch should be Sep 2026, got {epoch}"
        );
        assert!(ts.contains("2026-09-09"), "unexpected ts: {ts}");
        assert_eq!(r.threshold.unwrap().len(), 2);
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn minimal_row_has_no_ts_or_thresholds() {
        let r = parse_jsonl_row("s", row(r#"{"value":"ok"}"#)).unwrap();
        assert_eq!(r.value, "ok");
        assert!(r.ts.is_none());
        assert!(r.threshold.is_none());
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn unknown_field_fails_the_row() {
        let err = parse_jsonl_row("s", row(r#"{"value":"ok","color":"blue"}"#)).unwrap_err();
        assert!(
            err.to_string().contains("color"),
            "error should name the field: {err}"
        );
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn missing_or_non_string_value_fails_the_row() {
        parse_jsonl_row("s", row(r#"{"ts":"2026-09-09T12:00:00Z"}"#)).unwrap_err();
        parse_jsonl_row("s", row(r#"{"value":42}"#)).unwrap_err();
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn invalid_threshold_degrades_to_none() {
        // Unknown level: reading still recorded, bands unchanged.
        let r = parse_jsonl_row(
            "s",
            row(r#"{"value":"ok","threshold":[{"bound":1.0,"level":"blue"},{"bound":2.0,"level":"red"}]}"#),
        )
        .unwrap();
        assert!(r.threshold.is_none());
        // Lone band: same treatment.
        let r = parse_jsonl_row(
            "s",
            row(r#"{"value":"ok","threshold":[{"bound":1.0,"level":"red"}]}"#),
        )
        .unwrap();
        assert!(r.threshold.is_none());
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn unusable_ts_degrades_to_none() {
        let r = parse_jsonl_row("s", row(r#"{"value":"ok","ts":true}"#)).unwrap();
        assert!(r.ts.is_none());
    }

    /// (spec: source-configuration — JSONL row schema)
    #[test]
    fn epoch_ts_resolves_to_utc() {
        let r = parse_jsonl_row("s", row(r#"{"value":"ok","ts":1000.5}"#)).unwrap();
        let (epoch, ts) = r.ts.unwrap().resolve((0.0, "fallback".into()));
        assert!((epoch - 1000.5).abs() < 0.001, "got {epoch}");
        assert!(ts.contains("1970-01-01"), "unexpected ts: {ts}");
    }

    /// A pre-epoch fractional `ts` (half a second before 1969-12-31T23:59:59Z)
    /// must resolve to that exact instant, not one second off (spec:
    /// source-configuration — JSONL row schema).
    #[test]
    fn pre_epoch_fractional_ts_resolves_correctly() {
        let r = parse_jsonl_row("s", row(r#"{"value":"ok","ts":-1.5}"#)).unwrap();
        let (epoch, ts) = r.ts.unwrap().resolve((0.0, "fallback".into()));
        assert!((epoch - (-1.5)).abs() < 0.001, "got {epoch}");
        assert!(
            ts.starts_with("1969-12-31T23:59:58.5"),
            "expected 23:59:58.5, got {ts}"
        );
    }

    /// Bands are resolved by bound, not by declaration order, in either
    /// encoding direction; a value above every bound falls to the highest
    /// band and a non-numeric reading has no band at all (spec:
    /// source-configuration — Threshold bands).
    #[test]
    fn level_for_resolves_bands_independently_of_declaration_order() {
        let ascending = [
            Threshold {
                bound: 60.0,
                level: Level::Green,
            },
            Threshold {
                bound: 85.0,
                level: Level::Yellow,
            },
            Threshold {
                bound: 100.0,
                level: Level::Red,
            },
        ];
        let mut shuffled = ascending.to_vec();
        shuffled.reverse();
        for bands in [&ascending[..], &shuffled[..]] {
            assert_eq!(level_for(bands, "10"), Some(Level::Green));
            assert_eq!(level_for(bands, "60"), Some(Level::Green), "boundary is ≤");
            assert_eq!(level_for(bands, "60.5"), Some(Level::Yellow));
            assert_eq!(level_for(bands, "100"), Some(Level::Red));
            assert_eq!(
                level_for(bands, "999"),
                Some(Level::Red),
                "above every bound falls to the highest band"
            );
            assert_eq!(level_for(bands, "  42  "), Some(Level::Green));
            assert_eq!(level_for(bands, "n/a"), None);
        }
        assert_eq!(level_for(&[], "1"), None);
    }

    /// (spec: source-configuration — Ingest source type)
    #[test]
    fn ingest_source_parses() {
        let s: SourceCfg = toml::from_str(
            r#"name = "webhook"
type = "ingest"
expected_interval = "1m""#,
        )
        .unwrap();
        assert!(matches!(s, SourceCfg::Ingest { .. }));
        assert_eq!(s.name(), "webhook");
        assert_eq!(s.expected_interval(), Some(Duration::from_mins(1)));
    }

    /// (spec: source-configuration — Ingest source type)
    #[test]
    fn ingest_source_missing_expected_interval_fails() {
        let err = toml::from_str::<SourceCfg>(
            r#"name = "webhook"
type = "ingest""#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("expected_interval"));
    }

    fn composite_cfg(root_extra: &str, children: &str) -> Config {
        toml::from_str(&format!(
            "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"cat\"\ninterval = \"10s\"\n{root_extra}\n{children}"
        ))
        .unwrap()
    }

    fn child_toml(name: &str, extra: &str) -> String {
        format!("[[sources.children]]\nname = \"{name}\"\n{extra}\n")
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_expands_children_into_addressable_sources() {
        let mut cfg = composite_cfg(
            "",
            &(child_toml("1m", "") + &child_toml("5m", "") + &child_toml("15m", "")),
        );
        expand_composites(&mut cfg).unwrap();
        assert_eq!(cfg.sources.len(), 4);
        for full in ["load::1m", "load::5m", "load::15m"] {
            let child = cfg.sources.iter().find(|s| s.name() == full).unwrap();
            assert!(child.is_child());
            assert_eq!(child.parent(), Some("load"));
        }
        assert!(cfg.sources[0].is_composite());
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_child_declaring_command_is_rejected() {
        let err = toml::from_str::<Config>(&format!(
            "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"cat\"\ninterval = \"10s\"\n{}",
            child_toml("1m", "command = \"echo 0\"")
        ))
        .unwrap_err();
        assert!(err.to_string().contains("command"), "{err}");
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_child_declaring_schedule_field_is_rejected() {
        for field in ["interval = \"5s\"", "timeout = \"5s\"", "setup = \"true\"", "retry_interval = \"5s\""] {
            let err = toml::from_str::<Config>(&format!(
                "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"cat\"\ninterval = \"10s\"\n{}",
                child_toml("1m", field)
            ))
            .unwrap_err();
            assert!(err.to_string().contains(field.split(' ').next().unwrap()), "{err}");
        }
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_duplicate_full_name_is_rejected() {
        let mut cfg: Config = toml::from_str(&format!(
            "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"cat\"\ninterval = \"10s\"\n{}\n[[sources]]\nname = \"load::1m\"\ntype = \"query\"\ncommand = \"echo 0\"\ninterval = \"10s\"\n",
            child_toml("1m", "")
        ))
        .unwrap();
        expand_composites(&mut cfg).unwrap();
        let err = super::super::validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("load::1m"), "{err}");
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_child_name_with_separator_is_rejected() {
        let mut cfg = composite_cfg("", &child_toml("1m::x", ""));
        let err = expand_composites(&mut cfg).unwrap_err();
        assert!(err.to_string().contains("1m::x"), "{err}");
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_children_on_stream_is_rejected() {
        let err = toml::from_str::<Config>(&format!(
            "[[sources]]\nname = \"s\"\ntype = \"stream\"\ncommand = \"cat\"\nexpected_interval = \"1m\"\n{}",
            child_toml("1m", "")
        ))
        .unwrap_err();
        assert!(err.to_string().contains("children"), "{err}");
    }

    /// (spec: source-configuration — Composite source children)
    #[test]
    fn composite_empty_children_list_is_rejected() {
        let mut cfg: Config = toml::from_str(
            "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"cat\"\ninterval = \"10s\"\nchildren = []\n",
        )
        .unwrap();
        let err = expand_composites(&mut cfg).unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
    }

    /// (spec: source-configuration — Composite root field restrictions)
    #[test]
    fn composite_root_declaring_per_value_field_is_rejected() {
        for extra in [
            "unit = \"x\"",
            "thresholds = [{bound = 1.0, level = \"green\"}, {bound = 2.0, level = \"red\"}]",
            "value_type = \"double\"",
            "format = \"markdown\"",
            "show_history = false",
        ] {
            let mut cfg = composite_cfg(extra, &child_toml("1m", ""));
            let err = expand_composites(&mut cfg).unwrap_err();
            assert!(err.to_string().contains("must not declare"), "{err}");
        }
    }

    /// (spec: source-configuration — Composite root field restrictions)
    #[test]
    fn composite_root_operational_fields_remain_valid() {
        let mut cfg = composite_cfg(
            "title = \"Load\"\nshow_in = \"tui\"\ntimeout = \"5s\"",
            &child_toml("1m", ""),
        );
        expand_composites(&mut cfg).unwrap();
        super::super::validate(&cfg).unwrap();
    }

    /// A hand-authored `type = "child"` entry bypasses every composite
    /// check `expand_composites` runs, so it must never be accepted as a
    /// real source declaration (spec: source-configuration — Composite
    /// source children).
    #[test]
    fn explicit_child_type_is_rejected() {
        let mut cfg: Config =
            toml::from_str("[[sources]]\nname = \"x\"\ntype = \"child\"\nparent = \"load\"\n")
                .unwrap();
        let err = expand_composites(&mut cfg).unwrap_err();
        assert!(err.to_string().contains("child"), "{err}");
    }

    /// A child has no schedule of its own — it reports its composite root's
    /// (spec: source-configuration — Composite source children).
    #[test]
    fn composite_children_denormalize_parent_schedule_and_timeout() {
        let mut cfg = composite_cfg("timeout = \"7s\"", &child_toml("1m", ""));
        expand_composites(&mut cfg).unwrap();
        let child = cfg.sources.iter().find(|s| s.name() == "load::1m").unwrap();
        assert_eq!(child.effective_interval(), Duration::from_secs(10));
        assert_eq!(child.timeout(), Duration::from_secs(7));
        assert_eq!(child.cron(), None);
        assert_eq!(child.interval(), None, "a child reports no schedule of its own");

        let mut cron_cfg: Config = toml::from_str(&format!(
            "[[sources]]\nname = \"backup\"\ntype = \"query\"\ncommand = \"cat\"\ncron = \"0 0 3 * * *\"\n{}",
            child_toml("a", "")
        ))
        .unwrap();
        expand_composites(&mut cron_cfg).unwrap();
        let child = cron_cfg
            .sources
            .iter()
            .find(|s| s.name() == "backup::a")
            .unwrap();
        assert_eq!(child.cron(), Some("0 0 3 * * *"));
    }
}
