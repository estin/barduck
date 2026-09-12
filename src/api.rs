use crate::{
    AppState, collector,
    config::{Threshold, validate_thresholds},
    db::Origin,
    health, source,
};
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

#[derive(Debug, Deserialize)]
struct IngestBody {
    source: String,
    value: serde_json::Value,
    #[serde(default)]
    ts: Option<serde_json::Value>,
    #[serde(default)]
    thresholds: Option<Vec<Threshold>>,
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
    let req: IngestBody = match serde_json::from_slice(&body) {
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
    // Plain-text values stay as-is; JSON values are compacted to their
    // canonical form so numbers and objects store deterministically.
    let value = match &req.value {
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    if let Some(bands) = &req.thresholds
        && let Err(e) = validate_thresholds(src.name(), bands)
    {
        return Ok(json_err(StatusCode::BAD_REQUEST, &format!("{e:#}")));
    }
    let arrival = crate::db::now();
    let (ts_epoch, ts) = match resolve_ingest_ts(req.ts.as_ref(), arrival, src.name()) {
        Ok(t) => t,
        Err(e) => return Ok(json_err(StatusCode::BAD_REQUEST, &e)),
    };
    // Pre-validate so a value the source's type rejects is a 400 recording
    // nothing — `store_parsed_value` would log it as a failed attempt.
    if let Err(e) = source::convert_value_type(&value, src.effective_value_type()) {
        return Ok(json_err(StatusCode::BAD_REQUEST, &format!("{e:#}")));
    }
    let parsed = source::ParsedOutput {
        value: value.clone(),
        ts_epoch,
        ts: ts.clone(),
        threshold: req.thresholds.clone(),
    };
    collector::store_parsed_value(
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
    if let Some(tx) = st.resets.get(src.name()) {
        let _ = tx.send(());
    }
    Ok(json_ok(&serde_json::json!({
        "source": src.name(),
        "value": value,
        "ts_epoch": ts_epoch,
        "ts": ts,
        "unit": src.unit(),
        "origin": Origin::Push,
    })))
}

/// Strict ingest `ts`: epoch number or RFC 3339 string; anything else is a
/// caller error naming the problem (unlike row `ts`, which degrades to
/// arrival time).
fn resolve_ingest_ts(
    raw: Option<&serde_json::Value>,
    arrival: (f64, String),
    source: &str,
) -> std::result::Result<(f64, String), String> {
    let Some(v) = raw else {
        return Ok(arrival);
    };
    match v {
        serde_json::Value::Null => Ok(arrival),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(secs) => epoch_to_ts(secs, source),
            None => Err(format!("ingest `ts` for `{source}` is not a finite number")),
        },
        serde_json::Value::String(s) => {
            chrono::DateTime::parse_from_rfc3339(s).map_or_else(
                |_| Err(format!("ingest `ts` for `{source}` is not RFC 3339: `{s}`")),
                |d| {
                    #[allow(clippy::cast_precision_loss)] // epoch millis fit exactly enough
                    let epoch = d.timestamp_millis() as f64 / 1000.0;
                    Ok((epoch, d.to_rfc3339()))
                },
            )
        }
        _ => Err(format!(
            "ingest `ts` for `{source}` must be epoch seconds or RFC 3339"
        )),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn epoch_to_ts(secs: f64, source: &str) -> std::result::Result<(f64, String), String> {
    if !secs.is_finite() {
        return Err(format!("ingest `ts` for `{source}` is not a finite number"));
    }
    // `floor`, not `trunc`: for a pre-epoch fractional value (e.g. `-1.5`),
    // `trunc` rounds toward zero (`-1.0`), and an `.abs()` on the negative
    // remainder that follows would flip it positive, landing exactly one
    // second late. Flooring keeps `secs - whole` in `[0, 1)` for either
    // sign, so the remainder is already the correct positive nanosecond
    // offset with no `.abs()` needed.
    let whole = secs.floor();
    let nanos = ((secs - whole) * 1_000_000_000.0).round() as u32;
    match chrono::DateTime::from_timestamp(whole as i64, nanos) {
        Some(d) => {
            #[allow(clippy::cast_precision_loss)]
            let epoch = d.timestamp_millis() as f64 / 1000.0;
            Ok((epoch, d.to_rfc3339()))
        }
        None => Err(format!("ingest `ts` for `{source}` is out of range")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// A pre-epoch fractional `ts` (e.g. `-1.5`, half a second before
    /// 1969-12-31T23:59:59Z) must resolve to that exact instant, not one
    /// second off (spec: http-api — HTTP ingest endpoint).
    #[test]
    fn epoch_to_ts_handles_pre_epoch_fractional_seconds() {
        let (epoch, ts) = epoch_to_ts(-1.5, "s").unwrap();
        assert!((epoch - (-1.5)).abs() < 0.001, "got epoch {epoch}");
        assert!(
            ts.starts_with("1969-12-31T23:59:58.5"),
            "expected 23:59:58.5, got {ts}"
        );
    }

    #[test]
    fn epoch_to_ts_handles_positive_fractional_seconds() {
        let (epoch, ts) = epoch_to_ts(1.5, "s").unwrap();
        assert!((epoch - 1.5).abs() < 0.001, "got epoch {epoch}");
        assert!(ts.starts_with("1970-01-01T00:00:01.5"), "got {ts}");
    }

    #[test]
    fn epoch_to_ts_handles_whole_seconds_either_side_of_the_epoch() {
        assert!((epoch_to_ts(-1.0, "s").unwrap().0 - (-1.0)).abs() < 0.001);
        assert!((epoch_to_ts(0.0, "s").unwrap().0).abs() < 0.001);
        assert!((epoch_to_ts(1.0, "s").unwrap().0 - 1.0).abs() < 0.001);
    }

    #[test]
    fn epoch_to_ts_rejects_non_finite() {
        assert!(epoch_to_ts(f64::NAN, "s").is_err());
        assert!(epoch_to_ts(f64::INFINITY, "s").is_err());
    }

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
