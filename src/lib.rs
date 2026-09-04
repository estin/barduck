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
pub async fn run_daemon(cfg: Config) -> Result<()> {
    let db = Db::open_rw(&cfg.database_path)
        .with_context(|| format!("opening database {}", cfg.database_path.display()))?;
    let listener = tokio::net::TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("binding {}", cfg.listen))?;
    collector::spawn_all(&db, &cfg);
    let state = AppState {
        db,
        cfg: Arc::new(cfg.clone()),
    };
    println!(
        "barduck daemon listening on http://{} (web UI at /)",
        cfg.listen
    );
    topcoat::serve(listener, build_router(state)).await?;
    Ok(())
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

/// One collection round over every source; used by tests and warmups.
pub async fn collect_once(db: &Db, cfg: &Config) {
    for src in &cfg.sources {
        let Ok(kind) = source::build(src) else { continue };
        let mut last_status = db
            .last_health_sync(&src.name)
            .ok()
            .flatten()
            .unwrap_or_else(|| "healthy".into());
        collector::fetch_once(db, cfg, src, &kind, &mut last_status).await;
    }
}
