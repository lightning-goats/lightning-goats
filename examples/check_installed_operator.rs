//! TEST ONLY: signed HTTP commands against the disposable installed member.
//! Fixed loopback destination, synthetic keys, protected CI/netns attestation.
//! No provider, signing broker, relay, core ledger or feeder request is used.
#![forbid(unsafe_code)]

use anyhow::{Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use lightning_goats::{nostr::SignedNostrEvent, nostr_event::canonical_bytes};
use reqwest::{Client, Method, Response, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    io::Write as _,
    os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const GUARD: &str = "/run/lightning-goats-contact-fixture/ALLOW";
const STATE: &str = "/run/lightning-goats-contact-fixture/operator-client/state.json";
const ORIGIN: &str = "https://example.com";
const ME: &str = "/api/v1/cyberherd/me";
const OVERVIEW: &str = "/api/v1/cyberherd/operator/state";
const COMMAND: &str = "/api/v1/cyberherd/operator/command";
const TEMPLATES: &str = "/api/v1/cyberherd/operator/templates?category=member_added";

fn isolated_fixture() -> Result<()> {
    let uid = std::fs::metadata("/proc/self")?.uid();
    ensure!(uid != 0, "non-root disposable client required");
    ensure!(
        std::env::var_os("CREDENTIALS_DIRECTORY").is_none(),
        "client must not load credentials"
    );
    let path = Path::new(GUARD);
    for parent in path.ancestors().skip(1) {
        let info = std::fs::symlink_metadata(parent)?;
        ensure!(
            info.is_dir() && info.uid() == 0 && info.gid() == 0 && info.mode() & 0o022 == 0,
            "protected disposable guard parent required"
        );
    }
    let info = std::fs::symlink_metadata(path)?;
    ensure!(
        info.is_file()
            && info.uid() == 0
            && info.gid() == 0
            && info.nlink() == 1
            && info.mode() & 0o022 == 0,
        "protected disposable guard required"
    );
    let namespace = std::fs::read_link("/proc/self/ns/net")?;
    let expected = format!(
        "LIGHTNING_GOATS_DISPOSABLE_CONTACT_UPGRADE_CI_v1\n{}\n",
        namespace.to_string_lossy()
    );
    ensure!(
        info.len() <= 256 && std::fs::read(path)? == expected.as_bytes(),
        "client network namespace differs from root attestation"
    );
    let parent = std::fs::symlink_metadata(Path::new(STATE).parent().unwrap())?;
    ensure!(
        parent.is_dir() && parent.uid() == uid && parent.mode() & 0o077 == 0,
        "private synthetic client state required"
    );
    Ok(())
}

fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn authorization(secret: u8, method: &str, path: &str, body: &[u8]) -> Result<String> {
    let mut bytes = [0; 32];
    bytes[31] = secret;
    let secp = Secp256k1::new();
    let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&bytes)?);
    let mut tags = vec![
        vec!["u".into(), format!("{ORIGIN}{path}")],
        vec!["method".into(), method.into()],
        vec!["nonce".into(), uuid::Uuid::new_v4().to_string()],
    ];
    if method == "POST" {
        tags.push(vec!["payload".into(), hex::encode(Sha256::digest(body))]);
    }
    let mut event = SignedNostrEvent {
        id: String::new(),
        pubkey: key.x_only_public_key().0.to_string(),
        created_at: now()?,
        kind: 27235,
        tags,
        content: String::new(),
        sig: String::new(),
    };
    let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event)?).into();
    event.id = hex::encode(hash);
    event.sig = secp
        .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &key)
        .to_string();
    lightning_goats::nostr_event::VerifiedEvent::verify(event.clone())?;
    Ok(format!(
        "Nostr {}",
        STANDARD.encode(serde_json::to_vec(&event)?)
    ))
}

async fn send(
    client: &Client,
    method: Method,
    path: &str,
    body: &[u8],
    header: &str,
    origin: &str,
) -> Result<Response> {
    let response = client
        .request(method, format!("http://127.0.0.1:3340{path}"))
        .header("Authorization", header)
        .header("Origin", origin)
        .header("Host", "untrusted.example")
        .header("X-Forwarded-Host", "untrusted.example")
        .header("Content-Type", "application/json")
        .body(body.to_vec())
        .send()
        .await?;
    ensure!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            == Some("no-store"),
        "private HTTP response omitted no-store"
    );
    Ok(response)
}

