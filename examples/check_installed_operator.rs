//! TEST ONLY: signed HTTP commands against the disposable installed member.
//! Fixed loopback destination, synthetic keys, protected CI/netns attestation.
//! No provider, signing broker, relay, direct ledger access or feeder command.
#![forbid(unsafe_code)]

use anyhow::{Context as _, Result, bail, ensure};
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
const READY_STATE: &str = "/run/lightning-goats-contact-fixture/operator-client/ready.json";
const CONTROLS_STATE: &str = "/run/lightning-goats-contact-fixture/operator-client/controls.json";
const EDGE_CERTIFICATE: &str = "/run/lightning-goats-contact-fixture/edge.crt";
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
        "native HTTP request failed: {}",
        response.status()
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
    // Allow three five-second intake polls. Back off rather than consuming the
    // bridge's two-per-second budget before an activation's two reads can finish.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let header = authorization(2, "POST", COMMAND, &bytes)?;
        let response = send(client, Method::POST, COMMAND, &bytes, &header, ORIGIN).await?;
        if response.status() != StatusCode::CONFLICT || tokio::time::Instant::now() >= deadline {
            return value(response)
                .await
                .with_context(|| format!("disposable operator action {}", body["action"]["type"]));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
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

fn save_fixture(path: &str, saved: &Value) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&serde_json::to_vec(saved)?)?;
    file.sync_all()?;
    Ok(())
}

