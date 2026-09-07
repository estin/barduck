pub mod age;
pub mod api;
pub mod cli_report;
pub mod collector;
pub mod components;
pub mod config;
pub mod db;
pub mod health;
pub mod query;
pub mod source;
pub mod tui;
pub mod web;

use crate::db::Db;
use anyhow::{Context as _, Result};
use config::Config;
use std::sync::Arc;
use topcoat::{
    asset::{AssetBundle, RouterBuilderAssetExt},
    cookie::RouterBuilderCookieExt,
    router::RouterBuilderDiscoverExt,
};

/// Long-lived values shared with topcoat handlers via app context.
pub struct AppState {
    pub db: Db,
    pub cfg: Arc<Config>,
}

/// Daemon mode: collector + HTTP server (API and web UI) in one process.
///
/// Shuts down gracefully on Ctrl+C/`SIGTERM`: `topcoat::serve_until` stops
/// accepting new HTTP requests and gives in-flight ones its own shutdown
/// timeout, and the same signal tells every collector task (spec:
/// data-collection — daemon shutdown lets in-flight collection finish) and
/// the retention task to stop after their current tick rather than being
/// hard-killed when the process exits.
pub async fn run_daemon(cfg: Config) -> Result<()> {
    let db = Db::open_rw_daemon(&cfg.database_path)
        .with_context(|| format!("opening database {}", cfg.database_path.display()))?;
    let listener = tokio::net::TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("binding {}", cfg.listen))?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut tasks = collector::spawn_graceful(&db, &cfg, &shutdown_rx);
    if let Some(retention) = cfg.retention {
        tasks.push(spawn_retention(db.clone(), retention, shutdown_rx));
    }
    let state = AppState {
        db,
        cfg: Arc::new(cfg.clone()),
    };
    println!(
        "barduck daemon listening on http://{} (web UI at /)",
        cfg.listen
    );
    let router = build_router(state);
    topcoat::serve_until(listener, router, async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    })
    .await?;
    for task in tasks {
        let _ = task.await;
    }
    Ok(())
}

/// Resolves on Ctrl+C or (on Unix) `SIGTERM` — the same signals
/// `topcoat::serve` itself watches for, mirrored here so collector/retention
/// tasks can react to the identical shutdown trigger.
async fn shutdown_signal() {
    let ctrl_c = async {
        #[allow(clippy::expect_used)] // no reasonable fallback if this fails
        tokio::signal::ctrl_c().await.expect("failed to install the Ctrl+C signal handler");
    };
    #[cfg(unix)]
    let terminate = async {
        #[allow(clippy::expect_used)] // no reasonable fallback if this fails
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install the SIGTERM signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

/// Periodically deletes readings/fetch logs/health events older than
/// `retention` (spec: data-storage — retention), stopping after its current
/// pass once `shutdown` reports `true`.
fn spawn_retention(
    db: Db,
    retention: std::time::Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // `interval`'s first tick fires immediately, so the loop's very
        // first pass runs right away; every later pass waits a full hour.
        let mut interval = tokio::time::interval(std::time::Duration::from_hours(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                res = shutdown.changed() => {
                    if res.is_err() || *shutdown.borrow() {
                        return;
                    }
                    continue;
                }
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64());
            if let Err(e) = db.purge_older_than(now - retention.as_secs_f64()).await {
                tracing::error!("retention purge: {e:#}");
            }
        }
    })
}

#[must_use]
pub fn build_router(state: AppState) -> topcoat::router::Router {
    // `AssetBundle::load()` looks next to the current executable — correct
    // for the real `barduck` binary; a test binary has no such
    // bundle, so tests use `build_router_with_bundle` instead.
    build_router_with_bundle(state, AssetBundle::load().ok())
}

/// Same as [`build_router`], but takes the asset bundle explicitly. Used by
/// tests: `AssetBundle::load()`'s next-to-executable convention doesn't
/// apply to the test harness binary, which isn't `barduck`.
#[must_use]
pub fn build_router_with_bundle(state: AppState, bundle: Option<AssetBundle>) -> topcoat::router::Router {
    let builder = topcoat::router::Router::builder().discover().cookies();
    let builder = if let Some(b) = bundle {
        builder.assets(b)
    } else {
        tracing::warn!("no asset bundle loaded; pages using bundled assets will fail to render");
        builder
    };
    builder.app_context(state).build()
}

/// Re-exported so `barduck::collect_once` (used throughout the test suite)
/// gets the real one-shot collection round — including its setup-command
/// gating — rather than a second, drifted copy.
pub use collector::collect_once;
