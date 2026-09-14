use crate::{
    config::{Config, SourceCfg},
    db::{Db, Origin},
    health, source,
};
use anyhow::Result;
use croner::Cron;
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::timeout;

/// A source's per-tick wait: either an interval-scheduled source's next
/// attempt time (advanced after each attempt — `interval` after success,
/// `retry_interval` after a failed fetch, spec: data-collection —
/// per-source schedules), or a cron expression's next occurrence, computed
/// fresh each tick and unaffected by fetch outcome.
enum Schedule {
    Interval {
        effective_interval: std::time::Duration,
        retry_interval: std::time::Duration,
        next: tokio::time::Instant,
    },
    Cron(Box<Cron>),
}

impl Schedule {
    /// Built once per query source at collector startup. The cron string is
    /// already validated by `config::load` for the daemon's own config, but
    /// this is a `pub` entry point's transitive dependency — a caller that
    /// builds a `Config`/`SourceCfg` without going through `config::load`
    /// (a library caller, or a future internal one) could still reach this
    /// with an unvalidated cron string, so parsing is a proper `Err`
    /// instead of an `expect`, matching [`source::build`]'s equivalent
    /// re-check of command presence. An interval-scheduled query's first
    /// tick resumes from its most recent fetch log entry (any outcome): a
    /// fresh success defers until the remaining `interval` elapses, a fresh
    /// failure defers until the remaining `retry_interval` elapses, and an
    /// overdue (or absent) last run is due immediately (spec:
    /// data-collection — Per-source schedules: daemon startup resumes from
    /// the last run). Stream sources never reach this constructor — they
    /// are ingested continuously.
    async fn new(db: &Db, src: &SourceCfg) -> Result<Self> {
        if let Some(expr) = src.cron() {
            let cron: Cron = expr.parse().map_err(|e| {
                anyhow::anyhow!(
                    "source `{}` has invalid cron expression `{expr}`: {e}",
                    src.name()
                )
            })?;
            Ok(Schedule::Cron(Box::new(cron)))
        } else {
            let effective_interval = src.effective_interval();
            let retry_interval = src.effective_retry_interval();
            let next =
                first_interval_tick(db, src.name(), effective_interval, retry_interval).await?;
            Ok(Schedule::Interval {
                effective_interval,
                retry_interval,
                next,
            })
        }
    }

    async fn tick(&mut self) {
        match self {
            Schedule::Interval { next, .. } => {
                tokio::time::sleep_until(*next).await;
            }
            Schedule::Cron(cron) => {
                let delta = next_cron_delay(cron, chrono::Utc::now());
                tokio::time::sleep_until(tokio::time::Instant::now() + delta).await;
            }
        }
    }

    /// Sets an interval-scheduled source's next wait: `retry_interval` when
    /// `retry` is true (the just-attempted fetch failed), `interval`
    /// otherwise (fetch succeeded, or setup isn't satisfied yet — setup
    /// retries stay on the normal schedule tick, spec: data-collection —
    /// setup gates first fetch). No-op for a cron schedule, whose next tick
    /// is always the next cron occurrence regardless of outcome.
    fn advance(&mut self, retry: bool) {
        if let Schedule::Interval {
            effective_interval,
            retry_interval,
            next,
        } = self
        {
            let wait = if retry {
                *retry_interval
            } else {
                *effective_interval
            };
            *next = tokio::time::Instant::now() + wait;
        }
    }
}

/// Pure delay-until-next-occurrence computation for a cron schedule,
/// factored out of `Schedule::tick` so it's independently testable. Always
/// recomputed fresh from `now` rather than advanced from a stored baseline,
/// so a wall-clock adjustment (NTP step) can skew at most the delay for the
/// tick in progress — it never accumulates across ticks, since every tick
/// re-derives its target from the current wall clock.
fn next_cron_delay(cron: &Cron, now: chrono::DateTime<chrono::Utc>) -> std::time::Duration {
    cron.find_next_occurrence(&now, false)
        .ok()
        .and_then(|next| (next - now).to_std().ok())
        .unwrap_or(std::time::Duration::from_mins(1))
}

/// An interval-scheduled source's first tick after collector startup,
/// resumed from its most recent fetch log entry: due immediately if it has
/// never run; otherwise deferred until the remaining window elapses —
/// `interval` after a success, `retry_interval` after a failure (error
/// detail present) — or due immediately when already overdue (spec:
/// data-collection — Per-source schedules: daemon startup resumes from the
/// last run).
async fn first_interval_tick(
    db: &Db,
    source: &str,
    interval: std::time::Duration,
    retry_interval: std::time::Duration,
) -> Result<tokio::time::Instant> {
    let Some(last) = db.last_attempt(source).await? else {
        return Ok(tokio::time::Instant::now());
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs_f64();
    let age = now - last.ts_epoch;
    let wait_secs = if last.error.is_none() {
        interval.as_secs_f64()
    } else {
        retry_interval.as_secs_f64()
    };
    Ok(if age >= wait_secs {
        tokio::time::Instant::now()
    } else {
        tokio::time::Instant::now() + std::time::Duration::from_secs_f64(wait_secs - age)
    })
}

/// A message to one source's collector task.
pub enum Control {
    /// Re-arm an interval wait from now, as an ingested value does (spec:
    /// data-collection — Ingested values reset interval schedules). A no-op
    /// for a cron schedule.
    ResetSchedule,
    /// Fetch now regardless of the schedule, and reply with the outcome for
    /// the named source (spec: data-collection — Forced polls are
    /// serialized with a source's schedule). For an ordinary source this is
    /// always that source's own name; a composite root and every one of its
    /// children share one task and one channel, so the name says which of
    /// them the caller actually asked about — the fetch itself always runs
    /// the whole family exactly once regardless (spec: data-collection —
    /// Force polling a composite source or its children). A dropped reply
    /// channel is fine: the fetch still ran and was recorded, only nobody is
    /// listening for the result any more.
    PollNow(String, tokio::sync::oneshot::Sender<PollOutcome>),
}

pub type ControlSender = tokio::sync::mpsc::UnboundedSender<Control>;
pub type ControlReceiver = tokio::sync::mpsc::UnboundedReceiver<Control>;

/// Control wiring between the HTTP handlers and collector tasks: one
/// channel per *pollable* source — every query source, interval- or
/// cron-scheduled. Stream and ingest sources are excluded: neither has a
/// fetch a message could ask for (a stream is a continuously running
/// process, an ingest source receives data only by HTTP push), which makes
/// "has no channel" the one place that knows a source cannot be polled.
pub struct ControlHub {
    pub txs: HashMap<String, ControlSender>,
    pub rxs: HashMap<String, ControlReceiver>,
}

/// Why this source cannot be force-polled, or `None` when it can (spec: cli
/// — Force poll rejects sources with nothing to fetch). An `ingest` source
/// receives data only by HTTP push; a `stream` source is one continuously
/// running process rather than a per-tick fetch. Shared by the HTTP
/// endpoint and the CLI so both reject the same sources for the same
/// stated reason.
#[must_use]
pub fn unpollable_reason(src: &SourceCfg) -> Option<&'static str> {
    if src.is_ingest() {
        Some("receives data via HTTP push, not by fetching")
    } else if src.is_stream() {
        Some("is collected continuously, not by fetching")
    } else {
        None
    }
}

/// Sources the collector drives with a task: every source except `ingest`
/// (which has no schedule and receives data only via HTTP push, spec:
/// data-collection — Ingest sources have no collector task) and a composite
/// source's `Child` entries, which have no command or schedule of their
/// own — their composite root's task fetches for the whole family (spec:
/// source-configuration — Composite source children). Shared by every entry
/// point that iterates `cfg.sources` so the exclusion lives in exactly one
/// place.
fn collectible(cfg: &Config) -> impl Iterator<Item = &SourceCfg> {
    cfg.sources.iter().filter(|src| !src.is_ingest() && !src.is_child())
}

