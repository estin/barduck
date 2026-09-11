#![allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]

use crate::config::Threshold;
use anyhow::{Context as _, Result};
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

/// Handle to the `DuckDB` database file.
///
/// `DuckDB` takes an exclusive cross-process lock on the file and a read-only
/// open racing a live write can see torn blocks (spec: data-storage —
/// concurrent access safety). So every operation — daemon writes and
/// direct-mode reads alike — takes an advisory flock on a sidecar lockfile
/// for its duration, then opens its own short-lived `DuckDB` connection.
/// `ponytail`: reopen per operation (~ms); move to a resident reader process
/// only if this shows up in practice.
///
/// The daemon (spec: data-storage — single daemon writer) is the one
/// exception: [`Db::open_rw_daemon`] starts a single writer task holding one
/// persistent connection for every insert/purge, instead of each one
/// reopening its own. It still takes the advisory lock per-operation, not
/// for the connection's whole lifetime, so a direct-mode CLI/TUI read
/// against the same file can still interleave between writes.
///
/// The lock-acquire + connect retry loop is synchronous (`std::thread::sleep`
/// between attempts) by design — every async caller runs it via
/// [`Db::connect_async`], which offloads the whole retry loop to
/// `spawn_blocking` so it never parks a Tokio worker thread.
#[derive(Clone)]
pub struct Db {
    path: PathBuf,
    ro: bool,
    /// Set only by [`Db::open_rw_daemon`]; every insert/purge method sends
    /// its request here instead of taking the per-op `connect_async` path
    /// when this is `Some`.
    writer: Option<mpsc::UnboundedSender<WriteCmd>>,
    /// `jsonl` threshold overrides for the current process only (spec:
    /// source-configuration — Threshold bands): written by ingest, read by
    /// health/renderers, forgotten when the handle is dropped. A fresh
    /// handle (daemon start) therefore always reseeds from config bands.
    session_bands: SessionBands,
}

/// In-memory threshold overrides keyed by source name. `std` (not Tokio)
/// lock: holders only clone a small `Vec`, never hold across `.await`.
pub type SessionBands =
    std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, Vec<Threshold>>>>;

