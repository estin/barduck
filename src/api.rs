use crate::{AppState, health};use serde::Deserialize;
use topcoat::{
    Result,
    context::{Cx, app_context},
    router::{
        Body, StatusCode,
        error::bad_request,
        path_param, request::uri, response::Response, route,
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
        Err(e) => Ok(json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("{e:#}"),
        )),
    }
}

path_param!(source_name: String, error = not_found);

#[derive(Debug, Deserialize)]
struct RangeQuery {
    from: Option<f64>,
    to: Option<f64>,
}

#[route(GET "/api/sources/{source_name}/history")]
pub async fn history(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let source = path_param::<SourceName>(cx)?.clone();
    let known = st.cfg.sources.iter().any(|s| s.name == source);
    if !known {
        return Ok(json_err(
            StatusCode::NOT_FOUND,
            &format!("unknown source `{source}`"),
        ));
    }
    let q: RangeQuery =
        serde_urlencoded::from_str(uri(cx).query().unwrap_or("")).map_err(|e| {
            topcoat::Error::from(bad_request(format!("invalid time range query: {e}")))
        })?;
    match st.db.history(&source, q.from, q.to).await {
        Ok(rows) => Ok(json_ok(&rows)),
        Err(e) => Ok(json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("{e:#}"),
        )),
    }
}

#[route(GET "/api/health")]
pub async fn health_all(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let mut out = Vec::new();
    for s in &st.cfg.sources {
        match health::compute(&st.db, &st.cfg, &s.name) {
            Ok(h) => out.push(h),
            Err(e) => {
                return Ok(json_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("{e:#}"),
                ))
            }
        }
    }
    Ok(json_ok(&out))
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    limit: Option<i64>,
}

#[route(GET "/api/logs")]
pub async fn logs(cx: &Cx) -> Result<Response> {
    let st = app_context::<AppState>(cx);
    let q: LogsQuery = serde_urlencoded::from_str(uri(cx).query().unwrap_or(""))
        .map_err(|e| topcoat::Error::from(bad_request(format!("invalid query: {e}"))))?;
    match st.db.logs(None, q.limit.unwrap_or(50)).await {
        Ok(rows) => Ok(json_ok(&rows)),
        Err(e) => Ok(json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("{e:#}"),
        )),
    }
}
