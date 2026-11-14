//! TEST ONLY: new synthetic historical journal for a disposable CI installation.
//! Never packaged as a production binary; refuses root, existing files and paths
//! outside /run. It contains no real payment, key, food action or provider call.
#![forbid(unsafe_code)]
use anyhow::{Context, Result, bail};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().context("new /run fixture path")?);
    let paid = match args.next().as_deref() {
        None => false,
        Some("--paid-fixture") => true,
        _ => bail!("unknown test-only fixture option"),
    };
    if args.next().is_some() {
        bail!("unexpected test-only fixture argument");
    }
    let parent = path.parent().context("fixture parent")?;
    if tokio::net::UnixStream::pair()?.0.peer_cred()?.uid() == 0
        || !path.starts_with("/run")
        || path.file_name().is_none()
        || parent.canonicalize()? != parent
    {
        bail!("fixture requires a non-root UID and a new canonical /run file");
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .foreign_keys(true),
        )
        .await?;
    let mut migrations = sqlx::migrate!("./migrations");
    migrations.migrations.to_mut().retain(|m| m.version <= 13);
    migrations.run(&pool).await?;
    let epoch = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO overlay_identity(singleton,stream_id) VALUES(1,?)")
        .bind(epoch.to_string())
        .execute(&pool)
        .await?;
    let pending = if paid {
        use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
        use lightning_goats::nostr::SignedNostrEvent;
        use sha2::{Digest as _, Sha256};
        let secp = Secp256k1::new();
        let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[1; 32])?);
        let mut signed = SignedNostrEvent {
            id: String::new(),
            pubkey: key.x_only_public_key().0.to_string(),
            created_at: 100,
            kind: 1,
            tags: vec![],
            content: "TEST ONLY: retain exact original signed bytes; never publish".into(),
            sig: String::new(),
        };
        let digest: [u8; 32] =
            Sha256::digest(lightning_goats::nostr_event::canonical_bytes(&signed)?).into();
        signed.id = hex::encode(digest);
        signed.sig = secp
            .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key)
            .to_string();
        let raw = serde_json::to_string(&signed)?;
        lightning_goats::nostr_event::VerifiedEvent::from_json(&raw)?;
        sqlx::query("INSERT INTO settled_payments(source,source_id,address_user,credit_pool,amount_msat) VALUES('strike','TEST-ONLY-original-receipt','herd','herd',2340000)").execute(&pool).await?;
        sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats,payment_source,payment_source_id,address_user,credit_pool) VALUES('CREDIT','TEST-ONLY-original-receipt',2340,'strike','TEST-ONLY-original-receipt','herd','herd')").execute(&pool).await?;
        let pending = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO feed_attempts(id,status,threshold_sats) VALUES(?,'intent_committed',1000)",
        )
        .bind(pending.to_string())
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO event_log(event_type,payload_json) VALUES('payment_received','{}')",
        )
        .execute(&pool)
        .await?;
        sqlx::query("INSERT INTO message_outbox(event_id,signed_event_json,status,source_event_seq) VALUES(?,?,'pending',1)")
            .bind(&signed.id).bind(raw).execute(&pool).await?;
        Some(pending)
    } else {
        None
    };
    let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await?;
    if engine != "3.54.1" {
        bail!("fixture used a different native engine");
    }
    pool.close().await;
    println!(
        "{}",
        serde_json::json!({"synthetic":true,"epoch":epoch,"engine":engine,"payments":u8::from(paid),"pending_feed":pending})
    );
    Ok(())
}