struct DbLock(#[allow(dead_code)] File);

/// Best-effort text for a `catch_unwind` payload: panics from `panic!("{}", ...)`
/// and friends carry a `&str` or `String`; anything else falls back to a
/// generic label rather than failing to log at all.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

fn is_lock_conflict(e: &anyhow::Error) -> bool {
    let msg = format!("{e:#}");
    msg.contains("Conflicting lock") || msg.contains("Could not read enough bytes")
}

/// One write request sent to the daemon's single writer task (spec:
/// data-storage — single daemon writer), each carrying a reply channel so
/// the caller still gets back the exact `Result<()>` a direct call would
/// have returned.
enum WriteCmd {
    InsertReading {
        source: String,
        value: String,
        unit: Option<String>,
        value_bigint: Option<i64>,
        value_double: Option<f64>,
        value_json: Option<String>,
        ts_epoch: f64,
        ts: String,
        reply: oneshot::Sender<Result<()>>,
    },
    InsertLog {
        source: String,
        ts_epoch: f64,
        ts: String,
        duration_ms: i64,
        error: Option<String>,
        value: Option<String>,
        origin: Origin,
        reply: oneshot::Sender<Result<()>>,
    },
    InsertHealthEvent {
        source: String,
        ts_epoch: f64,
        ts: String,
        status: String,
        reply: oneshot::Sender<Result<()>>,
    },
    PurgeOlderThan {
        cutoff_epoch: f64,
        reply: oneshot::Sender<Result<()>>,
    },
}

impl WriteCmd {
    /// Executes this command against the writer's persistent connection,
    /// taking the advisory lock only for this one statement.
    #[allow(clippy::too_many_lines)]
    fn run(self, db: &Db, conn: &Connection) {
        let (result, reply) = match self {
            WriteCmd::InsertReading {
                source,
                value,
                unit,
                value_bigint,
                value_double,
                value_json,
                ts_epoch,
                ts,
                reply,
            } => (
                db.with_lock(|| {
                    conn.execute(
                        "INSERT INTO readings (id, source, value, unit, ts_epoch, ts, value_bigint, value_double, value_json)
                         VALUES (nextval('readings_id_seq'), ?, ?, ?, ?, ?, ?, ?, CAST(? AS JSON))",
                        params![source, value, unit, ts_epoch, ts, value_bigint, value_double, value_json],
                    )?;
                    Ok(())
                }),
                reply,
            ),
            WriteCmd::InsertLog {
                source,
                ts_epoch,
                ts,
                duration_ms,
                error,
                value,
                origin,
                reply,
            } => (
                db.with_lock(|| {
                    conn.execute(
                        "INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value, origin)
                         VALUES (nextval('fetch_logs_id_seq'), ?, ?, ?, ?, ?, ?, ?)",
                        params![source, ts_epoch, ts, duration_ms, error, value, origin.as_str()],
                    )?;
                    Ok(())
                }),
                reply,
            ),
            WriteCmd::InsertHealthEvent {
                source,
                ts_epoch,
                ts,
                status,
                reply,
            } => (
                db.with_lock(|| {
                    conn.execute(
                        "INSERT INTO health_events (id, source, ts_epoch, ts, status)
                         VALUES (nextval('health_events_id_seq'), ?, ?, ?, ?)",
                        params![source, ts_epoch, ts, status],
                    )?;
                    Ok(())
                }),
                reply,
            ),
            WriteCmd::PurgeOlderThan {
                cutoff_epoch,
                reply,
            } => (
                db.with_lock(|| {
                    conn.execute(
                        "DELETE FROM readings WHERE ts_epoch < ?",
                        params![cutoff_epoch],
                    )?;
                    conn.execute(
                        "DELETE FROM fetch_logs WHERE ts_epoch < ?",
                        params![cutoff_epoch],
                    )?;
                    conn.execute(
                        "DELETE FROM health_events WHERE ts_epoch < ?",
                        params![cutoff_epoch],
                    )?;
                    Ok(())
                }),
                reply,
            ),
        };
        // A read-only connection opened by another process (direct-mode
        // CLI/TUI, `Db::open_ro`) can only see what's checkpointed into the
        // main file — never this connection's WAL. Left to `DuckDB`'s own
        // size-based auto-checkpoint, that can go a long time without
        // happening for this app's small, infrequent writes, so a
        // concurrent direct-mode read appears stuck while the daemon runs
        // (spec: data-storage — concurrent access safety). Checkpointing
        // here, before the reply, means a caller's successful `.await`
        // usually already guarantees visibility, not just a race with it;
        // writes are minutes apart, so the extra fsync is free at this
        // scale. A checkpoint failure (e.g. a concurrent direct-mode reader
        // briefly holding the file lock) is logged and tolerated rather
        // than folded into `result`: the write above already committed, so
        // reporting it as a failure would be a false negative — durably
        // stored data reported to the caller (an ingest POST, a collector
        // insert) as an error.
        if result.is_ok()
            && let Err(e) = db.with_lock(|| conn.execute_batch("CHECKPOINT").map_err(Into::into))
        {
            tracing::warn!("checkpoint after write: {e:#}");
        }
        let _ = reply.send(result);
    }

    /// Fails every queued command with `err` — used when the writer's
    /// connection itself never opened, so a caller waiting on its reply
    /// gets a clear error instead of hanging forever.
    fn fail(self, err: &anyhow::Error) {
        let reply = match self {
            WriteCmd::InsertReading { reply, .. }
            | WriteCmd::InsertLog { reply, .. }
            | WriteCmd::InsertHealthEvent { reply, .. }
            | WriteCmd::PurgeOlderThan { reply, .. } => reply,
        };
        let _ = reply.send(Err(anyhow::anyhow!("db writer unavailable: {err:#}")));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingRow {
    pub source: String,
    pub value: String,
    pub unit: Option<String>,
    pub ts_epoch: f64,
    pub ts: String,
}

/// How a fetch-log row's value arrived (spec: data-collection — Fetch
/// attempts logged): pushed via `POST /api/ingest`, or gathered by a
/// scheduled fetch. Serializes to its lowercase name in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Value gathered by a scheduled fetch; also the backfill for rows
    /// written before the origin column existed.
    #[default]
    Poll,
    /// Value pushed via `POST /api/ingest`.
    Push,
}

impl Origin {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Poll => "poll",
            Origin::Push => "push",
        }
    }

    /// Lenient database read: only `push` is meaningful, anything else
    /// (including future values) falls back to `poll`, mirroring the
    /// `COALESCE(origin,'poll')` at every log SELECT.
    #[must_use]
    pub fn from_db(s: &str) -> Self {
        match s {
            "push" => Origin::Push,
            _ => Origin::Poll,
        }
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRow {
    pub source: String,
    pub ts_epoch: f64,
    pub ts: String,
    pub duration_ms: i64,
    pub error: Option<String>,
    pub value: Option<String>,
    /// Not stored; filled from config where the row is presented, so log
    /// output can show values the same way panels do. Defaults for rows
    /// serialized before this field existed.
    #[serde(default)]
    pub unit: Option<String>,
    /// Always resolved via `COALESCE(origin,'poll')`, so never missing from
    /// the database; defaults to `poll` for JSON serialized by older daemons.
    #[serde(default)]
    pub origin: Origin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEventRow {
    pub source: String,
    pub ts_epoch: f64,
    pub ts: String,
    pub status: String,
}

/// Bounds accepted by [`Db::history`]/[`Db::history_async`] for the HTTP
/// history endpoint (spec: http-api — bounded history queries): a request
/// without `limit` gets `DEFAULT_HISTORY_LIMIT` rows; any requested limit is
/// capped at `MAX_HISTORY_LIMIT` so a client can never force an unbounded
/// table scan.
pub const DEFAULT_HISTORY_LIMIT: i64 = 1000;
pub const MAX_HISTORY_LIMIT: i64 = 10_000;

/// Cap accepted by [`Db::logs`]/[`Db::logs_for_sources`] — same rationale as
/// [`MAX_HISTORY_LIMIT`]: a requested `limit` (HTTP `/api/logs?limit=`, the
/// CLI's `--limit`, or a caller's own default) is clamped rather than passed
/// straight into `LIMIT ?`, so neither an unbounded nor a negative value
/// (which `DuckDB` treats as unbounded) can force a full-table scan.
pub const MAX_LOGS_LIMIT: i64 = 10_000;

pub(crate) fn now() -> (f64, String) {
    let t: DateTime<Utc> = Utc::now();
    (t.timestamp_millis() as f64 / 1000.0, t.to_rfc3339())
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE SEQUENCE IF NOT EXISTS readings_id_seq;
        CREATE SEQUENCE IF NOT EXISTS fetch_logs_id_seq;
        CREATE SEQUENCE IF NOT EXISTS health_events_id_seq;
        -- Explicit monotonic id on every table, not just `readings`: two
        -- rows can share the same millisecond `ts_epoch` (e.g. every source
        -- fetched at daemon startup), and `rowid` is a physical-storage
        -- identifier `DuckDB` doesn't guarantee as a stable ordering key —
        -- `id` is the only reliable tie-break for `ORDER BY ts_epoch DESC`.
        CREATE TABLE IF NOT EXISTS readings (
            id     BIGINT NOT NULL,
            source VARCHAR NOT NULL,
            value  VARCHAR NOT NULL,
            unit   VARCHAR,
            ts_epoch DOUBLE NOT NULL,
            ts     VARCHAR NOT NULL,
            -- Populated only when the source declares a non-default
            -- `value_type` (spec: source-configuration — configurable
            -- stored value type); `value` above is always populated
            -- regardless, so existing consumers are unaffected.
            value_bigint BIGINT,
            value_double DOUBLE,
            value_json   JSON
        );
        -- `CREATE TABLE IF NOT EXISTS` above is a no-op on a database file
        -- from before these columns existed, so — same as `fetch_logs`
        -- below — bringing an old database forward needs its own
        -- statement; no backfill needed, `NULL` (every pre-existing row's
        -- value_type was `string`, so it never populated these) is already
        -- the correct value.
        ALTER TABLE readings ADD COLUMN IF NOT EXISTS value_bigint BIGINT;
        ALTER TABLE readings ADD COLUMN IF NOT EXISTS value_double DOUBLE;
        ALTER TABLE readings ADD COLUMN IF NOT EXISTS value_json JSON;
        CREATE TABLE IF NOT EXISTS fetch_logs (
            id          BIGINT NOT NULL,
            source      VARCHAR NOT NULL,
            ts_epoch    DOUBLE NOT NULL,
            ts          VARCHAR NOT NULL,
            duration_ms BIGINT  NOT NULL,
            -- Outcome is derived from whether `error` is set, not stored
            -- separately (spec: data-collection — fetch attempts logged): a
            -- failed attempt always carries error detail, a successful one
            -- never does.
            error       VARCHAR,
            value       VARCHAR,
            -- How the value arrived: `push` (POST /api/ingest) or `poll`
            -- (scheduled fetch). Absent on rows written before this column
            -- existed; those read as `poll` via COALESCE at every SELECT.
            origin      VARCHAR
        );
        -- No migrator: bringing an old database forward is this statement
        -- plus a backfill (spec: data-collection — Fetch attempts logged).
        ALTER TABLE fetch_logs ADD COLUMN IF NOT EXISTS origin VARCHAR;
        UPDATE fetch_logs SET origin = 'poll' WHERE origin IS NULL;
        CREATE TABLE IF NOT EXISTS health_events (
            id       BIGINT NOT NULL,
            source   VARCHAR NOT NULL,
            ts_epoch DOUBLE NOT NULL,
            ts       VARCHAR NOT NULL,
            status   VARCHAR NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_readings_source_ts ON readings(source, ts_epoch);
        CREATE INDEX IF NOT EXISTS idx_fetch_logs_source_ts ON fetch_logs(source, ts_epoch);
        CREATE INDEX IF NOT EXISTS idx_health_events_source_ts ON health_events(source, ts_epoch);",
    )?;
    Ok(())
}

/// Starts the daemon's single writer task (spec: data-storage — single
/// daemon writer): opens one `DuckDB` connection and keeps it for as long as
/// any sender (i.e. any clone of the `Db` this came from) is still alive,
/// executing each queued [`WriteCmd`] in turn. If the connection itself
/// fails to open, every already- and later-queued command is failed with a
/// clear error instead of hanging forever waiting on a reply.
/// Opens a fresh writer connection. The open races the same "read-only open
/// sees a torn write" hazard as any other operation, so it takes the
/// advisory lock too — briefly, same as every write that follows. Shared by
/// [`spawn_writer`]'s initial open and its post-panic reconnect.
fn open_writer_conn(db: &Db) -> Result<Connection> {
    db.acquire().and_then(|guard| {
        let c = db.connect();
        drop(guard);
        c
    })
}

fn spawn_writer(path: PathBuf) -> mpsc::UnboundedSender<WriteCmd> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WriteCmd>();
    tokio::task::spawn_blocking(move || {
        let db = Db {
            path,
            ro: false,
            writer: None,
            session_bands: SessionBands::default(),
        };
        let mut conn = match open_writer_conn(&db) {
            Ok(conn) => conn,
            Err(e) => {
                tracing::error!("db writer: failed to open connection: {e:#}");
                while let Some(cmd) = rx.blocking_recv() {
                    cmd.fail(&e);
                }
                return;
            }
        };
        while let Some(cmd) = rx.blocking_recv() {
            // One daemon-wide writer serves every source (spec: data-storage
            // — single daemon writer), so a panic here — a bug tripped by
            // one source's unusual value — must not unwind out of this loop
            // and kill the writer thread, or persistence for every other
            // source dies with it until the daemon restarts (spec:
            // data-collection — collector resilience). The caller still gets
            // an error either way: `run`'s reply sender is dropped mid-panic,
            // which resolves its `oneshot::Receiver` await to an error.
            if let Err(panic) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cmd.run(&db, &conn)))
            {
                tracing::error!(
                    "db writer: write command panicked: {}",
                    panic_message(&panic)
                );
                // The panic may have left `conn` mid-statement or otherwise
                // in an unknown state — there's no reliable way to tell, so
                // reopen rather than risk every subsequent write silently
                // failing against a wedged connection until the daemon
                // restarts.
                match open_writer_conn(&db) {
                    Ok(fresh) => conn = fresh,
                    Err(e) => {
                        tracing::error!("db writer: failed to reopen after panic: {e:#}");
                    }
                }
            }
        }
    });
    tx
}