async fn value(response: Response) -> Result<Value> {
    ensure!(
        response.status() == StatusCode::OK,
        "native HTTP request failed"
    );
    let bytes = response.bytes().await?;
    ensure!(bytes.len() <= 98304, "native response exceeds bound");
    Ok(serde_json::from_slice(&bytes)?)
}

async fn get(client: &Client, secret: u8, path: &str) -> Result<Value> {
    value(
        send(
            client,
            Method::GET,
            path,
            &[],
            &authorization(secret, "GET", path, &[])?,
            ORIGIN,
        )
        .await?,
    )
    .await
}

async fn command(client: &Client, body: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(body)?;
    // A not-yet-caught-up core fence may refuse a new mutation. Every retry
    // preserves UUID/body and signs a fresh authorization; no receipt is assumed.
    for attempt in 0..15 {
        let header = authorization(2, "POST", COMMAND, &bytes)?;
        let response = send(client, Method::POST, COMMAND, &bytes, &header, ORIGIN).await?;
        if response.status() != StatusCode::CONFLICT || attempt == 14 {
            return value(response).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    bail!("unreachable bounded command loop")
}

async fn exercise(client: &Client) -> Result<()> {
    let saved_auth = authorization(3, "GET", ME, &[])?;
    let member = value(send(client, Method::GET, ME, &[], &saved_auth, ORIGIN).await?).await?;
    ensure!(
        member["operator"] == false && member["member"]["active"] == false,
        "guest acquired authority"
    );
    ensure!(
        send(client, Method::GET, ME, &[], &saved_auth, ORIGIN)
            .await?
            .status()
            == StatusCode::UNAUTHORIZED,
        "HTTP nonce replay accepted"
    );
    ensure!(
        send(
            client,
            Method::GET,
            OVERVIEW,
            &[],
            &authorization(3, "GET", OVERVIEW, &[])?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "guest acquired operator reads"
    );
    ensure!(
        send(
            client,
            Method::GET,
            ME,
            &[],
            &authorization(2, "GET", ME, &[])?,
            "https://wrong.example"
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "wrong Origin accepted"
    );
    let before = get(client, 2, OVERVIEW).await?;
    ensure!(
        before["total_members"] == 0,
        "fixture roster must start empty"
    );
    let minimum = before["settings"]["minimum_sats"]
        .as_str()
        .unwrap()
        .parse::<u64>()?
        + 1;
    let settings = json!({"id":uuid::Uuid::new_v4(),"expected_revision":before["revision"],
        "action":{"type":"settings","minimum_sats":minimum.to_string(),"max_members":before["settings"]["max_members"],"member_percent":0}});
    let receipt = command(client, &settings).await?;
    ensure!(receipt["changed"] == true, "settings were not applied");
    let edited = get(client, 2, OVERVIEW).await?;
    ensure!(
        edited["settings"]["minimum_sats"] == settings["action"]["minimum_sats"]
            && edited["settings"]["member_percent"] == 0,
        "settings readback differs"
    );
    let edit = json!({"id":uuid::Uuid::new_v4(),"expected_revision":edited["revision"],
        "action":{"type":"template","edit":{"DisableCategory":{"category":"member_added"}}}});
    let template_receipt = command(client, &edit).await?;
    ensure!(
        template_receipt["changed"] == true && get(client, 2, TEMPLATES).await?["enabled"] == false,
        "template opt-out was not retained"
    );
    ensure!(
        command(client, &settings).await? == receipt,
        "exact committed retry changed its receipt"
    );
    let current = get(client, 2, OVERVIEW).await?;
    let rejected = json!({"id":uuid::Uuid::new_v4(),"expected_revision":current["revision"],
        "action":{"type":"activate","pubkey":member["pubkey"],"profile":{"name":"Synthetic operator fixture","picture":null,"lud16":"fixture@example.com"}}});
    let bytes = serde_json::to_vec(&rejected)?;
    ensure!(
        send(
            client,
            Method::POST,
            COMMAND,
            &bytes,
            &authorization(2, "POST", COMMAND, &bytes)?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::CONFLICT,
        "paused activation was accepted"
    );
    ensure!(
        send(
            client,
            Method::POST,
            COMMAND,
            &bytes,
            &authorization(3, "POST", COMMAND, &bytes)?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "guest mutation was accepted"
    );
    ensure!(
        send(
            client,
            Method::POST,
            COMMAND,
            b"{}",
            &authorization(2, "POST", COMMAND, &bytes)?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "changed signed body was accepted"
    );
    let after = get(client, 2, OVERVIEW).await?;
    ensure!(
        after["revision"] == current["revision"] && after["total_members"] == 0,
        "refused command changed membership"
    );
    let saved = json!({"saved_at":now()?,"saved_auth":saved_auth,"settings":settings,"receipt":receipt,
        "template_receipt":template_receipt,"revision":after["revision"],"minimum":minimum.to_string()});
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(STATE)?;
    file.write_all(&serde_json::to_vec(&saved)?)?;
    file.sync_all()?;
    println!(
        "{}",
        json!({"exercise_accepted":true,"real_native_http":true,"synthetic_keys_only":true,
        "settings_and_template_applied":true,"guest_origin_body_and_pause_refusals":true,"same_command_receipt_retained":true})
    );
    Ok(())
}

async fn after_restart(client: &Client) -> Result<()> {
    let info = std::fs::symlink_metadata(STATE)?;
    ensure!(
        info.is_file()
            && info.nlink() == 1
            && info.len() < 32768
            && info.uid() == std::fs::metadata("/proc/self")?.uid()
            && info.mode() & 0o077 == 0,
        "private fixture evidence required"
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(STATE)?)?;
    let saved_header = saved["saved_auth"].as_str().unwrap();
    let encoded = saved_header
        .strip_prefix("Nostr ")
        .ok_or_else(|| anyhow::anyhow!("invalid fixture authorization"))?;
    let decoded = STANDARD.decode(encoded)?;
    let event =
        lightning_goats::nostr_event::VerifiedEvent::from_json(std::str::from_utf8(&decoded)?)?;
    ensure!(
        now()?.saturating_sub(event.event().created_at) <= 30,
        "nonce proof expired before restart verification"
    );
    let current = get(client, 2, OVERVIEW).await?;
    ensure!(
        current["revision"] == saved["revision"]
            && current["settings"]["minimum_sats"] == saved["minimum"]
            && current["settings"]["member_percent"] == 0
            && current["total_members"] == 0,
        "restart changed operator state"
    );
    ensure!(
        get(client, 2, TEMPLATES).await?["enabled"] == false,
        "restart lost template opt-out"
    );
    ensure!(
        send(
            client,
            Method::GET,
            ME,
            &[],
            saved["saved_auth"].as_str().unwrap(),
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "unexpired nonce replay survived restart"
    );
    ensure!(
        command(client, &saved["settings"]).await? == saved["receipt"],
        "restart lost exact command receipt"
    );
    let mut conflicting = saved["settings"].clone();
    conflicting["action"]["minimum_sats"] = "9999".into();
    let body = serde_json::to_vec(&conflicting)?;
    ensure!(
        send(
            client,
            Method::POST,
            COMMAND,
            &body,
            &authorization(2, "POST", COMMAND, &body)?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::CONFLICT,
        "changed command identity was accepted"
    );
    ensure!(
        get(client, 2, OVERVIEW).await?["revision"] == saved["revision"],
        "replayed command mutated revision"
    );
    println!(
        "{}",
        json!({"restart_accepted":true,"actual_unexpired_nonce_replay_refused":true,
        "settings_and_template_retained":true,"same_command_receipt_retained":true,"changed_command_conflict_refused":true,
        "roster_empty":true,"member_percent_zero":true,"provider_operations":0,"publications":0})
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    isolated_fixture()?;
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    match std::env::args().nth(1).as_deref() {
        Some("exercise") => exercise(&client).await,
        Some("after-restart") => after_restart(&client).await,
        _ => bail!("explicit disposable exercise or restart verification required"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinary_execution_has_no_authority_to_contact_a_service() {
        assert!(super::isolated_fixture().is_err());
    }
}