/// Builds the [`ControlHub`] for a config. Fails on a source whose kind
/// cannot be built, mirroring what its collector task would report at
/// startup. A composite root's channel is also registered under every one
/// of its children's full names — all pointing at the same task, since
/// forcing a child fans out to the whole family exactly like forcing the
/// root does (spec: data-collection — Force polling a composite source or
/// its children).
pub fn control_channels(cfg: &Config) -> Result<ControlHub> {
    let mut hub = ControlHub {
        txs: HashMap::new(),
        rxs: HashMap::new(),
    };
    for src in collectible(cfg) {
        if !matches!(source::build(src)?, source::SourceKind::Stream { .. }) {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            hub.txs.insert(src.name().to_string(), tx.clone());
            hub.rxs.insert(src.name().to_string(), rx);
            for child in crate::config::composite_children(cfg, src.name()) {
                hub.txs.insert(child.name().to_string(), tx.clone());
            }
        }
    }
    Ok(hub)
}

/// Spawns one independent task per source so a hanging fetch cannot block
/// others (spec: data-collection — failure isolation). Fire-and-forget, with
/// no cooperative shutdown — used by tests and one-shot callers. The daemon
/// uses [`spawn_graceful`] instead.
pub fn spawn_all(db: &Db, cfg: &Config) {
    for src in collectible(cfg) {
        let db = db.clone();
        let src = src.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let name = src.name().to_string();
            if let Err(e) = loop_source(db, src, cfg, None, None).await {
                tracing::error!("collector for `{name}` stopped: {e:#}");
            }
        });
    }
}

/// Same as [`spawn_all`], but each task stops after finishing its
/// currently-scheduled tick once `shutdown` reports `true`, instead of being
/// hard-killed by process exit mid-fetch/mid-write (spec: data-collection —
/// daemon shutdown lets in-flight collection finish). Returns the tasks'
/// handles so the daemon can await them after the HTTP server's own graceful
/// shutdown completes. Consumes the [`ControlHub`]'s receivers, one per
/// pollable source.
#[must_use]
pub fn spawn_graceful(
    db: &Db,
    cfg: &Config,
    shutdown: &tokio::sync::watch::Receiver<bool>,
    hub: ControlHub,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut rxs = hub.rxs;
    collectible(cfg)
        .map(|src| {
            let db = db.clone();
            let src = src.clone();
            let cfg = cfg.clone();
            let shutdown = shutdown.clone();
            let control = rxs.remove(src.name());
            tokio::spawn(async move {
                let name = src.name().to_string();
                if let Err(e) = loop_source(db, src, cfg, Some(shutdown), control).await {
                    tracing::error!("collector for `{name}` stopped: {e:#}");
                }
            })
        })
        .collect()
}
/// Drives one non-`ingest` source's collection for the task's lifetime.
/// Callers (`spawn_all`, `spawn_graceful`) only ever reach this through
/// [`collectible`], so `src` is never `ingest` here.
async fn loop_source(
    db: Db,
    src: SourceCfg,
    cfg: Config,
    mut shutdown: Option<tokio::sync::watch::Receiver<bool>>,
    mut control: Option<ControlReceiver>,
) -> Result<()> {
    let kind = source::build(&src)?;
    // Re-derive the last known status so restarts don't duplicate transitions.
    let mut last_status = db
        .last_health(src.name())
        .await?
        .unwrap_or_else(|| "healthy".into());
    // A declared setup command runs once before the first fetch; on failure it
    // is retried on this source's schedule tick instead of fetching. Stream
    // sources retry setup on `retry_interval` instead of opening the stream.
    let mut setup_done = src.setup().is_none();


    if matches!(kind, source::SourceKind::Stream { .. }) {
        return loop_stream(
            db,
            src,
            cfg,
            &mut last_status,
            &mut setup_done,
            &mut shutdown,
        )
        .await;
    }

    let mut schedule = Schedule::new(&db, &src).await?;
    loop {
        match wait_for_work(&mut schedule, &mut shutdown, &mut control).await {
            Work::Stop => return Ok(()),
            Work::Nothing => {}
            // Once every sender is gone (the HTTP handlers' `AppState`
            // dropped), the receiver resolves to `None` *immediately and
            // forever* — so it is dropped rather than re-armed, or that
            // branch would spin the select.
            Work::ControlClosed => control = None,
            // An ingested value moves the interval wait to the success path
            // without fetching (spec: data-collection — Ingested values
            // reset interval schedules). `advance` is a no-op for cron.
            Work::ResetSchedule => schedule.advance(false),
            Work::Tick => {
                attempt(
                    &db,
                    &cfg,
                    &src,
                    &kind,
                    &mut last_status,
                    &mut setup_done,
                    &mut schedule,
                    src.name(),
                )
                .await;
            }
            // A forced poll runs here, on this source's own task, so it can
            // never overlap that source's scheduled fetch and its schedule
            // restarts from the forced attempt (spec: data-collection —
            // Forced polls are serialized with a source's schedule). The
            // requested name is the caller's own — this source's name for an
            // ordinary source, or possibly a child's full name when `src` is
            // a composite root, since a child's control messages arrive on
            // this same task (spec: data-collection — Force polling a
            // composite source or its children).
            Work::PollNow(requested, reply) => {
                let outcome = attempt(
                    &db,
                    &cfg,
                    &src,
                    &kind,
                    &mut last_status,
                    &mut setup_done,
                    &mut schedule,
                    &requested,
                )
                .await;
                let _ = reply.send(outcome);
            }
        }
    }
}

/// What [`loop_source`] woke up to do.
enum Work {
    /// The schedule came due.
    Tick,
    /// Shut down after this.
    Stop,
    /// Woke for something that needs no action (a shutdown channel that
    /// changed without becoming `true`).
    Nothing,
    /// Every control sender is gone; stop listening on that channel.
    ControlClosed,
    ResetSchedule,
    PollNow(String, tokio::sync::oneshot::Sender<PollOutcome>),
}

/// Waits for whichever comes first: this source's schedule, a control
/// message, or shutdown. A `None` shutdown/control (one-shot callers, which
/// have neither) simply never fires.
async fn wait_for_work(
    schedule: &mut Schedule,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
    control: &mut Option<ControlReceiver>,
) -> Work {
    tokio::select! {
        () = schedule.tick() => Work::Tick,
        stop = async {
            match shutdown.as_mut() {
                // `Err` means every sender was dropped, which
                // `spawn_graceful` never does before the daemon exits — but
                // never busy-loop on it regardless.
                Some(sd) => sd.changed().await.is_err() || *sd.borrow(),
                None => std::future::pending().await,
            }
        } => {
            if stop { Work::Stop } else { Work::Nothing }
        }
        msg = async {
            match control.as_mut() {
                Some(rx) => rx.recv().await,
                None => std::future::pending().await,
            }
        } => match msg {
            Some(Control::ResetSchedule) => Work::ResetSchedule,
            Some(Control::PollNow(name, reply)) => Work::PollNow(name, reply),
            None => Work::ControlClosed,
        },
    }
}

