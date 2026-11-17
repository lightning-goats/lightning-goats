//! Kernel identity plus real HTTP and WebSocket presentation of wallet metadata.
use super::*;
use crate::{
    cashu::intake::{Forwarder, Policy},
    ledger::LedgerStore,
    overlay::{OverlayResume, serve_overlay_socket},
    presentation::MessageRenderer,
};
use axum::{Router, extract::ws::WebSocketUpgrade, response::IntoResponse, routing::get};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    socket: PathBuf,
    wallet_uid: u32,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    descriptor: Descriptor,
    submissions: Vec<serde_json::Value>,
}
#[tokio::test]
#[ignore = "requires isolated public metadata cross-UID runner"]
async fn isolated_public_request_core_child() -> Result<()> {
    let root = PathBuf::from(std::env::var("LG_CASHU_PUBLIC_FIXTURE")?);
    let phase = std::env::var("LG_CASHU_PUBLIC_PHASE")?;
    ensure!(
        ["initial", "paused", "rogue"].contains(&phase.as_str()),
        "invalid phase"
    );
    let ready: Ready = serde_json::from_slice(&std::fs::read(root.join("ready.json"))?)?;
    let policy = |epoch| Policy {
        socket: ready.socket.clone(),
        wallet_uid: ready.wallet_uid,
        wallet_instance: ready.wallet_instance,
        wallet_epoch: epoch,
        receive_cashu: true,
        receive_nutzap: false,
        body_bytes: 65536,
        concurrent_requests: 2,
        deadline: Duration::from_secs(2),
    };
    let client = Arc::new(Forwarder::new(policy(ready.wallet_epoch))?);
    if phase == "rogue" {
        ensure!(client.public_request().await.is_err(), "rogue accepted");
        println!("PUBLIC_ROGUE_PEER_REJECTED");
        return Ok(());
    }
    ensure!(
        Forwarder::new(policy(Uuid::new_v4()))?
            .public_request()
            .await
            .is_err(),
        "stale epoch accepted"
    );
    let expected = if phase == "initial" {
        Some(ready.descriptor)
    } else {
        None
    };
    ensure!(
        client.public_request().await? == expected,
        "descriptor mismatch"
    );
    let directory = tempfile::tempdir()?;
    let ledger = LedgerStore::connect(&format!(
        "sqlite://{}",
        directory.path().join("ledger.sqlite").display()
    ))
    .await?;
    let ws_ledger = ledger.clone();
    let ws_client = client.clone();
    let app = Router::new()
        .route(
            "/ws",
            get(move |ws: WebSocketUpgrade| {
                let ledger = ws_ledger.clone();
                let client = ws_client.clone();
                async move {
                    ws.on_upgrade(move |socket| async move {
                        let _ = serve_overlay_socket(
                            socket,
                            ledger,
                            MessageRenderer::embedded().unwrap(),
                            1000,
                            OverlayResume::default(),
                            Some(client),
                        )
                        .await;
                    })
                    .into_response()
                }
            }),
        )
        .merge(crate::cashu::http::router(Some(client)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()?;
    let response = http
        .get(format!("http://{address}/api/v1/cashu/request"))
        .send()
        .await?;
    ensure!(
        response.headers()["cache-control"] == "no-store",
        "public metadata cached"
    );
    let expected_value = expected
        .map(public_value)
        .unwrap_or(serde_json::Value::Null);
    if phase == "initial" {
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "public request unavailable"
        );
        ensure!(
            response.json::<serde_json::Value>().await? == expected_value,
            "HTTP payload changed"
        );
    } else {
        ensure!(
            response.status() == reqwest::StatusCode::NOT_FOUND,
            "paused QR advertised"
        );
    }
    // Real HTTP -> authenticated separate-UID wallet admission. No mock IPC replies.
    let capability_file = root.join("core/status-capabilities.json");
    let capabilities: Vec<serde_json::Value> = if phase == "initial" {
        // A malformed wrapper for the same proof must not reserve its identity
        // or prevent the subsequent valid payment from being admitted.
        let mut malformed = ready.submissions[0].clone();
        malformed["proofs"][0]["C"] = serde_json::json!("00");
        let rejected = http
            .post(format!("http://{address}/api/v1/cashu"))
            .json(&malformed)
            .send()
            .await?;
        ensure!(
            rejected.status() == reqwest::StatusCode::BAD_REQUEST,
            "malformed proof was admitted"
        );
        ensure!(
            rejected.json::<serde_json::Value>().await? == serde_json::json!({"status":"rejected"}),
            "rejected proof exposed private validation data"
        );
        let mut saved = Vec::new();
        for submission in &ready.submissions {
            let response = http
                .post(format!("http://{address}/api/v1/cashu"))
                .json(submission)
                .send()
                .await?;
            ensure!(
                response.status() == reqwest::StatusCode::ACCEPTED,
                "admission failed"
            );
            ensure!(
                response.headers()["cache-control"] == "no-store",
                "capability cached"
            );
            let reply: serde_json::Value = response.json().await?;
            ensure!(
                reply["status"] == "pending",
                "admission falsely claims acquired funds"
            );
            let capability = reply["capability"].clone();
            ensure!(
                capability.as_str().is_some_and(|value| value.len() == 64),
                "missing capability"
            );
            // More than the journal's 64-alias bound: lost replies must not
            // consume a fresh source alias per identical HTTP submission.
            for _ in 0..80 {
                let repeated = http
                    .post(format!("http://{address}/api/v1/cashu"))
                    .json(submission)
                    .send()
                    .await?;
                ensure!(
                    repeated.status() == reqwest::StatusCode::ACCEPTED,
                    "HTTP replay did not acknowledge durable admission"
                );
                let repeated: serde_json::Value = repeated.json().await?;
                ensure!(repeated == reply, "HTTP replay changed pending receipt");
            }
            saved.push(capability);
        }
        ensure!(
            saved.len() == 2 && saved[0] != saved[1],
            "descriptor used as donation identity"
        );
        std::fs::write(&capability_file, serde_json::to_vec(&saved)?)?;
        println!("PUBLIC_HTTP_80_IDENTICAL_RETRIES_PER_PAYMENT_VERIFIED");
        saved
    } else {
        let response = http
            .post(format!("http://{address}/api/v1/cashu"))
            .json(&ready.submissions[0])
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "paused wallet admitted new work"
        );
        serde_json::from_slice(&std::fs::read(&capability_file)?)?
    };
    for capability in &capabilities {
        let response = http
            .post(format!("http://{address}/api/v1/cashu/status"))
            .json(&serde_json::json!({"capability":capability}))
            .send()
            .await?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "retained status unavailable"
        );
        ensure!(
            response.headers()["cache-control"] == "no-store",
            "status cached"
        );
        let status: serde_json::Value = response.json().await?;
        ensure!(
            status == serde_json::json!({"status":"pending"}),
            "status leaks or invents evidence"
        );
    }
    let unknown = http
        .post(format!("http://{address}/api/v1/cashu/status"))
        .json(&serde_json::json!({"capability":"0".repeat(64)}))
        .send()
        .await?;
    ensure!(
        unknown.status() == reqwest::StatusCode::NOT_FOUND,
        "unknown capability accepted"
    );
    let oversized = http
        .post(format!("http://{address}/api/v1/cashu/status"))
        .header("content-type", "application/json")
        .body("x".repeat(513))
        .send()
        .await?;
    ensure!(
        oversized.status() == reqwest::StatusCode::PAYLOAD_TOO_LARGE,
        "status body bound missing"
    );
    println!("PUBLIC_HTTP_PENDING_STATUS_REPLAY_VERIFIED_{phase}");
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing snapshot"))??;
    let snapshot: serde_json::Value = serde_json::from_str(message.to_text()?)?;
    ensure!(
        snapshot["type"] == "snapshot" && snapshot["cashu"] == expected_value,
        "snapshot metadata mismatch"
    );
    ensure!(
        snapshot["feed_credit_sats"] == 0 && snapshot["seq"] == 0,
        "metadata changed credit"
    );
    server.abort();
    println!("PUBLIC_CORE_HTTP_SNAPSHOT_VERIFIED_{phase}");
    Ok(())
}
