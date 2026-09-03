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

/// Handle to the `DuckDB` database file.
///
/// `DuckDB` takes an exclusive cross-process lock on the file and a read-only
/// open racing a live write can see torn blocks (spec: data-storage —
/// concurrent access safety). So every operation — daemon writes and
/// direct-mode reads alike — takes an advisory flock on a sidecar lockfile
/// for its duration, then opens its own short-lived `DuckDB` connection.
/// `ponytail`: reopen per operation (~ms); move to a resident reader process
/// only if this shows up in practice.
#[derive(Clone)]
pub struct Db {
    path: PathBuf,
    ro: bool,
}

struct DbLock(#[allow(dead_code)] File);

fn is_lock_conflict(e: &anyhow::Error) -> bool {
    let msg = format!("{e:#}");
    msg.contains("Conflicting lock") || msg.contains("Could not read enough bytes")
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

fn now() -> (f64, String) {
    let t: DateTime<Utc> = Utc::now();
    (t.timestamp_millis() as f64 / 1000.0, t.to_rfc3339())
}

fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS readings (
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
        ALTER TABLE fetch_logs ADD COLUMN IF NOT EXISTS value VARCHAR;",
    )?;
    Ok(())
}

impl Db {
    /// Read-write handle for the daemon; creates schema.
    pub fn open_rw(path: &Path) -> Result<Self> {
        let db = Self {
            path: path.to_path_buf(),
            ro: false,
        };
        db.with_conn(create_schema)?;
        Ok(db)
    }

    /// Read-only handle for direct-mode CLI/TUI while the daemon may be running.
    pub fn open_ro(path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
            ro: true,
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
                DROP TABLE IF EXISTS health_events;",
            )?;
            create_schema(conn)
        })
    }

    /// Takes the advisory lock serializing all database access.
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

    /// Opens a `DuckDB` connection, retrying on transient conflicts.
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

    async fn connect_async(&self) -> Result<(DbLock, Connection)> {
        // Same retry logic without blocking an async executor thread.
        const ATTEMPTS: u32 = 50;
        let mut last = None;
        for i in 0..ATTEMPTS {
            let guard = match self.acquire() {
                Ok(g) => g,
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
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
                    tokio::time::sleep(Duration::from_millis(u64::from(100 + 10 * (i % 10)))).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("could not open {}", self.path.display())))
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let _guard = self.acquire()?;
        let conn = self.connect()?;
        f(&conn)
    }

    pub async fn insert_reading(&self, source: &str, value: &str, unit: Option<&str>) -> Result<()> {
        let (e, ts) = now();
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO readings VALUES (?, ?, ?, ?, ?)",
            params![source, value, unit, e, ts],
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
        let (e, ts) = now();
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO fetch_logs VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![source, e, ts, ok, duration_ms, error, value],
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
        let (e, ts) = now();
        let (_guard, conn) = self.connect_async().await?;
        conn.execute(
            "INSERT INTO health_events VALUES (?, ?, ?, ?)",
            params![source, e, ts, status],
        )?;
        Ok(())
    }

    /// Latest reading per source.
    pub async fn latest_values(&self) -> Result<Vec<ReadingRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT r.source, r.value, r.unit, r.ts_epoch, r.ts
             FROM readings r
             JOIN (SELECT source, MAX(rowid) AS rid FROM readings GROUP BY source) t
               ON r.rowid = t.rid
             ORDER BY r.source",
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

    pub async fn history(&self, source: &str, from: Option<f64>, to: Option<f64>) -> Result<Vec<ReadingRow>> {
        let (_guard, conn) = self.connect_async().await?;
        let mut stmt = conn.prepare(
            "SELECT source, value, unit, ts_epoch, ts FROM readings
             WHERE source = ?
               AND (?::DOUBLE IS NULL OR ts_epoch >= ?::DOUBLE)
               AND (?::DOUBLE IS NULL OR ts_epoch <= ?::DOUBLE)
             ORDER BY ts_epoch",
        )?;
        let rows = stmt
            .query_map(params![source, from, from, to, to], |r| {
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
}

// Synchronous wrappers used where no runtime context exists (health compute).
impl Db {
    pub fn last_health_sync(&self, source: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT status FROM health_events WHERE source = ? ORDER BY ts_epoch DESC LIMIT 1",
            )?;
            let mut rows = stmt.query(params![source])?;
            Ok(rows.next()?.map(|r| r.get(0)).transpose()?)
        })
    }

    pub fn latest_values_sync(&self) -> Result<Vec<ReadingRow>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT r.source, r.value, r.unit, r.ts_epoch, r.ts
                 FROM readings r
                 JOIN (SELECT source, MAX(rowid) AS rid FROM readings GROUP BY source) t
                   ON r.rowid = t.rid
                 ORDER BY r.source",
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
        })
    }

    /// Last `limit` readings for `source`, oldest first (spec: web-ui —
    /// panel retrospective history bar).
    pub fn history_sync(&self, source: &str, limit: i64) -> Result<Vec<ReadingRow>> {
        self.with_conn(|conn| {
            // Tie-break on rowid: ts_epoch is millisecond-resolution, so
            // readings collected in quick succession can share a timestamp.
            let mut stmt = conn.prepare(
                "SELECT source, value, unit, ts_epoch, ts FROM readings
                 WHERE source = ?
                 ORDER BY ts_epoch DESC, rowid DESC LIMIT ?",
            )?;
            let mut rows = stmt
                .query_map(params![source, limit], |r| {
                    Ok(ReadingRow {
                        source: r.get(0)?,
                        value: r.get(1)?,
                        unit: r.get(2)?,
                        ts_epoch: r.get(3)?,
                        ts: r.get(4)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.reverse();
            Ok(rows)
        })
    }

    pub fn logs_sync(&self, source: Option<&str>, limit: i64) -> Result<Vec<LogRow>> {
        self.with_conn(|conn| {
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
        })
    }
}
