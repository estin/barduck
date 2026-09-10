use crate::config::{Config, JsonlRow, SourceCfg, Threshold, ValueType, parse_jsonl_row};
use anyhow::{Context as _, Result, bail};
use std::path::Path;

/// A fetchable data source. Enum-dispatched by config `type`;
/// new types = new variant + a match arm here and in [`build`](fn@build).
#[derive(Clone)]
pub enum SourceKind {
    Query { command: String },
    Stream { command: String },
}

/// Validates type-specific params at startup (spec: source-configuration).
/// Command presence is re-checked here (not just in config validation) so
/// callers building kinds directly — `collect_once`, tests — get the same
/// guarantee rather than a second, drifted check.
pub fn build(cfg: &SourceCfg) -> Result<SourceKind> {
    let name = cfg.name().to_string();
    let command = cfg.command().to_string();
    if command.is_empty() {
        bail!("{} source `{name}` requires `command`", cfg.kind().as_str());
    }
    match cfg {
        SourceCfg::Query { .. } => Ok(SourceKind::Query { command }),
        SourceCfg::Stream { .. } => Ok(SourceKind::Stream { command }),
    }
}

/// Converts a fetched value against a source's declared `value_type` (spec:
/// source-configuration — configurable stored value type), returning the
/// `(bigint, double, json)` triple to store alongside the existing string
/// value — at most one is ever `Some`. `Json` deliberately isn't validated
/// here: the raw string is cast to `DuckDB`'s `JSON` type at insert time
/// instead, so invalid JSON surfaces through the same "fetched but failed to
/// store" path as any other insert failure, rather than a second bespoke
/// validation error for the same underlying problem.
pub fn convert_value_type(
    value: &str,
    value_type: ValueType,
) -> Result<(Option<i64>, Option<f64>, Option<String>)> {
    match value_type {
        ValueType::String => Ok((None, None, None)),
        ValueType::Bigint => {
            let v = value
                .trim()
                .parse::<i64>()
                .with_context(|| format!("value `{value}` does not convert to bigint"))?;
            Ok((Some(v), None, None))
        }
        ValueType::Double => {
            let v = value
                .trim()
                .parse::<f64>()
                .with_context(|| format!("value `{value}` does not convert to double"))?;
            Ok((None, Some(v), None))
        }
        ValueType::Json => Ok((None, None, Some(value.trim().to_string()))),
    }
}

impl SourceKind {
    /// Returns the fetched value as a trimmed string for a `query` source.
    /// `dir` is the config file's own directory, used as the working
    /// directory for the spawned process (spec: source-configuration —
    /// config-relative working directory). Stream output never flows
    /// through here — it is ingested line by line (see [`StreamProc`]).
    pub async fn fetch(&self, dir: &Path) -> Result<String> {
        match self {
            SourceKind::Query { command } => fetch_query(command, dir).await,
            SourceKind::Stream { .. } => {
                bail!("stream sources are ingested continuously, not fetched per tick")
            }
        }
    }
}

/// One extracted command output: either a plain value or a structured
/// `jsonl` row (spec: source-configuration — Generic query source type).
/// `ts_epoch`/`ts` are resolved (row `ts` or the arrival fallback) and
/// `threshold` carries validated replacement bands, when the row had them.
#[derive(Debug, Clone)]
pub struct ParsedOutput {
    pub value: String,
    pub ts_epoch: f64,
    pub ts: String,
    pub threshold: Option<Vec<Threshold>>,
}

/// Splits oneshot command output into plain vs structural: a trimmed
/// stdout that parses as a JSON object containing a `value` key is a
/// `jsonl` row (applied via [`parse_jsonl_row`]); anything else — plain
/// text, numbers, JSON without a `value` key — is stored as-is for
/// backward compatibility. `arrival` is the collection time used when
/// the row carries no (or no usable) `ts`.
pub fn parse_output(source: &str, output: &str, arrival: (f64, String)) -> Result<ParsedOutput> {
    let trimmed = output.trim();
    let row: Option<JsonlRow> = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Object(map)) if map.contains_key("value") => {
            Some(parse_jsonl_row(source, map)?)
        }
        _ => None,
    };
    match row {
        Some(r) => {
            let (ts_epoch, ts) =
                r.ts.as_ref()
                    .map_or(arrival.clone(), |t| t.resolve(arrival));
            Ok(ParsedOutput {
                value: r.value,
                ts_epoch,
                ts,
                threshold: r.threshold,
            })
        }
        None => Ok(ParsedOutput {
            value: trimmed.to_string(),
            ts_epoch: arrival.0,
            ts: arrival.1,
            threshold: None,
        }),
    }
}

async fn fetch_query(command: &str, dir: &Path) -> Result<String> {
    let out = run_shell(command, dir).await?;
    Ok(out)
}

