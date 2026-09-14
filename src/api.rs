use crate::{AppState, collector, db::Origin, health, source};
use serde::Deserialize;
use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{Cookie, Cookies, cookie, cookies, time::Duration},
    router::{
        Body, StatusCode,
        content::Json,
        error::bad_request,
        path_param,
        request::{Bytes, uri},
        response::Response,
        route,
    },
};

fn json_ok<T: serde::Serialize>(v: &T) -> Response {
    let body = Body::from(serde_json::to_string(v).unwrap_or_else(|_| "{}".into()));
    let mut resp = Response::new(body);
    resp.headers_mut().insert(
        topcoat::router::header::CONTENT_TYPE,
        topcoat::router::HeaderValue::from_static("application/json"),
    );
    resp
}

/// Spec: http-api — unknown source / bad input are 4xx with a JSON message.
fn json_err(status: StatusCode, msg: &str) -> Response {
    let body = Body::from(serde_json::json!({ "error": msg }).to_string());
    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        topcoat::router::header::CONTENT_TYPE,
        topcoat::router::HeaderValue::from_static("application/json"),
    );
    resp
}

/// A genuine internal failure (DB connection/query error, not caller
/// input): logged server-side with full detail, answered with a generic
/// message. Unlike a 400 for bad request input — where echoing back what's
/// wrong with the caller's *own* data is expected UX — a 500 here reflects
/// internal state (file paths, connection detail, `DuckDB` internals) that
/// `/api/ingest` and friends being unauthenticated by default makes fair
/// game to any caller that can reach the listen address (spec: http-api).
fn internal_error(context: &str, e: &anyhow::Error) -> Response {
    tracing::error!("{context}: {e:#}");
    json_err(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

/// The vendored topcoat browser runtime (see assets/README note in repo);
/// served here so no external bundler step is needed.
#[route(GET "/assets/bd-runtime.js")]
pub async fn runtime_script(_cx: &Cx) -> Result<Response> {
    let js = include_bytes!("../assets/topcoat-runtime.js");
    let body = Body::from(js.as_slice());
    let mut resp = Response::new(body);
    resp.headers_mut().insert(
        topcoat::router::header::CONTENT_TYPE,
        topcoat::router::HeaderValue::from_static("application/javascript"),
    );
    Ok(resp)
}

/// Frontend-initiated ping/pong health check (spec: http-api — ping/pong
/// health endpoint). Cheap by design: no DB access.
#[route(GET "/api/ping")]
pub async fn ping(_cx: &Cx) -> Result<Response> {
    Ok(json_ok(&serde_json::json!({
        "server_time": chrono::Utc::now().to_rfc3339(),
    })))
}

#[route(GET "/api/sources/latest")]
pub async fn latest(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    match st.db.latest_values().await {
        Ok(rows) => Ok(json_ok(&rows)),
        Err(e) => Ok(internal_error("GET /api/sources/latest", &e)),
    }
}

path_param!(source_name: String, error = not_found);

#[derive(Debug, Deserialize)]
struct RangeQuery {
    from: Option<f64>,
    to: Option<f64>,
    /// Capped at `db::MAX_HISTORY_LIMIT`, defaults to `db::DEFAULT_HISTORY_LIMIT`
    /// (spec: http-api — bounded history queries) — an HTTP client can never
    /// force an unbounded table scan by omitting it.
    limit: Option<i64>,
}

#[route(GET "/api/sources/{source_name}/history")]
pub async fn history(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let source = path_param::<SourceName>(cx)?.clone();
    let known = st.cfg.sources.iter().any(|s| s.name() == source);
    if !known {
        return Ok(json_err(
            StatusCode::NOT_FOUND,
            &format!("unknown source `{source}`"),
        ));
    }
    let q: RangeQuery = serde_urlencoded::from_str(uri(cx).query().unwrap_or(""))
        .map_err(|e| topcoat::Error::from(bad_request(format!("invalid time range query: {e}"))))?;
    match st.db.history(&source, q.from, q.to, q.limit).await {
        Ok(rows) => Ok(json_ok(&rows)),
        Err(e) => Ok(internal_error("GET /api/sources/{source}/history", &e)),
    }
}

#[route(GET "/api/health")]
pub async fn health_all(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    match health::compute_all(&st.db, &st.cfg).await {
        Ok(out) => Ok(json_ok(&out)),
        Err(e) => Ok(internal_error("GET /api/health", &e)),
    }
}

/// `GET /api/logs`'s query string (spec: cli — Filter query output by
/// source; http-api — bounded queries).
///
/// Parsed from raw key/value pairs rather than deserialized into a struct
/// with a `Vec<String>` field: `serde_urlencoded` has no notion of a
/// repeated key, so `?source=a` (let alone `?source=a&source=b`) failed the
/// whole request with `invalid type: string ..., expected a sequence` —
/// making the source filter unusable over HTTP, and with it `barduck logs
/// --daemon --source`, which builds exactly that URL.
#[derive(Debug, Default)]
struct LogsQuery {
    limit: Option<i64>,
    source: Vec<String>,
}

impl LogsQuery {
    fn parse(query: &str) -> std::result::Result<Self, String> {
        let pairs: Vec<(String, String)> =
            serde_urlencoded::from_str(query).map_err(|e| format!("invalid query: {e}"))?;
        let mut out = Self::default();
        for (key, value) in pairs {
            match key.as_str() {
                // Repeated `limit` keeps the last, matching how a struct
                // field would have resolved it.
                "limit" => {
                    out.limit = Some(
                        value
                            .parse()
                            .map_err(|e| format!("invalid `limit` `{value}`: {e}"))?,
                    );
                }
                "source" => out.source.push(value),
                // Unknown parameters stay ignored, as before.
                _ => {}
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
struct ThemeBody {
    theme: String,
}

/// Persists the browser's chosen theme in a cookie so the server can render
/// the correct `dark`/`light` class on `<html>` for every subsequent page
/// load — "the backend knows the user's theme" (spec: web-ui — light/dark
/// theme toggle). The client sends the theme it just switched to (computed
/// from its own current DOM state), not a request to "toggle" blindly:
/// a stateless toggle-on-the-server can't tell a missing cookie (never
/// chosen) apart from "was light", so a viewer whose page is currently dark
/// via the OS-preference media query (spec: web-ui) could see their first
/// click appear to do nothing.
#[route(POST "/api/theme")]
pub async fn set_theme(cx: &Cx, Json(body): Json<ThemeBody>) -> Result<Response> {
    if body.theme != "dark" && body.theme != "light" {
        return Ok(json_err(
            StatusCode::BAD_REQUEST,
            "theme must be \"dark\" or \"light\"",
        ));
    }
    let name = crate::web::THEME_COOKIE;
    let c: Cookie = cookie! {
        name = body.theme.clone();
        Path = "/";
        MaxAge = Duration::days(365)
    };
    cookies(cx).add(c);
    Ok(json_ok(&serde_json::json!({ "theme": body.theme })))
}

#[route(GET "/api/logs")]
pub async fn logs(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let q = LogsQuery::parse(uri(cx).query().unwrap_or(""))
        .map_err(|e| topcoat::Error::from(bad_request(e)))?;
    match st
        .db
        .logs_for_sources(&q.source, q.limit.unwrap_or(50))
        .await
    {
        Ok(rows) => Ok(json_ok(&rows)),
        Err(e) => Ok(internal_error("GET /api/logs", &e)),
    }
}

/// Fetches one source now, regardless of its schedule, and answers with the
/// attempt's outcome (spec: http-api — Force poll endpoint). The fetch is
/// carried out by that source's own collector task, so it can never overlap
/// that source's scheduled fetch and the schedule restarts from the forced
/// attempt (spec: data-collection — Forced polls are serialized with a
/// source's schedule).
///
/// A fetch that ran and *failed* is a completed request, not a server
/// error: it answers success with `success: false`, so a caller can tell a
/// failing source command from a daemon it could not reach at all.
#[route(POST "/api/sources/{source_name}/poll")]
pub async fn poll(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let source = path_param::<SourceName>(cx)?.clone();
    let Some(src) = st.cfg.sources.iter().find(|s| s.name() == source) else {
        return Ok(json_err(
            StatusCode::NOT_FOUND,
            &format!("unknown source `{source}`"),
        ));
    };
    if let Some(why) = collector::unpollable_reason(src) {
        return Ok(json_err(
            StatusCode::BAD_REQUEST,
            &format!("source `{source}` {why}"),
        ));
    }
    // No sender, a closed channel, or a dropped reply all mean the same
    // thing: this source has no live collector task to do the work. Say so
    // rather than hanging or reporting a fetch that never ran.
    let gone = || {
        json_err(
            StatusCode::SERVICE_UNAVAILABLE,
            &format!("no collector is running for source `{source}`"),
        )
    };
    let Some(tx) = st.controls.get(src.name()) else {
        return Ok(gone());
    };
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if tx
        .send(collector::Control::PollNow(source.clone(), reply_tx))
        .is_err()
    {
        return Ok(gone());
    }
    match reply_rx.await {
        Ok(outcome) => Ok(json_ok(&outcome)),
        Err(_) => Ok(gone()),
    }
}

/// Stores a pushed reading (spec: http-api — HTTP ingest endpoint). Reads
/// the raw body instead of the `Json` extractor so every malformed request
/// — wrong content type, bad JSON, missing fields — answers with the same
/// `{"error": ...}` shape as the other endpoints rather than a framework
/// default.
#[route(POST "/api/ingest")]
pub async fn ingest(cx: &Cx, body: Bytes) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let start = std::time::Instant::now();
    #[allow(clippy::cast_possible_truncation)] // handler-measured, fits easily
    let elapsed_ms = || start.elapsed().as_millis() as i64;
    let req: source::IngestItem = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return Ok(json_err(
                StatusCode::BAD_REQUEST,
                &format!("invalid ingest body: {e}"),
            ));
        }
    };
    let Some(src) = st.cfg.sources.iter().find(|s| s.name() == req.source) else {
        return Ok(json_err(
            StatusCode::NOT_FOUND,
            &format!("unknown source `{}`", req.source),
        ));
    };
    let arrival = crate::db::now();
    let parsed = match source::resolve_ingest_item(&req, arrival, src.effective_value_type()) {
        Ok(p) => p,
        Err(e) => return Ok(json_err(StatusCode::BAD_REQUEST, &e)),
    };
    let _ = collector::store_parsed_value(
        &st.db,
        &st.cfg,
        src,
        None,
        &parsed,
        elapsed_ms(),
        Origin::Push,
    )
    .await;
    // Move the source's interval wait to the success path; cron and stream
    // sources have no sender and are unaffected (spec: data-collection —
    // Ingested values reset interval schedules). A closed channel only
    // means its collector task already exited — the value is still stored.
    if let Some(tx) = st.controls.get(src.name()) {
        let _ = tx.send(collector::Control::ResetSchedule);
    }
    Ok(json_ok(&serde_json::json!({
        "source": src.name(),
        "value": parsed.value,
        "ts_epoch": parsed.ts_epoch,
        "ts": parsed.ts,
        "unit": src.unit(),
        "origin": Origin::Push,
    })))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A repeated `source` key is how both the HTTP API and `barduck logs
    /// --daemon --source` express a multi-source filter; deserializing the
    /// query into a struct rejected even a single occurrence (spec: cli —
    /// Filter query output by source).
    #[test]
    fn logs_query_accepts_repeated_source_keys() {
        let q = LogsQuery::parse("limit=3&source=disk-root").unwrap();
        assert_eq!(q.limit, Some(3));
        assert_eq!(q.source, vec!["disk-root".to_string()]);

        let q = LogsQuery::parse("source=a&limit=5&source=b").unwrap();
        assert_eq!(q.limit, Some(5));
        assert_eq!(q.source, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn logs_query_defaults_and_ignores_unknown_parameters() {
        let q = LogsQuery::parse("").unwrap();
        assert_eq!(q.limit, None);
        assert!(q.source.is_empty());

        let q = LogsQuery::parse("unknown=1&source=a").unwrap();
        assert_eq!(q.source, vec!["a".to_string()]);
    }

    #[test]
    fn logs_query_rejects_an_unparseable_limit() {
        let err = LogsQuery::parse("limit=many").unwrap_err();
        assert!(err.contains("limit"), "error should name the field: {err}");
    }
}
