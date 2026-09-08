use crate::{
    config::{Config, VERSION},
    db::ReadingRow,
    query::Backend,
};
use anyhow::Result;

/// Client-side `--source` filter; applies to both backends uniformly.
fn filter_named<T: Named>(rows: Vec<T>, sources: &[String]) -> Vec<T> {
    if sources.is_empty() {
        rows
    } else {
        rows.into_iter()
            .filter(|r| sources.iter().any(|s| s == r.name()))
            .collect()
    }
}

/// A markdown-format source's value is prose (links, lists, headings), not a
/// short scalar — cramming it into `latest`'s fixed-width table would break
/// the table's alignment, so it's set aside as a "text source" instead.
fn is_text_source(cfg: &Config, name: &str) -> bool {
    cfg.sources
        .iter()
        .find(|s| s.name == name)
        .and_then(|s| s.format)
        == Some(crate::config::ValueFormat::Markdown)
}

trait Named {
    fn name(&self) -> &str;
}
impl Named for ReadingRow {
    fn name(&self) -> &str {
        &self.source
    }
}
impl Named for crate::db::LogRow {
    fn name(&self) -> &str {
        &self.source
    }
}
impl Named for crate::health::SourceHealth {
    fn name(&self) -> &str {
        &self.source
    }
}

fn header(subtitle: &str) {
    println!("barduck v{VERSION} — {subtitle}");
}

pub async fn print_latest(
    backend: &Backend,
    cfg: &Config,
    sources: &[String],
    json: bool,
    exclude_text: bool,
) -> Result<()> {
    let rows = filter_named(backend.latest().await?, sources);
    // Text (markdown-format) sources are set aside from the scalar-value
    // table — `--no-text` drops them entirely; otherwise they're appended
    // after the table (or after the array, in `--json` mode) instead of
    // being interleaved with the tabular rows.
    let (mut values, mut text): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|r| !is_text_source(cfg, &r.source));
    if exclude_text {
        text.clear();
    }

    if json {
        values.extend(text);
        println!("{}", serde_json::to_string_pretty(&values)?);
        return Ok(());
    }
    header("latest values");
    println!("{:<24} {:>14}  {:<6} TIMESTAMP", "SOURCE", "VALUE", "UNIT");
    for r in values {
        println!(
            "{:<24} {:>14}  {:<6} {}",
            r.source,
            r.value,
            r.unit.unwrap_or_default(),
            r.ts
        );
    }
    if !text.is_empty() {
        println!();
        println!("TEXT SOURCES");
        for r in text {
            println!("--- {} ({}) ---", r.source, r.ts);
            println!("{}", r.value);
            println!();
        }
    }
    Ok(())
}

pub async fn print_history(
    backend: &Backend,
    source: &str,
    from: Option<f64>,
    to: Option<f64>,
    json: bool,
) -> Result<()> {
    let rows = backend.history(source, from, to).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    header(&format!("history for `{source}`"));
    println!("{:<24} {:>14}  {:<6} TIMESTAMP", "SOURCE", "VALUE", "UNIT");
    for r in rows {
        println!(
            "{:<24} {:>14}  {:<6} {}",
            r.source,
            r.value,
            r.unit.unwrap_or_default(),
            r.ts
        );
    }
    Ok(())
}

pub async fn print_health(
    backend: &Backend,
    cfg: &crate::config::Config,
    sources: &[String],
    json: bool,
) -> Result<()> {
    let rows = filter_named(backend.health(cfg).await?, sources);
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    header("source health");
    println!(
        "{:<24} {:<10} {:>10} LAST SUCCESS",
        "SOURCE", "STATUS", "FAILS"
    );
    for h in rows {
        println!(
            "{:<24} {:<10} {:>10} {}",
            h.source,
            h.status.as_str(),
            h.consecutive_failures,
            h.last_success_ts.unwrap_or_else(|| "never".into())
        );
    }
    Ok(())
}

pub async fn print_logs(
    backend: &Backend,
    limit: i64,
    sources: &[String],
    json: bool,
) -> Result<()> {
    let rows = filter_named(backend.logs(limit).await?, sources);
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    header("recent fetch logs");
    println!("{:<24} {:<7} {:>9}  TIMESTAMP  ERROR", "SOURCE", "OK", "MS");
    for l in rows {
        println!(
            "{:<24} {:<7} {:>9}  {}  {}",
            l.source,
            l.ok,
            l.duration_ms,
            l.ts,
            l.error.unwrap_or_default()
        );
    }
    Ok(())
}

/// Parses RFC3339 or bare date (`2026-01-31`) into a unix epoch.
#[allow(clippy::cast_precision_loss)] // sub-microsecond precision is irrelevant here
pub fn parse_time(s: &str) -> Result<f64> {
    use chrono::DateTime;
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Ok(t.timestamp_millis() as f64 / 1000.0);
    }
    let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|e| anyhow::anyhow!("invalid time `{s}` (use RFC3339 or YYYY-MM-DD): {e}"))?;
    let midnight = d
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid date `{s}`"))?;
    Ok(midnight.and_utc().timestamp_millis() as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn source_filter_is_noop_when_empty_and_filters_otherwise() {
        let rows = vec![
            ReadingRow {
                source: "a".into(),
                value: "1".into(),
                unit: None,
                ts_epoch: 0.0,
                ts: String::new(),
            },
            ReadingRow {
                source: "b".into(),
                value: "2".into(),
                unit: None,
                ts_epoch: 0.0,
                ts: String::new(),
            },
        ];
        assert_eq!(filter_named(rows.clone(), &[]).len(), 2);
        let filtered = filter_named(rows, &["b".into()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source, "b");
    }
}