/// One dry-run result for the `fetch` debug command (spec: cli — Source
/// debug fetch command): the full parse pipeline output for a single
/// command run (query) or line (stream), without touching the database.
/// `error` is `Some` when the fetch would have failed (bad row, failed
/// conversion); transport failures (spawn error, timeout) are `Err`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DebugRow {
    pub value: String,
    pub ts_epoch: f64,
    pub ts: String,
    pub threshold: Option<Vec<Threshold>>,
    pub value_bigint: Option<i64>,
    pub value_double: Option<f64>,
    pub value_json: Option<String>,
    pub error: Option<String>,
}

/// Parses one output through the real pipeline (row split + value-type
/// conversion), recording failures in [`DebugRow::error`] instead of the
/// fetch log. Shared by the query and stream dry-runs.
fn debug_parse(src: &SourceCfg, text: &str, arrival: (f64, String)) -> DebugRow {
    let name = src.name();
    let parsed = match parse_output(name, text, arrival.clone()) {
        Ok(p) => p,
        Err(e) => {
            return DebugRow {
                value: String::new(),
                ts_epoch: arrival.0,
                ts: arrival.1,
                threshold: None,
                value_bigint: None,
                value_double: None,
                value_json: None,
                error: Some(format!("{e:#}")),
            };
        }
    };
    match convert_value_type(&parsed.value, src.effective_value_type()) {
        Ok((value_bigint, value_double, value_json)) => DebugRow {
            value: parsed.value,
            ts_epoch: parsed.ts_epoch,
            ts: parsed.ts,
            threshold: parsed.threshold,
            value_bigint,
            value_double,
            value_json,
            error: None,
        },
        Err(e) => DebugRow {
            value: parsed.value,
            ts_epoch: parsed.ts_epoch,
            ts: parsed.ts,
            threshold: None,
            value_bigint: None,
            value_double: None,
            value_json: None,
            error: Some(format!("{e:#}")),
        },
    }
}

/// Runs a query source's command once under its configured timeout and
/// returns the parsed result. Writes nothing anywhere (spec: cli — Source
/// debug fetch command).
pub async fn debug_query(cfg: &Config, src: &SourceCfg) -> Result<DebugRow> {
    let out = tokio::time::timeout(src.timeout(), run_shell(src.command(), &cfg.config_dir))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "timed out after {}",
                humantime::format_duration(src.timeout())
            )
        })?
        .map_err(|e| anyhow::anyhow!("{e:#}"))?;
    Ok(debug_parse(src, &out, crate::db::now()))
}

/// Runs a stream source's command, parses up to 5 stdout lines within the
/// source timeout, then kills the command. Writes nothing anywhere (spec:
/// cli — Source debug fetch command).
pub async fn debug_stream(cfg: &Config, src: &SourceCfg) -> Result<Vec<DebugRow>> {
    let mut proc = StreamProc::spawn(src.command(), &cfg.config_dir)?;
    // A stream may never produce 5 lines or EOF (it can stay silent with
    // stdout held open), so a hit deadline returns whatever arrived rather
    // than failing: only silence from the start is a timeout error.
    let deadline = tokio::time::sleep(src.timeout());
    tokio::pin!(deadline);
    let mut rows = Vec::new();
    loop {
        if rows.len() >= 5 {
            break;
        }
        tokio::select! {
            line = proc.next_line() => match line? {
                Some(text) => rows.push(debug_parse(src, &text, crate::db::now())),
                None => break,
            },
            () = &mut deadline => break,
        }
    }
    proc.shutdown().await;
    if rows.is_empty() {
        bail!(
            "no lines arrived within {}",
            humantime::format_duration(src.timeout())
        );
    }
    Ok(rows)
}

/// A running stream process with line-split stdout (spec: data-collection
/// — Stream collection). Owns the child: drop the value without
/// [`StreamProc::shutdown`] and the process group is killed on best effort
/// rather than orphaned (see [`KillGroupOnDrop`]).
pub struct StreamProc {
    child: tokio::process::Child,
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
    guard: KillGroupOnDrop,
}