/// One attempt at a source, shared by its scheduled ticks and its forced
/// polls so the two cannot drift: run the declared setup command first if it
/// hasn't succeeded yet, then fetch, then re-arm the schedule from now —
/// `retry_interval` after a failed fetch, `interval` otherwise (setup
/// retries stay on the normal tick, spec: data-collection — setup gates
/// first fetch).
///
/// `src` is always this task's own source — a composite root's task never
/// changes what it fetches — but `requested` names whichever of {`src`, one
/// of its children} the caller actually asked about, so the returned
/// [`PollOutcome`] is scoped to that name even though the fetch always runs
/// the whole family (spec: data-collection — Force polling a composite
/// source or its children). For an ordinary source `requested` is always
/// `src.name()`.
#[allow(clippy::too_many_arguments)] // one per piece of task-local schedule/setup state
async fn attempt(
    db: &Db,
    cfg: &Config,
    src: &SourceCfg,
    kind: &source::SourceKind,
    last_status: &mut String,
    setup_done: &mut bool,
    schedule: &mut Schedule,
    requested: &str,
) -> PollOutcome {
    if !*setup_done && !try_setup(db, src, cfg).await {
        schedule.advance(false);
        return PollOutcome::failed(requested, "setup command failed".into());
    }
    *setup_done = true;
    if src.is_composite() {
        let result = composite_fetch_once(db, cfg, src, last_status).await;
        schedule.advance(!result.root.success);
        if requested == src.name() {
            result.root
        } else {
            result.children.get(requested).cloned().unwrap_or_else(|| {
                PollOutcome::failed(requested, "not a declared child of this composite".into())
            })
        }
    } else {
        let outcome = fetch_once(db, cfg, src, kind, last_status).await;
        schedule.advance(!outcome.success);
        outcome
    }
}

/// Both halves of one composite fetch (spec: data-collection — Composite
/// source command output): the root's own command/parse outcome, and every
/// declared child's resulting outcome, keyed by full name.
struct CompositeResult {
    root: PollOutcome,
    children: HashMap<String, PollOutcome>,
}

/// Runs a composite source's command once, parses its stdout as a JSON array
/// of ingest-shaped items, and stores each declared child's value exactly as
/// `POST /api/ingest` would for it (spec: data-collection — Composite source
/// command output). An item naming an id that isn't a declared child fails
/// the whole attempt; a declared child simply absent from the array fails
/// only that child (spec: data-collection — Composite fan-out error
/// handling). Marks the root and every declared child polling for the
/// duration of the one command (spec: data-collection — Force polling a
/// composite source or its children).
async fn composite_fetch_once(
    db: &Db,
    cfg: &Config,
    root: &SourceCfg,
    last_status: &mut String,
) -> CompositeResult {
    let name = root.name();
    let declared = crate::config::composite_children(cfg, name);
    let _guards: Vec<_> = std::iter::once(name)
        .chain(declared.iter().map(|c| c.name()))
        .map(|n| db.mark_polling(n))
        .collect();

    let start = Instant::now();
    let run = tokio::time::timeout(root.timeout(), source::run_shell(root.command(), &cfg.config_dir)).await;
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let ms = start.elapsed().as_millis() as i64;

    let failed_children = |msg: &str| {
        declared
            .iter()
            .map(|c| (c.name().to_string(), PollOutcome::failed(c.name(), msg.to_string())))
            .collect()
    };

    let stdout = match run {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            record_failure(db, name, ms, &msg).await;
            refresh_health(db, cfg, name, last_status).await;
            return CompositeResult {
                root: PollOutcome::failed(name, msg),
                children: failed_children("composite command failed"),
            };
        }
        Err(_) => {
            let msg = format!(
                "timed out after {}",
                humantime::format_duration(root.timeout())
            );
            record_failure(db, name, ms, &msg).await;
            refresh_health(db, cfg, name, last_status).await;
            return CompositeResult {
                root: PollOutcome::failed(name, msg),
                children: failed_children("composite command timed out"),
            };
        }
    };

    let items: Vec<source::IngestItem> = match serde_json::from_str(&stdout) {
        Ok(items) => items,
        Err(e) => {
            let msg = format!("composite output is not a JSON array of ingest items: {e}");
            record_failure(db, name, ms, &msg).await;
            refresh_health(db, cfg, name, last_status).await;
            return CompositeResult {
                root: PollOutcome::failed(name, msg),
                children: failed_children("composite output was malformed"),
            };
        }
    };

    if let Some(bad) = items.iter().find(|it| !declared.iter().any(|c| c.name() == it.source)) {
        let msg = format!("composite output names unknown child `{}`", bad.source);
        record_failure(db, name, ms, &msg).await;
        refresh_health(db, cfg, name, last_status).await;
        return CompositeResult {
            root: PollOutcome::failed(name, msg),
            children: failed_children("a sibling entry failed validation"),
        };
    }

    if let Err(e) = db.insert_log(name, ms, None, None, Origin::Poll).await {
        tracing::error!("insert log `{name}`: {e:#}");
    }
    refresh_health(db, cfg, name, last_status).await;

    CompositeResult {
        root: PollOutcome::composite_root_ok(name),
        children: store_composite_children(db, cfg, &declared, &items, ms).await,
    }
}

/// Stores each declared child's value from a composite fetch's already-parsed
/// items — one `PollOutcome` per child, keyed by full name (spec:
/// data-collection — Composite fan-out error handling). A child present in
/// `items` is converted, validated, and stored exactly as `POST /api/ingest`
/// would; a declared child absent from `items` is recorded as a failed
/// attempt for just that child.
async fn store_composite_children(
    db: &Db,
    cfg: &Config,
    declared: &[&SourceCfg],
    items: &[source::IngestItem],
    ms: i64,
) -> HashMap<String, PollOutcome> {
    let arrival = crate::db::now();
    let mut children = HashMap::new();
    for child in declared {
        let cname = child.name();
        let Some(item) = items.iter().find(|it| it.source == cname) else {
            let msg = "missing from composite output".to_string();
            record_failure(db, cname, ms, &msg).await;
            refresh_health_opt(db, cfg, cname, None).await;
            children.insert(cname.to_string(), PollOutcome::failed(cname, msg));
            continue;
        };
        let outcome = match source::resolve_ingest_item(item, arrival.clone(), child.effective_value_type())
        {
            Ok(parsed) => {
                match store_parsed_value(db, cfg, child, None, &parsed, ms, Origin::Poll).await {
                    Ok(()) => PollOutcome::stored(cname, &parsed),
                    Err(e) => PollOutcome::failed(cname, e),
                }
            }
            Err(e) => {
                record_failure(db, cname, ms, &e).await;
                refresh_health_opt(db, cfg, cname, None).await;
                PollOutcome::failed(cname, e)
            }
        };
        children.insert(cname.to_string(), outcome);
    }
    children
}

