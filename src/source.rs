use crate::config::{Config, JsonlRow, SourceCfg, Threshold, ValueType, parse_jsonl_row};
use anyhow::{Context as _, Result, bail};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::path::Path;

/// A fetchable data source. Enum-dispatched by config `type`;
/// new types = new variant + a match arm here and in [`build`](fn@build).
#[derive(Clone)]
pub enum SourceKind {
    Query { command: String },
    Stream { command: String },
    Ingest,
}

/// Validates type-specific params at startup (spec: source-configuration).
/// Command presence is re-checked here (not just in config validation) so
/// callers building kinds directly — `collect_once`, tests — get the same
/// guarantee rather than a second, drifted check.
pub fn build(cfg: &SourceCfg) -> Result<SourceKind> {
    match cfg {
        SourceCfg::Query { .. } => Ok(SourceKind::Query {
            command: require_command(cfg, "query")?,
        }),
        SourceCfg::Stream { .. } => Ok(SourceKind::Stream {
            command: require_command(cfg, "stream")?,
        }),
        SourceCfg::Ingest { .. } => Ok(SourceKind::Ingest),
        // A child never fetches on its own — its composite root's command
        // fans out to it (spec: data-collection — Composite source command
        // output). Callers reaching a child here would be a routing bug:
        // `collectible`/`control_channels` never spawn a task for one.
        SourceCfg::Child { .. } => {
            bail!(
                "source `{}` is a composite child and has no fetch of its own",
                cfg.name()
            )
        }
    }
}

