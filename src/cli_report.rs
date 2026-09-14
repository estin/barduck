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

/// Fetches the named sources now, ignoring their schedules, and reports
/// each attempt (spec: cli — Force poll command). Unlike [`print_fetch`],
/// this writes: every attempt is recorded exactly as a scheduled fetch is.
///
/// Every name is validated before anything is polled, so a typo in the
/// third name cannot leave the first two already fetched. A fetch that ran
/// and failed does not stop the remaining sources — the command reports all
/// of them and only then exits non-zero.
pub async fn print_poll(
    backend: &Backend,
    cfg: &Config,
    sources: &[String],
    json: bool,
    out: &mut impl std::io::Write,
) -> Result<()> {
    let mut targets = Vec::with_capacity(sources.len());
    for name in sources {
        let Some(src) = cfg.sources.iter().find(|s| s.name() == name) else {
            bail!("unknown source `{name}`");
        };
        if let Some(why) = crate::collector::unpollable_reason(src) {
            bail!("source `{name}` {why}");
        }
        targets.push(src);
    }

    let mut outcomes = Vec::with_capacity(targets.len());
    for src in targets {
        outcomes.push(backend.poll(cfg, src).await?);
    }

    if json {
        writeln!(out, "{}", serde_json::to_string_pretty(&outcomes)?)?;
    } else {
        writeln!(out, "barduck v{VERSION} — forced poll")?;
        let width = outcomes
            .iter()
            .map(|o| o.source.len())
            .max()
            .unwrap_or_default();
        for o in &outcomes {
            let detail = if o.success {
                format!(
                    "{}  {}",
                    one_line(o.value.as_deref().unwrap_or("")),
                    o.ts.as_deref().unwrap_or("")
                )
            } else {
                o.error.clone().unwrap_or_else(|| "failed".into())
            };
            let status = if o.success { "ok" } else { "FAILED" };
            writeln!(out, "{:<width$}  {status:<6}  {detail}", o.source)?;
        }
    }

    if let Some(failed) = outcomes.iter().find(|o| !o.success) {
        bail!(
            "poll failed for `{}`: {}",
            failed.source,
            failed.error.as_deref().unwrap_or("unknown error")
        );
    }
    Ok(())
}

/// A value as one table cell: markdown-format sources store prose, and a
/// multi-line (or very long) value would otherwise tear the row apart. The
/// full value is always available in `--json`.
fn one_line(value: &str) -> String {
    let first = value.lines().next().unwrap_or_default().trim();
    let truncated: String = first.chars().take(60).collect();
    if truncated.len() < first.len() || value.lines().nth(1).is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
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

    #[test]
    fn one_line_keeps_short_values_and_collapses_prose() {
        assert_eq!(one_line("42"), "42");
        assert_eq!(one_line("# Weekly report\n\nHours: 36.5"), "# Weekly report…");
        assert_eq!(one_line(&"x".repeat(80)), format!("{}…", "x".repeat(60)));
    }

    fn poll_config(db_path: &std::path::Path) -> Config {
        toml::from_str(&format!(
            "database_path = \"{db}\"\n\
             [[sources]]\nname = \"ok\"\ntype = \"query\"\ncommand = \"echo 42\"\ninterval = \"1h\"\n\
             [[sources]]\nname = \"bad\"\ntype = \"query\"\ncommand = \"exit 3\"\ninterval = \"1h\"\n\
             [[sources]]\nname = \"pushed\"\ntype = \"ingest\"\nexpected_interval = \"1h\"\n",
            db = db_path.display(),
        ))
        .unwrap()
    }

    /// Direct mode runs the fetch in this process and writes it, with no
    /// daemon anywhere — twice in a row, which is what exercises the
    /// read-only probe followed by a read-write open (spec: cli — Force poll
    /// works with and without a daemon).
    #[tokio::test]
    async fn poll_writes_directly_and_repeats_within_one_process() {
        use crate::db::Db;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.duckdb");
        let cfg = poll_config(&db_path);
        drop(Db::open_rw(&db_path).unwrap()); // create the schema, release the file

        let backend = Backend::new(&cfg, false).unwrap();
        let mut buf = Vec::new();
        for _ in 0..2 {
            print_poll(&backend, &cfg, &["ok".into()], false, &mut buf)
                .await
                .unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("42"), "the stored value is reported:\n{text}");

        let db = Db::open_rw(&db_path).unwrap();
        let logs = db.logs(Some("ok"), 10).await.unwrap();
        assert_eq!(logs.len(), 2, "both polls were recorded");
        assert!(logs.iter().all(|l| l.error.is_none()));
        assert_eq!(db.latest_values().await.unwrap()[0].value, "42");
    }

    /// (spec: cli — Force poll command: unknown source rejected before
    /// polling; Force poll rejects sources with nothing to fetch)
    #[tokio::test]
    async fn poll_validates_every_name_before_polling_any() {
        use crate::db::Db;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.duckdb");
        let cfg = poll_config(&db_path);
        drop(Db::open_rw(&db_path).unwrap());
        let backend = Backend::new(&cfg, false).unwrap();

        let err = print_poll(
            &backend,
            &cfg,
            &["ok".into(), "nope".into()],
            false,
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("nope"), "{err:#}");

        let err = print_poll(&backend, &cfg, &["pushed".into()], false, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("HTTP push"), "{err:#}");

        let db = Db::open_rw(&db_path).unwrap();
        assert!(
            db.logs(None, 10).await.unwrap().is_empty(),
            "a rejected name must not leave earlier sources already polled"
        );
    }

    /// Every named source is attempted even after one fails, and only then
    /// does the command report failure (spec: cli — Force poll command).
    #[tokio::test]
    async fn poll_reports_every_outcome_then_fails() {
        use crate::db::Db;
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.duckdb");
        let cfg = poll_config(&db_path);
        drop(Db::open_rw(&db_path).unwrap());
        let backend = Backend::new(&cfg, false).unwrap();
        let names = ["bad".to_string(), "ok".to_string()];

        let mut buf = Vec::new();
        let err = print_poll(&backend, &cfg, &names, false, &mut buf)
            .await
            .unwrap_err();
        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("FAILED"), "{text}");
        assert!(text.contains("42"), "the later source still ran:\n{text}");
        assert!(err.to_string().contains("bad"), "{err:#}");

        let db = Db::open_rw(&db_path).unwrap();
        assert_eq!(db.logs(Some("bad"), 10).await.unwrap().len(), 1);
        assert_eq!(db.logs(Some("ok"), 10).await.unwrap().len(), 1);
        drop(db);

        let mut buf = Vec::new();
        assert!(
            print_poll(&backend, &cfg, &names, true, &mut buf)
                .await
                .is_err()
        );
        let rows: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(rows[0]["source"], "bad");
        assert_eq!(rows[0]["success"], false);
        assert!(rows[0]["error"].is_string());
        assert_eq!(rows[1]["source"], "ok");
        assert_eq!(rows[1]["success"], true);
        assert_eq!(rows[1]["value"], "42");
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
