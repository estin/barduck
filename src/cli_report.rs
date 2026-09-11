use crate::{
    config::{Config, VERSION},
    db::ReadingRow,
    query::Backend,
    source::DebugRow,
};
use anyhow::{Result, bail};

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
        .find(|s| s.name() == name)
        .and_then(crate::config::SourceCfg::format)
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

pub async fn print_logs(
    backend: &Backend,
    cfg: &Config,
    limit: i64,
    sources: &[String],
    json: bool,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let mut rows = backend.logs(sources, limit).await?;
    // Units live in config, not in fetch logs: resolve per row so log
    // output shows values the same way panels do (spec: cli — Query
    // commands; web-ui — Per-source log view linked from panels).
    for r in &mut rows {
        r.unit = cfg
            .sources
            .iter()
            .find(|s| s.name() == r.source)
            .and_then(|s| s.unit())
            .map(str::to_string);
    }
    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?;
        return Ok(());
    }
    header("recent fetch logs");
    writeln!(
        out,
        "{:<24} {:<40} {:<6} {:<6} {:>9}  TIMESTAMP  ERROR",
        "SOURCE", "VALUE", "UNIT", "ORIGIN", "MS"
    )?;
    for l in rows {
        writeln!(
            out,
            "{:<24} {:<40} {:<6} {:<6} {:>9}  {}  {}",
            l.source,
            short_value(l.value.as_deref().unwrap_or("—")),
            l.unit.as_deref().unwrap_or_default(),
            l.origin,
            l.duration_ms,
            l.ts,
            l.error.unwrap_or_default()
        )?;
    }
    Ok(())
}

/// One-line table form of a fetched value: newlines collapsed, long values
/// (markdown bodies, JSON blobs) truncated. Full values stay in `--json`.
fn short_value(value: &str) -> String {
    let one_line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 40 {
        format!("{}…", one_line.chars().take(39).collect::<String>())
    } else {
        one_line
    }
}

/// Runs one source once and prints the parsed result without touching the
/// database (spec: cli — Source debug fetch command). Prints first, then
/// reports failure: a transport error, or any row the real pipeline would
/// have rejected, exits non-zero after its output.
pub async fn print_fetch(cfg: &Config, source: &str, json: bool) -> Result<()> {
    let Some(src) = cfg.sources.iter().find(|s| s.name() == source) else {
        bail!("unknown source `{source}`");
    };
    let rows = match src {
        crate::config::SourceCfg::Query { .. } => {
            vec![crate::source::debug_query(cfg, src).await?]
        }
        crate::config::SourceCfg::Stream { .. } => crate::source::debug_stream(cfg, src).await?,
        crate::config::SourceCfg::Ingest { .. } => {
            bail!("ingest sources receive data via HTTP push, not fetch");
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        header(&format!("debug fetch for `{source}` (no database writes)"));
        for (i, row) in rows.iter().enumerate() {
            if rows.len() > 1 {
                println!("--- line {} ---", i + 1);
            }
            print_debug_row(row);
        }
    }
    if let Some(bad) = rows.iter().find(|r| r.error.is_some()) {
        bail!(
            "fetch would have failed: {}",
            bad.error.as_deref().unwrap_or("unknown error")
        );
    }
    Ok(())
}

fn print_debug_row(row: &DebugRow) {
    println!("VALUE      {}", row.value);
    println!("TS         {}", row.ts);
    match &row.threshold {
        Some(bands) => println!(
            "THRESHOLDS {}",
            bands
                .iter()
                .map(|t| format!("{}→{}", t.bound, t.level.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => println!("THRESHOLDS none"),
    }
    println!(
        "TYPED      bigint={:?} double={:?} json={:?}",
        row.value_bigint, row.value_double, row.value_json
    );
    if let Some(e) = &row.error {
        println!("ERROR      {e}");
    }
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

    /// `logs` renders one row per attempt with its origin (spec: cli —
    /// Query commands).
    #[tokio::test]
    async fn logs_table_shows_push_and_poll_origins() {
        use crate::db::{Db, Origin};
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.duckdb");
        let toml = format!(
            "database_path = \"{db}\"\n[[sources]]\nname = \"s\"\ntype = \"query\"\ncommand = \"echo 1\"\n",
            db = db_path.display(),
        );
        let cfg: Config = toml::from_str(&toml).unwrap();
        let db = Db::open_rw(&db_path).unwrap();
        db.insert_log("s", 1, None, Some("a"), Origin::Poll)
            .await
            .unwrap();
        db.insert_log("s", 1, None, Some("b"), Origin::Push)
            .await
            .unwrap();
        let backend = Backend::new(&cfg, false).unwrap();
        let mut buf = Vec::new();
        print_logs(&backend, &cfg, 10, &[], false, &mut buf)
            .await
            .unwrap();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("ORIGIN"), "header names the column:\n{text}");
        assert!(text.contains("poll"), "scheduled row shows poll:\n{text}");
        assert!(text.contains("push"), "ingested row shows push:\n{text}");
    }
}