fn require_command(cfg: &SourceCfg, kind: &str) -> Result<String> {
    let command = cfg.command().to_string();
    if command.is_empty() {
        bail!("{kind} source `{}` requires `command`", cfg.name());
    }
    Ok(command)
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
            SourceKind::Ingest => {
                bail!("ingest sources receive data via HTTP push, not fetch")
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

/// One item of an ingest payload (spec: http-api — HTTP ingest endpoint):
/// shared by `POST /api/ingest` (one item per request body) and a composite
/// source's fan-out (spec: data-collection — Composite source command
/// output), whose command output is a JSON array of these, one per declared
/// child.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct IngestItem {
    pub source: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub ts: Option<serde_json::Value>,
    #[serde(default)]
    pub thresholds: Option<Vec<Threshold>>,
}

/// Converts one ingest item into a storable [`ParsedOutput`], applying the
/// same rules `POST /api/ingest` uses: a string value is stored as-is, other
/// JSON is compacted to its canonical text; declared thresholds are
/// validated; `ts` is strict (epoch seconds or RFC 3339), unlike the lenient
/// `jsonl` fallback a `query` row gets. `value_type` is the target source's
/// own — `Ok` only once the value has also been confirmed to convert to it.
/// Shared by the ingest endpoint and a composite source's fan-out so the two
/// can never drift (spec: data-collection — Composite source command
/// output).
pub fn resolve_ingest_item(
    item: &IngestItem,
    arrival: (f64, String),
    value_type: ValueType,
) -> std::result::Result<ParsedOutput, String> {
    let value = match &item.value {
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    if let Some(bands) = &item.thresholds {
        crate::config::validate_thresholds(&item.source, bands).map_err(|e| format!("{e:#}"))?;
    }
    let (ts_epoch, ts) = resolve_ingest_ts(item.ts.as_ref(), arrival, &item.source)?;
    convert_value_type(&value, value_type).map_err(|e| format!("{e:#}"))?;
    Ok(ParsedOutput {
        value,
        ts_epoch,
        ts,
        threshold: item.thresholds.clone(),
    })
}

/// Strict ingest `ts`: epoch number or RFC 3339 string; anything else is a
/// caller error naming the problem (unlike row `ts`, which degrades to
/// arrival time).
pub(crate) fn resolve_ingest_ts(
    raw: Option<&serde_json::Value>,
    arrival: (f64, String),
    source: &str,
) -> std::result::Result<(f64, String), String> {
    let Some(v) = raw else {
        return Ok(arrival);
    };
    match v {
        serde_json::Value::Null => Ok(arrival),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(secs) => epoch_to_ts(secs, source),
            None => Err(format!("ingest `ts` for `{source}` is not a finite number")),
        },
        serde_json::Value::String(s) => chrono::DateTime::parse_from_rfc3339(s).map_or_else(
            |_| Err(format!("ingest `ts` for `{source}` is not RFC 3339: `{s}`")),
            |d| {
                #[allow(clippy::cast_precision_loss)] // epoch millis fit exactly enough
                let epoch = d.timestamp_millis() as f64 / 1000.0;
                Ok((epoch, d.to_rfc3339()))
            },
        ),
        _ => Err(format!(
            "ingest `ts` for `{source}` must be epoch seconds or RFC 3339"
        )),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn epoch_to_ts(secs: f64, source: &str) -> std::result::Result<(f64, String), String> {
    if !secs.is_finite() {
        return Err(format!("ingest `ts` for `{source}` is not a finite number"));
    }
    // `floor`, not `trunc`: for a pre-epoch fractional value (e.g. `-1.5`),
    // `trunc` rounds toward zero (`-1.0`), and an `.abs()` on the negative
    // remainder that follows would flip it positive, landing exactly one
    // second late. Flooring keeps `secs - whole` in `[0, 1)` for either
    // sign, so the remainder is already the correct positive nanosecond
    // offset with no `.abs()` needed.
    let whole = secs.floor();
    let nanos = ((secs - whole) * 1_000_000_000.0).round() as u32;
    match chrono::DateTime::from_timestamp(whole as i64, nanos) {
        Some(d) => {
            #[allow(clippy::cast_precision_loss)]
            let epoch = d.timestamp_millis() as f64 / 1000.0;
            Ok((epoch, d.to_rfc3339()))
        }
        None => Err(format!("ingest `ts` for `{source}` is out of range")),
    }
}

/// Runs a composite source's command once and returns one [`DebugRow`] per
/// declared child (or just `only`, when given), labeled with that child's
/// full name. Writes nothing anywhere (spec: cli — Poll and fetch accept
/// composite roots and children).
pub async fn debug_composite(
    cfg: &Config,
    root: &SourceCfg,
    only: Option<&str>,
) -> Result<Vec<DebugRow>> {
    let out = tokio::time::timeout(root.timeout(), run_shell(root.command(), &cfg.config_dir))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "timed out after {}",
                humantime::format_duration(root.timeout())
            )
        })?
        .map_err(|e| anyhow::anyhow!("{e:#}"))?;
    let items: Vec<IngestItem> = serde_json::from_str(&out).map_err(|e| {
        anyhow::anyhow!("composite output is not a JSON array of ingest items: {e}")
    })?;
    let arrival = crate::db::now();
    let mut rows = Vec::new();
    for child in crate::config::composite_children(cfg, root.name()) {
        if only.is_some_and(|o| o != child.name()) {
            continue;
        }
        let found = items.iter().find(|it| it.source == child.name());
        rows.push(match found {
            Some(item) => {
                match resolve_ingest_item(item, arrival.clone(), child.effective_value_type()) {
                    Ok(parsed) => {
                        let (value_bigint, value_double, value_json) =
                            convert_value_type(&parsed.value, child.effective_value_type())
                                .unwrap_or_default();
                        DebugRow {
                            name: Some(child.name().to_string()),
                            value: parsed.value,
                            ts_epoch: parsed.ts_epoch,
                            ts: parsed.ts,
                            threshold: parsed.threshold,
                            value_bigint,
                            value_double,
                            value_json,
                            error: None,
                        }
                    }
                    Err(e) => DebugRow {
                        name: Some(child.name().to_string()),
                        value: String::new(),
                        ts_epoch: arrival.0,
                        ts: arrival.1.clone(),
                        threshold: None,
                        value_bigint: None,
                        value_double: None,
                        value_json: None,
                        error: Some(e),
                    },
                }
            }
            None => DebugRow {
                name: Some(child.name().to_string()),
                value: String::new(),
                ts_epoch: arrival.0,
                ts: arrival.1.clone(),
                threshold: None,
                value_bigint: None,
                value_double: None,
                value_json: None,
                error: Some("missing from composite output".into()),
            },
        });
    }
    Ok(rows)
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
    /// The child's full name, for a composite source's fan-out debug fetch
    /// (spec: cli — Poll and fetch accept composite roots and children).
    /// `None` for an ordinary `query`/`stream` dry-run, which has only one
    /// unnamed value (or line) to report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
                name: None,
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
            name: None,
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
            name: None,
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
/// — Stream collection). Owns the process tree even when dropped without
/// calling [`StreamProc::shutdown`].
pub struct StreamProc {
    child: ShellProcess,
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::process::ChildStdout>>,
}

impl StreamProc {
    /// Spawns the platform shell with stdout piped and line-buffered.
    pub fn spawn(command: &str, dir: &Path) -> Result<Self> {
        let mut child = spawn_shell(command, dir, std::process::Stdio::null())
            .context("spawning stream command")?;
        let stdout = child
            .child
            .stdout()
            .take()
            .context("stream stdout not piped")?;
        Ok(Self {
            child,
            lines: tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout)),
        })
    }

    /// The next stdout line, or `None` on EOF (process ended or closed
    /// stdout). I/O errors terminate the tree before surfacing as `Err`.
    pub async fn next_line(&mut self) -> Result<Option<String>> {
        match self.lines.next_line().await {
            Ok(line) => Ok(line),
            Err(error) => {
                self.shutdown().await;
                Err(error).context("reading stream line")
            }
        }
    }

    /// Waits for the direct process's exit status, then terminates any
    /// descendants it left running, including ones that closed stdout.
    pub async fn wait_for_exit(&mut self) -> Result<std::process::ExitStatus> {
        let status = self.child.wait().await.context("waiting for stream exit");
        self.child.shutdown().await;
        status
    }

    /// Terminates the entire process tree and reaps the direct child.
    pub async fn shutdown(&mut self) {
        self.child.shutdown().await;
    }
}

