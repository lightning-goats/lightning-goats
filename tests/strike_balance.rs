use axum::{
    Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::any,
};
use lightning_goats::strike::StrikeClient;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::net::TcpListener;

#[derive(Clone)]
struct Mock {
    body: String,
    status: StatusCode,
    calls: Arc<AtomicUsize>,
}
async fn balance(
    State(state): State<Mock>,
    method: Method,
    headers: HeaderMap,
) -> impl IntoResponse {
    assert_eq!(method, Method::GET);
    assert_eq!(headers["authorization"], "Bearer synthetic-only");
    state.calls.fetch_add(1, Ordering::SeqCst);
    (
        state.status,
        [
            ("location", "/forbidden"),
            ("content-type", "application/json"),
        ],
        state.body,
    )
}
async fn check(body: String, status: StatusCode, expected: Option<u64>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let state = Mock {
        body,
        status,
        calls: calls.clone(),
    };
    let app = Router::new()
        .route("/v1/balances", any(balance))
        .fallback(|State(state): State<Mock>| async move {
            state.calls.fetch_add(1, Ordering::SeqCst);
            StatusCode::INTERNAL_SERVER_ERROR
        })
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let result = StrikeClient::new(&url, "synthetic-only".into())
        .unwrap()
        .btc_balance()
        .await;
    server.abort();
    let _ = server.await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    match expected {
        Some(value) => assert_eq!(result.unwrap().current_sats(), value),
        None => {
            let error = result.err().expect("invalid balance accepted");
            assert!(!format!("{error:#}").contains("sensitive-provider-value"));
        }
    }
}
#[tokio::test]
async fn authoritative_current_includes_pending_and_never_uses_available_or_total() {
    check(json!([{"currency":"USD","current":"500"}, {"currency":"BTC","current":"0.00001234","available":"0.00000001","pending":"0.00001233","total":"999"}]).to_string(), StatusCode::OK, Some(1234)).await;
    check(
        json!([{"currency":"BTC","current":"0"}]).to_string(),
        StatusCode::OK,
        Some(0),
    )
    .await;
}
#[tokio::test]
async fn missing_duplicate_malformed_or_fractional_balance_is_never_zero_or_rounded() {
    for value in [
        json!([]),
        json!([{"currency":"USD","current":"1"}]),
        json!([{"currency":"BTC","total":"1"}]),
        json!([{"currency":"BTC","current":"1"},{"currency":"BTC","current":"2"}]),
        json!([{"currency":"BTC","current":123}]),
    ] {
        check(value.to_string(), StatusCode::OK, None).await;
    }
    for amount in [
        "-1",
        "+1",
        "1e-8",
        " 1",
        "1.",
        "0.000000001",
        "NaN",
        "18446744073709551616",
        "sensitive-provider-value",
    ] {
        check(
            json!([{"currency":"BTC","current":amount}]).to_string(),
            StatusCode::OK,
            None,
        )
        .await;
    }
}
#[tokio::test]
async fn auth_errors_redirects_and_oversized_responses_never_yield_balance() {
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::FORBIDDEN,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::FOUND,
    ] {
        check("sensitive-provider-value".into(), status, None).await;
    }
    check("x".repeat(256 * 1024 + 1), StatusCode::OK, None).await;
}
