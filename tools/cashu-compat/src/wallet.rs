//! The wallet constructor always installs the constrained mint transport.

use crate::{mint_policy::MintPolicy, transport::MintTransport};
use anyhow::Result;
use cdk::{
    cdk_database::{self as database, WalletDatabase},
    mint_url::MintUrl,
    nuts::CurrencyUnit,
    wallet::{CustomHttpClient, Wallet, WalletBuilder},
};
use std::{str::FromStr, sync::Arc, time::Duration};

/// Construction makes no network calls. Actual acceptance still requires the
/// registry's per-transport policy gate and a serialized acquisition engine.
pub fn build_wallet(
    policy: &MintPolicy,
    storage: Arc<dyn WalletDatabase<database::Error> + Send + Sync>,
    seed: [u8; 64],
    operation_timeout: Duration,
    message_limit: usize,
) -> Result<Wallet> {
    let mint_url = MintUrl::from_str(&policy.advertised_url)?;
    let transport = MintTransport::new(
        url::Url::parse(&policy.advertised_url)?,
        operation_timeout,
        message_limit,
    )?;
    let client = CustomHttpClient::with_transport(mint_url.clone(), transport, None);
    Ok(WalletBuilder::new()
        .mint_url(mint_url)
        .unit(CurrencyUnit::Sat)
        .localstore(storage)
        .seed(seed)
        .client(client)
        .use_http_subscription()
        .build()?)
}

/// Receiving workers must retain the returned gate and arm one admitted operation
/// before invoking receive_proofs. The gate denies melts and unbound swaps.
#[allow(clippy::too_many_arguments)]
pub fn build_journaled_wallet(
    policy: &MintPolicy,
    storage: Arc<dyn WalletDatabase<database::Error> + Send + Sync>,
    seed: [u8; 64],
    operation_timeout: Duration,
    message_limit: usize,
    journal: Arc<std::sync::Mutex<crate::journal::Journal>>,
    writer: Arc<crate::writer_lock::WriterLock>,
    keys: Arc<crate::intake::PinnedKeys>,
) -> Result<(Wallet, crate::operation_gate::OperationGate<MintTransport>)> {
    let mint_url = MintUrl::from_str(&policy.advertised_url)?;
    let transport = MintTransport::new(
        url::Url::parse(&policy.advertised_url)?,
        operation_timeout,
        message_limit,
    )?;
    let gate =
        crate::operation_gate::OperationGate::new(transport, journal, writer, message_limit)?
            .with_wallet_database(storage.clone())
            .with_key_policy(policy.id.clone(), keys);
    let client = CustomHttpClient::with_transport(mint_url.clone(), gate.clone(), None);
    let wallet = WalletBuilder::new()
        .mint_url(mint_url)
        .unit(CurrencyUnit::Sat)
        .localstore(storage)
        .seed(seed)
        .client(client)
        .use_http_subscription()
        .build()?;
    Ok((wallet, gate))
}

/// Opens only private wallet storage; core ledger access is not part of this API.
pub async fn open_wallet_storage(
    directory: &std::path::Path,
) -> Result<Arc<cdk_sqlite::WalletSqliteDatabase>> {
    let path = directory.join("wallet.sqlite");
    let _private_file = crate::writer_lock::private_state_file(&path)?;
    Ok(Arc::new(cdk_sqlite::WalletSqliteDatabase::new(path).await?))
}

pub(crate) fn mint_store_filename(mint_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("lightning-goats/cdk-mint-store/v1\0{mint_id}"));
    format!("mint-{}.sqlite", cdk::util::hex::encode(hash))
}

pub(crate) fn require_mint_store_binding(
    connection: &rusqlite::Connection,
    mint_id: &str,
    mint_url: &str,
) -> Result<()> {
    let retained: (String, String) = connection.query_row(
        "SELECT mint_id,mint_url FROM lg_mint_namespace WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    anyhow::ensure!(
        (retained.0.as_str(), retained.1.as_str()) == (mint_id, mint_url),
        "mint storage namespace binding changed"
    );
    Ok(())
}

/// A CDK proof store belongs to exactly one configured mint. CDK's proof primary
/// key is Y, so sharing this store across issuers can overwrite a pending proof.
/// The common application writer/journal remain the cross-mint authority.
pub async fn open_mint_wallet_storage(
    directory: &std::path::Path,
    policy: &MintPolicy,
    _writer: &crate::writer_lock::WriterLock,
    initialize: bool,
) -> Result<Arc<cdk_sqlite::WalletSqliteDatabase>> {
    use anyhow::ensure;
    crate::private_ipc::private_state(directory)?;
    let path = directory.join(mint_store_filename(&policy.id));
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => ensure!(
            !initialize && meta.is_file() && meta.len() > 0,
            "mint store conflicts with startup mode"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                initialize,
                "mint store missing; reconcile retained namespace"
            );
        }
        Err(error) => return Err(error.into()),
    }
    if initialize {
        for suffix in ["-wal", "-shm", "-journal"] {
            match std::fs::symlink_metadata(format!("{}{suffix}", path.display())) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => anyhow::bail!("orphan mint-store sidecar requires reconciliation"),
            }
        }
    }
    let _private_file = crate::writer_lock::private_state_file(&path)?;
    {
        // Bind before CDK migrations; retain partial initialization on failure.
        let flags =
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW;
        let mut connection = rusqlite::Connection::open_with_flags(&path, flags)?;
        connection.execute_batch("PRAGMA synchronous=FULL;")?;
        if initialize {
            connection.execute_batch("CREATE TABLE lg_mint_namespace(singleton INTEGER PRIMARY KEY CHECK(singleton=1),mint_id TEXT NOT NULL,mint_url TEXT NOT NULL);")?;
            connection.execute(
                "INSERT INTO lg_mint_namespace VALUES(1,?,?)",
                rusqlite::params![policy.id, policy.advertised_url],
            )?;
        }
        let tx = connection.transaction()?;
        require_mint_store_binding(&tx, &policy.id, &policy.advertised_url)?;
        tx.commit()?;
    }
    Ok(Arc::new(cdk_sqlite::WalletSqliteDatabase::new(path).await?))
}