/// Owns the group/job as well as the direct child. Tokio's kill-on-drop
/// only kills the direct child on Unix, so Drop must signal the group too.
/// Tokio retains responsibility for best-effort reaping on cancellation;
/// ordinary completion and explicit shutdown await the direct child.
struct ShellProcess {
    child: Box<dyn ChildWrapper>,
    armed: bool,
}

impl ShellProcess {
    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        // These wrappers' inner_mut() delegates to the raw Tokio child.
        // Waiting on the group/job wrapper instead can wait indefinitely
        // for background descendants, and on Windows creates an uncancellable
        // blocking completion-port wait. We own descendant cleanup ourselves.
        self.child.inner_mut().wait().await
    }

    fn terminate(&mut self) {
        if self.armed {
            self.armed = false;
            let _ = self.child.start_kill();
            // Still kill the direct child if group/job termination failed.
            let _ = self.child.inner_mut().start_kill();
        }
    }

    async fn shutdown(&mut self) {
        self.terminate();
        let _ = self.wait().await;
    }
}

impl Drop for ShellProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Shared shell selection and tree ownership for queries, setup and streams.
fn spawn_shell(command: &str, dir: &Path, stderr: std::process::Stdio) -> Result<ShellProcess> {
    #[cfg(unix)]
    let mut shell = CommandWrap::with_new("sh", |shell| {
        shell.arg("-c").arg(command);
    });
    #[cfg(windows)]
    let mut shell = CommandWrap::with_new("cmd.exe", |shell| {
        // /S strips the outer pair of quotes. Keep the command inside them
        // verbatim: argv quoting would escape shell operators and quotes.
        shell
            .args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{command}\""));
    });
    shell
        .command_mut()
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(stderr);
    shell.wrap(KillOnDrop);
    #[cfg(unix)]
    shell.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    // JobObject spawns suspended, assigns the child, then resumes it.
    // KillOnDrop also sets the job's kill-on-close flag.
    shell.wrap(process_wrap::tokio::JobObject);
    Ok(ShellProcess {
        child: shell.spawn()?,
        armed: true,
    })
}

/// Ceiling on a single command's captured stdout/stderr. A reading is a
/// dashboard value, not a payload, so anything approaching this is a
/// misbehaving source (`cat /dev/urandom`, a paging API that never
/// terminates) — and `wait_with_output` would otherwise buffer it all in the
/// daemon's memory and then hand it to `INSERT` (spec: data-collection —
/// collector resilience: one source's misbehavior must not degrade the whole
/// daemon).
const MAX_COMMAND_OUTPUT: usize = 1 << 20; // 1 MiB

