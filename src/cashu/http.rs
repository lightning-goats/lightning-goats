//! NUT-18 token delivery only. A 202 reply acknowledges durable wallet admission.
use super::intake::Forwarder;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, OriginalUri, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use lightning_goats_cashu_receipt::receive_status::{
    AdmissionOutcome, Capability, http_delivery_id,
};
use std::sync::Arc;

pub fn router(forwarder: Option<Arc<Forwarder>>) -> Router {
    let limit = forwarder
        .as_ref()
        .filter(|f| f.ordinary_receiving_enabled())
        .map_or(1024, |f| f.body_limit());
    Router::new()
        .route("/api/v1/cashu", post(deliver).options(preflight))
        .route(
            "/api/v1/cashu/request",
            get(public_request).options(preflight),
        )
        .route(
            "/api/v1/cashu/status",
            post(receipt_status)
                .options(status_preflight)
                .layer(DefaultBodyLimit::max(512)),
        )
        .layer(DefaultBodyLimit::max(limit))
        .with_state(forwarder)
        .layer(middleware::from_fn(private_cors))
}
async fn private_cors(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("access-control-allow-origin", "*"),
        ("access-control-allow-methods", "GET, POST, OPTIONS"),
        ("access-control-allow-headers", "content-type"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}
async fn preflight(State(forwarder): State<Option<Arc<Forwarder>>>) -> StatusCode {
    if forwarder
        .as_ref()
        .is_some_and(|f| f.ordinary_receiving_enabled())
    {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
async fn public_request(
    State(forwarder): State<Option<Arc<Forwarder>>>,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let Some(forwarder) = forwarder.filter(|f| f.ordinary_receiving_enabled()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if uri.query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match forwarder.public_request().await {
        Ok(Some(descriptor)) => {
            Json(super::public_request::public_value(descriptor)).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"Cashu receiving unavailable"})),
        )
            .into_response(),
    }
}

async fn deliver(
    State(forwarder): State<Option<Arc<Forwarder>>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(forwarder) = forwarder.filter(|f| f.ordinary_receiving_enabled()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if uri.query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(body) = String::from_utf8(body.to_vec()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // All mint/proof/descriptor/attribution validation remains in the wallet.
    if !serde_json::from_str::<serde_json::Value>(&body).is_ok_and(|v| v.is_object()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    // A sender may have lost every previous reply. Identical bounded bodies
    // must reuse their private delivery binding rather than consume one durable
    // source alias per HTTP attempt. This ID grants no status/credit authority.
    let delivery_id = http_delivery_id(body.as_bytes());
    match forwarder.submit_with_status(delivery_id, body).await {
        Ok(reply) => match reply.result {
            AdmissionOutcome::Rejected => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status":"rejected"})),
            )
                .into_response(),
            AdmissionOutcome::Admitted { capability, status } => (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({"status":status,"capability":capability})),
            )
                .into_response(),
        },
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"Cashu receiving unavailable"})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusBody {
    capability: Capability,
}
async fn status_preflight(State(forwarder): State<Option<Arc<Forwarder>>>) -> StatusCode {
    if forwarder.is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
async fn receipt_status(
    State(forwarder): State<Option<Arc<Forwarder>>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(forwarder) = forwarder else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if uri.query().is_some() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(value) = serde_json::from_slice::<StatusBody>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if value.capability.validate().is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match forwarder.receipt_status(value.capability).await {
        Ok(Some(status)) => Json(serde_json::json!({"status":status})).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"Cashu status unavailable"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::intake::Policy;
    use std::time::Duration;
    use uuid::Uuid;

    async fn start(forwarder: Option<Arc<Forwarder>>) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router(forwarder)).await.unwrap();
        });
        (base, task)
    }
    #[tokio::test]
    async fn status_lookup_survives_receiving_pause_but_stays_bounded_and_coarse()
    -> anyhow::Result<()> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()?;
        let (base, task) = start(None).await;
        assert_eq!(
            client
                .post(format!("{base}/api/v1/cashu/status"))
                .json(&serde_json::json!({"capability":"a".repeat(64)}))
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
        task.abort();
        let forwarder = Arc::new(Forwarder::new(Policy {
            socket: "/run/lightning-goats-cashu/not-installed.sock".into(),
            wallet_uid: crate::private_ipc::uid()?.checked_add(1).unwrap(),
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            receive_cashu: false,
            receive_nutzap: false,
            body_bytes: 65536,
            concurrent_requests: 1,
            deadline: Duration::from_secs(2),
        })?);
        let (base, task) = start(Some(forwarder)).await;
        let url = format!("{base}/api/v1/cashu/status");
        assert_eq!(
            client
                .request(reqwest::Method::OPTIONS, &url)
                .send()
                .await?
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            client
                .request(reqwest::Method::OPTIONS, format!("{base}/api/v1/cashu"))
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
        for body in [
            serde_json::json!({"capability":"synthetic-private-capability"}),
            serde_json::json!({"capability":"a".repeat(64),"id":"forbidden"}),
        ] {
            assert_eq!(
                client.post(&url).json(&body).send().await?.status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            client
                .post(&url)
                .header("content-type", "application/json")
                .body("a".repeat(513))
                .send()
                .await?
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            client
                .post(format!("{url}?capability=private-marker"))
                .json(&serde_json::json!({"capability":"a".repeat(64)}))
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
        let response = client
            .post(&url)
            .json(&serde_json::json!({"capability":"a".repeat(64)}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            response.json::<serde_json::Value>().await?,
            serde_json::json!({"error":"Cashu status unavailable"})
        );
        task.abort();
        Ok(())
    }

    #[tokio::test]
    async fn default_off_and_bounded_cross_origin_delivery_never_echo_tokens() -> anyhow::Result<()>
    {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()?;
        let (base, task) = start(None).await;
        assert_eq!(
            client
                .post(format!("{base}/api/v1/cashu"))
                .body("{}")
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            client
                .get(format!("{base}/api/v1/cashu/request"))
                .send()
                .await?
                .status(),
            StatusCode::NOT_FOUND
        );
        task.abort();
        let forwarder = Arc::new(Forwarder::new(Policy {
            socket: "/run/lightning-goats-cashu/not-installed.sock".into(),
            wallet_uid: crate::private_ipc::uid()?.checked_add(1).unwrap(),
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            receive_cashu: true,
            receive_nutzap: false,
            body_bytes: 128,
            concurrent_requests: 1,
            deadline: Duration::from_secs(1),
        })?);
        let (base, task) = start(Some(forwarder)).await;
        let url = format!("{base}/api/v1/cashu");
        let unavailable = client
            .get(format!("{base}/api/v1/cashu/request"))
            .send()
            .await?;
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(unavailable.headers()["cache-control"], "no-store");
        assert_eq!(unavailable.headers()["access-control-allow-origin"], "*");
        assert_eq!(
            unavailable.json::<serde_json::Value>().await?,
            serde_json::json!({"error":"Cashu receiving unavailable"})
        );
        assert_eq!(
            client
                .get(format!("{base}/api/v1/cashu/request?token=fixture"))
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );

        let preflight = client
            .request(reqwest::Method::OPTIONS, &url)
            .header("origin", "https://wallet.example")
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "content-type")
            .send()
            .await?;
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        assert_eq!(preflight.headers()["access-control-allow-origin"], "*");
        assert!(
            preflight
                .headers()
                .get("access-control-allow-credentials")
                .is_none()
        );
        for (suffix, content_type, body, expected) in [
            (
                "",
                "text/plain",
                "{}".to_owned(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                "?token=refused",
                "application/json",
                "{}".to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                "",
                "application/json",
                "not json".to_owned(),
                StatusCode::BAD_REQUEST,
            ),
            (
                "",
                "application/json",
                "x".repeat(129),
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            (
                "",
                "application/json; charset=utf-8",
                r#"{"proofs":"private-fixture-secret"}"#.to_owned(),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let response = client
                .post(format!("{url}{suffix}"))
                .header("content-type", content_type)
                .header("origin", "https://wallet.example")
                .body(body)
                .send()
                .await?;
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(response.headers()["access-control-allow-origin"], "*");
            assert!(!response.text().await?.contains("private-fixture-secret"));
        }
        assert_eq!(
            client.get(&url).send().await?.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        task.abort();
        Ok(())
    }
}
