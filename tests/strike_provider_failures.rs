use std::time::Duration;

use axum::{
    Json, Router,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
};
use lightning_goats::strike::StrikeClient;
use serde_json::json;
use tokio::{net::TcpListener, time::sleep};

const DESCRIPTION_HASH: &str =
    "1111111111111111111111111111111111111111111111111111111111111111";

async fn spawn(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}/")
}

async fn rate_limited() -> impl IntoResponse {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({"error": "rate limited"})),
    )
}

async fn deliberately_slow() -> impl IntoResponse {
    sleep(Duration::from_secs(11)).await;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "too late"})),
    )
}

#[tokio::test]
async fn receive_request_rate_limit_fails_closed() {
    let base_url = spawn(Router::new().route("/v1/receive-requests", post(rate_limited))).await;
    let client = StrikeClient::new(&base_url, "test-receive-only-key".to_owned()).unwrap();

    let error = client
        .create_bolt11_receive_request(1_000_000, DESCRIPTION_HASH, 300)
        .await
        .unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("rate limited"));
    assert!(message.contains("HTTP 429"));
}

#[tokio::test]
async fn receive_request_timeout_fails_closed() {
    let base_url = spawn(Router::new().route("/v1/receive-requests", post(deliberately_slow))).await;
    let client = StrikeClient::new(&base_url, "test-receive-only-key".to_owned()).unwrap();

    let started = tokio::time::Instant::now();
    let error = client
        .create_bolt11_receive_request(1_000_000, DESCRIPTION_HASH, 300)
        .await
        .unwrap_err();

    let elapsed = started.elapsed();
    let message = format!("{error:#}");
    assert!(message.contains("Strike create receive request request failed"));
    assert!(elapsed >= Duration::from_secs(9));
    assert!(elapsed < Duration::from_secs(11));
}
