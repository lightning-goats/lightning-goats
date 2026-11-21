#![forbid(unsafe_code)]

mod provider;
mod store;

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use breez_sdk_spark::BreezSdk;
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescriptionRef, Currency};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env, fs,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path as FilePath, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::UnixListener,
    signal::unix::{SignalKind, signal},
    sync::watch,
};
use uuid::Uuid;

use sha2::{Digest, Sha256};
use store::{Admission, Journal};

const INVOICE_MAX_SAT: u64 = 1_000_000;
const MELT_MAX_SAT: u64 = INVOICE_MAX_SAT;
const FEE_MAX_SAT: u64 = 100;

struct Wallet {
    sdk: BreezSdk,
    identity: String,
    network: &'static str,
    journal: Mutex<Journal>,
    spending_enabled: bool,
}

type Shared = Arc<Wallet>;
type Api = Result<Json<Value>, ApiError>;

struct ApiError(StatusCode, &'static str);
const INVALID: ApiError = ApiError(StatusCode::BAD_REQUEST, "invalid request");
const MISSING: ApiError = ApiError(StatusCode::NOT_FOUND, "unknown operation");
const CONFLICT: ApiError = ApiError(
    StatusCode::CONFLICT,
    "operation conflicts with existing intent",
);
const HELD: ApiError = ApiError(
    StatusCode::SERVICE_UNAVAILABLE,
    "operation held for reconciliation",
);
const UNAVAILABLE: ApiError = ApiError(StatusCode::SERVICE_UNAVAILABLE, "reserve unavailable");

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Create {
    operation_id: Uuid,
    amount_sat: u64,
    expiry_secs: u32,
    description: Description,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "purpose", rename_all = "snake_case", deny_unknown_fields)]