impl StreamProc {
    /// Spawns `sh -c <command>` with stdout piped and line-buffered.
    /// Synchronous: spawning never blocks, only reading does.
    pub fn spawn(command: &str, dir: &Path) -> Result<Self> {
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(dir)
            .process_group(0)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("spawning stream command")?;
        let stdout = child.stdout.take().context("stream stdout not piped")?;
        #[allow(clippy::cast_possible_wrap)]
        let guard = KillGroupOnDrop(child.id().map(|id| id as i32));
        Ok(Self {
            child,
            lines: tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout)),
            guard,
        })
    }

    /// The next stdout line, or `None` on EOF (process ended or closed
    /// stdout). I/O errors surface as `Err` and are treated like an exit.
    pub async fn next_line(&mut self) -> Result<Option<String>> {
        self.lines.next_line().await.context("reading stream line")
    }
    /// log; disarms the orphan-killer since the child is reaped.
    pub async fn wait_for_exit(&mut self) -> Result<std::process::ExitStatus> {
        let status = self.child.wait().await.context("waiting for stream exit")?;
        self.guard.0 = None;
        Ok(status)
    }

    /// Kills the process group and reaps the child (daemon shutdown).
    pub async fn shutdown(&mut self) {
        self.guard.0 = None;
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

/// Kills a spawned command's whole process group (not just its direct PID)
/// if dropped before the command finishes — e.g. the collector's caller
/// cancels the surrounding `tokio::time::timeout` on a hung `query` source.
/// Without this, a hanging command (a slow API call, a `whois` lookup with
/// no timeout of its own) and everything it spawned are simply abandoned as
/// orphans instead of being cleaned up, quietly leaking processes/sockets on
/// every timeout (spec: data-collection — collector resilience: one
/// source's misbehavior must not degrade the whole daemon over time).
struct KillGroupOnDrop(Option<i32>);

impl Drop for KillGroupOnDrop {
    fn drop(&mut self) {
        if let Some(pgid) = self.0.take() {
            // Fire-and-forget: the negative pid targets the whole process
            // group `process_group(0)` put the command in at spawn time.
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .arg(format!("-{pgid}"))
                .spawn();
        }
    }
}

/// Runs a shell command (working directory `dir` — the config file's own
/// directory, spec: source-configuration — config-relative working
/// directory) and returns its trimmed stdout; non-zero exit is an error
/// carrying stderr. Shared by query sources and setup commands. Spawned in
/// its own process group so a cancelled/timed-out fetch can be cleaned up
/// as a whole (see [`KillGroupOnDrop`]) instead of leaving orphans behind.
pub async fn run_shell(command: &str, dir: &Path) -> Result<String> {
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("spawning sh")?;
    let mut guard = KillGroupOnDrop(child.id().map(u32::cast_signed));
    let out = child.wait_with_output().await.context("waiting for sh")?;
    guard.0 = None; // exited on its own — nothing left to clean up
    if !out.status.success() {
        bail!(
            "command failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::time::Duration;

    fn process_alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// A cancelled fetch (the collector's `tokio::time::timeout` racing a
    /// hung command) must not leave the command's process tree running as
    /// orphans — one misbehaving source command must not leak resources
    /// indefinitely (spec: data-collection — collector resilience).
    #[tokio::test]
    async fn cancelled_command_kills_its_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // The outer `sh -c` from `run_shell` is itself the process-group
        // leader; `$$` inside it is that leader's pid.
        let cmd = format!("echo $$ > {}; sleep 5", pid_file.display());

        let outcome =
            tokio::time::timeout(Duration::from_millis(200), run_shell(&cmd, dir.path())).await;
        assert!(
            outcome.is_err(),
            "expected the outer timeout to fire before `sleep 5` finishes"
        );

        let pid = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(pid) = std::fs::read_to_string(&pid_file) {
                    return pid.trim().to_string();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        // Give the fire-and-forget `kill -KILL` a moment to land, then
        // confirm the group leader (and thus `sleep 5`) is actually gone.
        let mut alive = true;
        for _ in 0..50 {
            alive = process_alive(&pid);
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            !alive,
            "process group {pid} is still alive after cancellation"
        );
    }

    /// Plain stdout stays a plain value, even when it is valid JSON without
    /// a `value` key (spec: source-configuration — Generic query source
    /// type, backward compatibility).
    #[test]
    fn plain_and_json_values_pass_through_as_is() {
        let arrival = (7.0, "arr".to_string());
        for text in ["42%", "{\"load\": 0.42, \"cores\": 8}", "[1,2]", "123"] {
            let p = parse_output("s", text, arrival.clone()).unwrap();
            assert_eq!(p.value, text.trim(), "plain output changed: {text}");
            assert!((p.ts_epoch - 7.0).abs() < 0.001, "arrival ts changed");
            assert!(p.threshold.is_none());
        }
    }

    /// A JSON object with a `value` key is structural (spec:
    /// source-configuration — Generic query source type).
    #[test]
    fn jsonl_object_applies_structurally() {
        let arrival = (7.0, "arr".to_string());
        let p = parse_output(
            "s",
            r#"{"value":"42%","ts":"2026-09-09T12:00:00Z"}"#,
            arrival,
        )
        .unwrap();
        assert_eq!(p.value, "42%");
        assert!(p.ts_epoch > 1_780_000_000.0);
        assert!(p.threshold.is_none());
    }

    /// Unknown fields and non-string values fail the row (spec:
    /// source-configuration — JSONL row schema).
    #[test]
    fn invalid_rows_fail() {
        let arrival = (7.0, "arr".to_string());
        parse_output("s", r#"{"value":"ok","color":"blue"}"#, arrival.clone()).unwrap_err();
        parse_output("s", r#"{"value":42}"#, arrival).unwrap_err();
    }

    /// A spawned stream yields its lines in order, then EOF (spec:
    /// data-collection — Stream collection).
    #[tokio::test]
    async fn stream_proc_yields_lines_then_eof() {
        let dir = tempfile::tempdir().unwrap();
        let mut proc = StreamProc::spawn(
            r#"printf '%s\n' '{"value":"a"}' '{"value":"b"}'"#,
            dir.path(),
        )
        .unwrap();
        assert_eq!(
            proc.next_line().await.unwrap().as_deref(),
            Some(r#"{"value":"a"}"#)
        );
        assert_eq!(
            proc.next_line().await.unwrap().as_deref(),
            Some(r#"{"value":"b"}"#)
        );
        assert_eq!(proc.next_line().await.unwrap(), None);
        let status = proc.wait_for_exit().await.unwrap();
        assert!(status.success());
    }

    fn debug_cfg(toml_sources: &str) -> crate::config::Config {
        toml::from_str(&format!(
            "database_path = \"/nonexistent-debug-db/db.duckdb\"\n{toml_sources}"
        ))
        .unwrap()
    }

    fn find<'a>(cfg: &'a crate::config::Config, name: &str) -> &'a crate::config::SourceCfg {
        cfg.sources.iter().find(|s| s.name() == name).unwrap()
    }

    /// A query dry-run parses plain output without touching the database
    /// (spec: cli — Source debug fetch command).
    #[tokio::test]
    async fn debug_query_parses_plain_output() {
        let cfg =
            debug_cfg("[[sources]]\nname = \"q\"\ntype = \"query\"\ncommand = \"echo 42%\"\n");
        let row = debug_query(&cfg, find(&cfg, "q")).await.unwrap();
        assert_eq!(row.value, "42%");
        assert!(row.error.is_none());
        assert!(!std::path::Path::new("/nonexistent-debug-db/db.duckdb").exists());
    }

    /// A query dry-run applies `jsonl` structurally (spec: cli — Source
    /// debug fetch command).
    #[tokio::test]
    async fn debug_query_applies_jsonl_row() {
        let cfg = debug_cfg(
            "[[sources]]\nname = \"q\"\ntype = \"query\"\ncommand = \"echo '{\\\"value\\\":\\\"7\\\",\\\"threshold\\\":[{\\\"bound\\\":1.0,\\\"level\\\":\\\"green\\\"},{\\\"bound\\\":10.0,\\\"level\\\":\\\"red\\\"}]}'\"\n",
        );
        let row = debug_query(&cfg, find(&cfg, "q")).await.unwrap();
        assert_eq!(row.value, "7");
        assert_eq!(row.threshold.as_ref().unwrap().len(), 2);
        assert!(row.error.is_none());
    }

    /// A hanging command fails the dry-run at the source timeout (spec:
    /// cli — Source debug fetch command).
    #[tokio::test]
    async fn debug_query_times_out() {
        let cfg = debug_cfg(
            "[[sources]]\nname = \"q\"\ntype = \"query\"\ncommand = \"sleep 30\"\ntimeout = \"100ms\"\n",
        );
        debug_query(&cfg, find(&cfg, "q")).await.unwrap_err();
    }

    /// A stream dry-run prints the first lines then kills the command,
    /// writing nothing (spec: cli — Source debug fetch command).
    #[tokio::test]
    async fn debug_stream_prints_first_lines() {
        let cfg = debug_cfg(
            "[[sources]]\nname = \"s\"\ntype = \"stream\"\ncommand = \"printf '%s\\n' '{\\\"value\\\":\\\"a\\\"}' 'oops' '{\\\"value\\\":\\\"b\\\"}'\"\nexpected_interval = \"30s\"\n",
        );
        let rows = debug_stream(&cfg, find(&cfg, "s")).await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].value, "a");
        // Plain non-JSON text stays a plain value, even in streams.
        assert_eq!(rows[1].value, "oops");
        assert_eq!(rows[2].value, "b");
        assert!(!std::path::Path::new("/nonexistent-debug-db/db.duckdb").exists());
    }

    /// A stream that goes silent after one line returns that line instead
    /// of waiting out the timeout (spec: cli — Source debug fetch command).
    #[tokio::test]
    async fn debug_stream_returns_partial_lines_on_silence() {
        let cfg = debug_cfg(
            "[[sources]]\nname = \"s\"\ntype = \"stream\"\ncommand = \"echo '{\\\"value\\\":\\\"a\\\"}'; sleep 30\"\ntimeout = \"300ms\"\nexpected_interval = \"30s\"\n",
        );
        let rows = debug_stream(&cfg, find(&cfg, "s")).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, "a");
    }
}
