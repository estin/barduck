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

/// The vendored topcoat browser runtime, compiled into the binary
/// (`assets/topcoat-runtime.js`) and served from here, so no external
/// bundler step is needed.
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
path_param!(user_js_name: String, error = not_found);

/// A configured user JavaScript file (spec: web-ui — user-defined scripts
/// injected into the web UI). `name` is only ever looked up in the
/// startup-resolved registry — never joined to the filesystem — so only
/// configured files are reachable and `../` traversal is impossible by
/// construction. Unknown names are 404, like unknown sources.
#[route(GET "/assets/user-js/{user_js_name}")]
pub async fn user_script(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let name = path_param::<UserJsName>(cx)?.clone();
    let Some(script) = st.user_scripts.iter().find(|s| s.name == name) else {
        return Ok(json_err(
            StatusCode::NOT_FOUND,
            &format!("unknown user script `{name}`"),
        ));
    };
    match std::fs::read(&script.path) {
        Ok(js) => {
            let mut resp = Response::new(Body::from(js));
            resp.headers_mut().insert(
                topcoat::router::header::CONTENT_TYPE,
                topcoat::router::HeaderValue::from_static("application/javascript"),
            );
            Ok(resp)
        }
        Err(e) => Ok(internal_error("GET /assets/user-js/{name}", &e.into())),
    }
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
/// Streams refresh generations as Server-Sent Events (spec: web-ui —
/// immediate panel refresh): a `refresh` event whenever the generation moves,
/// `id:` carrying it so a reconnecting `EventSource` resumes via
/// `Last-Event-ID` (or `?from=`). A resume whose id differs from the current
/// generation — values were stored while disconnected, or the daemon
/// restarted — gets one catch-up event; a fresh connection gets none, since
/// the page it belongs to was just rendered. Several stores between polls of
/// the stream coalesce into one event. The stream ends when the client
/// disconnects or the daemon shuts down.
#[route(GET "/api/refresh-events")]
pub async fn refresh_events(
    cx: &Cx,
) -> Result<
    topcoat::router::content::sse::Sse<
        impl futures_core::Stream<Item = Result<topcoat::router::content::sse::Event>> + use<>,
    >,
> {
    use topcoat::router::content::sse::{KeepAlive, Sse, last_event_id};
    let st = app_context::<AppState>(cx);
    let mut rx = st.db.subscribe_refresh();
    let current = *rx.borrow_and_update();
    let resume_from: Option<u64> = last_event_id(cx)
        .and_then(|id| id.parse().ok())
        .or_else(|| {
            uri(cx)
                .query()
                .and_then(|q| {
                    serde_urlencoded::from_str::<std::collections::HashMap<String, String>>(q)
                        .ok()
                        .and_then(|m| m.get("from").cloned())
                })
                .and_then(|v| v.parse().ok())
        });
    let stream = RefreshEventStream {
        catch_up: resume_from
            .is_some_and(|seen| seen != current)
            .then_some(current),
        hub: st.refresh.clone(),
        next: Some(next_refresh(rx, st.refresh.clone())),
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::new()))
}

type NextRefresh = std::pin::Pin<
    Box<dyn Future<Output = Option<(u64, tokio::sync::watch::Receiver<u64>)>> + Send>,
>;

/// Waits for the next refresh generation, handing the receiver back so the
/// stream can wait again; `None` once the daemon shuts down.
fn next_refresh(mut rx: tokio::sync::watch::Receiver<u64>, hub: crate::RefreshHub) -> NextRefresh {
    Box::pin(async move {
        tokio::select! {
            changed = rx.changed() => {
                changed.ok()?;
                let generation = *rx.borrow_and_update();
                Some((generation, rx))
            }
            () = hub.closed() => None,
        }
    })
}

fn refresh_event(generation: u64) -> topcoat::router::content::sse::Event {
    topcoat::router::content::sse::Event::new()
        .event("refresh")
        .id(generation.to_string())
        .data(generation.to_string())
}

/// `Sse` event stream for [`refresh_events`]: an optional catch-up event,
/// then one event per observed generation change.
struct RefreshEventStream {
    catch_up: Option<u64>,
    hub: crate::RefreshHub,
    next: Option<NextRefresh>,
}

impl futures_core::Stream for RefreshEventStream {
    type Item = Result<topcoat::router::content::sse::Event>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if let Some(generation) = self.catch_up.take() {
            return Poll::Ready(Some(Ok(refresh_event(generation))));
        }
        let Some(next) = self.next.as_mut() else {
            return Poll::Ready(None);
        };
        match next.as_mut().poll(cx) {
            Poll::Ready(Some((generation, rx))) => {
                self.next = Some(next_refresh(rx, self.hub.clone()));
                Poll::Ready(Some(Ok(refresh_event(generation))))
            }
            Poll::Ready(None) => {
                self.next = None;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
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
        // The collector task publishes the refresh for a successful forced
        // poll itself (spec: web-ui — immediate panel refresh).
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
    // A value that was parsed but not persisted is a server-side failure, and
    // the client has to be able to tell it apart from a delivered one: the
    // reading is absent while a *failure* row went into the fetch log. The
    // schedule reset below is also skipped, since nothing was stored for the
    // source's interval to resume from.
    if let Err(msg) = collector::store_parsed_value(
        &st.db,
        &st.cfg,
        src,
        None,
        &parsed,
        elapsed_ms(),
        Origin::Push,
    )
    .await
    {
        return Ok(internal_error(
            &format!("POST /api/ingest source `{}`", src.name()),
            &anyhow::anyhow!("{msg}"),
        ));
    }
    // A stored value re-renders connected browsers immediately over the SSE
    // refresh stream (spec: web-ui — immediate panel refresh).
    st.db.publish_refresh();
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