/// Runs a shell command (working directory `dir` — the config file's own
/// directory, spec: source-configuration — config-relative working
/// directory) and returns its trimmed stdout; non-zero exit is an error
/// carrying stderr. Shared by query sources and setup commands. The whole
/// process tree is cleaned up on completion, cancellation or output errors.
/// Output past [`MAX_COMMAND_OUTPUT`] fails the fetch rather than being
/// buffered without limit.
pub async fn run_shell(command: &str, dir: &Path) -> Result<String> {
    let mut child = spawn_shell(command, dir, std::process::Stdio::piped())
        .context("spawning shell command")?;
    let stdout = child
        .child
        .stdout()
        .take()
        .context("shell stdout not piped")?;
    let stderr = child
        .child
        .stderr()
        .take()
        .context("shell stderr not piped")?;
    // Both pipes must be drained concurrently with the wait: a command that
    // fills one pipe's buffer blocks until it is read, so reading them in
    // sequence would deadlock against a command writing to both.
    let output = tokio::try_join!(
        async { child.wait().await.context("waiting for shell command") },
        read_capped(stdout, "stdout"),
        read_capped(stderr, "stderr"),
    );
    child.shutdown().await;
    let (status, out, err) = output?;
    if !status.success() {
        bail!(
            "command failed ({status}): {}",
            String::from_utf8_lossy(&err).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out).trim().to_string())
}