fn read_fixture(path: &str) -> Result<Value> {
    let info = std::fs::symlink_metadata(path)?;
    ensure!(
        info.is_file()
            && info.nlink() == 1
            && info.len() < 32768
            && info.uid() == std::fs::metadata("/proc/self")?.uid()
            && info.mode() & 0o077 == 0,
        "private synthetic command evidence required"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

async fn action(client: &Client, action: Value) -> Result<(Value, Value)> {
    // Pace distinct operator mutations too, leaving admission for intake and
    // overlay delivery. No service admission limit or safety fence is changed.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let state = get(client, 2, OVERVIEW).await?;
    let body =
        json!({"id":uuid::Uuid::new_v4(),"expected_revision":state["revision"],"action":action});
    let receipt = command(client, &body).await?;
    Ok((body, receipt))
}

async fn refused(client: &Client, body: &Value) -> Result<()> {
    // A preceding burst must not stand in for the explicit refusal under test.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let bytes = serde_json::to_vec(body)?;
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
        "conflicting membership mutation was accepted"
    );
    Ok(())
}

async fn ready(client: &Client) -> Result<()> {
    let guest = get(client, 3, ME).await?;
    ensure!(
        guest["member"]["active"] == false,
        "guest must start inactive"
    );
    let before = get(client, 2, OVERVIEW).await?;
    let activate = json!({"type":"activate","pubkey":guest["pubkey"],
        "profile":{"name":"Synthetic ready member","picture":null,"lud16":"fixture@example.com"}});
    let (_, activated) = action(client, activate.clone()).await?;
    ensure!(
        activated["changed"] == true,
        "ready activation did not commit"
    );
    let member = get(client, 3, ME).await?;
    ensure!(
        member["member"]["active"] == true && member["member"]["amount_sats"] == "0",
        "activation fabricated an initial score"
    );
    action(
        client,
        json!({"type":"amount","pubkey":guest["pubkey"],"amount_sats":"21"}),
    )
    .await?;
    let revision = get(client, 2, OVERVIEW).await?["revision"].clone();
    refused(
        client,
        &json!({"id":uuid::Uuid::new_v4(),"expected_revision":before["revision"],
        "action":{"type":"amount","pubkey":guest["pubkey"],"amount_sats":"22"}}),
    )
    .await?;
    ensure!(
        get(client, 2, OVERVIEW).await?["revision"] == revision,
        "stale revision changed state"
    );
    action(client, json!({"type":"ban","pubkey":guest["pubkey"]})).await?;
    let banned = get(client, 3, ME).await?;
    ensure!(
        banned["member"]["banned"] == true
            && banned["member"]["active"] == false
            && banned["member"]["amount_sats"] == "0"
            && banned["member"]["lifetime_sats"] == "0",
        "ban did not clear only the current manual score"
    );
    let revision = get(client, 2, OVERVIEW).await?["revision"].clone();
    refused(
        client,
        &json!({"id":uuid::Uuid::new_v4(),"expected_revision":revision,"action":activate}),
    )
    .await?;
    action(client, json!({"type":"unban","pubkey":guest["pubkey"]})).await?;
    ensure!(
        get(client, 3, ME).await?["member"]["active"] == false,
        "unban implicitly reactivated a member"
    );
    let (_, activated) = action(client, activate).await?;
    ensure!(
        activated["changed"] == true,
        "explicit unbanned activation failed"
    );
    let (score, receipt) = action(
        client,
        json!({"type":"amount","pubkey":guest["pubkey"],"amount_sats":"21"}),
    )
    .await?;
    let current = get(client, 2, OVERVIEW).await?;
    let member = get(client, 3, ME).await?;
    ensure!(
        member["member"]["active"] == true
            && member["member"]["banned"] == false
            && member["member"]["amount_sats"] == "21"
            && member["member"]["lifetime_sats"] == "0"
            && current["settings"]["member_percent"] == 0,
        "manual score changed paid history or zero allocation"
    );
    save_fixture(
        READY_STATE,
        &json!({"score":score,"receipt":receipt,"revision":current["revision"],
        "cycle":current["cycle"],"pubkey":guest["pubkey"]}),
    )?;
    println!(
        "{}",
        json!({"ready_commands_accepted":true,"activation_and_score_applied":true,
        "stale_revision_refused":true,"explicit_unban_required":true,"manual_score_lifetime_zero":true,
        "member_percent_zero":true})
    );
    Ok(())
}

async fn after_ready_restart(client: &Client) -> Result<()> {
    let saved = read_fixture(READY_STATE)?;
    let current = get(client, 2, OVERVIEW).await?;
    let member = get(client, 3, ME).await?;
    ensure!(
        current["revision"] == saved["revision"]
            && current["cycle"] == saved["cycle"]
            && member["member"]["active"] == true
            && member["member"]["amount_sats"] == "21"
            && member["member"]["lifetime_sats"] == "0",
        "restart changed actual manual member state"
    );
    ensure!(
        command(client, &saved["score"]).await? == saved["receipt"]
            && get(client, 2, OVERVIEW).await?["revision"] == saved["revision"],
        "restart replayed a manual score mutation"
    );
    println!(
        "{}",
        json!({"ready_restart_accepted":true,"active_score_and_cycle_retained":true,
        "original_command_receipt_retained":true,"manual_score_lifetime_zero":true})
    );
    Ok(())
}

async fn paused_controls(client: &Client) -> Result<()> {
    let saved = read_fixture(READY_STATE)?;
    let before = get(client, 2, OVERVIEW).await?;
    refused(
        client,
        &json!({"id":uuid::Uuid::new_v4(),"expected_revision":before["revision"],
        "action":{"type":"amount","pubkey":saved["pubkey"],"amount_sats":"22"}}),
    )
    .await?;
    ensure!(
        command(client, &saved["score"]).await? == saved["receipt"]
            && get(client, 2, OVERVIEW).await?["revision"] == before["revision"],
        "paused committed retry mutated its original score"
    );
    action(client, json!({"type":"remove","pubkey":saved["pubkey"]})).await?;
    let member = get(client, 3, ME).await?;
    ensure!(
        member["member"]["active"] == false && member["member"]["amount_sats"] == "0",
        "paused removal did not clear active score"
    );
    action(client, json!({"type":"ban","pubkey":saved["pubkey"]})).await?;
    let (reset, reset_receipt) = action(client, json!({"type":"reset"})).await?;
    let after = get(client, 2, OVERVIEW).await?;
    ensure!(
        after["cycle"].as_i64() == saved["cycle"].as_i64().map(|v| v + 1)
            && get(client, 3, ME).await?["member"]["banned"] == true,
        "manual reset lost the ban or failed to start a new cycle"
    );
    action(client, json!({"type":"unban","pubkey":saved["pubkey"]})).await?;
    let final_state = get(client, 2, OVERVIEW).await?;
    let member = get(client, 3, ME).await?;
    ensure!(
        member["member"]["active"] == false
            && member["member"]["banned"] == false
            && member["member"]["amount_sats"] == "0"
            && member["member"]["lifetime_sats"] == "0"
            && final_state["settings"]["member_percent"] == 0
            && get(client, 2, TEMPLATES).await?["enabled"] == false,
        "paused control changed retained settings, template or paid history"
    );
    save_fixture(
        CONTROLS_STATE,
        &json!({"reset":reset,"reset_receipt":reset_receipt,
        "cycle":final_state["cycle"],"revision":final_state["revision"]}),
    )?;
    println!(
        "{}",
        json!({"paused_controls_accepted":true,"new_score_refused_while_paused":true,
        "original_score_receipt_retrievable":true,"remove_ban_unban_work_while_paused":true,
        "manual_reset_preserves_ban":true,"settings_and_template_retained":true})
    );
    Ok(())
}

async fn after_controls_restart(client: &Client) -> Result<()> {
    let saved = read_fixture(CONTROLS_STATE)?;
    let ready = read_fixture(READY_STATE)?;
    let state = get(client, 2, OVERVIEW).await?;
    ensure!(
        state["cycle"] == saved["cycle"] && state["revision"] == saved["revision"],
        "restart lost the reset or final revision"
    );
    ensure!(
        command(client, &saved["reset"]).await? == saved["reset_receipt"]
            && command(client, &ready["score"]).await? == ready["receipt"],
        "cross-cycle command retry lost its original receipt"
    );
    let member = get(client, 3, ME).await?;
    ensure!(
        member["member"]["active"] == false
            && member["member"]["banned"] == false
            && member["member"]["amount_sats"] == "0"
            && member["member"]["lifetime_sats"] == "0"
            && get(client, 2, OVERVIEW).await?["revision"] == saved["revision"],
        "cross-cycle retry reset again or restored the old score"
    );
    println!(
        "{}",
        json!({"controls_restart_accepted":true,"reset_and_revision_retained":true,
        "cross_cycle_receipts_retained_without_mutation":true,"final_member_inactive":true,
        "manual_score_lifetime_zero":true})
    );
    Ok(())
}

async fn diagnose_stream() -> Result<()> {
    use sqlx::{
        Row as _,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    let path = Path::new("/var/lib/cyberherd/cyberherd.db");
    let info = std::fs::symlink_metadata(path)?;
    ensure!(
        info.is_file()
            && info.nlink() == 1
            && info.mode() & 0o077 == 0
            && info.uid() == std::fs::metadata("/proc/self")?.uid(),
        "private fixture journal required"
    );
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .create_if_missing(false)
                .pragma("query_only", "ON")
                .pragma("trusted_schema", "OFF")
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    let rows = sqlx::query(
        "SELECT received_through,binding_json FROM core_streams ORDER BY owner LIMIT 2",
    )
    .fetch_all(&pool)
    .await?;
    let cursors = rows.into_iter().map(|row| -> Result<Value> {
        let binding: Value = serde_json::from_str(&row.try_get::<String,_>("binding_json")?)?;
        Ok(json!({"received_through":row.try_get::<i64,_>("received_through")?,"epoch":binding["epoch"]}))
    }).collect::<Result<Vec<_>>>()?;
    pool.close().await;
    println!(
        "{}",
        json!({"synthetic_only":true,"native_member_cursors":cursors})
    );
    Ok(())
}

async fn edge_send(
    client: &Client,
    method: Method,
    path: &str,
    body: &[u8],
    header: &str,
    origin: &str,
) -> Result<Response> {
    // Retain the shipped read admission rate, including across exact locations.
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let response = client
        .request(method, format!("{ORIGIN}{path}"))
        .header("Authorization", header)
        .header("Origin", origin)
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
        "TLS edge omitted no-store"
    );
    Ok(response)
}

async fn edge_get(client: &Client, secret: u8, path: &str) -> Result<Value> {
    value(
        edge_send(
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

async fn edge_command(client: &Client, body: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(body)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let response = edge_send(
            client,
            Method::POST,
            COMMAND,
            &bytes,
            &authorization(2, "POST", COMMAND, &bytes)?,
            ORIGIN,
        )
        .await?;
        if response.status() != StatusCode::CONFLICT || tokio::time::Instant::now() >= deadline {
            return value(response).await.context("disposable TLS edge command");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn edge() -> Result<()> {
    let certificate = std::fs::symlink_metadata(EDGE_CERTIFICATE)?;
    ensure!(
        certificate.is_file()
            && certificate.nlink() == 1
            && certificate.uid() == 0
            && certificate.gid() == 0
            && certificate.mode() & 0o022 == 0
            && certificate.len() <= 8192,
        "protected synthetic TLS certificate required"
    );
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(
            EDGE_CERTIFICATE,
        )?)?)
        // No DNS or public destination: the fixed canonical HTTPS name is bound
        // to this attested namespace's loopback TLS listener. Verify its cert.
        .resolve("example.com", "127.0.0.1:443".parse()?)
        .build()?;
    let saved = authorization(3, "GET", ME, &[])?;
    let guest = value(edge_send(&client, Method::GET, ME, &[], &saved, ORIGIN).await?).await?;
    ensure!(
        guest["operator"] == false && guest["member"]["active"] == false,
        "TLS edge granted guest authority"
    );
    ensure!(
        edge_send(&client, Method::GET, ME, &[], &saved, ORIGIN)
            .await?
            .status()
            == StatusCode::UNAUTHORIZED,
        "TLS edge admitted replayed authorization"
    );
    ensure!(
        edge_send(
            &client,
            Method::GET,
            OVERVIEW,
            &[],
            &authorization(3, "GET", OVERVIEW, &[])?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "TLS edge granted guest operator access"
    );
    ensure!(
        edge_send(
            &client,
            Method::GET,
            ME,
            &[],
            &authorization(2, "GET", ME, &[])?,
            "https://wrong.example"
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "TLS edge accepted wrong Origin"
    );
    let before = edge_get(&client, 2, OVERVIEW).await?;
    ensure!(
        before["total_members"] == 0 && before["settings"]["member_percent"] == 0,
        "TLS edge fixture scope changed"
    );
    ensure!(
        edge_get(&client, 2, TEMPLATES).await?["enabled"] == false,
        "TLS edge changed the signed template query or retained opt-out"
    );
    let minimum = before["settings"]["minimum_sats"]
        .as_str()
        .unwrap()
        .parse::<u64>()?
        + 1;
    let body = json!({"id":uuid::Uuid::new_v4(),"expected_revision":before["revision"],
        "action":{"type":"settings","minimum_sats":minimum.to_string(),
        "max_members":before["settings"]["max_members"],"member_percent":0}});
    let bytes = serde_json::to_vec(&body)?;
    ensure!(
        edge_send(
            &client,
            Method::POST,
            COMMAND,
            b"{}",
            &authorization(2, "POST", COMMAND, &bytes)?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "TLS edge accepted changed signed body"
    );
    ensure!(
        edge_send(
            &client,
            Method::POST,
            COMMAND,
            &bytes,
            &authorization(3, "POST", COMMAND, &bytes)?,
            ORIGIN
        )
        .await?
        .status()
            == StatusCode::UNAUTHORIZED,
        "TLS edge accepted guest mutation"
    );
    let receipt = edge_command(&client, &body).await?;
    ensure!(
        receipt["changed"] == true,
        "TLS edge settings command not applied"
    );
    ensure!(
        edge_command(&client, &body).await? == receipt,
        "TLS edge retry changed the original receipt"
    );
    let after = edge_get(&client, 2, OVERVIEW).await?;
    ensure!(
        after["revision"] == receipt["revision"]
            && after["cycle"] == before["cycle"]
            && after["settings"]["minimum_sats"] == body["action"]["minimum_sats"]
            && after["settings"]["member_percent"] == 0
            && after["total_members"] == 0,
        "TLS edge command readback differs"
    );
    println!(
        "{}",
        json!({"edge_accepted":true,"verified_synthetic_tls":true,
        "real_native_http_through_nginx":true,"canonical_signed_query_and_body":true,
        "guest_origin_body_and_nonce_refusals":true,"settings_and_original_receipt_retained":true,
        "member_percent_zero":true,"provider_operations":0,"publications":0})
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
        Some("ready") => ready(&client).await,
        Some("after-ready-restart") => after_ready_restart(&client).await,
        Some("paused-controls") => paused_controls(&client).await,
        Some("after-controls-restart") => after_controls_restart(&client).await,
        Some("diagnose-stream") => diagnose_stream().await,
        Some("edge") => edge().await,
        _ => bail!("explicit disposable operator exercise required"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinary_execution_has_no_authority_to_contact_a_service() {
        assert!(super::isolated_fixture().is_err());
    }
}
