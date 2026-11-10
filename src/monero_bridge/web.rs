use super::{Bridge, CreateReceive};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::Duration;
use uuid::Uuid;

pub(crate) fn secret_eq(expected: &str, given: &str) -> bool {
    if given.len() != 64 {
        return false;
    }
    let mut mac =
        Hmac::<Sha256>::new_from_slice(b"lg-monero-constant-time-comparison").expect("HMAC key");
    mac.update(expected.as_bytes());
    let expected = mac.finalize().into_bytes();
    let mut mac =
        Hmac::<Sha256>::new_from_slice(b"lg-monero-constant-time-comparison").expect("HMAC key");
    mac.update(given.as_bytes());
    mac.verify_slice(&expected).is_ok()
}

impl Bridge {
    pub fn router(&self) -> Router {
        Router::new()
            .route("/healthz", get(|| async { axum::Json(serde_json::json!({"service":"monero-bridge-v1", "wallet_sync":"checked_per_snapshot"})) }))
            .route("/v1/receives", post(create))
            .route("/v1/receives/{id}", get(status))
            .layer(middleware::from_fn_with_state(self.clone(), auth))
            .layer(middleware::from_fn_with_state(self.clone(), bounded))
            .with_state(self.clone())
    }
    pub fn callback_router(&self) -> Router {
        Router::new()
            .route("/callbacks/{id}/{token}", post(callback))
            .layer(middleware::from_fn_with_state(self.clone(), bounded))
            .with_state(self.clone())
    }
}
async fn auth(State(bridge): State<Bridge>, request: Request, next: Next) -> Response {
    let headers: Vec<_> = request
        .headers()
        .get_all(header::AUTHORIZATION)
        .iter()
        .collect();
    let valid = headers.len() == 1
        && headers[0]
            .to_str()
            .ok()
            .and_then(|s| s.strip_prefix("Bearer "))
            .is_some_and(|token| secret_eq(&bridge.token, token));
    if !valid {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}
async fn bounded(State(bridge): State<Bridge>, request: Request, next: Next) -> Response {
    let Ok(_slot) = bridge.http_slots.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    if request.uri().query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let (parts, body) = request.into_parts();
    let limit = if parts.uri.path().starts_with("/callbacks/") {
        64 * 1024
    } else {
        4096
    };
    let bytes = match tokio::time::timeout(Duration::from_secs(5), to_bytes(body, limit)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let mut response = match tokio::time::timeout(
        Duration::from_secs(bridge.config.timeout_seconds * 4 + 10),
        next.run(Request::from_parts(parts, Body::from(bytes))),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}
async fn create(State(b): State<Bridge>, request: Request) -> Response {
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| {
            !v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(bytes) = to_bytes(request.into_body(), 4096).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(input) = serde_json::from_slice::<CreateReceive>(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match b.create(input).await {
        Ok(snapshot) => axum::Json(snapshot).into_response(),
        // No raw provider/database/path errors or request fields leave the bridge.
        Err(_) => (
            StatusCode::CONFLICT,
            "receive unavailable; preserve intent ID",
        )
            .into_response(),
    }
}
async fn status(State(b): State<Bridge>, Path(id): Path<Uuid>) -> Response {
    match b.status(id).await {
        Ok(snapshot) => axum::Json(snapshot).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn callback(State(b): State<Bridge>, Path((id, token)): Path<(Uuid, String)>) -> Response {
    match b.notify(id, &token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
