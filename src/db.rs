#![allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]

use anyhow::{Context as _, Result};
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};
use fs2::FileExt;
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
}

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
        ts_epoch: f64,
        ts: String,
        reply: oneshot::Sender<Result<()>>,
    },
    InsertLog {
        source: String,
        ts_epoch: f64,
        ts: String,
        ok: bool,
        duration_ms: i64,
        error: Option<String>,
        value: Option<String>,
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
    fn run(self, db: &Db, conn: &Connection) {
        let (result, reply) = match self {
            WriteCmd::InsertReading { source, value, unit, ts_epoch, ts, reply } => (
                db.with_lock(|| {
                    conn.execute(
                        "INSERT INTO readings (id, source, value, unit, ts_epoch, ts)
                         VALUES (nextval('readings_id_seq'), ?, ?, ?, ?, ?)",
                        params![source, value, unit, ts_epoch, ts],
                    )?;
                    Ok(())
                }),
                reply,
            ),
            WriteCmd::InsertLog { source, ts_epoch, ts, ok, duration_ms, error, value, reply } => (
                db.with_lock(|| {
                    conn.execute(
                        "INSERT INTO fetch_logs VALUES (?, ?, ?, ?, ?, ?, ?)",
                        params![source, ts_epoch, ts, ok, duration_ms, error, value],
                    )?;
                    Ok(())
                }),
                reply,
            ),
            WriteCmd::InsertHealthEvent { source, ts_epoch, ts, status, reply } => (
                db.with_lock(|| {
                    conn.execute(
                        "INSERT INTO health_events VALUES (?, ?, ?, ?)",
                        params![source, ts_epoch, ts, status],
                    )?;
                    Ok(())
                }),
                reply,
            ),
            WriteCmd::PurgeOlderThan { cutoff_epoch, reply } => (
                db.with_lock(|| {
                    conn.execute("DELETE FROM readings WHERE ts_epoch < ?", params![cutoff_epoch])?;
                    conn.execute("DELETE FROM fetch_logs WHERE ts_epoch < ?", params![cutoff_epoch])?;
                    conn.execute("DELETE FROM health_events WHERE ts_epoch < ?", params![cutoff_epoch])?;
                    Ok(())
                }),
                reply,
            ),
        };
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRow {
    pub source: String,
    pub ts_epoch: f64,
    pub ts: String,
    pub ok: bool,
    pub duration_ms: i64,
    pub error: Option<String>,
    pub value: Option<String>,
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

fn now() -> (f64, String) {
    let t: DateTime<Utc> = Utc::now();
    (t.timestamp_millis() as f64 / 1000.0, t.to_rfc3339())
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE SEQUENCE IF NOT EXISTS readings_id_seq;
        CREATE TABLE IF NOT EXISTS readings (
            source VARCHAR NOT NULL,
            value  VARCHAR NOT NULL,
            unit   VARCHAR,
            ts_epoch DOUBLE NOT NULL,
            ts     VARCHAR NOT NULL
        );
        CREATE TABLE IF NOT EXISTS fetch_logs (
            source      VARCHAR NOT NULL,
            ts_epoch    DOUBLE NOT NULL,
            ts          VARCHAR NOT NULL,
            ok          BOOLEAN NOT NULL,
            duration_ms BIGINT  NOT NULL,
            error       VARCHAR,
            value       VARCHAR
        );
        CREATE TABLE IF NOT EXISTS health_events (
            source   VARCHAR NOT NULL,
            ts_epoch DOUBLE NOT NULL,
            ts       VARCHAR NOT NULL,
            status   VARCHAR NOT NULL
        );
        ALTER TABLE fetch_logs ADD COLUMN IF NOT EXISTS value VARCHAR;
        -- Explicit monotonic id, replacing `rowid` (a physical-storage
        -- identifier `DuckDB` doesn't guarantee as a stable ordering key) for
        -- tie-breaking same-timestamp readings. NULL on rows inserted before
        -- this column existed; `latest_values`/`history` fall back to `rowid`
        -- for those via COALESCE, so old data keeps its previous ordering.
        ALTER TABLE readings ADD COLUMN IF NOT EXISTS id BIGINT;
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
fn spawn_writer(path: PathBuf) -> mpsc::UnboundedSender<WriteCmd> {
    let (tx, mut rx) = mpsc::unbounded_channel::<WriteCmd>();
    tokio::task::spawn_blocking(move || {
        let db = Db { path, ro: false, writer: None };
        // The initial open races the same "read-only open sees a torn write"
        // hazard as any other operation, so it takes the lock too — briefly,
        // same as every write that follows.
        let conn = match db.acquire().and_then(|guard| { let c = db.connect(); drop(guard); c }) {
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
            if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cmd.run(&db, &conn))) {
                tracing::error!("db writer: write command panicked: {}", panic_message(&panic));
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
        })
    }

    /// Drops every table (readings, fetch logs, health events) and recreates
    /// an empty schema. Callers are responsible for confirming with the user
    /// first — this is irreversible.
    pub fn reset(&self) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute_batch(
                "DROP TABLE IF EXISTS readings;
                DROP TABLE IF EXISTS fetch_logs;
                DROP TABLE IF EXISTS health_events;
                DROP SEQUENCE IF EXISTS readings_id_seq;",
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
            match file.try_lock_exclusive() {
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

    pub async fn insert_reading(&self, source: &str, value: &str, unit: Option<&str>) -> Result<()> {
        let (ts_epoch, ts) = now();
        if let Some(writer) = &self.writer {
            let (reply, rx) = oneshot::channel();
            let cmd = WriteCmd::InsertReading {
                source: source.to_string(),
                value: value.to_string(),
                unit: unit.map(str::to_string),
                ts_epoch,
                ts,
                reply,
            };
            writer.send(cmd).map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO readings (id, source, value, unit, ts_epoch, ts)
             VALUES (nextval('readings_id_seq'), ?, ?, ?, ?, ?)",
            params![source, value, unit, ts_epoch, ts],
        )?;
        Ok(())
    }
    pub async fn insert_log(
        &self,
        source: &str,
        ok: bool,
        duration_ms: i64,
        error: Option<&str>,
        value: Option<&str>,
    ) -> Result<()> {
        let (ts_epoch, ts) = now();
        if let Some(writer) = &self.writer {
            let (reply, rx) = oneshot::channel();
            let cmd = WriteCmd::InsertLog {
                source: source.to_string(),
                ts_epoch,
                ts,
                ok,
                duration_ms,
                error: error.map(str::to_string),
                value: value.map(str::to_string),
                reply,
            };
            writer.send(cmd).map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO fetch_logs VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![source, ts_epoch, ts, ok, duration_ms, error, value],
        )?;
        Ok(())
    }

    pub async fn last_health(&self, source: &str) -> Result<Option<String>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT status FROM health_events WHERE source = ? ORDER BY ts_epoch DESC LIMIT 1",
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
            writer.send(cmd).map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO health_events VALUES (?, ?, ?, ?)",
            params![source, ts_epoch, ts, status],
        )?;
        Ok(())
    }

    /// Latest reading per source: highest `ts_epoch`, with
    /// `COALESCE(id, rowid)` only as a tie-break for same-timestamp rows
    /// (matching `history`'s own ordering). `ts_epoch` must be the primary
    /// key here, not `COALESCE(id, rowid)` alone — a row inserted before the
    /// `id` column existed keeps `id = NULL` and falls back to its `rowid`,
    /// a physical offset that isn't comparable to the fresh `id` sequence
    /// newer rows get; ordering by that value directly stuck this query on
    /// the last pre-migration row per source forever, regardless of how
    /// many newer readings came in after.
    pub async fn latest_values(&self) -> Result<Vec<ReadingRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, value, unit, ts_epoch, ts FROM (
                SELECT source, value, unit, ts_epoch, ts,
                       ROW_NUMBER() OVER (
                           PARTITION BY source
                           ORDER BY ts_epoch DESC, COALESCE(id, rowid) DESC
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
        let limit = limit.unwrap_or(DEFAULT_HISTORY_LIMIT).clamp(1, MAX_HISTORY_LIMIT);
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, value, unit, ts_epoch, ts FROM (
                SELECT source, value, unit, ts_epoch, ts, COALESCE(id, rowid) AS ord FROM readings
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
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, ts_epoch, ts, ok, duration_ms, error, value FROM fetch_logs
             WHERE (?::VARCHAR IS NULL OR source = ?::VARCHAR)
             ORDER BY ts_epoch DESC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![source, source, limit], |r| {
                Ok(LogRow {
                    source: r.get(0)?,
                    ts_epoch: r.get(1)?,
                    ts: r.get(2)?,
                    ok: r.get(3)?,
                    duration_ms: r.get(4)?,
                    error: r.get(5)?,
                    value: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Most recent fetch log for `source`, if any (spec: data-collection —
    /// consecutive failures flip to failing). Used by [`crate::health::compute`]
    /// instead of scanning the source's entire log history.
    pub async fn last_success(&self, source: &str) -> Result<Option<LogRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, ts_epoch, ts, ok, duration_ms, error, value FROM fetch_logs
             WHERE source = ? AND ok
             ORDER BY ts_epoch DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![source])?;
        Ok(rows
            .next()?
            .map(|r| {
                Ok::<_, duckdb::Error>(LogRow {
                    source: r.get(0)?,
                    ts_epoch: r.get(1)?,
                    ts: r.get(2)?,
                    ok: r.get(3)?,
                    duration_ms: r.get(4)?,
                    error: r.get(5)?,
                    value: r.get(6)?,
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
                .send(WriteCmd::PurgeOlderThan { cutoff_epoch, reply })
                .map_err(|_| anyhow::anyhow!("db writer task has stopped"))?;
            return rx.await.context("db writer task dropped the reply")?;
        }
        let (_guard, conn) = self.connect_async().await?;
        conn.execute("DELETE FROM readings WHERE ts_epoch < ?", params![cutoff_epoch])?;
        conn.execute("DELETE FROM fetch_logs WHERE ts_epoch < ?", params![cutoff_epoch])?;
        conn.execute("DELETE FROM health_events WHERE ts_epoch < ?", params![cutoff_epoch])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::panic_message;

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
}
