//! Combined core/wallet fixture: synthetic mint, real CDK acquisition and melt.
//! Selected explicitly by an offline runner; no installed wallet or credentials.
use super::*;
use lightning_goats_cashu_receipt::treasury::{Destination, Issued};
use serde::Deserialize;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

include!("multi_mint_settlement_fixture.rs");

fn snapshot_funded_wallet(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir(destination)?;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700))?;
    for name in ["journal.sqlite", "wallet.sqlite"] {
        let input = rusqlite::Connection::open_with_flags(
            source.join(name),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let mut output = rusqlite::Connection::open(destination.join(name))?;
        let result = rusqlite::backup::Backup::new(&input, &mut output)?.step(-1)?;
        ensure!(
            matches!(result, rusqlite::backup::StepResult::Done),
            "funded checkpoint backup did not finish"
        );
        let mode: String = output.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
        ensure!(
            mode == "delete",
            "funded checkpoint requires main-only images"
        );
        drop(output);
        std::fs::set_permissions(
            destination.join(name),
            std::fs::Permissions::from_mode(0o600),
        )?;
    }
    Ok(())
}

fn assert_restored_wallet_fenced(directory: &Path) -> Result<()> {
    let journal = std::fs::read(directory.join("journal.sqlite"))?;
    let cdk = std::fs::read(directory.join("wallet.sqlite"))?;
    std::fs::write(
        directory.join("CASHU_RECOVERY_REQUIRED"),
        b"synthetic restore requires reconciliation",
    )?;
    let Err(error) = WriterLock::acquire(directory) else {
        anyhow::bail!("fenced wallet unexpectedly opened");
    };
    assert_eq!(
        error.to_string(),
        "Cashu recovery requires explicit reconciliation"
    );
    assert_eq!(std::fs::read(directory.join("journal.sqlite"))?, journal);
    assert_eq!(std::fs::read(directory.join("wallet.sqlite"))?, cdk);
    assert_eq!(std::fs::read_dir(directory)?.count(), 3);
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    core_epoch: Uuid,
    destination: Destination,
    treasury_policy: String,
    #[serde(default)]
    receipt_socket: Option<std::path::PathBuf>,
    #[serde(default)]
    core_uid: Option<u32>,
    #[serde(default)]
    treasury_socket: Option<std::path::PathBuf>,
    #[serde(default)]
    http_origin: Option<String>,
}

async fn fixture_json_response(response: reqwest::Response) -> Result<Value> {
    let body = response.bytes().await?;
    ensure!(body.len() <= 65536, "fixture HTTP response exceeds bound");
    Ok(serde_json::from_slice(&body)?)
}

async fn fixture_sdk_sender(input: &Value) -> Result<(Value, Value)> {
    use tokio::io::AsyncWriteExt;
    let node = std::env::var_os("LG_CASHU_COMBINED_SENDER_NODE")
        .ok_or_else(|| anyhow::anyhow!("SDK sender node missing"))?;
    let script = std::env::var_os("LG_CASHU_COMBINED_SENDER_SCRIPT")
        .ok_or_else(|| anyhow::anyhow!("SDK sender script missing"))?;
    let installation = std::env::var_os("LG_CASHU_COMBINED_SENDER_INSTALLATION")
        .ok_or_else(|| anyhow::anyhow!("SDK sender dependencies missing"))?;
    let bytes = serde_json::to_vec(input)?;
    ensure!(bytes.len() <= 65536, "SDK fixture input exceeds bound");
    let mut child = tokio::process::Command::new(node)
        .arg(script)
        .arg(installation)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("SDK stdin missing"))?;
    stdin.write_all(&bytes).await?;
    drop(stdin);
    let output = tokio::time::timeout(std::time::Duration::from_secs(20), child.wait_with_output())
        .await??;
    ensure!(
        output.status.success() && output.stderr.is_empty() && output.stdout.len() <= 131072,
        "SDK sender failed or exceeded reply bound"
    );
    let result: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        result["library"] == "@cashu/cashu-ts"
            && result["version"] == "4.7.2"
            && result["offline_send"] == true
            && result["lost_admission_reply_retried"] == true,
        "SDK sender evidence missing"
    );
    Ok((result["payload"].clone(), result["accepted"].clone()))
}

fn publish(root: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let temporary = root.join(format!("{name}.pending"));
    let target = root.join(name);
    ensure!(
        !temporary.exists() && !target.exists(),
        "fixture output already exists"
    );
    std::fs::write(&temporary, serde_json::to_vec(value)?)?;
    std::fs::rename(temporary, target)?;
    Ok(())
}

