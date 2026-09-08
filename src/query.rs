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

impl Backend {
    pub fn new(cfg: &Config, use_daemon: bool) -> Result<Self> {
        if use_daemon {
            Ok(Self::Daemon {
                base: format!("http://{}", cfg.listen),
                client: reqwest::Client::new(),
            })
        } else {
            Ok(Self::Direct(Db::open_ro(&cfg.database_path)?))
        }
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
            Backend::Direct(db) => {
                let mut out = Vec::new();
                for s in &cfg.sources {
                    out.push(health::compute(db, cfg, &s.name).await?);
                }
                Ok(out)
            }
            Backend::Daemon { base, client } => get(client, &format!("{base}/api/health")).await,
        }
    }

    pub async fn logs(&self, limit: i64) -> Result<Vec<db::LogRow>> {
        match self {
            Backend::Direct(db) => db.logs(None, limit).await,
            Backend::Daemon { base, client } => {
                get(client, &format!("{base}/api/logs?limit={limit}")).await
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
