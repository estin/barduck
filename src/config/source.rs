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
}

impl SourceType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::Query => "query",
            SourceType::Stream => "stream",
            SourceType::Ingest => "ingest",
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
#[must_use]
pub fn level_for(thresholds: &[Threshold], value: &str) -> Option<Level> {
    let v: f64 = value.trim().parse().ok()?;
    let mut bands: Vec<&Threshold> = thresholds.iter().collect();
    bands.sort_by(|a, b| a.bound.total_cmp(&b.bound));
    bands
        .iter()
        .find(|t| v <= t.bound)
        .or_else(|| bands.last())
        .map(|t| t.level)
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
}

impl SourceCfg {
    #[must_use]
    pub fn kind(&self) -> SourceType {
        match self {
            SourceCfg::Query { .. } => SourceType::Query,
            SourceCfg::Stream { .. } => SourceType::Stream,
            SourceCfg::Ingest { .. } => SourceType::Ingest,
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
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            SourceCfg::Query { name, .. } | SourceCfg::Stream { name, .. } | SourceCfg::Ingest { name, .. } => name,
        }
    }

    #[must_use]
    pub fn display_title(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { title, .. } | SourceCfg::Stream { title, .. } | SourceCfg::Ingest { title, .. } => title.as_deref(),
        }
    }

    #[must_use]
    pub fn timeout(&self) -> Duration {
        match self {
            SourceCfg::Query { timeout, .. } | SourceCfg::Stream { timeout, .. } => *timeout,
            SourceCfg::Ingest { .. } => super::defaults::default_timeout(),
        }
    }

    #[must_use]
    pub fn unit(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { unit, .. } | SourceCfg::Stream { unit, .. } | SourceCfg::Ingest { unit, .. } => unit.as_deref(),
        }
    }

    #[must_use]
    pub fn setup(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { setup, .. } | SourceCfg::Stream { setup, .. } => setup.as_deref(),
            SourceCfg::Ingest { .. } => None,
        }
    }

    #[must_use]
    pub fn format(&self) -> Option<ValueFormat> {
        match self {
            SourceCfg::Query { format, .. } | SourceCfg::Stream { format, .. } | SourceCfg::Ingest { format, .. } => *format,
        }
    }

    #[must_use]
    pub fn thresholds(&self) -> &[Threshold] {
        match self {
            SourceCfg::Query { thresholds, .. } | SourceCfg::Stream { thresholds, .. } | SourceCfg::Ingest { thresholds, .. } => {
                thresholds
            }
        }
    }

    #[must_use]
    pub fn history_points(&self) -> Option<u32> {
        match self {
            SourceCfg::Query { history_points, .. } | SourceCfg::Stream { history_points, .. } | SourceCfg::Ingest { history_points, .. } => {
                *history_points
            }
        }
    }

    #[must_use]
    pub fn show_history(&self) -> Option<bool> {
        match self {
            SourceCfg::Query { show_history, .. } | SourceCfg::Stream { show_history, .. } | SourceCfg::Ingest { show_history, .. } => {
                *show_history
            }
        }
    }

    #[must_use]
    pub fn show_in(&self) -> Option<View> {
        match self {
            SourceCfg::Query { show_in, .. } | SourceCfg::Stream { show_in, .. } | SourceCfg::Ingest { show_in, .. } => *show_in,
        }
    }

    #[must_use]
    pub fn value_type(&self) -> Option<ValueType> {
        match self {
            SourceCfg::Query { value_type, .. } | SourceCfg::Stream { value_type, .. } | SourceCfg::Ingest { value_type, .. } => {
                *value_type
            }
        }
    }

    #[must_use]
    pub fn command(&self) -> &str {
        match self {
            SourceCfg::Query { command, .. } | SourceCfg::Stream { command, .. } => command,
            SourceCfg::Ingest { .. } => "",
        }
    }

    /// A query source's `cron` expression, if scheduled by cron. Always
    /// `None` for streams, which are continuous rather than scheduled.
    #[must_use]
    pub fn cron(&self) -> Option<&str> {
        match self {
            SourceCfg::Query { cron, .. } => cron.as_deref(),
            SourceCfg::Stream { .. } | SourceCfg::Ingest { .. } => None,
        }
    }

    /// A query source's declared `interval`, if scheduled by interval.
    /// Always `None` for streams.
    #[must_use]
    pub fn interval(&self) -> Option<Duration> {
        match self {
            SourceCfg::Query { interval, .. } => *interval,
            SourceCfg::Stream { .. } | SourceCfg::Ingest { .. } => None,
        }
    }

    #[must_use]
    pub fn retry_interval(&self) -> Option<Duration> {
        match self {
            SourceCfg::Query { retry_interval, .. } | SourceCfg::Stream { retry_interval, .. } => {
                *retry_interval
            }
            SourceCfg::Ingest { .. } => None,
        }
    }

    /// A stream source's maximum silence between values. Always `None` for
    /// queries, which are scheduled rather than continuous.
    #[must_use]
    pub fn expected_interval(&self) -> Option<Duration> {
        match self {
            SourceCfg::Query { .. } => None,
            SourceCfg::Stream {
                expected_interval, ..
            }
            | SourceCfg::Ingest { expected_interval, .. } => Some(*expected_interval),
        }
    }

    /// The interval to schedule a query source on when it has no `cron`
    /// expression: its declared `interval`, or the default when neither is set.
    /// For streams this is the staleness window, i.e. `expected_interval`.
    #[must_use]
    pub fn effective_interval(&self) -> Duration {
        match self {
            SourceCfg::Query { interval, .. } => interval.unwrap_or_else(default_interval),
            SourceCfg::Stream {
                expected_interval, ..
            }
            | SourceCfg::Ingest { expected_interval, .. } => *expected_interval,
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
}
