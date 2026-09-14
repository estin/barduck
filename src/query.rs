use crate::{
    collector::{self, PollOutcome},
    config::{Config, SourceCfg},
    db::{self, Db},
    health,
};
use anyhow::{Context as _, Result};

/// Read path shared by CLI and TUI: direct read-only `DuckDB` by default, or
/// the daemon's HTTP API (spec: cli — dual modes).
#[derive(Clone)]
pub enum Backend {
    Direct(Db),
    Daemon {
        base: String,
        client: reqwest::Client,
    },
}

/// Per-request timeout for the daemon-mode HTTP client. Without one, a
/// stalled daemon response (network hang, a wedged handler) leaves the
/// request pending forever — in the TUI that means `refresh_in_flight`
/// never clears, silently freezing the display on stale data with no error
/// shown, since nothing ever completes to report one.
const DAEMON_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl Backend {
    /// Picks the read path for a CLI/TUI invocation (spec: cli — dual
    /// modes). `use_daemon` forces the HTTP API; otherwise this reads the
    /// database file directly — *unless* a running daemon already holds the
    /// file's `DuckDB` lock, in which case it transparently uses the daemon's
    /// API instead.
    ///
    /// That fallback is what makes "direct mode while the daemon may be
    /// running" actually work. `DuckDB` allows only one process to hold a
    /// database at a time, and the daemon keeps its connection open for its
    /// whole lifetime (spec: data-storage — single daemon writer, resident
    /// reader connection), so every direct-mode read while the daemon runs
    /// otherwise failed outright with `Conflicting lock is held`. The
    /// sidecar advisory lock doesn't help here: it serializes *operations*
    /// between processes, but cannot make `DuckDB` hand over a lock the
    /// daemon never lets go of.
    pub fn new(cfg: &Config, use_daemon: bool) -> Result<Self> {
        if !use_daemon {
            let db = Db::open_ro(&cfg.database_path)?;
            if !db.is_locked_by_another_process() {
                return Ok(Self::Direct(db));
            }
            tracing::debug!(
                "{} is locked by another process (a running daemon?); \
                 reading through the daemon API instead",
                cfg.database_path.display()
            );
        }
        Ok(Self::Daemon {
            base: format!("http://{}", cfg.listen),
            client: reqwest::Client::builder()
                .timeout(DAEMON_REQUEST_TIMEOUT)
                .build()
                .context("building HTTP client")?,
        })
    }

    pub async fn latest(&self) -> Result<Vec<db::ReadingRow>> {
        match self {
            Backend::Direct(db) => db.latest_values().await,
            Backend::Daemon { base, client } => {
                get(client, &format!("{base}/api/sources/latest")).await
            }
        }
    }

    pub async fn history(
        &self,
        source: &str,
        from: Option<f64>,
        to: Option<f64>,
    ) -> Result<Vec<db::ReadingRow>> {
        match self {
            Backend::Direct(db) => db.history(source, from, to, None).await,
            Backend::Daemon { base, client } => {
                let mut url = format!("{base}/api/sources/{source}/history");
                let mut q = Vec::new();
                if let Some(f) = from {
                    q.push(format!("from={f}"));
                }
                if let Some(t) = to {
                    q.push(format!("to={t}"));
                }
                if !q.is_empty() {
                    url.push('?');
                    url.push_str(&q.join("&"));
                }
                get(client, &url).await
            }
        }
    }

    pub async fn health(&self, cfg: &Config) -> Result<Vec<health::SourceHealth>> {
        match self {
            Backend::Direct(db) => health::compute_all(db, cfg).await,
            Backend::Daemon { base, client } => get(client, &format!("{base}/api/health")).await,
        }
    }

    /// Fetches one source now, ignoring its schedule, and reports the
    /// attempt (spec: cli — Force poll command). In direct mode the fetch
    /// runs here, in the CLI process, writing through a short-lived
    /// read-write handle — `Backend::Direct`'s own handle is read-only and
    /// must stay that way, since every other caller (the TUI, the query
    /// commands) must not take a write lock. Under a running daemon the
    /// request goes to it instead, and its collector task does the work.
    pub async fn poll(&self, cfg: &Config, src: &SourceCfg) -> Result<PollOutcome> {
        match self {
            Backend::Direct(_) => {
                let db = Db::open_rw(&cfg.database_path)?;
                Ok(collector::poll_once(&db, cfg, src).await)
            }
            Backend::Daemon { base, client } => {
                let url = format!("{base}/api/sources/{}/poll", src.name());
                let resp = client
                    .post(&url)
                    .timeout(poll_timeout(src.timeout()))
                    .send()
                    .await
                    .with_context(|| format!("connecting to daemon at {url}"))?;
                read_json(resp, &url).await
            }
        }
    }

    pub async fn logs(&self, sources: &[String], limit: i64) -> Result<Vec<db::LogRow>> {
        match self {
            Backend::Direct(db) => db.logs_for_sources(sources, limit).await,
            Backend::Daemon { base, client } => {
                let mut url = format!("{base}/api/logs?limit={limit}");
                for s in sources {
                    let _ = std::fmt::write(&mut url, format_args!("&source={s}"));
                }
                get(client, &url).await
            }
        }
    }
}

/// How long a daemon-mode forced poll waits for its answer. The daemon runs
/// the source's own command — bounded by that source's configured timeout —
/// and the request can first queue behind one already-running fetch of the
/// same source (spec: data-collection — Forced polls are serialized with a
/// source's schedule). So: two of those, plus the ordinary request budget
/// for everything that isn't the fetch. Reads keep
/// [`DAEMON_REQUEST_TIMEOUT`] unchanged — this is the one request that can
/// legitimately outlast it.
fn poll_timeout(source_timeout: std::time::Duration) -> std::time::Duration {
    source_timeout
        .saturating_mul(2)
        .saturating_add(DAEMON_REQUEST_TIMEOUT)
}

async fn get<T: serde::de::DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("connecting to daemon at {url}"))?;
    read_json(resp, url).await
}

/// Shared response handling for both verbs: a non-success status is an
/// error naming what the daemon said, a success decodes into `T`.
async fn read_json<T: serde::de::DeserializeOwned>(resp: reqwest::Response, url: &str) -> Result<T> {
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("daemon returned {status}: {}", truncate(&body, 300));
    }
    resp.json()
        .await
        .with_context(|| format!("decoding response from {url}"))
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::{DAEMON_REQUEST_TIMEOUT, poll_timeout};
    use std::time::Duration;

    /// A forced poll's budget has to cover the source's own command, not the
    /// query default it would otherwise inherit (spec: cli — Force poll
    /// command).
    #[test]
    fn poll_timeout_covers_two_source_timeouts_plus_the_request_budget() {
        assert_eq!(
            poll_timeout(Duration::from_secs(30)),
            Duration::from_secs(70)
        );
        assert!(
            poll_timeout(Duration::from_secs(1)) > DAEMON_REQUEST_TIMEOUT,
            "even a fast source gets more than the read budget"
        );
        // A pathological configured timeout saturates rather than panicking
        // on overflow.
        assert!(poll_timeout(Duration::MAX) > DAEMON_REQUEST_TIMEOUT);
    }
}