/// Continuous ingest for one stream source (spec: data-collection — Stream
/// collection): setup first (retried on `retry_interval` while failing),
/// then open → ingest lines → on exit, log the outcome and reopen after
/// `retry_interval`. Shutdown stops reopening after the current wait and
/// kills the child instead of orphaning it.
async fn loop_stream(
    db: Db,
    src: SourceCfg,
    cfg: Config,
    last_status: &mut String,
    setup_done: &mut bool,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<()> {
    let retry = src.effective_retry_interval();
    loop {
        if !*setup_done {
            if !try_setup(&db, &src, &cfg).await {
                if sleep_or_stopped(shutdown, retry).await {
                    return Ok(());
                }
                continue;
            }
            *setup_done = true;
        }
        if is_stopped(shutdown.as_ref()) {
            return Ok(());
        }
        ingest_stream_run(&db, &src, &cfg, last_status, shutdown).await;
        if sleep_or_stopped(shutdown, retry).await {
            return Ok(());
        }
    }
}

/// The latest broadcast shutdown value, without consuming anything.
/// `None` (one-shot callers) never reports shutdown.
fn is_stopped(shutdown: Option<&tokio::sync::watch::Receiver<bool>>) -> bool {
    shutdown.is_some_and(|sd| *sd.borrow())
}

/// Sleeps `wait` unless shutdown arrives first; returns true when the
/// caller should stop. `None` always sleeps the full wait.
async fn sleep_or_stopped(
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
    wait: std::time::Duration,
) -> bool {
    let Some(sd) = shutdown else {
        tokio::time::sleep(wait).await;
        return false;
    };
    if *sd.borrow() {
        return true;
    }
    tokio::select! {
        () = tokio::time::sleep(wait) => false,
        res = sd.changed() => res.is_err() || *sd.borrow(),
    }
}

/// One open → ingest → exit cycle for a stream source: every well-formed
/// `jsonl` line becomes a reading, malformed lines are logged as failures
/// without killing the stream, and the process exit is logged before
/// returning to [`loop_stream`] for the reopen wait.
async fn ingest_stream_run(
    db: &Db,
    src: &SourceCfg,
    cfg: &Config,
    last_status: &mut String,
    shutdown: &mut Option<tokio::sync::watch::Receiver<bool>>,
) {
    let name = src.name().to_string();
    let start = Instant::now();
    let mut proc = match source::StreamProc::spawn(src.command(), &cfg.config_dir) {
        Ok(p) => p,
        Err(e) => {
            record_failure(db, &name, 0, &format!("stream spawn: {e:#}")).await;
            refresh_health(db, cfg, &name, last_status).await;
            return;
        }
    };
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let elapsed_ms = || start.elapsed().as_millis() as i64;
    loop {
        let line = match shutdown {
            Some(sd) => {
                tokio::select! {
                    line = proc.next_line() => line,
                    res = sd.changed() => {
                        if res.is_err() || *sd.borrow() {
                            proc.shutdown().await;
                            return;
                        }
                        continue;
                    }
                }
            }
            None => proc.next_line().await,
        };
        match line {
            Ok(Some(text)) => {
                ingest_stream_line(db, cfg, src, last_status, &text).await;
            }
            Ok(None) => break,
            Err(e) => {
                record_failure(db, &name, elapsed_ms(), &format!("stream read: {e:#}")).await;
                refresh_health(db, cfg, &name, last_status).await;
                break;
            }
        }
    }
    // Raced against shutdown the same way the read loop above is: the
    // process's stdout closing (natural `Ok(None)` EOF) doesn't guarantee
    // the process itself exits promptly — a hung or lingering child would
    // otherwise block this `.await` (and so the whole task, and so the
    // daemon's graceful shutdown, which awaits every collector task)
    // indefinitely.
    let exit = match shutdown {
        Some(sd) if !*sd.borrow() => {
            tokio::select! {
                status = proc.wait_for_exit() => status,
                res = sd.changed() => {
                    let _ = res;
                    proc.shutdown().await;
                    return;
                }
            }
        }
        Some(_) => {
            proc.shutdown().await;
            return;
        }
        None => proc.wait_for_exit().await,
    };
    match exit {
        Ok(status) if status.success() => {
            if let Err(e) = db
                .insert_log(&name, elapsed_ms(), None, None, Origin::Poll)
                .await
            {
                tracing::error!("insert log `{name}`: {e:#}");
            }
            refresh_health(db, cfg, &name, last_status).await;
        }
        Ok(status) => {
            record_failure(db, &name, elapsed_ms(), &format!("stream exited: {status}")).await;
            refresh_health(db, cfg, &name, last_status).await;
        }
        Err(e) => {
            record_failure(db, &name, elapsed_ms(), &format!("stream wait: {e:#}")).await;
            refresh_health(db, cfg, &name, last_status).await;
        }
    }
}

/// Ingests one stream line: parses it as plain-or-`jsonl`, converts and
/// stores the value, applies a valid threshold override, and refreshes
/// health. Malformed rows and conversion failures are logged as failures
/// with no reading, without disturbing the running stream.
async fn ingest_stream_line(
    db: &Db,
    cfg: &Config,
    src: &SourceCfg,
    last_status: &mut String,
    text: &str,
) {
    let name = src.name();
    let start = Instant::now();
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let elapsed_ms = || start.elapsed().as_millis() as i64;
    let (arr_epoch, arr_ts) = crate::db::now();
    let parsed = match source::parse_output(name, text, (arr_epoch, arr_ts)) {
        Ok(p) => p,
        Err(e) => {
            record_failure(db, name, elapsed_ms(), &format!("{e:#}")).await;
            refresh_health(db, cfg, name, last_status).await;
            return;
        }
    };
    let _ = store_parsed_value(
        db,
        cfg,
        src,
        Some(last_status),
        &parsed,
        elapsed_ms(),
        Origin::Poll,
    )
    .await;
}

/// Converts, stores, and logs one already-parsed value (shared by
/// [`fetch_once`] and [`ingest_stream_line`]); applies a valid threshold
/// override and refreshes health. `Ok` means the round trip counts as
/// successful for scheduling purposes; `Err` carries the same message that
/// was recorded as the attempt's failure, so a caller reporting the outcome
/// (a forced poll) doesn't have to reconstruct it.
pub(crate) async fn store_parsed_value(
    db: &Db,
    cfg: &Config,
    src: &SourceCfg,
    last_status: Option<&mut String>,
    parsed: &source::ParsedOutput,
    ms: i64,
    origin: Origin,
) -> std::result::Result<(), String> {
    let name = src.name();
    match source::convert_value_type(&parsed.value, src.effective_value_type()) {
        Ok((value_bigint, value_double, value_json)) => {
            let stored = db
                .insert_reading_at(
                    name,
                    &parsed.value,
                    src.unit(),
                    value_bigint,
                    value_double,
                    value_json.as_deref(),
                    parsed.ts_epoch,
                    &parsed.ts,
                )
                .await;
            if let Err(e) = &stored {
                tracing::error!("insert reading `{name}`: {e:#}");
            }
            match &stored {
                Ok(()) => {
                    if let Err(e) = db
                        .insert_log(name, ms, None, Some(&parsed.value), origin)
                        .await
                    {
                        tracing::error!("insert log `{name}`: {e:#}");
                    }
                    if let Some(bands) = &parsed.threshold {
                        db.set_session_bands(name, bands);
                    }
                    refresh_health_opt(db, cfg, name, last_status).await;
                    Ok(())
                }
                Err(e) => {
                    let msg = format!("fetched but failed to store: {e:#}");
                    record_failure(db, name, ms, &msg).await;
                    refresh_health_opt(db, cfg, name, last_status).await;
                    Err(msg)
                }
            }
        }
        Err(e) => {
            let msg = format!("{e:#}");
            record_failure(db, name, ms, &msg).await;
            refresh_health_opt(db, cfg, name, last_status).await;
            Err(msg)
        }
    }
}

/// Recomputes health and records a transition event when the status
/// changed, so restarts and steady state share one code path.
async fn refresh_health(db: &Db, cfg: &Config, name: &str, last_status: &mut String) {
    match health::compute(db, cfg, name).await {
        Ok(h) => {
            if h.status.as_str() != *last_status {
                if let Err(e) = db.insert_health_event(name, h.status.as_str()).await {
                    tracing::error!("insert health event `{name}`: {e:#}");
                }
                *last_status = h.status.as_str().to_string();
            }
        }
        Err(e) => tracing::error!("health compute `{name}`: {e:#}"),
    }
}

/// [`refresh_health`] for callers without task-local status memory (the
/// HTTP ingest handler): re-derives the previous status from the database
/// the way collector startup does, so transitions are still recorded once.
pub(crate) async fn refresh_health_opt(
    db: &Db,
    cfg: &Config,
    name: &str,
    last_status: Option<&mut String>,
) {
    if let Some(ls) = last_status {
        refresh_health(db, cfg, name, ls).await;
    } else {
        let mut prev = db
            .last_health(name)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| "healthy".into());
        refresh_health(db, cfg, name, &mut prev).await;
    }
}

