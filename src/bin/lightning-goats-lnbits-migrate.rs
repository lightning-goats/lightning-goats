#![forbid(unsafe_code)]

use std::{
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use lightning_goats::{
    config::AppConfig,
    ledger::{LegacySettledInvoice, LegacySettlementOutcome, LedgerStore},
    secrets::read_systemd_credential,
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use zeroize::Zeroizing;

const PAGE_SIZE: u64 = 100;

#[derive(Debug, Parser)]
#[command(name = "lightning-goats-lnbits-migrate")]
#[command(about = "Temporary read-only LNbits cutover snapshot and reconciliation utility")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Capture a stable wallet balance + pending incoming invoice allowlist.
    Snapshot {
        #[arg(long)]
        lnbits_url: String,
        /// Unix timestamp recorded when production Lightning Address routing moved away from LNbits.
        #[arg(long)]
        cutover_at: i64,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 2)]
        stable_rounds: u32,
        #[arg(long, default_value_t = 2)]
        interval_seconds: u64,
        #[arg(long, default_value_t = 30)]
        max_rounds: u32,
    },
    /// Recheck only allowlisted legacy invoices and import newly paid ones into the ledger.
    Reconcile {
        #[arg(long)]
        lnbits_url: String,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long, default_value = "/etc/lightning-goats/config.toml")]
        config: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    wallet_id: String,
    opening_credit_sats: u64,
    cutover_at: i64,
    snapshot_at: i64,
    pending_invoices: Vec<PendingInvoiceFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PendingInvoiceFile {
    payment_hash: String,
    checking_id: Option<String>,
    wallet_id: String,
    amount_sats: u64,
    created_at: Option<i64>,
    expiry_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StableSnapshot {
    wallet_id: String,
    balance_msat: i64,
    pending_invoices: Vec<PendingInvoiceFile>,
}

#[derive(Debug, Deserialize)]
struct WalletResponse {
    id: String,
    balance_msat: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct LnbitsPayment {
    checking_id: String,
    payment_hash: String,
    wallet_id: String,
    amount: i64,
    status: String,
}

#[derive(Debug, Deserialize)]
struct PaymentPage {
    data: Vec<LnbitsPayment>,
    total: u64,
}

#[derive(Debug, Deserialize)]
struct PaymentStatusResponse {
    paid: bool,
    status: Option<String>,
    details: Option<LnbitsPayment>,
}

#[derive(Clone)]
struct LnbitsClient {
    client: Client,
    base_url: Url,
    invoice_key: Arc<Zeroizing<String>>,
}

impl LnbitsClient {
    async fn new(base_url: &str) -> Result<Self> {
        let base_url = validate_loopback_url(base_url)?;
        let invoice_key = read_systemd_credential("lnbits-invoice-key").await?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()
            .context("failed building LNbits migration HTTP client")?;
        Ok(Self {
            client,
            base_url,
            invoice_key: Arc::new(Zeroizing::new(invoice_key)),
        })
    }

    async fn wallet(&self) -> Result<WalletResponse> {
        let url = self.base_url.join("api/v1/wallet")?;
        self.client
            .get(url)
            .header("X-Api-Key", self.invoice_key.as_str())
            .send()
            .await
            .context("failed requesting LNbits wallet")?
            .error_for_status()
            .context("LNbits wallet request returned an error")?
            .json()
            .await
            .context("LNbits wallet response was invalid JSON")
    }

    async fn payments(&self) -> Result<Vec<LnbitsPayment>> {
        let mut offset = 0u64;
        let mut payments = Vec::new();

        loop {
            let mut url = self.base_url.join("api/v1/payments/paginated")?;
            url.query_pairs_mut()
                .append_pair("limit", &PAGE_SIZE.to_string())
                .append_pair("offset", &offset.to_string())
                .append_pair("recheck_pending", "true");
            let page: PaymentPage = self
                .client
                .get(url)
                .header("X-Api-Key", self.invoice_key.as_str())
                .send()
                .await
                .context("failed requesting LNbits payments")?
                .error_for_status()
                .context("LNbits payments request returned an error")?
                .json()
                .await
                .context("LNbits payments response was invalid JSON")?;

            let received = page.data.len() as u64;
            payments.extend(page.data);
            if payments.len() as u64 >= page.total {
                break;
            }
            if received == 0 {
                bail!(
                    "LNbits paginated payments returned an empty page before total={} was reached",
                    page.total
                );
            }
            offset = offset.saturating_add(received);
        }

        Ok(payments)
    }

    async fn payment_status(&self, payment_hash: &str) -> Result<PaymentStatusResponse> {
        validate_payment_hash(payment_hash)?;
        let url = self
            .base_url
            .join(&format!("api/v1/payments/{payment_hash}"))?;
        self.client
            .get(url)
            .header("X-Api-Key", self.invoice_key.as_str())
            .send()
            .await
            .with_context(|| format!("failed requesting LNbits payment {payment_hash}"))?
            .error_for_status()
            .with_context(|| format!("LNbits payment {payment_hash} returned an error"))?
            .json()
            .await
            .with_context(|| format!("LNbits payment {payment_hash} returned invalid JSON"))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Command::Snapshot {
            lnbits_url,
            cutover_at,
            output,
            stable_rounds,
            interval_seconds,
            max_rounds,
        } => {
            if cutover_at <= 0 {
                bail!("cutover_at must be a positive Unix timestamp");
            }
            if stable_rounds < 2 {
                bail!("stable_rounds must be at least 2");
            }
            if max_rounds < stable_rounds {
                bail!("max_rounds must be at least stable_rounds");
            }

            let client = LnbitsClient::new(&lnbits_url).await?;
            let snapshot = capture_stable_snapshot(
                &client,
                stable_rounds,
                max_rounds,
                Duration::from_secs(interval_seconds),
            )
            .await?;
            let snapshot_at = unix_now()?;
            if snapshot_at < cutover_at {
                bail!("stable snapshot was captured before the recorded cutover boundary");
            }
            let opening_credit_sats = exact_sats(snapshot.balance_msat, "LNbits wallet balance")?;
            let manifest = ManifestFile {
                wallet_id: snapshot.wallet_id,
                opening_credit_sats,
                cutover_at,
                snapshot_at,
                pending_invoices: snapshot.pending_invoices,
            };
            write_manifest_exclusive(&output, &manifest).await?;
            println!("manifest={}", output.display());
            println!("wallet_id={}", manifest.wallet_id);
            println!("opening_credit_sats={}", manifest.opening_credit_sats);
            println!("pending_invoices={}", manifest.pending_invoices.len());
            println!("snapshot_at={}", manifest.snapshot_at);
        }
        Command::Reconcile {
            lnbits_url,
            manifest,
            config,
        } => {
            let client = LnbitsClient::new(&lnbits_url).await?;
            let manifest: ManifestFile = read_json_file(&manifest).await?;
            let config = AppConfig::load(&config)?;
            let ledger = LedgerStore::connect(&config.database.url).await?;
            let mut credited = 0u64;
            let mut already_imported = 0u64;
            let mut still_pending = 0u64;
            let mut failed = 0u64;

            for pending in &manifest.pending_invoices {
                let status = client.payment_status(&pending.payment_hash).await?;
                if status.paid {
                    let details = status.details.with_context(|| {
                        format!(
                            "LNbits reported payment {} paid without authenticated details",
                            pending.payment_hash
                        )
                    })?;
                    verify_settled_details(&manifest, pending, &details)?;
                    let settled = LegacySettledInvoice {
                        payment_hash: details.payment_hash,
                        checking_id: Some(details.checking_id),
                        wallet_id: details.wallet_id,
                        amount_sats: exact_sats(details.amount, "legacy paid invoice amount")?,
                        settled_at: None,
                    };
                    match ledger.import_legacy_settlement(&settled).await? {
                        LegacySettlementOutcome::Credited { .. } => credited += 1,
                        LegacySettlementOutcome::AlreadyImported => already_imported += 1,
                    }
                } else if status.status.as_deref() == Some("failed") {
                    failed += 1;
                } else {
                    still_pending += 1;
                }
            }

            println!("credited={credited}");
            println!("already_imported={already_imported}");
            println!("still_pending={still_pending}");
            println!("failed={failed}");
            println!("feed_credit_sats={}", ledger.feed_credit_sats().await?);
        }
    }
    Ok(())
}

async fn capture_stable_snapshot(
    client: &LnbitsClient,
    stable_rounds: u32,
    max_rounds: u32,
    interval: Duration,
) -> Result<StableSnapshot> {
    let mut previous: Option<StableSnapshot> = None;
    let mut identical_rounds = 0u32;

    for round in 1..=max_rounds {
        let current = capture_snapshot(client).await?;
        if previous.as_ref() == Some(&current) {
            identical_rounds = identical_rounds.saturating_add(1);
        } else {
            identical_rounds = 1;
        }

        if identical_rounds >= stable_rounds {
            return Ok(current);
        }
        previous = Some(current);
        if round < max_rounds {
            sleep(interval).await;
        }
    }

    bail!(
        "LNbits wallet/payment state did not remain identical for {stable_rounds} consecutive rounds"
    )
}

async fn capture_snapshot(client: &LnbitsClient) -> Result<StableSnapshot> {
    let wallet = client.wallet().await?;
    exact_sats(wallet.balance_msat, "LNbits wallet balance")?;

    let payments = client.payments().await?;
    let mut pending_invoices = Vec::new();
    for payment in payments {
        if payment.wallet_id != wallet.id {
            bail!(
                "LNbits invoice-key response included payment {} from unexpected wallet {}",
                payment.payment_hash,
                payment.wallet_id
            );
        }
        if payment.amount <= 0 || payment.status != "pending" {
            continue;
        }
        validate_payment_hash(&payment.payment_hash)?;
        let amount_sats = exact_sats(payment.amount, "pending legacy invoice amount")?;
        pending_invoices.push(PendingInvoiceFile {
            payment_hash: payment.payment_hash,
            checking_id: Some(payment.checking_id),
            wallet_id: payment.wallet_id,
            amount_sats,
            created_at: None,
            expiry_at: None,
        });
    }
    pending_invoices.sort_by(|left, right| left.payment_hash.cmp(&right.payment_hash));
    for pair in pending_invoices.windows(2) {
        if pair[0].payment_hash == pair[1].payment_hash {
            bail!("LNbits returned duplicate pending payment hashes");
        }
    }

    Ok(StableSnapshot {
        wallet_id: wallet.id,
        balance_msat: wallet.balance_msat,
        pending_invoices,
    })
}

fn verify_settled_details(
    manifest: &ManifestFile,
    pending: &PendingInvoiceFile,
    details: &LnbitsPayment,
) -> Result<()> {
    if details.status != "success" {
        bail!(
            "LNbits reported payment {} paid but details status is {}",
            pending.payment_hash,
            details.status
        );
    }
    if details.wallet_id != manifest.wallet_id || details.wallet_id != pending.wallet_id {
        bail!("paid legacy invoice wallet does not match the cutover manifest");
    }
    if details.payment_hash != pending.payment_hash {
        bail!("paid legacy invoice hash does not match the allowlisted hash");
    }
    if pending.checking_id.as_deref() != Some(details.checking_id.as_str()) {
        bail!("paid legacy invoice checking_id does not match the cutover manifest");
    }
    if exact_sats(details.amount, "legacy paid invoice amount")? != pending.amount_sats {
        bail!("paid legacy invoice amount does not match the cutover manifest");
    }
    Ok(())
}

fn validate_loopback_url(raw: &str) -> Result<Url> {
    let mut url = Url::parse(raw).context("invalid LNbits URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("LNbits migration URL must use http:// or https://");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("LNbits credentials must not be embedded in the URL");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("LNbits base URL must not contain a query or fragment");
    }
    let host = url.host_str().context("LNbits URL has no host")?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false);
    if !loopback {
        bail!("LNbits migration URL must resolve explicitly to localhost/loopback");
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

fn exact_sats(msat: i64, field: &str) -> Result<u64> {
    if msat < 0 {
        bail!("{field} is unexpectedly negative");
    }
    if msat % 1_000 != 0 {
        bail!("{field} is not sat-aligned ({msat} msat); refusing lossy migration");
    }
    u64::try_from(msat / 1_000).context("satoshi amount exceeds supported range")
}

fn validate_payment_hash(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || hash.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        bail!("LNbits payment_hash must be canonical 64-character lowercase hex");
    }
    Ok(())
}

fn unix_now() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    i64::try_from(seconds).context("current Unix timestamp exceeds i64 range")
}

async fn write_manifest_exclusive(path: &Path, manifest: &ManifestFile) -> Result<()> {
    if path.exists() {
        bail!(
            "refusing to overwrite existing migration manifest {}",
            path.display()
        );
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("manifest output path has invalid filename")?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let json =
        serde_json::to_vec_pretty(manifest).context("failed serializing migration manifest")?;
    tokio::fs::write(&temporary, json)
        .await
        .with_context(|| format!("failed writing temporary manifest {}", temporary.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .await
            .context("failed restricting migration manifest permissions")?;
    }
    tokio::fs::rename(&temporary, path)
        .await
        .with_context(|| format!("failed atomically installing manifest {}", path.display()))?;
    Ok(())
}

async fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed reading {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed parsing JSON from {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_lnbits_url() {
        assert!(validate_loopback_url("https://lnbits.example.com/").is_err());
        assert!(validate_loopback_url("http://127.0.0.1:5000/").is_ok());
        assert!(validate_loopback_url("http://[::1]:5000/").is_ok());
    }

    #[test]
    fn exact_sats_rejects_subsat_state() {
        assert_eq!(exact_sats(2_000, "amount").unwrap(), 2);
        assert!(exact_sats(2_001, "amount").is_err());
        assert!(exact_sats(-1, "amount").is_err());
    }
}
