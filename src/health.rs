#![allow(clippy::cast_precision_loss)]

use crate::{
    config::{Config, SourceCfg},
    db::Db,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Healthy,
    Failing,
    Stale,
}

impl Health {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Failing => "failing",
            Health::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealth {
    pub source: String,
    pub status: Health,
    pub consecutive_failures: u32,
    pub last_success_ts: Option<String>,
    /// Effective threshold bands: the latest `jsonl` override when one was
    /// stored, otherwise the source's declared bands (spec:
    /// source-configuration — JSONL row schema). Renderers color with
    /// these so overrides apply everywhere without extra plumbing.
    #[serde(default)]
    pub thresholds: Vec<crate::config::Threshold>,
}

/// Derives live health for one source from its recent fetch logs and the age
/// of its last success (spec: data-collection — consecutive failures flip to
/// failing, old successes go stale). Two bounded queries: the last
/// `failure_threshold` logs (for the consecutive-failure count) and the
/// single most recent successful fetch (for staleness) — neither scans the
/// source's full log history.
pub async fn compute(db: &Db, cfg: &Config, source: &str) -> Result<SourceHealth> {
    let logs = db
        .logs(Some(source), i64::from(cfg.failure_threshold))
        .await?;
    #[allow(clippy::cast_possible_truncation)] // counts are small
    let failures = logs.iter().take_while(|l| l.error.is_some()).count() as u32;

    let last_success = db.last_success(source).await?;
    let last_success_ts = last_success.as_ref().map(|l| l.ts.clone());

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs_f64();
    let last_ok_age = last_success.map_or(f64::INFINITY, |l| now - l.ts_epoch);
    let source_cfg = cfg.sources.iter().find(|s| s.name() == source);

    let status = if failures >= cfg.failure_threshold {
        Health::Failing
    } else if is_stale(last_ok_age, source_cfg, cfg.interval) {
        Health::Stale
    } else {
        Health::Healthy
    };

    let declared: &[crate::config::Threshold] = source_cfg.map_or(&[], |s| s.thresholds());
    // Session-only overrides (spec: source-configuration — Threshold
    // bands): the daemon-shared map wins; anything else (direct mode,
    // fresh process) colors with declared bands.
    let thresholds = db
        .session_bands(source)
        .unwrap_or_else(|| declared.to_vec());

    Ok(SourceHealth {
        source: source.to_string(),
        status,
        consecutive_failures: failures,
        last_success_ts,
        thresholds,
    })
}

/// Staleness derived from the source's own schedule, not a global window
/// (spec: data-collection — Health status derived from fetch outcomes): a
/// cron source has no fixed period to derive a window from, so it's stale
/// only until its first success (`last_ok_age` infinite); an interval source
/// is stale once it has missed its second expected call; a stream source is
/// stale whenever no value has arrived within its `expected_interval`
/// (each ingested line counts as a success, so `last_ok_age` measures
/// silence). `fallback_interval` covers a source name not found in
/// `cfg.sources` (shouldn't happen for a validated config, but callers pass
/// one regardless).
fn is_stale(last_ok_age: f64, source: Option<&SourceCfg>, fallback_interval: Duration) -> bool {
    let interval = match source {
        Some(s) if s.cron().is_some() => return last_ok_age.is_infinite(),
        Some(s) if s.is_stream() || s.is_ingest() => {
            // No doubling: streams and ingest sources emit continuously, so any silence past
            // one full expected interval already means missed values.
            return last_ok_age > s.effective_interval().as_secs_f64();
        }
        Some(s) => s.effective_interval(),
        None => fallback_interval,
    };
    last_ok_age > 2.0 * interval.as_secs_f64()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn source(name: &str, interval: Option<Duration>, cron: Option<&str>) -> SourceCfg {
        toml::from_str(&format!(
            "name = \"{name}\"\ntype = \"query\"\ncommand = \"echo 0\"\n{}{}",
            interval
                .map(|d| format!("interval = \"{}\"\n", humantime::format_duration(d)))
                .unwrap_or_default(),
            cron.map(|c| format!("cron = \"{c}\"\n"))
                .unwrap_or_default(),
        ))
        .unwrap()
    }

    fn ingest_source(name: &str, expected_interval: Duration) -> SourceCfg {
        let q = char::from(34);
        let s = format!(
            "name = {q}{0}{q}\ntype = {q}ingest{q}\nexpected_interval = {q}{1}{q}",
            name,
            humantime::format_duration(expected_interval),
        );
        toml::from_str(&s).unwrap()
    }

    #[test]
    fn ingest_source_within_window_is_not_stale() {
        let s = ingest_source("webhook", Duration::from_mins(1));
        assert!(!is_stale(30.0, Some(&s), Duration::from_mins(5)));

    }
    #[test]
    fn ingest_source_past_expected_interval_is_stale() {
        let s = ingest_source("webhook", Duration::from_mins(1));
        assert!(is_stale(90.0 * 60.0, Some(&s), Duration::from_mins(5)));

    }
    #[test]
    fn ingest_source_at_boundary_is_not_stale() {
        let s = ingest_source("webhook", Duration::from_mins(1));
        assert!(!is_stale(59.0, Some(&s), Duration::from_mins(5)));
    }

    #[test]
    fn interval_source_within_window_is_not_stale() {
        let s = source("cpu", Some(Duration::from_mins(5)), None);
        assert!(!is_stale(6.0 * 60.0, Some(&s), Duration::from_mins(5)));
    }

    #[test]
    fn interval_source_past_second_missed_call_is_stale() {
        let s = source("cpu", Some(Duration::from_mins(5)), None);
        assert!(is_stale(11.0 * 60.0, Some(&s), Duration::from_mins(5)));
    }

    #[test]
    fn cron_source_with_no_success_yet_is_stale() {
        let s = source("backup", None, Some("0 0 3 * * *"));
        assert!(is_stale(f64::INFINITY, Some(&s), Duration::from_mins(5)));
    }

    #[test]
    fn cron_source_with_an_old_success_is_not_stale() {
        let s = source("backup", None, Some("0 0 3 * * *"));
        assert!(!is_stale(
            365.0 * 24.0 * 60.0 * 60.0,
            Some(&s),
            Duration::from_mins(5)
        ));
    }

    #[test]
    fn unknown_source_falls_back_to_the_given_interval() {
        assert!(is_stale(11.0 * 60.0, None, Duration::from_mins(5)));
        assert!(!is_stale(6.0 * 60.0, None, Duration::from_mins(5)));
    }

    fn stream(name: &str) -> SourceCfg {
        toml::from_str(&format!(
            "name = \"{name}\"\ntype = \"stream\"\ncommand = \"tail -f /dev/null\"\nexpected_interval = \"1m\"\n"
        ))
        .unwrap()
    }

    /// (spec: data-collection — Health status derived from fetch outcomes)
    #[test]
    fn stream_silence_past_expected_interval_is_stale() {
        let s = stream("ticks");
        assert!(is_stale(61.0, Some(&s), Duration::from_mins(5)));
    }

    /// (spec: data-collection — Health status derived from fetch outcomes)
    #[test]
    fn stream_value_within_expected_interval_is_not_stale() {
        let s = stream("ticks");
        assert!(!is_stale(30.0, Some(&s), Duration::from_mins(5)));
    }

    /// Session overrides flow into the computed health payload, so every
    /// renderer colors with them without extra plumbing (spec:
    /// source-configuration — Threshold bands).
    #[tokio::test]
    async fn compute_uses_session_bands_over_declared() {
        use crate::config::{Level, Threshold};
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg: Config = toml::from_str(
            "[[sources]]\nname = \"s\"\ntype = \"query\"\ncommand = \"echo 1\"\nthresholds = [{bound = 1.0, level = \"green\"}, {bound = 2.0, level = \"red\"}]\n",
        )
        .unwrap();
        let h = compute(&db, &cfg, "s").await.unwrap();
        assert_eq!(h.thresholds.len(), 2);
        assert_eq!(h.thresholds[0].level, Level::Green);
        db.set_session_bands(
            "s",
            &[Threshold {
                bound: 5.0,
                level: Level::Yellow,
            }],
        );
        let h = compute(&db, &cfg, "s").await.unwrap();
        assert_eq!(h.thresholds.len(), 1);
        assert_eq!(h.thresholds[0].level, Level::Yellow);
    }
}
