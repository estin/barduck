use crate::{
    config::{Config, SourceCfg},
    db::Db,
    health, source,
};
use anyhow::Result;
use croner::Cron;
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
    /// Built once per source at collector startup; the cron string was
    /// already validated at config load, so parsing here cannot fail.
    fn new(src: &SourceCfg) -> Self {
        if let Some(expr) = &src.cron {
            #[allow(clippy::expect_used)]
            // validated at config load (spec: source-configuration — cron schedule)
            let cron: Cron = expr
                .parse()
                .expect("cron expression validated at config load");
            Schedule::Cron(Box::new(cron))
        } else {
            Schedule::Interval {
                effective_interval: src.effective_interval(),
                retry_interval: src.effective_retry_interval(),
                // Fires immediately, matching the collector's long-standing
                // "fetch right away at startup" behavior.
                next: tokio::time::Instant::now(),
            }
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

/// Spawns one independent task per source so a hanging fetch cannot block
/// others (spec: data-collection — failure isolation). Fire-and-forget, with
/// no cooperative shutdown — used by tests and one-shot callers. The daemon
/// uses [`spawn_graceful`] instead.
pub fn spawn_all(db: &Db, cfg: &Config) {
    for src in &cfg.sources {
        let db = db.clone();
        let src = src.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let name = src.name.clone();
            if let Err(e) = loop_source(db, src, cfg, None).await {
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
/// shutdown completes.
#[must_use]
pub fn spawn_graceful(
    db: &Db,
    cfg: &Config,
    shutdown: &tokio::sync::watch::Receiver<bool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    cfg.sources
        .iter()
        .map(|src| {
            let db = db.clone();
            let src = src.clone();
            let cfg = cfg.clone();
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                let name = src.name.clone();
                if let Err(e) = loop_source(db, src, cfg, Some(shutdown)).await {
                    tracing::error!("collector for `{name}` stopped: {e:#}");
                }
            })
        })
        .collect()
}

async fn loop_source(
    db: Db,
    src: SourceCfg,
    cfg: Config,
    mut shutdown: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<()> {
    let kind = source::build(&src)?;
    // Re-derive the last known status so restarts don't duplicate transitions.
    let mut last_status = db
        .last_health(&src.name)
        .await?
        .unwrap_or_else(|| "healthy".into());
    // A declared setup command runs once before the first fetch; on failure it
    // is retried on this source's schedule tick instead of fetching.
    let mut setup_done = src.setup.is_none();

    let mut schedule = Schedule::new(&src);
    match &schedule {
        Schedule::Interval {
            effective_interval,
            retry_interval,
            next,
        } => {
            tracing::debug!("Source next: {:?}", next);
        }
        _ => {}
    };
    loop {
        match &mut shutdown {
            Some(sd) => {
                tokio::select! {
                    () = schedule.tick() => {}
                    res = sd.changed() => {
                        // `Err` means every sender was dropped, which
                        // `spawn_graceful` never does before the daemon
                        // exits — but never busy-loop on it regardless.
                        if res.is_err() || *sd.borrow() {
                            return Ok(());
                        }
                        continue;
                    }
                }
            }
            None => schedule.tick().await,
        }
        if !setup_done && !try_setup(&db, &src, &cfg).await {
            schedule.advance(false);
            continue;
        }
        setup_done = true;
        let success = fetch_once(&db, &cfg, &src, &kind, &mut last_status).await;
        schedule.advance(!success);
    }
}

/// One collection round over every source, honoring setup gates; used by
/// tests and one-shot runs.
pub async fn collect_once(db: &Db, cfg: &Config) {
    for src in &cfg.sources {
        let Ok(kind) = source::build(src) else {
            continue;
        };
        if !try_setup(db, src, cfg).await {
            continue;
        }
        let mut last_status = db
            .last_health(&src.name)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| "healthy".into());
        fetch_once(db, cfg, src, &kind, &mut last_status).await;
    }
}

/// Runs a source's setup command. Returns true when setup is satisfied
/// (success or not declared); failures are logged and reported as false.
async fn try_setup(db: &Db, src: &SourceCfg, cfg: &Config) -> bool {
    let Some(cmd) = src.setup.as_deref() else {
        return true;
    };
    let start = Instant::now();
    let outcome = timeout(src.timeout, source::run_shell(cmd, &cfg.config_dir)).await;
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let ms = start.elapsed().as_millis() as i64;
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
                &format!(
                    "setup: timed out after {}",
                    humantime::format_duration(src.timeout)
                ),
            )
            .await;
            false
        }
    }
}

/// Runs one fetch attempt, logging its outcome and updating health. Returns
/// whether the round trip should be treated as successful for scheduling
/// purposes (spec: data-collection — per-source schedules): the upstream
/// fetch succeeded *and* the reading was durably stored. A fetch that
/// succeeds but fails to persist is recorded as a failed attempt, not a
/// silent success — otherwise health would report "healthy" for data that
/// was never actually written.
pub async fn fetch_once(
    db: &Db,
    cfg: &Config,
    src: &SourceCfg,
    kind: &source::SourceKind,
    last_status: &mut String,
) -> bool {
    let start = Instant::now();
    let outcome = tokio::time::timeout(src.timeout, kind.fetch(&cfg.config_dir)).await;
    #[allow(clippy::cast_possible_truncation)] // durations fit easily
    let ms = start.elapsed().as_millis() as i64;
    let success = match &outcome {
        Ok(Ok(value)) => {
            let stored = db
                .insert_reading(&src.name, value, src.unit.as_deref())
                .await;
            if let Err(e) = &stored {
                tracing::error!("insert reading `{}`: {e:#}", src.name);
            }
            match &stored {
                Ok(()) => {
                    if let Err(e) = db.insert_log(&src.name, true, ms, None, Some(value)).await {
                        tracing::error!("insert log `{}`: {e:#}", src.name);
                    }
                    true
                }
                Err(e) => {
                    record_failure(
                        db,
                        &src.name,
                        ms,
                        &format!("fetched but failed to store: {e:#}"),
                    )
                    .await;
                    false
                }
            }
        }
        Ok(Err(e)) => {
            record_failure(db, &src.name, ms, &format!("{e:#}")).await;
            false
        }
        Err(_) => {
            record_failure(
                db,
                &src.name,
                ms,
                &format!(
                    "timed out after {}",
                    humantime::format_duration(src.timeout)
                ),
            )
            .await;
            false
        }
    };

    // Record health transitions.
    match health::compute(db, cfg, &src.name).await {
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

    success
}

async fn record_failure(db: &Db, name: &str, ms: i64, error: &str) {
    if let Err(e) = db.insert_log(name, false, ms, Some(error), None).await {
        tracing::error!("insert log `{name}`: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

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
}