/// One collection round over every query source, honoring setup gates;
/// used by tests and one-shot runs. Stream sources are skipped: they are
/// continuous processes, not per-tick fetches.
pub async fn collect_once(db: &Db, cfg: &Config) {
    for src in collectible(cfg).filter(|src| !src.is_stream()) {
        poll_once(db, cfg, src).await;
    }
}

/// Fetches one source now, ignoring its schedule, and stores the result the
/// way a scheduled fetch does (spec: cli — Force poll command). This is the
/// direct-mode CLI's path: no collector task exists in that process, and
/// the database file lock excludes every other one, so there is nothing to
/// serialize against. Under a running daemon the source's own collector
/// task does this instead (spec: data-collection — Forced polls are
/// serialized with a source's schedule).
///
/// `src` may be a composite root, one of its children, or an ordinary
/// source. A child's own command is its composite root's — this runs that
/// root's command once, fans out to every declared child, and returns just
/// `src`'s own resulting outcome (spec: data-collection — Force polling a
/// composite source or its children).
pub async fn poll_once(db: &Db, cfg: &Config, src: &SourceCfg) -> PollOutcome {
    let name = src.name();
    if let Some(parent) = src.parent() {
        let Some(root) = cfg.sources.iter().find(|s| s.name() == parent) else {
            return PollOutcome::failed(name, format!("composite root `{parent}` not found"));
        };
        let mut last_status = db
            .last_health(parent)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| "healthy".into());
        let mut result = composite_fetch_once(db, cfg, root, &mut last_status).await;
        return result
            .children
            .remove(name)
            .unwrap_or_else(|| PollOutcome::failed(name, "child missing from composite result".into()));
    }
    if src.is_composite() {
        let mut last_status = db
            .last_health(name)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| "healthy".into());
        return composite_fetch_once(db, cfg, src, &mut last_status).await.root;
    }
    let kind = match source::build(src) {
        Ok(k) => k,
        Err(e) => return PollOutcome::failed(name, format!("{e:#}")),
    };
    if !try_setup(db, src, cfg).await {
        return PollOutcome::failed(name, "setup command failed".into());
    }
    let mut last_status = db
        .last_health(name)
        .await
        .unwrap_or(None)
        .unwrap_or_else(|| "healthy".into());
    fetch_once(db, cfg, src, &kind, &mut last_status).await
}

/// Runs a source's setup command. Returns true when setup is satisfied
/// (success or not declared); failures are logged and reported as false.
async fn try_setup(db: &Db, src: &SourceCfg, cfg: &Config) -> bool {
    let Some(cmd) = src.setup() else {
        return true;
    };
    let start = Instant::now();
    let outcome = timeout(src.timeout(), source::run_shell(cmd, &cfg.config_dir)).await;
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let ms = start.elapsed().as_millis() as i64;
    let name = src.name();
    match outcome {
        Ok(Ok(_)) => {
            tracing::info!("setup for `{name}` succeeded");
            true
        }
        Ok(Err(e)) => {
            record_failure(db, name, ms, &format!("setup: {e:#}")).await;
            false
        }
        Err(_) => {
            record_failure(
                db,
                name,
                ms,
                &format!(
                    "setup: timed out after {}",
                    humantime::format_duration(src.timeout())
                ),
            )
            .await;
            false
        }
    }
}

/// One fetch attempt's result, as reported to whoever asked for it (spec:
/// http-api — Force poll endpoint; cli — Force poll command). The same
/// shape crosses the HTTP boundary and is built locally by a direct-mode
/// CLI poll, so both transports report a poll identically — and a caller
/// can always tell a *fetch* that ran and failed (`success: false`, with
/// `error`) from a daemon it could not reach at all (a transport error).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PollOutcome {
    pub source: String,
    pub success: bool,
    /// The stored value, on success.
    pub value: Option<String>,
    /// The stored reading's timestamp, on success.
    pub ts: Option<String>,
    /// Why the attempt failed, on failure.
    pub error: Option<String>,
}

impl PollOutcome {
    fn stored(source: &str, parsed: &source::ParsedOutput) -> Self {
        Self {
            source: source.to_string(),
            success: true,
            value: Some(parsed.value.clone()),
            ts: Some(parsed.ts.clone()),
            error: None,
        }
    }

    pub(crate) fn failed(source: &str, error: String) -> Self {
        Self {
            source: source.to_string(),
            success: false,
            value: None,
            ts: None,
            error: Some(error),
        }
    }

    /// A composite root's own outcome: its command ran and its stdout parsed
    /// as the expected JSON array. Carries no `value`/`ts` — the root itself
    /// never stores a reading, only its declared children do (spec:
    /// data-collection — Composite source command output).
    fn composite_root_ok(source: &str) -> Self {
        Self {
            source: source.to_string(),
            success: true,
            value: None,
            ts: None,
            error: None,
        }
    }
}

/// Runs one fetch attempt, logging its outcome and updating health. The
/// returned outcome's `success` is whether the round trip should be treated
/// as successful for scheduling purposes (spec: data-collection —
/// per-source schedules): the upstream fetch succeeded *and* the reading was
/// durably stored. A fetch that succeeds but fails to persist is recorded as
/// a failed attempt, not a silent success — otherwise health would report
/// "healthy" for data that was never actually written.
///
/// This is the single place both a scheduled tick (via `attempt`) and a
/// direct-mode forced poll (via `poll_once`) run a source's command, so it's
/// also the single place that marks the source "polling" for the web
/// dashboard and TUI to show (spec: data-collection — Poll-in-progress is
/// visible). The mark covers the command itself, not `try_setup` — setup
/// only runs once per source's lifetime and failing it is already visible
/// as a failed fetch log entry, so it doesn't need its own indicator.
pub async fn fetch_once(
    db: &Db,
    cfg: &Config,
    src: &SourceCfg,
    kind: &source::SourceKind,
    last_status: &mut String,
) -> PollOutcome {
    let _polling = db.mark_polling(src.name());
    let start = Instant::now();
    let outcome = tokio::time::timeout(src.timeout(), kind.fetch(&cfg.config_dir)).await;
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let ms = start.elapsed().as_millis() as i64;
    let name = src.name();
    match &outcome {
        Ok(Ok(output)) => {
            let (arr_epoch, arr_ts) = crate::db::now();
            match source::parse_output(name, output, (arr_epoch, arr_ts)) {
                Ok(parsed) => {
                    match store_parsed_value(
                        db,
                        cfg,
                        src,
                        Some(last_status),
                        &parsed,
                        ms,
                        Origin::Poll,
                    )
                    .await
                    {
                        Ok(()) => PollOutcome::stored(name, &parsed),
                        Err(e) => PollOutcome::failed(name, e),
                    }
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    record_failure(db, name, ms, &msg).await;
                    refresh_health(db, cfg, name, last_status).await;
                    PollOutcome::failed(name, msg)
                }
            }
        }
        Ok(Err(e)) => {
            let msg = format!("{e:#}");
            record_failure(db, name, ms, &msg).await;
            refresh_health(db, cfg, name, last_status).await;
            PollOutcome::failed(name, msg)
        }
        Err(_) => {
            let msg = format!(
                "timed out after {}",
                humantime::format_duration(src.timeout())
            );
            record_failure(db, name, ms, &msg).await;
            refresh_health(db, cfg, name, last_status).await;
            PollOutcome::failed(name, msg)
        }
    }
}

