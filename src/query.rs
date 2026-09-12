use crate::{
    config::Config,
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

async fn get<T: serde::de::DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("connecting to daemon at {url}"))?;
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
