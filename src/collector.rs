use crate::{config::{Config, SourceCfg}, db::Db, health, source};
use anyhow::Result;
use croner::Cron;
use std::time::Instant;
use tokio::time::timeout;

/// A source's per-tick wait: either a fixed interval or a cron expression's
/// next occurrence, computed fresh each tick (spec: data-collection —
/// per-source schedules).
enum Schedule {
    Interval(tokio::time::Interval),
    Cron(Box<Cron>),
}

impl Schedule {
    /// Built once per source at collector startup; the cron string was
    /// already validated at config load, so parsing here cannot fail.
    fn new(src: &SourceCfg) -> Self {
        if let Some(expr) = &src.cron {
            #[allow(clippy::expect_used)] // validated at config load (spec: source-configuration — cron schedule)
            let cron: Cron = expr.parse().expect("cron expression validated at config load");
            Schedule::Cron(Box::new(cron))
        } else {
            let mut tick = tokio::time::interval(src.effective_interval());
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            Schedule::Interval(tick)
        }
    }

    async fn tick(&mut self) {
        match self {
            Schedule::Interval(t) => {
                t.tick().await;
            }
            Schedule::Cron(cron) => {
                let now = chrono::Utc::now();
                let delta = cron
                    .find_next_occurrence(&now, false)
                    .ok()
                    .and_then(|next| (next - now).to_std().ok())
                    .unwrap_or(std::time::Duration::from_mins(1));
                tokio::time::sleep_until(tokio::time::Instant::now() + delta).await;
            }
        }
    }
}

/// Spawns one independent task per source so a hanging fetch cannot block
/// others (spec: data-collection — failure isolation).
pub fn spawn_all(db: &Db, cfg: &Config) {
    for src in &cfg.sources {
        let db = db.clone();
        let src = src.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let name = src.name.clone();
            if let Err(e) = loop_source(db, src, cfg).await {
                tracing::error!("collector for `{name}` stopped: {e:#}");
            }
        });
    }
}

async fn loop_source(db: Db, src: SourceCfg, cfg: Config) -> Result<()> {
    let kind = source::build(&src)?;
    // Re-derive the last known status so restarts don't duplicate transitions.
    let mut last_status = db.last_health_sync(&src.name)?.unwrap_or_else(|| "healthy".into());
    // A declared setup command runs once before the first fetch; on failure it
    // is retried on this source's schedule tick instead of fetching.
    let mut setup_done = src.setup.is_none();

    let mut schedule = Schedule::new(&src);
    loop {
        schedule.tick().await;
        if !setup_done && !try_setup(&db, &src).await {
            continue;
        }
        setup_done = true;
        fetch_once(&db, &cfg, &src, &kind, &mut last_status).await;
    }
}

/// One collection round over every source, honoring setup gates; used by
/// tests and one-shot runs.
pub async fn collect_once(db: &Db, cfg: &Config) {
    for src in &cfg.sources {
        let Ok(kind) = source::build(src) else { continue };
        if !try_setup(db, src).await {
            continue;
        }
        let mut last_status = db
            .last_health_sync(&src.name)
            .unwrap_or(None)
            .unwrap_or_else(|| "healthy".into());
        fetch_once(db, cfg, src, &kind, &mut last_status).await;
    }
}

/// Runs a source's setup command. Returns true when setup is satisfied
/// (success or not declared); failures are logged and reported as false.
async fn try_setup(db: &Db, src: &SourceCfg) -> bool {
    let Some(cmd) = src.setup.as_deref() else {
        return true;
    };
    let start = Instant::now();
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let ms = start.elapsed().as_millis() as i64;
    let outcome = timeout(src.timeout, source::run_shell(cmd)).await;
    match outcome {
        Ok(Ok(_)) => {
            tracing::info!("setup for `{}` succeeded", src.name);
            true
        }
        Ok(Err(e)) => {
            record_failure(db, &src.name, ms, &format!("setup: {e:#}")).await;
            false
        }
        Err(_) => {
            record_failure(
                db,
                &src.name,
                ms,
                &format!("setup: timed out after {}", humantime::format_duration(src.timeout)),
            )
            .await;
            false
        }
    }
}

pub async fn fetch_once(db: &Db, cfg: &Config, src: &SourceCfg, kind: &source::SourceKind, last_status: &mut String) {
    let start = Instant::now();
    let outcome = tokio::time::timeout(src.timeout, kind.fetch()).await;
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    {
        let ms = start.elapsed().as_millis() as i64;
        match &outcome {
            Ok(Ok(value)) => {
                if let Err(e) = db.insert_reading(&src.name, value, src.unit.as_deref()).await {
                    tracing::error!("insert reading `{}`: {e:#}", src.name);
                }
                if let Err(e) = db.insert_log(&src.name, true, ms, None, Some(value)).await {
                    tracing::error!("insert log `{}`: {e:#}", src.name);
                }
            }
            Ok(Err(e)) => {
                record_failure(db, &src.name, ms, &format!("{e:#}")).await;
            }
            Err(_) => {
                record_failure(
                    db,
                    &src.name,
                    ms,
                    &format!("timed out after {}", humantime::format_duration(src.timeout)),
                )
                .await;
            }
        }
    }

    // Record health transitions.
    match health::compute(db, cfg, &src.name) {
        Ok(h) => {
            if h.status.as_str() != *last_status {
                if let Err(e) = db.insert_health_event(&src.name, h.status.as_str()).await {
                    tracing::error!("insert health event `{}`: {e:#}", src.name);
                }
                *last_status = h.status.as_str().to_string();
            }
        }
        Err(e) => tracing::error!("health compute `{}`: {e:#}", src.name),
    }
}

async fn record_failure(db: &Db, name: &str, ms: i64, error: &str) {
    if let Err(e) = db.insert_log(name, false, ms, Some(error), None).await {
        tracing::error!("insert log `{name}`: {e:#}");
    }
}
