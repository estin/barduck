//! Web UI: server-rendered dashboard + per-source log view (spec: web-ui).
//!
//! Split by concern: [`theme`] (light/dark cookie), [`markdown`]
//! (value-format rendering: markdown/JSON/plain text), [`panels`] (the
//! panel data model and the live panel-grid shard), and [`routes`] (the two
//! actual pages).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod markdown;
mod panels;
mod routes;
mod theme;

pub use routes::{dashboard, source_logs};
pub(crate) use theme::THEME_COOKIE;