impl Db {
    /// Read-write handle; creates schema. Every insert/purge still reopens
    /// its own connection per call — fine for one-shot callers (`reset`,
    /// `collect_once`, tests). The daemon uses [`Db::open_rw_daemon`] instead.
    pub fn open_rw(path: &Path) -> Result<Self> {
        let db = Self {
            path: path.to_path_buf(),
            ro: false,
            writer: None,
            session_bands: SessionBands::default(),
        };
        db.with_conn(create_schema)?;
        Ok(db)
    }

    /// Read-write handle for the daemon (spec: data-storage — single daemon
    /// writer): same as [`Db::open_rw`], but every insert/purge is instead
    /// routed to one persistent-connection writer task, shared by every
    /// clone of the returned handle (collector tasks, HTTP handlers, the
    /// retention task). Must be called from within a Tokio runtime.
    pub fn open_rw_daemon(path: &Path) -> Result<Self> {
        let mut db = Self::open_rw(path)?;
        db.writer = Some(spawn_writer(db.path.clone()));
        Ok(db)
    }

    /// Read-only handle for direct-mode CLI/TUI while the daemon may be running.
    pub fn open_ro(path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            ro: true,
            writer: None,
            session_bands: SessionBands::default(),
        })
    }

    /// Drops every table (readings, fetch logs, health events — plus the
    /// vestigial `source_thresholds` table from before overrides became
    /// session-only) and recreates an empty schema. Callers are responsible
    /// for confirming with the user first — this is irreversible.
    pub fn reset(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                "DROP TABLE IF EXISTS readings;
                DROP TABLE IF EXISTS fetch_logs;
                DROP TABLE IF EXISTS health_events;
                DROP TABLE IF EXISTS source_thresholds;
                DROP SEQUENCE IF EXISTS readings_id_seq;
                DROP SEQUENCE IF EXISTS fetch_logs_id_seq;
                DROP SEQUENCE IF EXISTS health_events_id_seq;",
            )?;
            create_schema(conn)
        })
    }

    /// Takes the advisory lock serializing all database access. Blocking —
    /// callers on an async runtime must go through [`Db::connect_async`]
    /// (`spawn_blocking`), never call this directly from an async fn.
    fn acquire(&self) -> Result<DbLock> {
        const ATTEMPTS: u32 = 50;
        std::fs::create_dir_all(self.path.parent().unwrap_or(Path::new(".")))?;
        let lock_path = self.path.with_extension("duckdb.lock");
        let file = File::create(&lock_path)
            .with_context(|| format!("creating lockfile {}", lock_path.display()))?;
        for i in 0..ATTEMPTS {
            match file.try_lock() {
                Ok(()) => return Ok(DbLock(file)),
                Err(e) if i + 1 < ATTEMPTS => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e))
                        .context(format!("locking {}", lock_path.display()));
                }
            }
        }
        unreachable!()
    }

    /// Opens a `DuckDB` connection, retrying on transient conflicts. Blocking
    /// — same caveat as [`Db::acquire`].
    fn connect(&self) -> Result<Connection> {
        const ATTEMPTS: u32 = 50;
        let mut last = None;
        for i in 0..ATTEMPTS {
            let res = if self.ro {
                let cfg = duckdb::Config::default().access_mode(duckdb::AccessMode::ReadOnly)?;
                Connection::open_with_flags(&self.path, cfg)
            } else {
                Connection::open(&self.path)
            }
            .context(format!("opening database {}", self.path.display()));
            match res {
                Ok(conn) => return Ok(conn),
                Err(e) if is_lock_conflict(&e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(u64::from(100 + 10 * (i % 10))));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("could not open {}", self.path.display())))
    }

    /// Blocking retry loop combining [`Db::acquire`] and [`Db::connect`],
    /// run entirely on a `spawn_blocking` thread by [`Db::connect_async`] so
    /// none of its sleeps ever park a Tokio worker.
    fn acquire_and_connect(&self) -> Result<(DbLock, Connection)> {
        const ATTEMPTS: u32 = 50;
        let mut last = None;
        for i in 0..ATTEMPTS {
            let guard = match self.acquire() {
                Ok(g) => g,
                Err(e) => {
                    last = Some(e);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            let res = if self.ro {
                let cfg = duckdb::Config::default().access_mode(duckdb::AccessMode::ReadOnly)?;
                Connection::open_with_flags(&self.path, cfg)
            } else {
                Connection::open(&self.path)
            }
            .context(format!("opening database {}", self.path.display()));
            match res {
                Ok(conn) => return Ok((guard, conn)),
                Err(e) if is_lock_conflict(&e) => {
                    last = Some(e);
                    drop(guard);
                    std::thread::sleep(Duration::from_millis(u64::from(100 + 10 * (i % 10))));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("could not open {}", self.path.display())))
    }

    /// Acquires the lock and opens a connection without blocking the calling
    /// Tokio worker thread: the whole synchronous retry loop runs on the
    /// blocking thread pool.
    async fn connect_async(&self) -> Result<(DbLock, Connection)> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.acquire_and_connect())
            .await
            .context("database worker thread panicked")?
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let _guard = self.acquire()?;
        let conn = self.connect()?;
        f(&conn)
    }

    /// Takes the advisory lock around `f`, then releases it — for the
    /// writer task's already-open connection, which only needs the lock
    /// itself, not a fresh `connect()` (see [`Db::open_rw_daemon`]).
    fn with_lock<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let _guard = self.acquire()?;
        f()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_reading(
        &self,
        source: &str,
        value: &str,
        unit: Option<&str>,
        value_bigint: Option<i64>,
        value_double: Option<f64>,
        value_json: Option<&str>,
    ) -> Result<()> {
        let (ts_epoch, ts) = now();
        self.insert_reading_at(
            source,
            value,
            unit,
            value_bigint,
            value_double,
            value_json,
            ts_epoch,
            &ts,
        )
        .await
    }

    /// Same as [`Db::insert_reading`], but stamps the caller-supplied
    /// timestamp instead of the arrival time (spec: data-storage — Readings
    /// persisted with provenance): used for `jsonl` rows carrying a valid
    /// `ts`. Callers pass arrival time when the row carries none.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_reading_at(
        &self,
        source: &str,
        value: &str,
        unit: Option<&str>,
        value_bigint: Option<i64>,
        value_double: Option<f64>,
        value_json: Option<&str>,
        ts_epoch: f64,
        ts: &str,
    ) -> Result<()> {
        if let Some(writer) = &self.writer {
            let (reply, rx) = oneshot::channel();
            let cmd = WriteCmd::InsertReading {
                source: source.to_string(),
                value: value.to_string(),
                unit: unit.map(str::to_string),
                value_bigint,
                value_double,
                value_json: value_json.map(str::to_string),
                ts_epoch,
                ts: ts.to_string(),
                reply,
            };
            writer
                .send(cmd)
                .map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO readings (id, source, value, unit, ts_epoch, ts, value_bigint, value_double, value_json)
             VALUES (nextval('readings_id_seq'), ?, ?, ?, ?, ?, ?, ?, CAST(? AS JSON))",
            params![source, value, unit, ts_epoch, ts, value_bigint, value_double, value_json],
        )?;
        Ok(())
    }

    /// Records one fetch attempt stamped with the arrival time: the row's
    /// timestamp is when the attempt happened, even when the value it
    /// carries arrived with its own `ts` (spec: http-api — HTTP ingest
    /// endpoint) — attempt recency drives retry backoff and startup
    /// freshness, so a backdated value must not present as a fresh attempt.
    pub async fn insert_log(
        &self,
        source: &str,
        duration_ms: i64,
        error: Option<&str>,
        value: Option<&str>,
        origin: Origin,
    ) -> Result<()> {
        let (ts_epoch, ts) = now();
        if let Some(writer) = &self.writer {
            let (reply, rx) = oneshot::channel();
            let cmd = WriteCmd::InsertLog {
                source: source.to_string(),
                ts_epoch,
                ts,
                duration_ms,
                error: error.map(str::to_string),
                value: value.map(str::to_string),
                origin,
                reply,
            };
            writer
                .send(cmd)
                .map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value, origin)
             VALUES (nextval('fetch_logs_id_seq'), ?, ?, ?, ?, ?, ?, ?)",
            params![
                source,
                ts_epoch,
                &ts,
                duration_ms,
                error,
                value,
                origin.as_str()
            ],
        )?;
        Ok(())
    }

    /// Replaces the effective threshold bands for `source` for the current
    /// process only (spec: source-configuration — Threshold bands).
    /// Forgotten when the handle is dropped; a fresh handle reseeds from
    /// config bands.
    pub fn set_session_bands(&self, source: &str, bands: &[Threshold]) {
        if let Ok(mut map) = self.session_bands.write() {
            map.insert(source.to_string(), bands.to_vec());
        }
    }

    /// Session override bands for `source`, if a `jsonl` row replaced them
    /// since process start. `None` means color with declared bands.
    #[must_use]
    pub fn session_bands(&self, source: &str) -> Option<Vec<Threshold>> {
        self.session_bands.read().ok()?.get(source).cloned()
    }

    pub async fn last_health(&self, source: &str) -> Result<Option<String>> {
        let (_guard, conn) = self.connect_async().await?;
        // `id DESC` breaks ties between events sharing the same millisecond
        // `ts_epoch`; `ORDER BY ts_epoch DESC` alone picked an arbitrary tied
        // row, not necessarily the actually-latest one.
        let mut stmt = conn.prepare(
            "SELECT status FROM health_events WHERE source = ? ORDER BY ts_epoch DESC, id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![source])?;
        Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
    }

    pub async fn insert_health_event(&self, source: &str, status: &str) -> Result<()> {
        let (ts_epoch, ts) = now();
        if let Some(writer) = &self.writer {
            let (reply, rx) = oneshot::channel();
            let cmd = WriteCmd::InsertHealthEvent {
                source: source.to_string(),
                ts_epoch,
                ts,
                status: status.to_string(),
                reply,
            };
            writer
                .send(cmd)
                .map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO health_events (id, source, ts_epoch, ts, status)
             VALUES (nextval('health_events_id_seq'), ?, ?, ?, ?)",
            params![source, ts_epoch, ts, status],
        )?;
        Ok(())
    }

    /// Latest reading per source: highest `ts_epoch`, with `id` (a
    /// monotonic sequence, not `rowid` — a physical-storage offset `DuckDB`
    /// doesn't guarantee as a stable ordering key) as a tie-break for
    /// same-timestamp rows (matching `history`'s own ordering) — readings
    /// for different sources routinely land in the same millisecond (e.g.
    /// every source fetched at daemon startup).
    pub async fn latest_values(&self) -> Result<Vec<ReadingRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, value, unit, ts_epoch, ts FROM (
                SELECT source, value, unit, ts_epoch, ts,
                       ROW_NUMBER() OVER (
                           PARTITION BY source
                           ORDER BY ts_epoch DESC, id DESC
                       ) AS rn
                FROM readings
             ) sub
             WHERE rn = 1
             ORDER BY source",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ReadingRow {
                    source: r.get(0)?,
                    value: r.get(1)?,
                    unit: r.get(2)?,
                    ts_epoch: r.get(3)?,
                    ts: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Readings for `source` within `[from, to]` (either bound optional),
    /// newest `limit` rows (capped at [`MAX_HISTORY_LIMIT`]), returned oldest
    /// first (spec: http-api — bounded history queries).
    pub async fn history(
        &self,
        source: &str,
        from: Option<f64>,
        to: Option<f64>,
        limit: Option<i64>,
    ) -> Result<Vec<ReadingRow>> {
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_LIMIT)
            .clamp(1, MAX_HISTORY_LIMIT);
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, value, unit, ts_epoch, ts FROM (
                SELECT source, value, unit, ts_epoch, ts, id AS ord FROM readings
                WHERE source = ?
                  AND (?::DOUBLE IS NULL OR ts_epoch >= ?::DOUBLE)
                  AND (?::DOUBLE IS NULL OR ts_epoch <= ?::DOUBLE)
                ORDER BY ts_epoch DESC, ord DESC
                LIMIT ?
             ) sub
             ORDER BY ts_epoch, ord",
        )?;
        let rows = stmt
            .query_map(params![source, from, from, to, to, limit], |r| {
                Ok(ReadingRow {
                    source: r.get(0)?,
                    value: r.get(1)?,
                    unit: r.get(2)?,
                    ts_epoch: r.get(3)?,
                    ts: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub async fn logs(&self, source: Option<&str>, limit: i64) -> Result<Vec<LogRow>> {
        let limit = limit.clamp(1, MAX_LOGS_LIMIT);
        let (_guard, conn) = self.connect_async().await?;
        // `id DESC` breaks ties between attempts sharing the same
        // millisecond `ts_epoch` — callers (e.g. the consecutive-failure
        // count in `health::compute`) rely on this being true newest-first
        // order, not an arbitrary tied order.
        let mut stmt = conn.prepare(
            "SELECT source, ts_epoch, ts, duration_ms, error, value, COALESCE(origin,'poll') FROM fetch_logs
             WHERE (?::VARCHAR IS NULL OR source = ?::VARCHAR)
             ORDER BY ts_epoch DESC, id DESC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![source, source, limit], |r| {
                Ok(LogRow {
                    source: r.get(0)?,
                    ts_epoch: r.get(1)?,
                    ts: r.get(2)?,
                    duration_ms: r.get(3)?,
                    error: r.get(4)?,
                    value: r.get(5)?,
                    unit: None,
                    origin: Origin::from_db(&r.get::<_, String>(6)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
    /// Recent fetch logs for the given sources, newest first (spec: cli —
    /// Filter query output by source). The filter applies inside the query
    /// so a quiet source's rows are not crowded out by the global limit the
    /// way fetch-then-filter would. Empty `sources` returns the newest rows
    /// across all sources.
    pub async fn logs_for_sources(&self, sources: &[String], limit: i64) -> Result<Vec<LogRow>> {
        let limit = limit.clamp(1, MAX_LOGS_LIMIT);
        if sources.is_empty() {
            return self.logs(None, limit).await;
        }
        let mut out = Vec::new();
        for s in sources {
            out.extend(self.logs(Some(s), limit).await?);
        }
        out.sort_by(|a, b| b.ts_epoch.total_cmp(&a.ts_epoch));
        #[allow(clippy::cast_sign_loss)] // clamped to >= 1 above
        out.truncate(limit as usize);
        Ok(out)
    }

    /// Most recent fetch log for `source`, if any (spec: data-collection —
    /// consecutive failures flip to failing). Used by [`crate::health::compute`]
    /// instead of scanning the source's entire log history. A successful
    /// attempt is one with no error detail (spec: data-collection — fetch
    /// attempts logged), so this filters on `error IS NULL` rather than a
    /// separately stored outcome flag.
    pub async fn last_success(&self, source: &str) -> Result<Option<LogRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, ts_epoch, ts, duration_ms, error, value, COALESCE(origin,'poll') FROM fetch_logs
             WHERE source = ? AND error IS NULL
             ORDER BY ts_epoch DESC, id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![source])?;
        Ok(rows
            .next()?
            .map(|r| {
                Ok::<_, duckdb::Error>(LogRow {
                    source: r.get(0)?,
                    ts_epoch: r.get(1)?,
                    ts: r.get(2)?,
                    duration_ms: r.get(3)?,
                    error: r.get(4)?,
                    value: r.get(5)?,
                    unit: None,
                    origin: Origin::from_db(&r.get::<_, String>(6)?),
                })
            })
            .transpose()?)
    }

    /// Most recent fetch log for `source` regardless of outcome, if any
    /// (spec: data-collection — Per-source schedules: daemon startup resumes
    /// from the last run). Outcome is derived from `error` presence (a
    /// successful attempt carries no error detail), mirroring
    /// [`Db::last_success`] but without its `error IS NULL` filter. Same
    /// `ts_epoch DESC, id DESC` ordering so same-millisecond ties resolve
    /// deterministically to the higher-id row.
    pub async fn last_attempt(&self, source: &str) -> Result<Option<LogRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, ts_epoch, ts, duration_ms, error, value, COALESCE(origin,'poll') FROM fetch_logs
             WHERE source = ? ORDER BY ts_epoch DESC, id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![source])?;
        Ok(rows
            .next()?
            .map(|r| {
                Ok::<_, duckdb::Error>(LogRow {
                    source: r.get(0)?,
                    ts_epoch: r.get(1)?,
                    ts: r.get(2)?,
                    duration_ms: r.get(3)?,
                    error: r.get(4)?,
                    value: r.get(5)?,
                    unit: None,
                    origin: Origin::from_db(&r.get::<_, String>(6)?),
                })
            })
            .transpose()?)
    }

    /// Deletes readings, fetch logs, and health events older than
    /// `cutoff_epoch` (spec: data-storage — retention). Run periodically by
    /// the daemon when `Config::retention` is set.
    pub async fn purge_older_than(&self, cutoff_epoch: f64) -> Result<()> {
        if let Some(writer) = &self.writer {
            let (reply, rx) = oneshot::channel();
            writer
                .send(WriteCmd::PurgeOlderThan {
                    cutoff_epoch,
                    reply,
                })
                .map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "DELETE FROM readings WHERE ts_epoch < ?",
            params![cutoff_epoch],
        )?;
        conn.execute(
            "DELETE FROM fetch_logs WHERE ts_epoch < ?",
            params![cutoff_epoch],
        )?;
        conn.execute(
            "DELETE FROM health_events WHERE ts_epoch < ?",
            params![cutoff_epoch],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A separate process's read-only connection (direct-mode CLI/TUI) must
    /// see a write as soon as the daemon's `insert_*` call returns success —
    /// not only after the daemon stops. Without a checkpoint after every
    /// write, `DuckDB`'s read-only mode can only see what was checkpointed
    /// into the main file, leaving concurrent reads stuck on stale data.
    #[tokio::test]
    async fn readonly_reader_sees_a_write_without_the_writer_stopping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.duckdb");
        let writer = Db::open_rw_daemon(&path).unwrap();
        writer
            .insert_reading("cpu", "42", None, None, None, None)
            .await
            .unwrap();

        let reader = Db::open_ro(&path).unwrap();
        let latest = reader.latest_values().await.unwrap();
        assert_eq!(
            latest.len(),
            1,
            "read-only reader should already see the write"
        );
        assert_eq!(latest[0].value, "42");
    }

    /// Two attempts landing in the same millisecond (routine — every source
    /// fires at daemon startup) must still resolve to a deterministic order:
    /// `id DESC`, not whatever order `DuckDB` happens to return equal
    /// `ts_epoch` rows in.
    #[tokio::test]
    async fn logs_and_last_success_break_ts_epoch_ties_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch(
                "INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value)
                 VALUES (nextval('fetch_logs_id_seq'), 'a', 1000.0, 't', 1, NULL, 'first');
                 INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value)
                 VALUES (nextval('fetch_logs_id_seq'), 'a', 1000.0, 't', 1, NULL, 'second');",
            )?;
            Ok(())
        })
        .unwrap();

        let logs = db.logs(Some("a"), 10).await.unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(
            logs[0].value.as_deref(),
            Some("second"),
            "the higher-id row should sort first despite an equal ts_epoch"
        );
        assert_eq!(logs[1].value.as_deref(), Some("first"));

        let last = db.last_success("a").await.unwrap().unwrap();
        assert_eq!(last.value.as_deref(), Some("second"));
    }

    /// A quiet source's rows survive the global limit when filtered by
    /// source: the filter applies inside the query instead of
    /// fetch-then-filter (spec: cli — Filter query output by source).
    #[tokio::test]
    async fn logs_for_sources_not_crowded_out_by_global_limit() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch(
                "INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value)
                 VALUES (nextval('fetch_logs_id_seq'), 'quiet', 1000.0, 't', 1, NULL, 'q');
                 INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value)
                 VALUES (nextval('fetch_logs_id_seq'), 'chatty', 2000.0, 't', 1, NULL, 'c1');
                 INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value)
                 VALUES (nextval('fetch_logs_id_seq'), 'chatty', 3000.0, 't', 1, NULL, 'c2');",
            )?;
            Ok(())
        })
        .unwrap();

        // A global limit of 2 never reaches the quiet row.
        let global = db.logs(None, 2).await.unwrap();
        assert!(global.iter().all(|r| r.source == "chatty"));

        let filtered = db
            .logs_for_sources(&["quiet".to_string()], 2)
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].value.as_deref(), Some("q"));
    }

    /// A negative or absurdly large requested `limit` (`GET
    /// /api/logs?limit=-1` or `limit=1000000000`, or the CLI's `--limit`)
    /// must not force an unbounded `fetch_logs` scan — `DuckDB` treats a
    /// negative `LIMIT` as unbounded, so this has to be clamped before it
    /// ever reaches the query (spec: http-api — bounded queries).
    #[tokio::test]
    async fn logs_limit_is_clamped_not_passed_through_unbounded() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        for i in 0..5 {
            db.insert_log("s", 1, None, Some(&i.to_string()), Origin::Poll)
                .await
                .unwrap();
        }

        // Same clamp `history()` already uses: a negative limit clamps up to
        // the floor of 1 rather than being treated as "no limit" — it never
        // reaches the query as a negative number, so `DuckDB` never sees the
        // value that would make it scan unbounded.
        let negative = db.logs(Some("s"), -1).await.unwrap();
        assert_eq!(negative.len(), 1, "negative limit clamps to the floor of 1");

        let huge = db.logs(Some("s"), 1_000_000_000).await.unwrap();
        assert_eq!(huge.len(), 5, "still returns every row that exists");

        let via_sources = db
            .logs_for_sources(&["s".to_string()], -1)
            .await
            .unwrap();
        assert_eq!(via_sources.len(), 1);
    }

    /// Pre-migration rows (no `origin` column) read as `poll` once the
    /// database is opened, while newly written rows keep their origin
    /// (spec: data-collection — Fetch attempts logged).
    #[tokio::test]
    async fn origin_backfills_poll_and_new_rows_keep_origin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.duckdb");
        {
            let conn = duckdb::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE SEQUENCE fetch_logs_id_seq;
                 CREATE TABLE fetch_logs (
                     id BIGINT NOT NULL, source VARCHAR NOT NULL,
                     ts_epoch DOUBLE NOT NULL, ts VARCHAR NOT NULL,
                     duration_ms BIGINT NOT NULL, error VARCHAR, value VARCHAR
                 );
                 INSERT INTO fetch_logs (id, source, ts_epoch, ts, duration_ms, error, value)
                 VALUES (nextval('fetch_logs_id_seq'), 'old', 1000.0, 't', 1, NULL, 'v');",
            )
            .unwrap();
        }
        let db = Db::open_rw(&path).unwrap();
        let rows = db.logs(Some("old"), 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].origin, Origin::Poll);

        db.insert_log("old", 1, None, Some("w"), Origin::Push)
            .await
            .unwrap();
        let rows = db.logs(Some("old"), 10).await.unwrap();
        assert_eq!(rows[0].origin, Origin::Push);
        assert_eq!(rows[1].origin, Origin::Poll);
    }

    /// Opening a pre-typed-columns database file (`readings` predates
    /// `value_bigint`/`value_double`/`value_json`) must not fail subsequent
    /// inserts with "column not found" — `CREATE TABLE IF NOT EXISTS` alone
    /// is a no-op on an existing table, so the typed columns need the same
    /// `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` migration `fetch_logs.origin`
    /// gets (spec: data-storage — typed value columns).
    #[tokio::test]
    async fn readings_typed_columns_migrate_onto_a_pre_typed_columns_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.duckdb");
        {
            let conn = duckdb::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE SEQUENCE readings_id_seq;
                 CREATE TABLE readings (
                     id BIGINT NOT NULL, source VARCHAR NOT NULL, value VARCHAR NOT NULL,
                     unit VARCHAR, ts_epoch DOUBLE NOT NULL, ts VARCHAR NOT NULL
                 );
                 INSERT INTO readings (id, source, value, unit, ts_epoch, ts)
                 VALUES (nextval('readings_id_seq'), 'old', 'v', NULL, 1000.0, 't');",
            )
            .unwrap();
        }
        let db = Db::open_rw(&path).unwrap();
        db.insert_reading("old", "42", None, Some(42), None, None)
            .await
            .unwrap();
        let rows = db.history("old", None, None, None).await.unwrap();
        assert_eq!(rows.len(), 2, "the pre-migration row must survive too");
    }

    /// `last_attempt` returns the newest entry whatever its outcome, unlike
    /// `last_success` (spec: data-collection — Per-source schedules: daemon
    /// startup resumes from the last run).
    #[tokio::test]
    async fn last_attempt_returns_newest_row_regardless_of_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        assert!(db.last_attempt("missing").await.unwrap().is_none());

        db.insert_log("s", 1, None, Some("ok"), Origin::Poll)
            .await
            .unwrap();
        let first = db.last_attempt("s").await.unwrap().unwrap();
        assert!(first.error.is_none());

        db.insert_log("s", 1, Some("boom"), None, Origin::Poll)
            .await
            .unwrap();
        let second = db.last_attempt("s").await.unwrap().unwrap();
        assert_eq!(second.error.as_deref(), Some("boom"));

        // A later success wins again, and `last_success` still skips failures.
        db.insert_log("s", 1, None, Some("ok2"), Origin::Poll)
            .await
            .unwrap();
        let third = db.last_attempt("s").await.unwrap().unwrap();
        assert!(third.error.is_none());
        assert_eq!(third.value.as_deref(), Some("ok2"));
        let success = db.last_success("s").await.unwrap().unwrap();
        assert_eq!(success.value.as_deref(), Some("ok2"));
    }

    #[tokio::test]
    async fn last_health_breaks_ts_epoch_ties_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch(
                "INSERT INTO health_events (id, source, ts_epoch, ts, status)
                 VALUES (nextval('health_events_id_seq'), 'a', 1000.0, 't', 'failing');
                 INSERT INTO health_events (id, source, ts_epoch, ts, status)
                 VALUES (nextval('health_events_id_seq'), 'a', 1000.0, 't', 'healthy');",
            )?;
            Ok(())
        })
        .unwrap();

        let status = db.last_health("a").await.unwrap();
        assert_eq!(status.as_deref(), Some("healthy"));
    }

    /// (spec: data-storage — typed value columns)
    #[tokio::test]
    async fn readings_table_has_typed_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        let cols: Vec<String> = db
            .with_conn(|conn| {
                let mut stmt = conn.prepare("DESCRIBE readings")?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap();
        for col in ["value_bigint", "value_double", "value_json"] {
            assert!(
                cols.contains(&col.to_string()),
                "missing column {col}: {cols:?}"
            );
        }
    }

    /// Each typed insert populates exactly its own typed column, leaving the
    /// other two `NULL` and `value` populated as always (spec: data-storage —
    /// typed value columns).
    #[tokio::test]
    async fn insert_reading_populates_only_its_declared_typed_column() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_reading("bi", "42", None, Some(42), None, None)
            .await
            .unwrap();
        db.insert_reading("db", "1.5", None, None, Some(1.5), None)
            .await
            .unwrap();
        db.insert_reading(
            "js",
            "{\"ok\":true}",
            None,
            None,
            None,
            Some("{\"ok\":true}"),
        )
        .await
        .unwrap();
        db.insert_reading("plain", "hello", None, None, None, None)
            .await
            .unwrap();

        let fetch = |source: &str| -> (Option<i64>, Option<f64>, Option<String>) {
            db.with_conn(|conn| {
                conn.query_row(
                    "SELECT value_bigint, value_double, value_json FROM readings WHERE source = ?",
                    params![source],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<String>>(2)?)),
                )
                .map_err(Into::into)
            })
            .unwrap()
        };

        assert_eq!(fetch("bi"), (Some(42), None, None));
        assert_eq!(fetch("db"), (None, Some(1.5), None));
        assert_eq!(fetch("js"), (None, None, Some("{\"ok\":true}".to_string())));
        assert_eq!(fetch("plain"), (None, None, None));
    }

    #[test]
    fn panic_message_extracts_str_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(&*payload), "boom");
    }

    #[test]
    fn panic_message_extracts_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("boom"));
        assert_eq!(panic_message(&*payload), "boom");
    }

    #[test]
    fn panic_message_falls_back_for_other_payloads() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        assert_eq!(panic_message(&*payload), "<non-string panic payload>");
    }

    /// Session overrides are visible on the same handle and its clones,
    /// but invisible on a fresh handle over the same file: a simulated
    /// restart forgets them and reseeds from config (spec:
    /// source-configuration — Threshold bands).
    #[tokio::test]
    async fn session_bands_apply_live_and_vanish_on_new_handle() {
        use crate::config::{Level, Threshold};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.duckdb");
        let db = Db::open_rw(&path).unwrap();
        assert!(db.session_bands("s").is_none());
        let over = vec![
            Threshold {
                bound: 10.0,
                level: Level::Red,
            },
            Threshold {
                bound: 20.0,
                level: Level::Green,
            },
        ];
        db.set_session_bands("s", &over);
        assert_eq!(db.session_bands("s").unwrap(), over);
        // A clone shares the session (the daemon's collectors and HTTP
        // handlers all see the same overrides).
        assert_eq!(db.clone().session_bands("s").unwrap(), over);
        // A fresh handle is a fresh session: overrides are forgotten.
        let restarted = Db::open_rw(&path).unwrap();
        assert!(restarted.session_bands("s").is_none());
    }

    /// Explicit timestamps land on the reading (spec: data-storage —
    /// Readings persisted with provenance).
    #[tokio::test]
    async fn insert_reading_at_stamps_caller_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_rw(&dir.path().join("t.duckdb")).unwrap();
        db.insert_reading_at("s", "v", None, None, None, None, 1000.5, "then")
            .await
            .unwrap();
        let rows = db.history("s", None, None, None).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].ts_epoch - 1000.5).abs() < 0.001);
        assert_eq!(rows[0].ts, "then");
    }
}
