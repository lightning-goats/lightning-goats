#![cfg(target_os = "linux")]

use lightning_goats::{
    config::{NostrConfig, RuntimeMode},
    ledger::LedgerStore,
    messaging::{process_next_message, run_outbox_publisher},
    nostr::{NakClient, SignedNostrEvent},
    presentation::MessageRenderer,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    net::TcpStream,
    time::{sleep, timeout},
};

// Publicly known test scalars 01 and 02. Never substitute project credentials.
const SIGNER: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const CLIENT: &str = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

struct Process(Child);
impl Process {
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

fn relay(nak: &Path, runtime: &Path, port: u16) -> Process {
    Process(
        Command::new(nak)
            .args(["--config-path"])
            .arg(runtime)
            .args([
                "serve",
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .env_clear()
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}

async fn ready(process: &mut Process, port: u16) {
    timeout(Duration::from_secs(10), async {
        loop {
            assert!(process.0.try_wait().unwrap().is_none(), "relay exited");
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

async fn read_notes(nak: &Path, runtime: &Path, url: &str) -> Vec<Value> {
    let output = timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(nak)
            .arg("--config-path")
            .arg(runtime)
            .args(["req", "--kind", "1", url])
            .env_clear()
            .env("NO_COLOR", "1")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success(), "relay query failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn verify_private_layer_signature(event: &Value) {
    use bitcoin::secp256k1::{Message, Secp256k1, XOnlyPublicKey, schnorr::Signature};
    let canonical = json!([
        0,
        event["pubkey"],
        event["created_at"],
        event["kind"],
        event["tags"],
        event["content"]
    ]);
    let digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&canonical).unwrap()).into();
    assert_eq!(event["id"], hex::encode(digest));
    Secp256k1::verification_only()
        .verify_schnorr(
            &Signature::from_slice(&hex::decode(event["sig"].as_str().unwrap()).unwrap()).unwrap(),
            &Message::from_digest(digest),
            &XOnlyPublicKey::from_slice(&hex::decode(event["pubkey"].as_str().unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
}

async fn decrypt_private_layer(
    nak: &Path,
    runtime: &Path,
    sender: &str,
    ciphertext: &str,
) -> Value {
    let output = timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(nak)
            .arg("--config-path")
            .arg(runtime)
            .args(["decrypt", "-p", sender, ciphertext])
            .env_clear()
            .env("NOSTR_SECRET_KEY", "02")
            .env("NO_COLOR", "1")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "synthetic private layer decryption failed"
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
#[ignore = "requires pinned real nak and a fresh loopback-only network namespace"]
async fn real_nip46_sign_verify_and_durable_retry_without_signer() {
    let links: Value = serde_json::from_slice(
        &Command::new("ip")
            .args(["-j", "link"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(
        links.as_array().unwrap().len(),
        1,
        "run in an isolated network namespace"
    );
    assert_eq!(links[0]["ifname"], "lo");
    let nak =
        fs::canonicalize(std::env::var("LG_TEST_NAK").expect("LG_TEST_NAK required")).unwrap();
    assert_eq!(
        hex::encode(Sha256::digest(fs::read(&nak).unwrap())),
        "b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9",
        "requires the pinned nak v0.20.6 linux-amd64 binary"
    );
    let root = TempDir::new().unwrap();
    let runtime = root.path().join("relay");
    fs::create_dir(&runtime).unwrap();
    let port = 18547;
    let url = format!("ws://127.0.0.1:{port}");
    let mut relay_process = relay(&nak, &runtime, port);
    ready(&mut relay_process, port).await;
    let credentials = root.path().join("credentials");
    let bunker_runtime = root.path().join("bunker");
    fs::create_dir(&credentials).unwrap();
    fs::create_dir(&bunker_runtime).unwrap();
    fs::write(credentials.join("nostr-key"), "01\n").unwrap();
    let mut bunker = Process(
        Command::new("/bin/sh")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/deploy/scripts/run-nak-bunker"
            ))
            .env_clear()
            .env("NO_COLOR", "1")
            .env("NAK_BIN", &nak)
            .env("CREDENTIALS_DIRECTORY", &credentials)
            .env("RUNTIME_DIRECTORY", &bunker_runtime)
            .env("LG_NOSTR_CLIENT_PUBKEY", CLIENT)
            .env("LG_NOSTR_RELAYS", &url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    sleep(Duration::from_millis(300)).await;
    assert!(
        bunker.0.try_wait().unwrap().is_none(),
        "bunker wrapper exited"
    );
    let config = NostrConfig {
        nak_path: nak.clone(),
        nak_config_path: root.path().join("client"),
        bunker_pubkey: SIGNER.to_owned(),
        relays: vec![url.clone()],
    };
    let client = NakClient::new(&config, "02".to_owned()).unwrap();
    let tags = vec![vec!["t".to_owned(), "isolated-test".to_owned()]];
    let event = client
        .sign_kind1("synthetic signing only 🐐", tags.clone())
        .await
        .unwrap();
    assert_eq!(event.pubkey, SIGNER);
    assert_eq!(event.tags, tags);
    client.verify_event(&event).await.unwrap();
    let mut tampered = event.clone();
    tampered.content.push_str(" tampered");
    assert!(client.verify_event(&tampered).await.is_err());
    assert!(
        read_notes(&nak, &runtime, &url).await.is_empty(),
        "signing must not publish kind 1"
    );

    // The actual application boundary must encrypt through this real bunker,
    // not merely prove that the nak CLI supports gift wrapping independently.
    let private_message = "Synthetic private application alert only";
    let private = client
        .wrap_private_message(CLIENT, private_message)
        .await
        .unwrap();
    assert!(!private.ciphertext_json().contains(private_message));
    let wrap: Value = serde_json::from_str(private.ciphertext_json()).unwrap();
    assert_eq!(wrap["kind"], 1059);
    let seal = decrypt_private_layer(
        &nak,
        &runtime,
        wrap["pubkey"].as_str().unwrap(),
        wrap["content"].as_str().unwrap(),
    )
    .await;
    assert_eq!(seal["kind"], 13);
    assert_eq!(seal["pubkey"], SIGNER);
    verify_private_layer_signature(&seal);
    let decoded =
        decrypt_private_layer(&nak, &runtime, SIGNER, seal["content"].as_str().unwrap()).await;
    assert_eq!(decoded["kind"], 14);
    assert_eq!(decoded["pubkey"], SIGNER);
    assert_eq!(decoded["content"], private_message);
    assert_eq!(decoded["tags"], json!([["p", CLIENT]]));
    let public_shape: SignedNostrEvent = serde_json::from_value(wrap).unwrap();
    assert!(client.publish_signed(&public_shape).await.is_err());
    assert!(read_notes(&nak, &runtime, &url).await.is_empty());

    let database = format!("sqlite://{}", root.path().join("ledger.db").display());
    let ledger = LedgerStore::connect(&database).await.unwrap();
    let renderer = MessageRenderer::embedded().unwrap();
    ledger
        .append_event(
            "payment_received",
            &json!({"amount_sats":250,
        "feed_credit_sats":250,"address_user":"dexter"}),
        )
        .await
        .unwrap();
    process_next_message(&ledger, Some(&client), &renderer, 1000, RuntimeMode::Active)
        .await
        .unwrap();
    let saved = ledger.next_outbox_entry().await.unwrap().unwrap();
    bunker.stop();
    let mut bunker_stdout = Vec::new();
    let mut bunker_stderr = Vec::new();
    bunker
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut bunker_stdout)
        .unwrap();
    bunker
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut bunker_stderr)
        .unwrap();
    assert!(bunker_stdout.is_empty(), "bunker stdout escaped wrapper");
    assert!(bunker_stderr.is_empty(), "bunker stderr escaped wrapper");
    relay_process.stop();
    let worker = tokio::spawn(run_outbox_publisher(ledger.clone(), client.clone()));
    timeout(Duration::from_secs(55), async {
        loop {
            if ledger.next_outbox_entry().await.unwrap().unwrap().attempts > 0 {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    let _ = worker.await;
    drop(ledger);
    let ledger = LedgerStore::connect(&database).await.unwrap();
    let recovered = ledger.next_outbox_entry().await.unwrap().unwrap();
    assert_eq!(saved.signed_event_json, recovered.signed_event_json);
    assert_eq!(saved.event_id, recovered.event_id);
    let mut relay_process = relay(&nak, &runtime, port);
    ready(&mut relay_process, port).await;
    let worker = tokio::spawn(run_outbox_publisher(ledger.clone(), client.clone()));
    timeout(Duration::from_secs(20), async {
        while ledger.next_outbox_entry().await.unwrap().is_some() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    let _ = worker.await;
    let signed: SignedNostrEvent = serde_json::from_str(&saved.signed_event_json).unwrap();
    client.publish_signed(&signed).await.unwrap();
    let notes = read_notes(&nak, &runtime, &url).await;
    assert_eq!(notes, vec![serde_json::to_value(&signed).unwrap()]);
    for (kind, payload) in [
        ("interface_info", json!({})),
        ("weather_status", json!({"message":"Synthetic weather"})),
    ] {
        ledger.append_event(kind, &payload).await.unwrap();
        process_next_message(&ledger, Some(&client), &renderer, 1000, RuntimeMode::Active)
            .await
            .unwrap();
    }
    assert!(ledger.next_outbox_entry().await.unwrap().is_none());
    assert_eq!(read_notes(&nak, &runtime, &url).await, notes);
    // The bunker is stopped. Restored ciphertext can still be verified and
    // retried without signing/encrypting again; the public lane stays unchanged.
    let recovered = client
        .restore_private_wrap(CLIENT, private.ciphertext_json())
        .await
        .unwrap();
    assert_eq!(recovered.ciphertext_json(), private.ciphertext_json());
    client
        .publish_private_wrap(CLIENT, &[url.clone()], &recovered)
        .await
        .unwrap();
    client
        .publish_private_wrap(CLIENT, &[url.clone()], &recovered)
        .await
        .unwrap();
    let output = timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(&nak)
            .arg("--config-path")
            .arg(&runtime)
            .args(["req", "--kind", "1059", &url])
            .env_clear()
            .env("NO_COLOR", "1")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(output.status.success());
    let delivered: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        delivered,
        vec![serde_json::from_str::<Value>(private.ciphertext_json()).unwrap()]
    );
    assert_eq!(read_notes(&nak, &runtime, &url).await, notes);
}