enum Description {
    MintQuote {
        memo: String,
    },
    ProjectQuote {
        description_hash: String,
        claim_pubkey: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InvoiceInput {
    invoice: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Pay {
    operation_id: Uuid,
    invoice: String,
    max_fee_sat: u64,
}

#[derive(Clone, Deserialize, Serialize)]
struct Invoice {
    invoice: String,
    payment_hash: String,
    lookup_reference: String,
    amount_sat: u64,
    expires_at: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("valid clock")
        .as_secs()
}

fn is_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_invoice(raw: &str, network: &str) -> Result<Bolt11Invoice> {
    ensure!(
        raw.len() <= 4096 && raw == raw.to_ascii_lowercase(),
        "invoice encoding"
    );
    let invoice: Bolt11Invoice = raw.parse()?;
    let currency = match network {
        "mainnet" => Currency::Bitcoin,
        "regtest" => Currency::Regtest,
        _ => anyhow::bail!("unsupported network"),
    };
    ensure!(invoice.currency() == currency, "invoice network");
    let amount = invoice.amount_milli_satoshis().context("missing amount")?;
    ensure!(
        amount > 0 && amount % 1000 == 0,
        "expected positive whole sats"
    );
    Ok(invoice)
}

fn check_created(
    request: &Create,
    raw: String,
    reference: String,
    network: &str,
) -> Result<Invoice> {
    let parsed = parse_invoice(&raw, network)?;
    ensure!(
        parsed.amount_milli_satoshis() == Some(request.amount_sat * 1000),
        "invoice amount"
    );
    ensure!(
        parsed.expiry_time().as_secs() == u64::from(request.expiry_secs),
        "invoice expiry"
    );
    let expires_at = parsed
        .expires_at()
        .context("invoice expiry overflow")?
        .as_secs();
    ensure!(
        expires_at > now() && parsed.duration_since_epoch().as_secs() <= now().saturating_add(30),
        "invoice timestamp"
    );
    ensure!(
        !reference.is_empty() && reference.len() <= 256,
        "invalid receive reference"
    );
    match (&request.description, parsed.description()) {
        (Description::MintQuote { memo }, Bolt11InvoiceDescriptionRef::Direct(description)) => {
            ensure!(description.to_string() == *memo, "invoice memo");
        }
        (
            Description::ProjectQuote {
                description_hash, ..
            },
            Bolt11InvoiceDescriptionRef::Hash(hash),
        ) => {
            ensure!(
                hash.0.to_string() == *description_hash,
                "invoice description hash"
            );
        }
        _ => anyhow::bail!("invoice description type"),
    }
    Ok(Invoice {
        invoice: raw,
        payment_hash: parsed.payment_hash().to_string(),
        lookup_reference: reference,
        amount_sat: request.amount_sat,
        expires_at,
    })
}

async fn create(State(wallet): State<Shared>, Json(request): Json<Create>) -> Api {
    if request.operation_id.is_nil()
        || !(1..=INVOICE_MAX_SAT).contains(&request.amount_sat)
        || !(60..=3600).contains(&request.expiry_secs)
        || match &request.description {
            Description::MintQuote { memo } => memo.is_empty() || memo.len() > 256,
            Description::ProjectQuote {
                description_hash,
                claim_pubkey,
            } => {
                !is_hash(description_hash)
                    || claim_pubkey.get(2..).is_none_or(|x| !is_hash(x))
                    || !(claim_pubkey.starts_with("02") || claim_pubkey.starts_with("03"))
            }
        }
    {
        return Err(INVALID);
    }
    // Only one handler can commit a new intent. Cancellation after this commit leaves a hold.
    let existing = wallet.with_journal(|journal| journal.admit_receive(&request))?;
    let invoice = match existing {
        Admission::Known(invoice) => invoice,
        Admission::Held => return Err(HELD),
        Admission::New => {
            let received = provider::create(&wallet.sdk, &request)
                .await
                .map_err(|_| HELD)?;
            let invoice = check_created(&request, received.0, received.1, wallet.network)
                .map_err(|_| HELD)?;
            wallet
                .with_journal(|journal| journal.record_receive(request.operation_id, &invoice))?;
            invoice
        }
    };
    Ok(Json(json!({
        "operation_id": request.operation_id, "invoice": invoice.invoice, "wallet": wallet.identity,
        "network": wallet.network, "amount_sat": invoice.amount_sat, "expires_at": invoice.expires_at,
        "payment_hash": invoice.payment_hash, "lookup_reference": invoice.lookup_reference,
    })))
}

async fn receive_status(State(wallet): State<Shared>, Path(hash): Path<String>) -> Api {
    if !is_hash(&hash) {
        return Err(INVALID);
    }
    let (invoice, saved) = wallet
        .with_journal(|journal| journal.receive(&hash))?
        .ok_or(MISSING)?;
    if let Some(settlement) = saved {
        return Ok(Json(settlement));
    }
    if let Some(settlement) = provider::receive_status(&wallet.sdk, &invoice)
        .await
        .map_err(|_| UNAVAILABLE)?
    {
        wallet.with_journal(|journal| journal.record_receive_settlement(&hash, &settlement))?;
        return Ok(Json(settlement));
    }
    Ok(Json(json!({"state": "pending", "payment_hash": hash})))
}

fn melt_invoice(raw: &str, network: &str) -> Result<(String, u64), ApiError> {
    let invoice = parse_invoice(raw, network).map_err(|_| INVALID)?;
    let amount = invoice.amount_milli_satoshis().ok_or(INVALID)? / 1000;
    if amount > MELT_MAX_SAT {
        return Err(INVALID);
    }
    Ok((invoice.payment_hash().to_string(), amount))
}

async fn estimate(State(wallet): State<Shared>, Json(request): Json<InvoiceInput>) -> Api {
    let (hash, amount) = melt_invoice(&request.invoice, wallet.network)?;
    let fee = provider::prepare(&wallet.sdk, &request.invoice, amount)
        .await
        .map_err(|_| UNAVAILABLE)?
        .1;
    if fee > FEE_MAX_SAT {
        return Err(INVALID);
    }
    Ok(Json(
        json!({"payment_hash": hash, "amount_sat": amount, "fee_sat": fee}),
    ))
}

async fn pay(State(wallet): State<Shared>, Json(request): Json<Pay>) -> Api {
    let (hash, amount) = melt_invoice(&request.invoice, wallet.network)?;
    if request.operation_id.is_nil() || request.max_fee_sat > FEE_MAX_SAT {
        return Err(INVALID);
    }
    let admission = wallet.with_journal(|journal| journal.send(&request, &hash, false))?;
    match admission {
        Admission::Known(settlement) => return Ok(Json(settlement)),
        Admission::Held => return reconcile_send(&wallet, &request).await,
        Admission::New => {}
    }
    if !wallet.spending_enabled {
        return Err(UNAVAILABLE);
    }
    if parse_invoice(&request.invoice, wallet.network)
        .map_err(|_| INVALID)?
        .is_expired()
    {
        return Err(INVALID);
    }
    let (prepared, quoted_fee) = provider::prepare(&wallet.sdk, &request.invoice, amount)
        .await
        .map_err(|_| UNAVAILABLE)?;
    if quoted_fee > request.max_fee_sat {
        return Err(INVALID);
    }
    // Store the exact operation and hash before the first effectful SDK call.
    let admission = wallet.with_journal(|journal| journal.send(&request, &hash, true))?;
    match admission {
        Admission::Known(settlement) => return Ok(Json(settlement)),
        Admission::Held => return reconcile_send(&wallet, &request).await,
        Admission::New => {}
    }
    if let Ok(payment) = provider::pay(&wallet.sdk, prepared, request.operation_id).await {
        return verify_send(&wallet, &request, &payment);
    }
    reconcile_send(&wallet, &request).await
}

async fn send_status(State(wallet): State<Shared>, Path(operation): Path<Uuid>) -> Api {
    let (request, saved) = wallet
        .with_journal(|journal| journal.lookup_send(operation))?
        .ok_or(MISSING)?;
    if let Some(settlement) = saved {
        return Ok(Json(settlement));
    }
    reconcile_send(&wallet, &request).await
}

async fn reconcile_send(wallet: &Wallet, request: &Pay) -> Api {
    let Some(payment) = provider::outgoing(&wallet.sdk, request.operation_id).await else {
        return Ok(Json(
            json!({"state": "pending", "operation_id": request.operation_id}),
        ));
    };
    verify_send(wallet, request, &payment)
}

fn verify_send(wallet: &Wallet, request: &Pay, payment: &breez_sdk_spark::Payment) -> Api {
    let (hash, amount) = melt_invoice(&request.invoice, wallet.network)?;
    let result = provider::send_status(request, payment, &hash, amount).map_err(|_| HELD)?;
    if result["state"] == "settled" || result["state"] == "failed" {
        wallet.with_journal(|journal| journal.record_send(request.operation_id, &result))?;
    }
    Ok(Json(result))
}

async fn health(State(wallet): State<Shared>) -> Api {
    Ok(Json(
        json!({"state": "ready", "wallet": wallet.identity, "network": wallet.network}),
    ))
}

async fn balance(State(wallet): State<Shared>) -> Api {
    let info = wallet
        .sdk
        .get_info(breez_sdk_spark::GetInfoRequest {
            ensure_synced: Some(false),
        })
        .await
        .map_err(|_| UNAVAILABLE)?;
    if info.last_sync_error.is_some()
        || info
            .last_synced_at
            .is_none_or(|at| now().saturating_sub(at) > 750)
    {
        return Err(UNAVAILABLE);
    }
    Ok(Json(
        json!({"wallet": wallet.identity, "network": wallet.network, "balance_sat": info.balance_sats,
                   "last_synced_at": info.last_synced_at, "spending_enabled": wallet.spending_enabled}),
    ))
}

impl Wallet {
    fn with_journal<T>(
        &self,
        action: impl FnOnce(&mut Journal) -> Result<T, store::Failure>,
    ) -> Result<T, ApiError> {
        let mut journal = self.journal.lock().map_err(|_| UNAVAILABLE)?;
        action(&mut journal).map_err(|error| match error {
            store::Failure::Conflict => CONFLICT,
            store::Failure::Store => UNAVAILABLE,
        })
    }
}

fn socket(path: &str) -> Result<UnixListener> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        ensure!(
            meta.file_type().is_socket(),
            "reserved path is not a socket"
        );
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    // Parent directories are controlled by systemd tmpfiles and determine role access.
    fs::set_permissions(path, fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

fn credential(dir: &FilePath, name: &str) -> Result<String> {
    let text =
        fs::read_to_string(dir.join(name)).with_context(|| format!("missing credential {name}"))?;
    ensure!(!text.trim().is_empty(), "empty credential {name}");
    Ok(text.trim().to_owned())
}

#[tokio::main]
async fn main() -> Result<()> {
    let state = PathBuf::from(env::var("STATE_DIRECTORY")?);
    let credentials = PathBuf::from(env::var("CREDENTIALS_DIRECTORY")?);
    let network = match env::var("LG_RESERVE_NETWORK")?.as_str() {
        "mainnet" => "mainnet",
        "regtest" => "regtest",
        _ => anyhow::bail!("unsupported reserve network"),
    };
    let spending_enabled = match env::var("LG_RESERVE_SPEND").as_deref() {
        Ok("yes") if network == "mainnet" => true,
        Ok("no") => false,
        _ => anyhow::bail!("invalid reserve spending policy"),
    };
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("owner.lock"))?;
    lock.try_lock()
        .context("reserve already owned by another process")?;
    let seed = credential(&credentials, "reserve-seed")?;
    let journal = Journal::open(
        &state,
        network,
        &hex::encode(Sha256::digest(seed.as_bytes())),
    )?;
    let sdk = provider::connect(
        &state,
        network,
        seed,
        if network == "mainnet" {
            Some(credential(&credentials, "breez-api-key")?)
        } else {
            None
        },
    )
    .await?;
    let info = sdk
        .get_info(breez_sdk_spark::GetInfoRequest {
            ensure_synced: Some(false),
        })
        .await?;
    journal.bind_wallet(&info.identity_pubkey)?;
    let wallet = Arc::new(Wallet {
        sdk,
        identity: info.identity_pubkey,
        network,
        journal: Mutex::new(journal),
        spending_enabled,
    });
    let receive = Router::new()
        .route("/health", get(health))
        .route("/v1/invoices", post(create))
        .route("/v1/invoices/{hash}", get(receive_status));
    let fund = receive
        .clone()
        .route("/v1/funding/balance", get(balance))
        .route("/v1/funding/fee", post(estimate))
        .route("/v1/funding/payments", post(pay))
        .route("/v1/funding/payments/{operation}", get(send_status));
    let receive_listener = socket(&env::var("LG_RESERVE_RECEIVE_SOCKET")?)?;
    let fund_listener = socket(&env::var("LG_RESERVE_FUND_SOCKET")?)?;
    let (stop, done) = watch::channel(false);
    let mut terminate = signal(SignalKind::terminate())?;
    let mut done_receive = done.clone();
    let mut done_fund = done;
    tokio::try_join!(
        async move {
            terminate.recv().await;
            let _ = stop.send(true);
            Ok::<(), std::io::Error>(())
        },
        axum::serve(receive_listener, receive.with_state(wallet.clone())).with_graceful_shutdown(
            async move {
                let _ = done_receive.wait_for(|stop| *stop).await;
            }
        ),
        axum::serve(fund_listener, fund.with_state(wallet.clone())).with_graceful_shutdown(
            async move {
                let _ = done_fund.wait_for(|stop| *stop).await;
            }
        ),
    )?;
    wallet.sdk.disconnect().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{
        hashes::{Hash, sha256},
        secp256k1::{Secp256k1, SecretKey},
    };
    use lightning_invoice::{InvoiceBuilder, PaymentSecret};

    fn signed_invoice(description: sha256::Hash) -> String {
        let key = SecretKey::from_slice(&[7; 32]).unwrap();
        InvoiceBuilder::new(Currency::Bitcoin)
            .description_hash(description)
            .payment_hash(sha256::Hash::hash(&[8; 32]))
            .payment_secret(PaymentSecret([9; 32]))
            .duration_since_epoch(std::time::Duration::from_secs(now()))
            .min_final_cltv_expiry_delta(18)
            .amount_milli_satoshis(21_000)
            .expiry_time(std::time::Duration::from_secs(900))
            .build_signed(|message| Secp256k1::new().sign_ecdsa_recoverable(message, &key))
            .unwrap()
            .to_string()
    }

    #[test]
    fn exact_signed_description_and_invoice_fields() {
        let metadata = b"[[\"text/plain\",\"Lightning Goats\"]]";
        // A signed zap envelope contains significant whitespace in its outer encoding.
        let zap_bytes = br#"{ "kind":9734,  "content":"", "sig":"fixture-checked-in-mint-test" }"#;
        for bytes in [metadata.as_slice(), zap_bytes.as_slice()] {
            let digest = sha256::Hash::hash(bytes);
            let fields = json!({"operation_id": Uuid::new_v4(), "amount_sat": 21, "expiry_secs": 900,
                "description": {"purpose": "project_quote", "description_hash": digest.to_string(),
                "claim_pubkey": format!("02{}", "a".repeat(64))}});
            let mut request: Create = serde_json::from_value(fields).unwrap();
            let raw = signed_invoice(digest);
            assert!(
                check_created(&request, raw.clone(), "ssp-reference".into(), "mainnet").is_ok()
            );
            assert!(
                check_created(&request, raw.clone(), "ssp-reference".into(), "regtest").is_err()
            );
            request.amount_sat = 22;
            assert!(
                check_created(&request, raw.clone(), "ssp-reference".into(), "mainnet").is_err()
            );
            request.amount_sat = 21;
            request.expiry_secs = 899;
            assert!(
                check_created(&request, raw.clone(), "ssp-reference".into(), "mainnet").is_err()
            );
            request.expiry_secs = 900;
            assert!(
                check_created(
                    &request,
                    signed_invoice(sha256::Hash::hash(b"changed")),
                    "ssp-reference".into(),
                    "mainnet"
                )
                .is_err()
            );
        }
    }

    #[test]
    fn journal_replays_and_holds_across_restart() {
        let home = tempfile::tempdir().unwrap();
        let mut request = Create {
            operation_id: Uuid::new_v4(),
            amount_sat: 21,
            expiry_secs: 900,
            description: Description::MintQuote {
                memo: "original".to_owned(),
            },
        };
        let mut db = Journal::open(home.path(), "mainnet", "seed").unwrap();
        db.bind_wallet("first wallet").unwrap();
        assert!(db.bind_wallet("other wallet").is_err());
        assert!(matches!(db.admit_receive(&request), Ok(Admission::New)));
        drop(db);
        assert!(Journal::open(home.path(), "regtest", "seed").is_err());
        let mut db = Journal::open(home.path(), "mainnet", "seed").unwrap();
        assert!(matches!(db.admit_receive(&request), Ok(Admission::Held)));
        request.amount_sat += 1;
        assert!(matches!(
            db.admit_receive(&request),
            Err(store::Failure::Conflict)
        ));
        request.amount_sat -= 1;
        let receive = Invoice {
            invoice: "signed fixture".into(),
            payment_hash: "receive-hash".into(),
            lookup_reference: "ssp-reference".into(),
            amount_sat: 21,
            expires_at: 1000,
        };
        db.record_receive(request.operation_id, &receive).unwrap();
        let receipt = json!({"state":"settled", "transfer_id":"original receive transfer"});
        db.record_receive_settlement("receive-hash", &receipt)
            .unwrap();
        let mut spend = Pay {
            operation_id: Uuid::new_v4(),
            invoice: "signed destination".into(),
            max_fee_sat: 2,
        };
        assert!(matches!(
            db.send(&spend, "receive-hash", true),
            Err(store::Failure::Conflict)
        ));
        assert!(matches!(
            db.send(&spend, "destination-hash", true),
            Ok(Admission::New)
        ));
        drop(db);
        let mut db = Journal::open(home.path(), "mainnet", "seed").unwrap();
        assert!(
            matches!(db.admit_receive(&request), Ok(Admission::Known(invoice)) if invoice.lookup_reference == "ssp-reference")
        );
        assert_eq!(
            db.receive("receive-hash").unwrap().unwrap().1,
            Some(receipt)
        );
        assert!(matches!(
            db.send(&spend, "destination-hash", true),
            Ok(Admission::Held)
        ));
        db.record_send(spend.operation_id, &json!({"state": "settled"}))
            .unwrap();
        assert!(matches!(
            db.send(&spend, "destination-hash", true),
            Ok(Admission::Known(_))
        ));
        spend.operation_id = Uuid::new_v4();
        assert!(matches!(
            db.send(&spend, "destination-hash", true),
            Err(store::Failure::Conflict)
        ));
    }
}
