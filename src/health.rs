#![allow(clippy::cast_precision_loss)]

use crate::{config::Config, db::Db};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Healthy,
    Failing,
    Stale,
}

impl Health {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Healthy => "healthy",
            Health::Failing => "failing",
            Health::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHealth {
    pub source: String,
    pub status: Health,
    pub consecutive_failures: u32,
    pub last_success_ts: Option<String>,
}

/// Derives live health for one source from its recent fetch logs and the age
/// of its last success (spec: data-collection — consecutive failures flip to
/// failing, old successes go stale). Two bounded queries: the last
/// `failure_threshold` logs (for the consecutive-failure count) and the
/// single most recent successful fetch (for staleness) — neither scans the
/// source's full log history.
pub async fn compute(db: &Db, cfg: &Config, source: &str) -> Result<SourceHealth> {
    let logs = db.logs(Some(source), i64::from(cfg.failure_threshold)).await?;
    #[allow(clippy::cast_possible_truncation)] // counts are small
    let failures = logs.iter().take_while(|l| !l.ok).count() as u32;

    let last_success = db.last_success(source).await?;
    let last_success_ts = last_success.as_ref().map(|l| l.ts.clone());

    let stale_after = cfg.stale_after.as_secs_f64();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs_f64();
    let last_ok_age = last_success.map_or(f64::INFINITY, |l| now - l.ts_epoch);

    let status = if failures >= cfg.failure_threshold {
        Health::Failing
    } else if last_ok_age > stale_after {
        Health::Stale
    } else {
        Health::Healthy
    };

    Ok(SourceHealth {
        source: source.to_string(),
        status,
        consecutive_failures: failures,
        last_success_ts,
    })
}