async fn record_failure(db: &Db, name: &str, ms: i64, error: &str) {
    if let Err(e) = db
        .insert_log(name, ms, Some(error), None, Origin::Poll)
        .await
    {
        tracing::error!("insert log `{name}`: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// `PollOutcome`'s field names are what the HTTP endpoint writes and
    /// what a daemon-mode CLI reads back (spec: http-api — Force poll
    /// endpoint), so they are part of the contract rather than an
    /// implementation detail — including that a failed *fetch* still
    /// round-trips as a complete outcome.
    #[test]
    fn poll_outcome_serializes_the_documented_fields() {
        let parsed = source::ParsedOutput {
            value: "42".into(),
            ts_epoch: 1000.5,
            ts: "2024-01-01T00:00:00+00:00".into(),
            threshold: None,
        };
        let ok = serde_json::to_value(PollOutcome::stored("cpu", &parsed)).unwrap();
        assert_eq!(ok["source"], "cpu");
        assert_eq!(ok["success"], true);
        assert_eq!(ok["value"], "42");
        assert_eq!(ok["ts"], "2024-01-01T00:00:00+00:00");
        assert!(ok["error"].is_null());

        let failed = PollOutcome::failed("cpu", "boom".into());
        let wire = serde_json::to_value(&failed).unwrap();
        assert_eq!(wire["success"], false);
        assert_eq!(wire["error"], "boom");
        assert!(wire["value"].is_null());

        let back: PollOutcome = serde_json::from_value(wire).unwrap();
        assert_eq!(back.source, "cpu");
        assert!(!back.success);
        assert_eq!(back.error.as_deref(), Some("boom"));
    }

    /// (spec: data-collection — cron-scheduled source ignores fetch outcome)
    #[test]
    fn cron_delay_targets_the_next_occurrence() {
        let cron: Cron = "* * * * * *".parse().unwrap();
        let now = chrono::Utc::now();
        let delay = next_cron_delay(&cron, now);
        assert!(
            delay <= std::time::Duration::from_secs(1),
            "a per-second cron should fire within a second: {delay:?}"
        );
    }

    #[test]
    fn cron_delay_falls_back_when_no_future_occurrence() {
        // An expression `find_next_occurrence` can't resolve (shouldn't happen
        // for a validated config, but the fallback must still be sane).
        let cron: Cron = "* * * * * *".parse().unwrap();
        let far_future = chrono::DateTime::<chrono::Utc>::MAX_UTC;
        let delay = next_cron_delay(&cron, far_future);
        assert_eq!(delay, std::time::Duration::from_mins(1));
    }

    fn cron_source(name: &str, expr: &str) -> SourceCfg {
        toml::from_str(&format!(
            "name = \"{name}\"\ntype = \"query\"\ncommand = \"echo 0\"\ncron = \"{expr}\"\n"
        ))
        .unwrap()
    }

    fn query_source(name: &str, command: &str, value_type: Option<&str>) -> SourceCfg {
        let vt = value_type
            .map(|v| format!("value_type = \"{v}\"\n"))
            .unwrap_or_default();
        toml::from_str(&format!(
            "name = \"{name}\"\ntype = \"query\"\ncommand = \"{command}\"\n{vt}"
        ))
        .unwrap()
    }

    /// A valid value converts and lands in exactly the typed column matching
    /// the source's declared `value_type` (spec: source-configuration —
    /// configurable stored value type).
    #[tokio::test]
    async fn fetch_once_stores_typed_value_matching_source_value_type() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();

        for (name, command, value_type) in [
            ("bi", "echo 42", "bigint"),
            ("db", "echo 98.6", "double"),
            ("js", "echo true", "json"),
        ] {
            let src = query_source(name, command, Some(value_type));
            let kind = source::build(&src).unwrap();
            let mut status = "healthy".to_string();
            let outcome = fetch_once(&db, &cfg, &src, &kind, &mut status).await;
            assert!(outcome.success, "{name} fetch should succeed");
        }

        let conn = duckdb::Connection::open(dir.path().join("t.duckdb")).unwrap();
        let row = |source: &str| -> (Option<i64>, Option<f64>, Option<String>) {
            conn.query_row(
                "SELECT value_bigint, value_double, value_json FROM readings WHERE source = ?",
                duckdb::params![source],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?)),
            )
            .unwrap()
        };
        assert_eq!(row("bi"), (Some(42), None, None));
        assert_eq!(row("db"), (None, Some(98.6), None));
        assert_eq!(row("js"), (None, None, Some("true".to_string())));
    }

    /// A value that fails to convert to the declared `value_type` is treated
    /// like any other fetch failure: no reading recorded, failure logged with
    /// the conversion error (spec: source-configuration — configurable
    /// stored value type).
    #[tokio::test]
    async fn fetch_once_fails_when_value_does_not_convert_to_declared_type() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();
        let src = query_source("bad", "echo not-a-number", Some("bigint"));
        let kind = source::build(&src).unwrap();
        let mut status = "healthy".to_string();

        let outcome = fetch_once(&db, &cfg, &src, &kind, &mut status).await;

        assert!(
            !outcome.success,
            "a non-convertible value must fail the fetch"
        );
        assert!(
            db.latest_values().await.unwrap().is_empty(),
            "no reading should be recorded"
        );
        let logs = db.logs(Some("bad"), 10).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].error.is_some());
        assert!(
            logs[0]
                .error
                .as_deref()
                .is_some_and(|e| e.contains("does not convert to bigint")),
            "error should name the conversion failure: {:?}",
            logs[0].error
        );
    }

    /// (spec: data-collection — Per-source schedules: daemon startup resumes
    /// from the last run)
    #[tokio::test]
    async fn first_tick_is_immediate_when_source_never_succeeded() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let before = tokio::time::Instant::now();

        let next = first_interval_tick(
            &db,
            "cpu",
            std::time::Duration::from_secs(30),
            std::time::Duration::from_secs(5),
        )
        .await
        .unwrap();

        assert!(
            next.saturating_duration_since(before) < std::time::Duration::from_millis(300),
            "a source with no prior run must be due immediately"
        );
    }

    #[tokio::test]
    async fn first_tick_is_deferred_when_last_success_is_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_log("cpu", 1, None, Some("42"), Origin::Poll)
            .await
            .unwrap();

        let before = tokio::time::Instant::now();
        let next = first_interval_tick(
            &db,
            "cpu",
            std::time::Duration::from_secs(3),
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap();

        let wait = next.saturating_duration_since(before);
        assert!(
            wait > std::time::Duration::from_millis(2500),
            "a fresh source's first fetch should be deferred close to the full interval, got {wait:?}"
        );
    }

    #[tokio::test]
    async fn first_tick_is_immediate_when_last_success_is_older_than_interval() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_log("cpu", 1, None, Some("42"), Origin::Poll)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let before = tokio::time::Instant::now();
        let next = first_interval_tick(
            &db,
            "cpu",
            std::time::Duration::from_millis(50),
            std::time::Duration::from_millis(10),
        )
        .await
        .unwrap();

        assert!(
            next.saturating_duration_since(before) < std::time::Duration::from_millis(300),
            "a source whose last success is already older than its interval must be due immediately"
        );
    }

    /// A fresh failure resumes on `retry_interval`, not the full interval
    /// (spec: data-collection — Per-source schedules: daemon startup resumes
    /// from the last run).
    #[tokio::test]
    async fn first_tick_defers_to_retry_interval_when_last_attempt_failed() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_log("cpu", 1, Some("timeout"), None, Origin::Poll)
            .await
            .unwrap();

        let before = tokio::time::Instant::now();
        let next = first_interval_tick(
            &db,
            "cpu",
            std::time::Duration::from_hours(1),
            std::time::Duration::from_secs(3),
        )
        .await
        .unwrap();

        let wait = next.saturating_duration_since(before);
        assert!(
            wait > std::time::Duration::from_millis(2500),
            "a freshly failed source should wait out its retry interval, got {wait:?}"
        );
        assert!(
            wait < std::time::Duration::from_secs(30),
            "a freshly failed source must not wait the full interval, got {wait:?}"
        );
    }

    #[tokio::test]
    async fn first_tick_is_immediate_when_last_failure_is_older_than_retry_interval() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_log("cpu", 1, Some("timeout"), None, Origin::Poll)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let before = tokio::time::Instant::now();
        let next = first_interval_tick(
            &db,
            "cpu",
            std::time::Duration::from_hours(1),
            std::time::Duration::from_millis(50),
        )
        .await
        .unwrap();

        assert!(
            next.saturating_duration_since(before) < std::time::Duration::from_millis(300),
            "a source whose last failure is already older than its retry interval must be due immediately"
        );
    }

    /// Cron scheduling never consults freshness at startup — it always waits
    /// for its next absolute occurrence regardless of `last_success` (spec:
    /// data-collection — cron-scheduled source ignores fetch outcome).
    #[tokio::test]
    async fn schedule_new_ignores_freshness_for_cron_sources() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_log("backup", 1, None, Some("ok"), Origin::Poll)
            .await
            .unwrap();
        let src = cron_source("backup", "0 0 3 * * *");

        let schedule = Schedule::new(&db, &src).await.unwrap();

        assert!(matches!(schedule, Schedule::Cron(_)));
    }

    /// An invalid cron expression is a proper `Err`, not a panic — `cron()`
    /// syntax is only validated by `config::validate`, which a caller
    /// reaching this constructor directly (as this test does, via
    /// `toml::from_str::<SourceCfg>` instead of `config::load`) can bypass.
    #[tokio::test]
    async fn schedule_new_rejects_invalid_cron_instead_of_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let src = cron_source("bad", "not a cron expression");

        let Err(err) = Schedule::new(&db, &src).await else {
            unreachable!("expected an invalid-cron error");
        };
        assert!(
            err.to_string().contains("invalid cron"),
            "error should name the problem: {err}"
        );
    }

    /// A `query` printing a `jsonl` row stores the extracted value with the
    /// row timestamp and persists the row's thresholds (spec:
    /// source-configuration — Generic query source type, JSONL row schema).
    #[tokio::test]
    async fn fetch_once_applies_jsonl_row_structurally() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();
        let src: SourceCfg = toml::from_str(
            "name = \"b\"\ntype = \"query\"\ncommand = \"echo '{\\\"value\\\":\\\"7\\\",\\\"ts\\\":1000.5,\\\"threshold\\\":[{\\\"bound\\\":1.0,\\\"level\\\":\\\"green\\\"},{\\\"bound\\\":10.0,\\\"level\\\":\\\"red\\\"}]}'\"\n",
        )
        .unwrap();
        let kind = source::build(&src).unwrap();
        let mut status = "healthy".to_string();

        assert!(fetch_once(&db, &cfg, &src, &kind, &mut status).await.success);

        let rows = db.history("b", None, None, None).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, "7");
        assert!((rows[0].ts_epoch - 1000.5).abs() < 0.001);
        let bands = db.session_bands("b").unwrap();
        assert_eq!(bands.len(), 2);
    }

    /// A `query` printing an invalid row logs a failure with no reading
    /// (spec: source-configuration — JSONL row schema).
    #[tokio::test]
    async fn fetch_once_logs_invalid_row_as_failure() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();
        let src: SourceCfg = toml::from_str(
            "name = \"b\"\ntype = \"query\"\ncommand = \"echo '{\\\"value\\\":42}'\"\n",
        )
        .unwrap();
        let kind = source::build(&src).unwrap();
        let mut status = "healthy".to_string();

        assert!(!fetch_once(&db, &cfg, &src, &kind, &mut status).await.success);
        assert!(db.latest_values().await.unwrap().is_empty());
        let logs = db.logs(Some("b"), 10).await.unwrap();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].error.is_some());
    }

    /// Once every schedule-reset sender is gone (the HTTP handler's
    /// `AppState` dropped at shutdown), the receiver resolves to `None`
    /// immediately and forever. Re-arming on it spins the `select!` and
    /// re-advances the schedule on every pass, so the source never fetches
    /// again — the collector must drop the receiver instead and fall back to
    /// its normal tick (spec: data-collection — Ingested values reset
    /// interval schedules).
    #[tokio::test]
    async fn a_dropped_reset_sender_does_not_wedge_the_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let src: SourceCfg = toml::from_str(
            "name = \"cpu\"\ntype = \"query\"\ncommand = \"echo 1\"\ninterval = \"300ms\"\n",
        )
        .unwrap();
        let cfg = Config {
            sources: vec![src.clone()],
            ..Config::default()
        };
        // A fresh attempt defers the first tick by the full interval, so the
        // reset branch — ready at once — is the only thing the `select!` can
        // pick on the first pass. That makes the wedge deterministic rather
        // than a coin flip.
        db.insert_log("cpu", 1, None, Some("seed"), Origin::Poll)
            .await
            .unwrap();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(tx); // every sender gone, exactly as at daemon shutdown
        let (_keep_alive, shutdown) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(loop_source(
            db.clone(),
            src,
            cfg,
            Some(shutdown),
            Some(rx),
        ));

        let stored = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if !db.history("cpu", None, None, None).await.unwrap().is_empty() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await;
        task.abort();
        assert!(
            stored.is_ok(),
            "the collector never fetched: a dead reset channel wedged its schedule"
        );
    }

    /// A stream process whose stdout closes without the process itself
    /// exiting (e.g. it backgrounds more work, or hangs) must not block
    /// shutdown: `wait_for_exit`, reached after the read loop hits natural
    /// EOF, is raced against the shutdown signal too — not just the read
    /// loop itself — since this whole future is what `run_daemon`'s
    /// graceful shutdown awaits per collector task (spec: data-collection —
    /// daemon shutdown lets in-flight collection finish).
    #[tokio::test]
    async fn ingest_stream_run_does_not_block_shutdown_on_a_lingering_process() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();
        let marker = dir.path().join("closed-stdout");
        let src: SourceCfg = toml::from_str(&format!(
            "name = \"s\"\ntype = \"stream\"\ncommand = \"exec 1>&-; touch {}; sleep 30\"\nexpected_interval = \"30s\"\n",
            marker.display()
        ))
        .unwrap();
        let mut status = "healthy".to_string();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let mut shutdown = Some(rx);

        let run = ingest_stream_run(&db, &src, &cfg, &mut status, &mut shutdown);
        tokio::pin!(run);
        // Deadline-poll for the marker (written right after stdout closes,
        // just before the 30s sleep) instead of a blind fixed sleep, so
        // this isn't sensitive to machine load — `run` is still driven
        // forward as the other `select!` branch while polling.
        tokio::select! {
            () = &mut run => unreachable!("must not finish before shutdown is even requested"),
            () = async {
                while !marker.exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            } => {}
        }
        let _ = tx.send(true);

        // Must return promptly once shutdown fires above, not block on the
        // process's own 30s sleep.
        tokio::time::timeout(std::time::Duration::from_secs(5), run)
            .await
            .unwrap();
    }

    /// A valid stream line becomes a reading; a malformed line becomes a
    /// failure without disturbing the stream (spec: data-collection —
    /// Stream collection).
    #[tokio::test]
    async fn ingest_stream_line_records_rows_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();
        let src: SourceCfg = toml::from_str(
            "name = \"s\"\ntype = \"stream\"\ncommand = \"true\"\nexpected_interval = \"30s\"\n",
        )
        .unwrap();
        let mut status = "healthy".to_string();

        ingest_stream_line(&db, &cfg, &src, &mut status, r#"{"value":"1"}"#).await;
        ingest_stream_line(&db, &cfg, &src, &mut status, "not json at all").await;
        ingest_stream_line(&db, &cfg, &src, &mut status, r#"{"value":"2"}"#).await;

        let rows = db.history("s", None, None, None).await.unwrap();
        // "not json at all" is plain text, so it is a third reading, not a
        // failure: only JSON objects with a `value` key go structural.
        assert_eq!(rows.len(), 3);
        let logs = db.logs(Some("s"), 10).await.unwrap();
        assert_eq!(logs.len(), 3);
        assert!(logs.iter().all(|l| l.error.is_none()));
    }

    /// A structurally-invalid stream line is logged as failed with no
    /// reading (spec: data-collection — Stream collection).
    #[tokio::test]
    async fn ingest_stream_line_logs_bad_rows_as_failures() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = Config::default();
        let src: SourceCfg = toml::from_str(
            "name = \"s\"\ntype = \"stream\"\ncommand = \"true\"\nexpected_interval = \"30s\"\n",
        )
        .unwrap();
        let mut status = "healthy".to_string();

        ingest_stream_line(&db, &cfg, &src, &mut status, r#"{"value":"1"}"#).await;
        ingest_stream_line(&db, &cfg, &src, &mut status, r#"{"value":42}"#).await;

        let rows = db.history("s", None, None, None).await.unwrap();
        assert_eq!(rows.len(), 1);
        let logs = db.logs(Some("s"), 10).await.unwrap();
        assert_eq!(logs.len(), 2);
        assert!(
            logs[0].error.is_some(),
            "newest (the bad row) should have failed"
        );
    }

    /// `[[sources.children]]` under a `[[sources]]` table, expanded via
    /// `config::expand_composites` exactly like `config::load` would.
    fn composite_cfg(command: &str, children: &[&str]) -> Config {
        let mut children_toml = String::new();
        for c in children {
            use std::fmt::Write as _;
            let _ = write!(children_toml, "[[sources.children]]\nname = \"{c}\"\n");
        }
        let escaped = command.replace('"', "\\\"");
        let mut cfg: Config = toml::from_str(&format!(
            "[[sources]]\nname = \"load\"\ntype = \"query\"\ncommand = \"{escaped}\"\ninterval = \"1h\"\n{children_toml}"
        ))
        .unwrap();
        crate::config::expand_composites(&mut cfg).unwrap();
        cfg
    }

    fn find_child<'a>(cfg: &'a Config, full: &str) -> &'a SourceCfg {
        cfg.sources.iter().find(|s| s.name() == full).unwrap()
    }

    /// A well-formed array stores every child exactly as an equivalent
    /// `/api/ingest` push would (spec: data-collection — Composite source
    /// command output).
    #[tokio::test]
    async fn poll_once_on_composite_root_stores_every_child() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = composite_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"},{"source":"load::5m","value":"0.2"}]'"#,
            &["1m", "5m"],
        );
        let root = cfg.sources.iter().find(|s| s.name() == "load").unwrap();

        let outcome = poll_once(&db, &cfg, root).await;
        assert!(outcome.success, "{outcome:?}");
        assert!(outcome.value.is_none(), "the root itself stores no value");

        assert_eq!(
            db.latest_values()
                .await
                .unwrap()
                .iter()
                .find(|r| r.source == "load::1m")
                .unwrap()
                .value,
            "0.1"
        );
        assert_eq!(
            db.latest_values()
                .await
                .unwrap()
                .iter()
                .find(|r| r.source == "load::5m")
                .unwrap()
                .value,
            "0.2"
        );
        assert!(
            db.logs(Some("load"), 10).await.unwrap()[0].error.is_none(),
            "root's own log entry records the command/parse success"
        );
    }

    /// Forcing a single child fans out to every declared sibling from the
    /// same command run, and returns just that child's own outcome (spec:
    /// data-collection — Force polling a composite source or its children).
    #[tokio::test]
    async fn poll_once_on_a_child_fans_out_to_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = composite_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"},{"source":"load::5m","value":"0.2"}]'"#,
            &["1m", "5m"],
        );

        let outcome = poll_once(&db, &cfg, find_child(&cfg, "load::1m")).await;
        assert!(outcome.success);
        assert_eq!(outcome.source, "load::1m");
        assert_eq!(outcome.value.as_deref(), Some("0.1"));

        assert_eq!(
            db.latest_values()
                .await
                .unwrap()
                .iter()
                .find(|r| r.source == "load::5m")
                .unwrap()
                .value,
            "0.2",
            "the sibling not named in the request is refreshed too"
        );
    }

    /// A declared child missing from the array fails only that child; the
    /// root and present siblings succeed normally (spec: data-collection —
    /// Composite fan-out error handling).
    #[tokio::test]
    async fn missing_declared_child_fails_only_that_child() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = composite_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"}]'"#,
            &["1m", "5m"],
        );
        let root = cfg.sources.iter().find(|s| s.name() == "load").unwrap();

        let outcome = poll_once(&db, &cfg, root).await;
        assert!(outcome.success, "root still succeeds: the command ran and parsed");

        let outcome_5m = poll_once(&db, &cfg, find_child(&cfg, "load::5m")).await;
        // The above re-runs the command, so re-check via direct log inspection
        // of the *first* run's effect instead of the second call's outcome.
        let _ = outcome_5m;
        assert_eq!(
            db.latest_values()
                .await
                .unwrap()
                .iter()
                .find(|r| r.source == "load::1m")
                .unwrap()
                .value,
            "0.1"
        );
        let logs_5m = db.logs(Some("load::5m"), 10).await.unwrap();
        assert!(
            logs_5m[0].error.as_deref().is_some_and(|e| e.contains("missing")),
            "{logs_5m:?}"
        );
    }

    /// An array entry naming an id that isn't a declared child fails the
    /// whole attempt: nothing is written for that tick (spec:
    /// data-collection — Composite fan-out error handling).
    #[tokio::test]
    async fn unknown_child_in_output_fails_the_whole_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = composite_cfg(
            r#"echo '[{"source":"load::1m","value":"0.1"},{"source":"load::nope","value":"9"}]'"#,
            &["1m"],
        );
        let root = cfg.sources.iter().find(|s| s.name() == "load").unwrap();

        let outcome = poll_once(&db, &cfg, root).await;
        assert!(!outcome.success);
        assert!(outcome.error.as_deref().is_some_and(|e| e.contains("nope")));
        assert!(
            db.latest_values().await.unwrap().is_empty(),
            "no child should be written when the output names an unknown id"
        );
    }

    /// Malformed command output (not a JSON array) fails the whole attempt
    /// the same way (spec: data-collection — Composite source command
    /// output).
    #[tokio::test]
    async fn malformed_composite_output_fails_the_whole_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = composite_cfg("echo not-json", &["1m"]);
        let root = cfg.sources.iter().find(|s| s.name() == "load").unwrap();

        let outcome = poll_once(&db, &cfg, root).await;
        assert!(!outcome.success);
        assert!(db.latest_values().await.unwrap().is_empty());
    }

    /// The root and every declared child are reported polling for the
    /// duration of the one shared command, whichever name triggered it
    /// (spec: data-collection — Force polling a composite source or its
    /// children).
    #[tokio::test]
    async fn composite_fetch_marks_root_and_every_child_polling() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cfg = composite_cfg(
            r#"sleep 0.3; echo '[{"source":"load::1m","value":"0.1"},{"source":"load::5m","value":"0.2"}]'"#,
            &["1m", "5m"],
        );
        let root = cfg.sources.iter().find(|s| s.name() == "load").unwrap().clone();
        let db2 = db.clone();
        let cfg2 = cfg.clone();
        let handle = tokio::spawn(async move { poll_once(&db2, &cfg2, &root).await });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(db.is_polling("load"), "root should be polling");
        assert!(db.is_polling("load::1m"), "child 1m should be polling");
        assert!(db.is_polling("load::5m"), "child 5m should be polling");

        handle.await.unwrap();
        assert!(!db.is_polling("load"));
        assert!(!db.is_polling("load::1m"));
        assert!(!db.is_polling("load::5m"));
    }
}