/// Reads `r` to EOF, failing once more than [`MAX_COMMAND_OUTPUT`] bytes
/// have arrived instead of growing the buffer without bound.
async fn read_capped<R>(r: R, what: &str) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt as _;
    let mut buf = Vec::new();
    // One byte over the cap is enough to tell "exactly at the cap" (fine)
    // from "more to come" (rejected) without reading the rest of it.
    let read = r
        .take(MAX_COMMAND_OUTPUT as u64 + 1)
        .read_to_end(&mut buf)
        .await
        .with_context(|| format!("reading command {what}"))?;
    if read > MAX_COMMAND_OUTPUT {
        bail!("command {what} exceeded {MAX_COMMAND_OUTPUT} bytes");
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::time::Duration;

    /// A pre-epoch fractional `ts` (e.g. `-1.5`, half a second before
    /// 1969-12-31T23:59:59Z) must resolve to that exact instant, not one
    /// second off (spec: http-api — HTTP ingest endpoint).
    #[test]
    fn epoch_to_ts_handles_pre_epoch_fractional_seconds() {
        let (epoch, ts) = epoch_to_ts(-1.5, "s").unwrap();
        assert!((epoch - (-1.5)).abs() < 0.001, "got epoch {epoch}");
        assert!(
            ts.starts_with("1969-12-31T23:59:58.5"),
            "expected 23:59:58.5, got {ts}"
        );
    }

    #[test]
    fn epoch_to_ts_handles_positive_fractional_seconds() {
        let (epoch, ts) = epoch_to_ts(1.5, "s").unwrap();
        assert!((epoch - 1.5).abs() < 0.001, "got epoch {epoch}");
        assert!(ts.starts_with("1970-01-01T00:00:01.5"), "got {ts}");
    }

    #[test]
    fn epoch_to_ts_handles_whole_seconds_either_side_of_the_epoch() {
        assert!((epoch_to_ts(-1.0, "s").unwrap().0 - (-1.0)).abs() < 0.001);
        assert!((epoch_to_ts(0.0, "s").unwrap().0).abs() < 0.001);
        assert!((epoch_to_ts(1.0, "s").unwrap().0 - 1.0).abs() < 0.001);
    }

    #[test]
    fn epoch_to_ts_rejects_non_finite() {
        assert!(epoch_to_ts(f64::NAN, "s").is_err());
        assert!(epoch_to_ts(f64::INFINITY, "s").is_err());
    }

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

    /// A command that never stops producing output must fail rather than be
    /// buffered into the daemon's memory (and then into an `INSERT`) without
    /// limit (spec: data-collection — collector resilience).
    #[tokio::test]
    async fn runaway_command_output_is_capped_not_buffered() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_shell(
            &format!("yes x | head -c {}", MAX_COMMAND_OUTPUT + 4096),
            dir.path(),
        )
        .await
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("exceeded"),
            "error should name the cap: {err:#}"
        );
    }

    /// Output at or just under the cap is still returned in full — the limit
    /// must not quietly truncate ordinary values.
    #[tokio::test]
    async fn output_within_the_cap_is_returned_whole() {
        let dir = tempfile::tempdir().unwrap();
        let out = run_shell("printf 'x%.0s' $(seq 1 1000)", dir.path())
            .await
            .unwrap();
        assert_eq!(out.len(), 1000);
    }

    /// A command writing heavily to *both* pipes must not deadlock: the two
    /// are drained concurrently with the wait, not one after the other.
    #[tokio::test]
    async fn a_command_filling_both_pipes_does_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let finished = tokio::time::timeout(
            Duration::from_secs(20),
            run_shell(
                "yes out | head -c 200000; yes err | head -c 200000 >&2",
                dir.path(),
            ),
        )
        .await;
        assert!(
            finished.is_ok(),
            "run_shell deadlocked on a command writing to both pipes"
        );
        // Trimmed, so a byte-exact length isn't meaningful — what matters is
        // that both pipes drained rather than one blocking the other.
        assert!(finished.unwrap().unwrap().len() > 190_000);
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

    fn composite_debug_cfg(command: &str, children: &[&str]) -> crate::config::Config {
        let mut children_toml = String::new();
        for c in children {
            use std::fmt::Write as _;
            let _ = write!(children_toml, "[[sources.children]]\nname = \"{c}\"\n");
        }
        let escaped = command.replace('"', "\\\"");
        let mut cfg = debug_cfg(&format!(
            "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"{escaped}\"\ninterval = \"1h\"\n{children_toml}"
        ));
        crate::config::expand_composites(&mut cfg).unwrap();
        cfg
    }

    /// `debug_composite` prints (writes nothing) one row per declared child,
    /// each labeled with its full name (spec: cli — Poll and fetch accept
    /// composite roots and children).
    #[tokio::test]
    async fn debug_composite_returns_one_row_per_child() {
        let cfg = composite_debug_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"},{"source":"load::5m","value":"0.2"}]'"#,
            &["1m", "5m"],
        );
        let root = find(&cfg, "load");
        let rows = debug_composite(&cfg, root, None).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name.as_deref(), Some("load::1m"));
        assert_eq!(rows[0].value, "0.1");
        assert_eq!(rows[1].name.as_deref(), Some("load::5m"));
        assert_eq!(rows[1].value, "0.2");
        assert!(!std::path::Path::new("/nonexistent-debug-db/db.duckdb").exists());
    }

    /// Fetching a specific child filters to just that child's own row from
    /// the same single command run (spec: cli — Poll and fetch accept
    /// composite roots and children).
    #[tokio::test]
    async fn debug_composite_filters_to_one_child_when_only_is_given() {
        let cfg = composite_debug_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"},{"source":"load::5m","value":"0.2"}]'"#,
            &["1m", "5m"],
        );
        let root = find(&cfg, "load");
        let rows = debug_composite(&cfg, root, Some("load::5m")).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name.as_deref(), Some("load::5m"));
        assert_eq!(rows[0].value, "0.2");
    }

    /// A declared child missing from the array reports as an error row
    /// rather than panicking or being silently omitted (spec:
    /// data-collection — Composite fan-out error handling).
    #[tokio::test]
    async fn debug_composite_reports_missing_child_as_error_row() {
        let cfg = composite_debug_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"}]'"#,
            &["1m", "5m"],
        );
        let root = find(&cfg, "load");
        let rows = debug_composite(&cfg, root, None).await.unwrap();
        let missing = rows
            .iter()
            .find(|r| r.name.as_deref() == Some("load::5m"))
            .unwrap();
        assert!(
            missing
                .error
                .as_deref()
                .is_some_and(|e| e.contains("missing"))
        );
    }
}