async fn read_when_ready<T: DeserializeOwned>(root: &Path, name: &str) -> Result<T> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        match std::fs::read(root.join(name)) {
            Ok(bytes) => {
                ensure!(bytes.len() <= 65536, "fixture input exceeds bound");
                return Ok(serde_json::from_slice(&bytes)?);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "fixture peer deadline"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

fn contribution(
    harness: &Harness,
    amount: u64,
    nutzap: bool,
    payment_index: usize,
) -> Result<ValidatedContribution> {
    let proofs = (0..10)
        .filter(|bit| amount & (1 << bit) != 0)
        .map(|bit| {
            let nonce = format!("core-settlement-{payment_index}-{amount}-{bit}");
            let secret = if nutzap {
                json!(["P2PK",{"nonce":nonce,"data":harness.receiver.public_key().x_only_public_key().public_key(cdk::secp256k1::Parity::Even).to_string()}])
                    .to_string()
            } else {
                nonce
            };
            harness.mint.issue_amount(secret, 1 << bit)
        })
        .collect::<Vec<_>>();
    let registry = MintRegistry::new(vec![harness.policy()])?;
    let limits = Limits {
        body_bytes: 65536,
        proof_count: 64,
        receipt_sats: 10000,
    };
    if !nutzap {
        return ordinary_json(
            &serde_json::to_vec(&json!({"id":"reusable",
            "mint":harness.policy().advertised_url,"unit":"sat","proofs":proofs}))?,
            &format!("combined-ordinary-{payment_index}"),
            &["reusable".into()],
            &registry,
            &harness.keys,
            &limits,
            harness.receiver.public_key(),
        );
    }
    use cdk::secp256k1::{Keypair, Message, Secp256k1};
    use lightning_goats_nostr_event::{SignedNostrEvent, canonical_bytes};
    use sha2::{Digest, Sha256};
    let (body, recipient) = harness.nutzap_body(proofs[0].clone())?;
    let mut event: SignedNostrEvent = serde_json::from_str(&body)?;
    event
        .tags
        .retain(|tag| tag.first().is_none_or(|tag| tag != "proof"));
    for proof in proofs {
        event
            .tags
            .push(vec!["proof".into(), serde_json::to_string(&proof)?]);
    }
    let digest: [u8; 32] = Sha256::digest(canonical_bytes(&event)?).into();
    let secp = Secp256k1::new();
    let key = Keypair::from_secret_key(&secp, &cdk::secp256k1::SecretKey::from_slice(&[2; 32])?);
    event.id = cdk::util::hex::encode(digest);
    event.sig = secp
        .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key)
        .to_string();
    nutzap_json(
        &serde_json::to_string(&event)?,
        &recipient,
        &registry,
        &harness.keys,
        &limits,
        harness.receiver.public_key(),
    )
}

#[tokio::test]
#[ignore = "requires explicit combined core/wallet fixture runner"]
async fn combined_settlement_wallet_child() -> Result<()> {
    use crate::treasury_scheduler::{Execution, Worker};
    let root = std::path::PathBuf::from(std::env::var("LG_CASHU_COMBINED_FIXTURE")?);
    let receipt_ipc = std::env::var("LG_CASHU_COMBINED_RECEIPT_IPC").as_deref() == Ok("1");
    let private = if receipt_ipc {
        std::path::PathBuf::from(std::env::var("LG_CASHU_COMBINED_PRIVATE")?)
    } else {
        root.clone()
    };
    crate::private_ipc::private_state(&private)?;
    let treasury_ipc = std::env::var("LG_CASHU_COMBINED_TREASURY_IPC").as_deref() == Ok("1");
    ensure!(
        !treasury_ipc || receipt_ipc,
        "treasury IPC requires receipt IPC identities"
    );
    let http_intake = std::env::var("LG_CASHU_COMBINED_HTTP_INTAKE").as_deref() == Ok("1");
    ensure!(
        !http_intake || treasury_ipc,
        "HTTP composition requires financial IPC"
    );
    let repeat_request = std::env::var("LG_CASHU_COMBINED_REPEAT_REQUEST").as_deref() == Ok("1");
    ensure!(
        !repeat_request || http_intake,
        "repeated request requires HTTP composition"
    );
    let settle_change = std::env::var("LG_CASHU_COMBINED_SETTLE_CHANGE").as_deref() == Ok("1");
    ensure!(
        !settle_change || (http_intake && !repeat_request),
        "residual settlement requires its own HTTP mode"
    );
    let receipt_count = 2 + usize::from(repeat_request || settle_change);
    let sdk_sender = std::env::var_os("LG_CASHU_COMBINED_SENDER_NODE").is_some();
    ensure!(
        !sdk_sender || http_intake,
        "SDK sender requires HTTP composition"
    );
    let lost_reply = std::env::var("LG_CASHU_COMBINED_LOST_REPLY").as_deref() == Ok("1");
    let funded_rollback = std::env::var("LG_CASHU_COMBINED_FUNDED_ROLLBACK").as_deref() == Ok("1");
    let fees = std::env::var("LG_CASHU_COMBINED_FEES").as_deref() == Ok("1");
    let returned_change =
        fees && std::env::var("LG_CASHU_COMBINED_NO_CHANGE").as_deref() != Ok("1");
    let conversion_fee_sats = if fees {
        2 + u64::from(repeat_request)
    } else {
        0
    };
    let deposit_sats = 400 - conversion_fee_sats;
    let bootstrap: Bootstrap = read_when_ready(&root, "bootstrap.json").await?;
    let mut harness = Harness::new(if fees { 100 } else { 0 })?;
    harness
        .mint
        .return_melt_change
        .store(returned_change, Ordering::SeqCst);
    harness
        .mint
        .consume_melt_reserve
        .store(fees && !returned_change, Ordering::SeqCst);
    harness.epoch = bootstrap.wallet_epoch;
    let shared_writer = if treasury_ipc {
        Some(Arc::new(WriterLock::acquire(harness.directory.path())?))
    } else {
        None
    };
    let mut coordinator = if let Some(writer) = &shared_writer {
        harness.open_with_writer(writer.clone()).await?
    } else {
        harness.open().await?
    };
    let delivery_journal = coordinator.journal.clone();
    let mut descriptor_id = String::new();
    let mut http_replays: Vec<(Value, Value)> = Vec::new();
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let intake_server = if http_intake {
        let policy = harness.policy();
        let registry = MintRegistry::new(vec![policy.clone()])?;
        let descriptor = crate::descriptor::Descriptor::prepare(
            &registry,
            "https://lightning-goats.example/api/v1/cashu",
            harness.receiver.public_key(),
            crate::descriptor::Encoding::CreqA,
        )?;
        descriptor_id = descriptor.id().to_owned();
        let keys = harness.keys.clone();
        let receiver = harness.receiver.public_key();
        let journal = delivery_journal.clone();
        let writer = shared_writer.as_ref().unwrap().clone();
        let socket = root.parent().unwrap().join("wallet-run/intake.sock");
        let core_uid = bootstrap
            .core_uid
            .ok_or_else(|| anyhow::anyhow!("core UID missing"))?;
        let instance = bootstrap.wallet_instance;
        let epoch = bootstrap.wallet_epoch;
        let group = std::env::var("LG_CASHU_COMBINED_IPC_GID")?.parse()?;
        Some(tokio::spawn(async move {
            let limits = Limits {
                body_bytes: 65536,
                proof_count: 64,
                receipt_sats: 10000,
            };
            let descriptors = vec![descriptor.id().to_owned()];
            let intake = crate::ingress::Intake {
                core_uid,
                wallet_instance: instance,
                wallet_epoch: epoch,
                receive_cashu: true,
                receive_nutzap: false,
                registry: &registry,
                keys: &keys,
                limits: &limits,
                receiver,
                recipient: "",
                descriptors: &descriptors,
                public_request: Some(&descriptor),
                journal,
                writer: &writer,
            };
            intake
                .serve(
                    &socket,
                    group,
                    std::time::Duration::from_secs(5),
                    64,
                    std::future::pending(),
                )
                .await
        }))
    } else {
        None
    };
    let receipt_client = if receipt_ipc {
        let socket = bootstrap
            .receipt_socket
            .clone()
            .ok_or_else(|| anyhow::anyhow!("receipt socket missing"))?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while !socket.exists() {
            ensure!(
                tokio::time::Instant::now() < deadline,
                "receipt listener deadline"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        Some(crate::receipt_delivery::Client::new(
            socket,
            bootstrap
                .core_uid
                .ok_or_else(|| anyhow::anyhow!("core UID missing"))?,
            bootstrap.core_epoch,
            std::time::Duration::from_secs(5),
        )?)
    } else {
        None
    };
    coordinator.wallet.set_target_proof_count(0);
    let mut receipts = Vec::new();
    let payments = if settle_change {
        vec![(250, true), (150, false), (16, false)]
    } else if repeat_request {
        vec![(250, true), (75, false), (75, false)]
    } else {
        vec![(250, true), (150, false)]
    };
    for (payment_index, (amount, nutzap)) in payments.into_iter().enumerate() {
        let input = contribution(&harness, amount + u64::from(fees), nutzap, payment_index)?;
        let id = if http_intake && !nutzap {
            let origin = bootstrap
                .http_origin
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("HTTP origin missing"))?;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                let ready = http_client
                    .get(format!("{origin}/api/v1/cashu/request"))
                    .send()
                    .await?;
                if ready.status().is_success() {
                    let public: Value = fixture_json_response(ready).await?;
                    ensure!(
                        public["enabled"] == true && public["id"] == descriptor_id,
                        "HTTP advertised a different receiving descriptor"
                    );
                    break;
                }
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "public descriptor deadline"
                );
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            let rejected = http_client
                .post(format!("{origin}/api/v1/cashu"))
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&json!({}))?)
                .send()
                .await?;
            ensure!(
                rejected.status() == reqwest::StatusCode::BAD_REQUEST,
                "malformed HTTP contribution was admitted"
            );
            assert_eq!(
                harness.mint.executions.load(Ordering::SeqCst),
                payment_index
            );
            let intake = input.intake();
            let (payload, accepted) = if sdk_sender {
                let result = fixture_sdk_sender(&json!({
                    "origin": origin, "descriptor_id": descriptor_id,
                    "mint": harness.policy().advertised_url, "keyset": harness.mint.keyset,
                    "gross_sats": amount + u64::from(fees), "proofs": intake.proofs,
                }))
                .await?;
                ensure!(
                    result.0["id"] == descriptor_id
                        && result.1["status"] == "pending"
                        && result.1["capability"]
                            .as_str()
                            .is_some_and(|s| s.len() == 64),
                    "SDK sender changed admission identity or outcome"
                );
                result
            } else {
                let payload = json!({"id":descriptor_id,"mint":harness.policy().advertised_url,"unit":"sat","proofs":intake.proofs});
                let response = http_client
                    .post(format!("{origin}/api/v1/cashu"))
                    .header("content-type", "application/json")
                    .body(serde_json::to_vec(&payload)?)
                    .send()
                    .await?;
                ensure!(
                    response.status() == reqwest::StatusCode::ACCEPTED
                        && response.headers()["cache-control"] == "no-store",
                    "HTTP admission unavailable or cached"
                );
                let accepted: Value = fixture_json_response(response).await?;
                ensure!(
                    accepted["status"] == "pending"
                        && accepted["capability"]
                            .as_str()
                            .is_some_and(|s| s.len() == 64),
                    "HTTP falsely claims acquired funds"
                );
                let repeated = http_client
                    .post(format!("{origin}/api/v1/cashu"))
                    .header("content-type", "application/json")
                    .body(serde_json::to_vec(&payload)?)
                    .send()
                    .await?;
                ensure!(
                    repeated.status() == reqwest::StatusCode::ACCEPTED
                        && fixture_json_response(repeated).await? == accepted,
                    "HTTP replay changed admission"
                );
                (payload, accepted)
            };
            ensure!(
                http_replays
                    .iter()
                    .all(|(old, cap)| old["id"] == payload["id"] && cap != &accepted["capability"]),
                "distinct payments to one descriptor reused a capability"
            );
            http_replays.push((payload, accepted["capability"].clone()));
            let connection = rusqlite::Connection::open_with_flags(
                harness.directory.path().join("journal.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM acquisitions WHERE state='validated'",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                count == 1,
                "HTTP did not admit exactly one original operation"
            );
            let id: String = connection.query_row(
                "SELECT id FROM acquisitions WHERE state='validated'",
                [],
                |r| r.get(0),
            )?;
            id.parse()?
        } else {
            new_id(coordinator.admit(&input)?)
        };
        let acquired = receipt(coordinator.acquire_new(id, &harness.receiver).await?);
        assert_eq!(acquired.net_sats, amount);
        if let Some(client) = &receipt_client {
            // Lose the wallet acknowledgment checkpoint, then retry the original receipt.
            let wire = crate::receipt_wire::from_private(bootstrap.wallet_instance, &acquired)?;
            let _uncommitted_ack = client.send(&wire).await?;
            client
                .deliver(&delivery_journal, bootstrap.wallet_instance, id)
                .await?;
        }
        receipts.push(
            crate::receipt_wire::from_private(bootstrap.wallet_instance, &acquired)?
                .data()
                .clone(),
        );
    }
    if !receipt_ipc {
        publish(&root, "receipts.json", &receipts)?;
    }
    let binding = crate::treasury::TreasuryBinding {
        core_epoch: bootstrap.core_epoch,
        wallet_instance: bootstrap.wallet_instance,
        wallet_epoch: bootstrap.wallet_epoch,
        destination: bootstrap.destination,
        mint_id: harness.policy().id,
        policy_version: bootstrap.treasury_policy,
    };
    let treasury_server = if treasury_ipc {
        let socket = bootstrap
            .treasury_socket
            .clone()
            .ok_or_else(|| anyhow::anyhow!("treasury socket missing"))?;
        let bindings = BTreeMap::from([(binding.mint_id.clone(), binding.clone())]);
        let journal = delivery_journal.clone();
        let writer = shared_writer.as_ref().unwrap().clone();
        let core_uid = bootstrap
            .core_uid
            .ok_or_else(|| anyhow::anyhow!("core UID missing"))?;
        let group = std::env::var("LG_CASHU_COMBINED_IPC_GID")?.parse()?;
        Some(tokio::spawn(async move {
            let intake = crate::treasury_ingress::Intake {
                core_uid,
                accept_new: true,
                bindings: &bindings,
                journal,
                writer: &writer,
            };
            intake
                .serve(
                    &socket,
                    group,
                    std::time::Duration::from_secs(5),
                    64,
                    std::future::pending(),
                )
                .await
        }))
    } else {
        None
    };
    let issued: Issued = if treasury_ipc {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let retained: Option<String> = {
                use rusqlite::OptionalExtension;
                let connection = rusqlite::Connection::open_with_flags(
                    harness.directory.path().join("journal.sqlite"),
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                connection
                    .query_row(
                        "SELECT id FROM treasury_instructions ORDER BY id LIMIT 1",
                        [],
                        |r| r.get(0),
                    )
                    .optional()?
            };
            if let Some(id) = retained {
                break delivery_journal
                    .lock()
                    .unwrap()
                    .treasury_instruction(&binding, id.parse()?)?;
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "treasury IPC admission deadline"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    } else {
        read_when_ready(&root, "issued.json").await?
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    binding.validate_new_invoice(&issued, now)?;
    assert_eq!(
        (
            issued.intent.input_sats,
            issued.intent.deposit_sats,
            issued.intent.maximum_fee_sats
        ),
        (400, deposit_sats, conversion_fee_sats)
    );
    if !treasury_ipc {
        coordinator
            .journal
            .lock()
            .unwrap()
            .bind_treasury_instruction(&binding, &issued, now)?;
    }
    drop(coordinator);
    let quote = json!({"quote":"combined-original-quote","amount":deposit_sats,"fee_reserve":u64::from(fees),
        "state":"UNPAID","expiry":now+240,"payment_preimage":null,"change":null,
        "request":issued.invoice,"unit":"sat","method":"bolt11"});
    *harness.mint.quote_reply.lock().unwrap() = Some(quote.clone());
    let mut paid = quote;
    paid["state"] = "PAID".into();
    paid["payment_preimage"] = "37".repeat(32).into();
    paid["change"] = json!([]);
    *harness.mint.melt_paid_reply.lock().unwrap() = Some(paid);
    harness
        .mint
        .lose_melt_reply
        .store(lost_reply, Ordering::SeqCst);
    harness
        .mint
        .melt_status_unavailable
        .store(lost_reply, Ordering::SeqCst);
    let path = harness.directory.path().join("journal.sqlite");
    if funded_rollback {
        snapshot_funded_wallet(
            harness.directory.path(),
            &private.join("funded-before-melt"),
        )?;
    }
    let writer = if let Some(writer) = &shared_writer {
        writer.clone()
    } else {
        Arc::new(WriterLock::acquire(harness.directory.path())?)
    };
    let database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
    let policy = Arc::new(crate::operation_gate::TreasuryPolicy {
        binding: binding.clone(),
        mint: harness.policy(),
        seed: harness.seed,
    });
    let operator_recovery = crate::treasury_recovery::Recovery {
        transport: &harness.mint,
        journal: delivery_journal.clone(),
        database: database.clone(),
        writer: writer.clone(),
        binding: &binding,
        keys: &harness.keys,
        seed: &harness.seed,
    };
    if funded_rollback {
        assert!(
            crate::operator_treasury::reconcile_original(&operator_recovery, issued.intent.id)
                .await
                .is_err(),
            "one-operation reconciliation accepted a merely bound instruction"
        );
    }
    let mut execution = Execution {
        execute: true,
        reconcile: true,
        maximum_attempts: 4,
        maximum_input_sats: 400,
        maximum_fee_sats: conversion_fee_sats,
        operations_per_24h: 1 + u64::from(settle_change),
        input_sats_per_24h: 400 + 17 * u64::from(settle_change),
        maximum_in_flight_sats: 400,
        split: Default::default(),
    };
    let mut report = None;
    let mut paused_after_loss = false;
    for tick in 0..80 {
        // Rebuild the scheduler and reopen its journal between every durable stage.
        let worker = Worker::from_parts(
            Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 64)?)),
            database.clone(),
            writer.clone(),
            harness.keys.clone(),
            vec![(policy.clone(), harness.mint.clone())],
            execution.clone(),
            std::time::Duration::from_secs(10),
            65536,
        )?;
        let outcome = worker.tick(now as i64 + tick).await?;
        drop(worker);
        if lost_reply && !paused_after_loss && harness.mint.melt_settled.load(Ordering::SeqCst) {
            assert_eq!(outcome, crate::treasury_scheduler::Tick::RetryRetained);
            assert!(
                Journal::open(&path, harness.epoch, 64)?
                    .treasury_melt_report(&binding, issued.intent.id)
                    .is_err()
            );
            if !treasury_ipc {
                publish(&root, "submitted.json", &issued.intent.id)?;
            }
            let verified: Uuid = read_when_ready(&root, "provider-verified.json").await?;
            assert_eq!(verified, issued.intent.id);
            // Reconciliation can continue while every new execution path is paused.
            execution.execute = false;
            paused_after_loss = true;
            harness
                .mint
                .melt_status_unavailable
                .store(false, Ordering::SeqCst);
            if funded_rollback {
                use crate::operator_treasury::{State, reconcile_original};
                assert_eq!(
                    reconcile_original(&operator_recovery, issued.intent.id).await?,
                    State::ReportRecorded
                );
                assert_eq!(
                    reconcile_original(&operator_recovery, issued.intent.id).await?,
                    State::ReportAlreadyRetained
                );
                println!("COMBINED_WALLET_ORIGINAL_TREASURY_RECONCILIATION_VERIFIED");
            }
        }
        if let Ok(value) = Journal::open(&path, harness.epoch, 64)?
            .treasury_melt_report(&binding, issued.intent.id)
        {
            report = Some(value);
            break;
        }
    }
    let report = report.ok_or_else(|| anyhow::anyhow!("combined wallet did not reconcile"))?;
    assert_eq!(paused_after_loss, lost_reply);
    if lost_reply {
        assert!(!execution.execute);
    }
    assert_eq!(
        harness.mint.executions.load(Ordering::SeqCst),
        receipt_count
    );
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
    assert!(harness.mint.melt_settled.load(Ordering::SeqCst));
    for receipt in &receipts {
        let retained =
            Journal::open(&path, harness.epoch, 64)?.export_receipt(receipt.acquisition_id)?;
        let wire = crate::receipt_wire::from_private(bootstrap.wallet_instance, &retained)?;
        assert_eq!(
            wire.encode()?,
            crate::receipt_wire::WireReceipt::validate(receipt.clone())?.encode()?
        );
    }
    if fees {
        assert_eq!(
            receipts
                .iter()
                .map(|receipt| receipt.gross_sats)
                .sum::<u64>(),
            400 + 16 * u64::from(settle_change) + u64::try_from(receipt_count)?
        );
        assert_eq!(
            receipts
                .iter()
                .map(|receipt| receipt.receive_fee_sats)
                .sum::<u64>(),
            u64::try_from(receipt_count)?
        );
        assert_eq!(
            report
                .sources
                .iter()
                .map(|source| source.input_sats)
                .sum::<u64>(),
            400
        );
        assert_eq!(
            report
                .sources
                .iter()
                .map(|source| source.deposit_sats)
                .sum::<u64>(),
            deposit_sats
        );
        assert_eq!(
            report
                .sources
                .iter()
                .map(|source| source.fee_sats)
                .sum::<u64>(),
            conversion_fee_sats - u64::from(returned_change)
        );
        assert_eq!(
            report
                .sources
                .iter()
                .map(|source| source.change_sats)
                .sum::<u64>(),
            u64::from(returned_change)
        );
        println!("COMBINED_WALLET_REAL_FEES_CHANGE_VERIFIED");
        if !returned_change {
            println!("COMBINED_WALLET_FEE_ONLY_ZERO_CHANGE_VERIFIED");
        }
    }
    if !treasury_ipc {
        publish(&root, "report.json", &report)?;
    }
    let mut residual_report = None;
    if settle_change {
        ensure!(
            returned_change && treasury_ipc,
            "residual mode requires financial IPC and retained change"
        );
        // Complete the first operation through its ordinary cleanup action before
        // authorizing another melt. Never delete a saga/proof to bypass a fence.
        let worker = Worker::from_parts(
            Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 64)?)),
            database.clone(),
            writer.clone(),
            harness.keys.clone(),
            vec![(policy.clone(), harness.mint.clone())],
            execution.clone(),
            std::time::Duration::from_secs(10),
            65536,
        )?;
        let cleanup = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        for tick in 0..8 {
            // Round two may already be admitted. Stop at the original cleanup
            // checkpoint before any new quote can use the old fixture reply.
            let completed: u64 = cleanup.query_row(
                "SELECT COUNT(*) FROM treasury_cdk_cleanup WHERE intent_id=?",
                [issued.intent.id.to_string()],
                |row| row.get(0),
            )?;
            if completed == 1 {
                break;
            }
            worker.tick(now as i64 + 20 + tick).await?;
        }
        drop(worker);
        assert_eq!(
            cleanup.query_row(
                "SELECT COUNT(*) FROM treasury_cdk_cleanup WHERE intent_id=?",
                [issued.intent.id.to_string()],
                |r| r.get::<_, u64>(0)
            )?,
            1
        );
        drop(cleanup);
        let id: Uuid = read_when_ready(&root, "residual-ready.json").await?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
        let second = loop {
            if let Ok(value) =
                Journal::open(&path, harness.epoch, 64)?.treasury_instruction(&binding, id)
            {
                break value;
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "residual treasury admission deadline"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        };
        assert_eq!(
            (
                second.intent.input_sats,
                second.intent.deposit_sats,
                second.intent.maximum_fee_sats
            ),
            (17, 15, 2)
        );
        assert_eq!(second.intent.sources.len(), 2);
        assert!(second.intent.sources.iter().any(|s| s.acquisition_id
            == receipts.last().unwrap().acquisition_id
            && s.input_sats == 16));
        let quote = json!({"quote":"combined-residual-quote","amount":15,"fee_reserve":1,"state":"UNPAID",
            "expiry":now+240,"payment_preimage":null,"change":null,"request":second.invoice,"unit":"sat","method":"bolt11"});
        *harness.mint.quote_reply.lock().unwrap() = Some(quote.clone());
        let mut paid = quote;
        paid["state"] = "PAID".into();
        paid["payment_preimage"] = "38".repeat(32).into();
        paid["change"] = json!([]);
        *harness.mint.melt_paid_reply.lock().unwrap() = Some(paid);
        harness
            .mint
            .return_melt_change
            .store(false, Ordering::SeqCst);
        harness
            .mint
            .consume_melt_reserve
            .store(true, Ordering::SeqCst);
        harness.mint.melt_settled.store(false, Ordering::SeqCst);
        harness
            .mint
            .lose_melt_reply
            .store(lost_reply, Ordering::SeqCst);
        harness
            .mint
            .melt_status_unavailable
            .store(lost_reply, Ordering::SeqCst);
        execution.execute = true;
        let mut paused = false;
        for tick in 0..80 {
            let worker = Worker::from_parts(
                Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 64)?)),
                database.clone(),
                writer.clone(),
                harness.keys.clone(),
                vec![(policy.clone(), harness.mint.clone())],
                execution.clone(),
                std::time::Duration::from_secs(10),
                65536,
            )?;
            let outcome = worker.tick(now as i64 + 40 + tick).await?;
            drop(worker);
            if lost_reply && !paused && harness.mint.melt_settled.load(Ordering::SeqCst) {
                assert_eq!(outcome, crate::treasury_scheduler::Tick::RetryRetained);
                assert!(
                    Journal::open(&path, harness.epoch, 64)?
                        .treasury_melt_report(&binding, id)
                        .is_err()
                );
                let observed: Uuid =
                    read_when_ready(&root, "residual-provider-verified.json").await?;
                assert_eq!(observed, id);
                execution.execute = false;
                paused = true;
                harness
                    .mint
                    .melt_status_unavailable
                    .store(false, Ordering::SeqCst);
            }
            if let Ok(value) =
                Journal::open(&path, harness.epoch, 64)?.treasury_melt_report(&binding, id)
            {
                residual_report = Some(value);
                break;
            }
        }
        let second_report = residual_report
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("residual change did not settle"))?;
        assert_eq!(paused, lost_reply);
        assert_eq!(
            second_report
                .sources
                .iter()
                .map(|s| s.input_sats)
                .sum::<u64>(),
            17
        );
        assert_eq!(
            second_report
                .sources
                .iter()
                .map(|s| s.deposit_sats)
                .sum::<u64>(),
            15
        );
        assert_eq!(
            second_report
                .sources
                .iter()
                .map(|s| s.fee_sats)
                .sum::<u64>(),
            2
        );
        assert_eq!(
            second_report
                .sources
                .iter()
                .map(|s| s.change_sats)
                .sum::<u64>(),
            0
        );
        assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 3);
        assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 2);
        println!("COMBINED_WALLET_RESIDUAL_CHANGE_SETTLED");
    }
    if let Some(client) = &receipt_client {
        let _: Uuid = read_when_ready(&root, "credit-consumed.json").await?;
        for receipt in &receipts {
            client
                .deliver(
                    &delivery_journal,
                    bootstrap.wallet_instance,
                    receipt.acquisition_id,
                )
                .await?;
        }
        for (payload, capability) in &http_replays {
            let origin = bootstrap.http_origin.as_ref().unwrap();
            let status = http_client
                .post(format!("{origin}/api/v1/cashu/status"))
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&json!({"capability":capability}))?)
                .send()
                .await?;
            ensure!(
                status.status() == reqwest::StatusCode::OK
                    && fixture_json_response(status).await? == json!({"status":"credited"}),
                "HTTP status lost durable credit"
            );
            let replay = http_client
                .post(format!("{origin}/api/v1/cashu"))
                .header("content-type", "application/json")
                .body(serde_json::to_vec(payload)?)
                .send()
                .await?;
            ensure!(
                replay.status() == reqwest::StatusCode::ACCEPTED
                    && fixture_json_response(replay).await?
                        == json!({"status":"credited","capability":capability}),
                "consumed-credit HTTP replay changed receipt"
            );
            assert_eq!(
                harness.mint.executions.load(Ordering::SeqCst),
                receipt_count
            );
            println!("COMBINED_WALLET_PUBLIC_HTTP_FINANCIAL_IPC_VERIFIED");
        }
        publish(
            &root,
            "receipt-replayed.json",
            &u64::try_from(receipt_count)?,
        )?;
        println!("COMBINED_WALLET_CDK_RECEIPT_SOCKET_VERIFIED");
    }
    if lost_reply {
        println!("COMBINED_WALLET_LOST_REPLY_PAUSED_RECOVERY_VERIFIED");
    }
    if treasury_ipc {
        println!("COMBINED_WALLET_FULL_FINANCIAL_IPC_VERIFIED");
    }
    if let Some(server) = intake_server {
        server.abort();
    }
    if let Some(server) = treasury_server {
        server.abort();
    }
    if repeat_request {
        ensure!(
            http_replays.len() == 2,
            "two ordinary payments were not retained"
        );
        println!("COMBINED_WALLET_REUSABLE_REQUEST_TWO_PAYMENTS_VERIFIED");
    }
    if sdk_sender {
        println!("COMBINED_WALLET_CASHU_TS_SENDER_VERIFIED");
    }
    let health_db = rusqlite::Connection::open(&path)?;
    let health = crate::status::Funds::read(&health_db, None, i64::try_from(now)?)?;
    assert_eq!(
        health.acquired_net_sats,
        400 + 16 * u64::from(settle_change)
    );
    assert_eq!(health.unverified_input_sats, 0);
    assert_eq!(
        health.journal_custody_sats,
        u64::from(returned_change && !settle_change)
    );
    assert_eq!(health.pending_input_reserved_sats, 0);
    assert_eq!(health.ambiguous_submitted_input_sats, 0);
    assert_eq!(
        health.reported_treasury_deposit_sats,
        std::iter::once(&report)
            .chain(residual_report.iter())
            .flat_map(|r| &r.sources)
            .map(|s| s.deposit_sats)
            .sum::<u64>()
    );
    assert_eq!(
        health.receive_fee_sats,
        receipts.iter().map(|r| r.receive_fee_sats).sum::<u64>()
    );
    assert_eq!(
        health.completed_conversion_fee_sats,
        std::iter::once(&report)
            .chain(residual_report.iter())
            .flat_map(|r| &r.sources)
            .map(|s| s.fee_sats)
            .sum::<u64>()
    );
    assert_eq!(
        health.total_accounted_fee_sats,
        health.receive_fee_sats + health.completed_conversion_fee_sats
    );
    if funded_rollback {
        snapshot_funded_wallet(
            harness.directory.path(),
            &private.join("funded-after-effects"),
        )?;
        assert_restored_wallet_fenced(&private.join("funded-before-melt"))?;
        assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
    }
    println!("COMBINED_WALLET_HEALTH_ACCOUNTING_VERIFIED");
    if settle_change {
        println!("COMBINED_WALLET_THREE_ACQUISITIONS_TWO_MELTS");
    } else {
        println!("COMBINED_WALLET_TWO_ACQUISITIONS_ONE_MELT");
    }
    Ok(())
}
