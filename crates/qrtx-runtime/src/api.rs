use crate::*;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use std::collections::HashMap;
use tokio_stream::{StreamExt, wrappers::IntervalStream};
fn token(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("")
        .into()
}
async fn execute(
    runtime: Runtime,
    headers: HeaderMap,
    method: &str,
    params: Value,
) -> axum::response::Response {
    if headers.contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"ok":false,"error":{"code":"ORIGIN_REJECTED"}})),
        )
            .into_response();
    }
    let request = Request {
        token: token(&headers),
        method: method.into(),
        params,
    };
    let response = tokio::task::spawn_blocking(move || runtime.authenticated(&request)).await;
    match response {
        Ok(r) => {
            let status = if r.ok {
                StatusCode::OK
            } else {
                match r.error.as_ref().and_then(|e| e["message"].as_str()) {
                    Some("authentication failed") => StatusCode::UNAUTHORIZED,
                    Some(message) if message.starts_with("permission denied") => {
                        StatusCode::FORBIDDEN
                    }
                    _ => StatusCode::BAD_REQUEST,
                }
            };
            (status, Json(r)).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"ok":false}))).into_response(),
    }
}
async fn status(State(r): State<Runtime>, h: HeaderMap) -> impl IntoResponse {
    execute(r, h, "status", Value::Null).await
}
async fn devices(State(r): State<Runtime>, h: HeaderMap) -> impl IntoResponse {
    execute(r, h, "devices", Value::Null).await
}
async fn transfers(State(r): State<Runtime>, h: HeaderMap) -> impl IntoResponse {
    execute(r, h, "transfers", Value::Null).await
}
async fn transfer(
    State(r): State<Runtime>,
    h: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> impl IntoResponse {
    execute(r, h, "transfer", json!({"id":id})).await
}
async fn enqueue(
    State(r): State<Runtime>,
    h: HeaderMap,
    Json(p): Json<Value>,
) -> impl IntoResponse {
    execute(r, h, "enqueue", p).await
}
async fn action(
    State(r): State<Runtime>,
    h: HeaderMap,
    UrlPath((id, action)): UrlPath<(String, String)>,
) -> impl IntoResponse {
    execute(r, h, "action", json!({"id":id,"action":action})).await
}
async fn receive(State(r): State<Runtime>, h: HeaderMap) -> impl IntoResponse {
    execute(r, h, "receive_start", Value::Null).await
}
async fn profiles(State(r): State<Runtime>, h: HeaderMap) -> impl IntoResponse {
    execute(r, h, "profiles", Value::Null).await
}
async fn rpc(State(r): State<Runtime>, h: HeaderMap, Json(p): Json<Value>) -> impl IntoResponse {
    execute(
        r,
        h,
        p["method"].as_str().unwrap_or(""),
        p["params"].clone(),
    )
    .await
}
async fn events(
    State(r): State<Runtime>,
    h: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> axum::response::Response {
    let auth = r.authenticated(&Request {
        token: token(&h),
        method: "events".into(),
        params: json!({"limit":1}),
    });
    if !auth.ok || h.contains_key("origin") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut cursor = q
        .get("after")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let stream = IntervalStream::new(tokio::time::interval(std::time::Duration::from_secs(1))).map(
        move |_| {
            let batch = r
                .engine
                .lock()
                .ok()
                .and_then(|e| e.events(cursor, 100).ok())
                .unwrap_or_default();
            if let Some((n, _)) = batch.last() {
                cursor = *n;
            }
            Ok::<_, std::convert::Infallible>(
                Event::default()
                    .id(cursor.to_string())
                    .event("qrtx.events")
                    .data(serde_json::to_string(&batch).unwrap_or_default()),
            )
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
pub(crate) async fn live() -> Json<Value> {
    Json(
        json!({"ok":true,"service":"qrtransceiver","version":env!("CARGO_PKG_VERSION"),"buildHash":option_env!("BUILD_HASH").unwrap_or("unknown")}),
    )
}
pub(crate) async fn ready(State(r): State<Runtime>) -> impl IntoResponse {
    let ok = r.engine.lock().map(|e| e.jobs().is_ok()).unwrap_or(false);
    (
        if ok {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({"ok":ok,"service":"qrtransceiver","version":env!("CARGO_PKG_VERSION")})),
    )
}
pub async fn serve(runtime: Runtime, port: u16) -> Result<()> {
    let router = Router::new()
        .route("/livez", get(live))
        .route("/readyz", get(ready))
        .route("/v1/status", get(status))
        .route("/v1/devices", get(devices))
        .route("/v1/transfers", get(transfers))
        .route("/v1/transfers/{id}", get(transfer))
        .route("/v1/transfers/files", post(enqueue))
        .route("/v1/transfers/directory", post(enqueue))
        .route("/v1/transfers/{id}/{action}", post(action))
        .route("/v1/receive/start", post(receive))
        .route("/v1/settings/profiles", get(profiles))
        .route("/v1/events", get(events))
        .route("/v1/rpc", post(rpc))
        .layer(DefaultBodyLimit::max(1_048_576))
        .with_state(runtime);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    axum::serve(listener, router).await?;
    Ok(())
}
