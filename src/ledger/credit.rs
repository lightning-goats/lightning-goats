//! Private asset receipt/provenance storage. No HTTP or wallet authority lives here.
//!
//! Callers must supply authenticated, current observations bound to a canonical
//! receive scope; callbacks are NOT observations. #97/#98 provide that boundary.
//! Receipt keys identify the provider's aggregation unit within ONE receive scope,
//! not a project-wide transaction hash. Pending/late funds are retained, not spent.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sqlx::{FromRow, Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{LedgerStore, append_event_in_transaction, feed_credit_in_transaction, to_i64, to_u64};
use crate::domain::credit::{
    Asset, AssetAmount, CreditTerms, MAX_ACCOUNTING_UNITS, XmrBtcRate, XmrQuote,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmrNetwork {
    Mainnet,
    Testnet,
    Stagenet,
}
impl XmrNetwork {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Testnet => "testnet",
            Self::Stagenet => "stagenet",
        }
    }
}

/// Private input: do not serialize this into an overlay/status/notification.
/// account_scope and receive_scope must be canonical, stable provider bindings.
/// Receive scope is the exact subaddress, not a caller-selected alias for it.
#[derive(Clone)]
pub struct XmrCreditIntent {
    pub id: Uuid,
    pub provider: String,
    pub network: XmrNetwork,
    pub account_scope: String,
    pub receive_scope: String,
    pub address_user: String,
    pub credit_pool: String,
    pub quote: XmrQuote,
    pub max_credit_sats: u64,
}

/// A current per-receipt observation, NOT a callback or whole-wallet total.
/// first_seen_at must come from the bridge's persisted trusted read history.
#[derive(Clone)]
pub struct XmrReceiptObservation {
    pub provider: String,
    pub network: XmrNetwork,
    pub account_scope: String,
    pub receive_scope: String,
    pub receipt_key: String,
    pub amount: AssetAmount,
    pub first_seen_at: i64,
    pub unlocked: bool,
    pub double_spend_seen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreditHold {
    ReceiptConflict,
    FinalityRegression,
    DoubleSpend,
    AllocationMismatch,
    CreditLimit,
    ProviderInconsistency,
}
impl CreditHold {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReceiptConflict => "receipt_conflict",
            Self::FinalityRegression => "finality_regression",
            Self::DoubleSpend => "double_spend",
            Self::AllocationMismatch => "allocation_mismatch",
            Self::CreditLimit => "credit_limit",
            Self::ProviderInconsistency => "provider_inconsistency",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreditReceiptOutcome {
    Duplicate,
    Pending,
    Held,
    Recorded {
        delta_sats: u64,
        feed_credit_sats: u64,
    },
}

/// Administrative view only; absence of a hold does not attest provider sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditAllocation {
    pub eligible_atomic: u64,
    pub credited_sats: u64,
    pub hold_reason: Option<String>,
}

#[derive(FromRow, PartialEq, Eq)]
struct Valuation {
    id: String,
    asset: String,
    network: String,
    provider: String,
    account_scope: String,
    receive_scope: String,
    address_user: String,
    credit_pool: String,
    expected_atomic: i64,
    target_sats: i64,
    max_credit_sats: i64,
    rate_numerator: Option<String>,
    rate_denominator: Option<String>,
    rate_source: Option<String>,
    rate_observed_at: Option<i64>,
    issued_at: Option<i64>,
    expires_at: Option<i64>,
    policy_version: String,
}
impl Valuation {
    async fn insert(&self, tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
        sqlx::query("INSERT INTO credit_valuations (id,asset,network,provider,account_scope,receive_scope,address_user,credit_pool,expected_atomic,target_sats,max_credit_sats,rate_numerator,rate_denominator,rate_source,rate_observed_at,issued_at,expires_at,policy_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&self.id).bind(&self.asset).bind(&self.network).bind(&self.provider)
            .bind(&self.account_scope).bind(&self.receive_scope).bind(&self.address_user)
            .bind(&self.credit_pool).bind(self.expected_atomic).bind(self.target_sats)
            .bind(self.max_credit_sats).bind(&self.rate_numerator).bind(&self.rate_denominator)
            .bind(&self.rate_source).bind(self.rate_observed_at).bind(self.issued_at)
            .bind(self.expires_at).bind(&self.policy_version).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO credit_allocations (valuation_id) VALUES (?)")
            .bind(&self.id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}

/// Intentionally an allowlisted public projection, not receipt serialization.
/// amount_sats is the incremental credit grant, for existing template compatibility.
#[derive(Serialize)]
struct PublicCreditEvent<'a> {
    amount_sats: u64,
    feed_credit_sats: u64,
    address_user: &'a str,
    credit_pool: &'a str,
}

impl LedgerStore {
    pub async fn register_xmr_credit_intent(&self, intent: &XmrCreditIntent) -> Result<()> {
        if intent.id.is_nil() {
            bail!("nil credit intent ID");
        }
        super::validate_source(&intent.provider)?;
        super::validate_external_id(&intent.account_scope, "account scope", 256)?;
        super::validate_external_id(&intent.receive_scope, "receive scope", 256)?;
        super::validate_user(&intent.address_user, "address_user")?;
        if intent.credit_pool != "herd" {
            bail!("only the shared herd credit pool is supported");
        }
        let terms = intent.quote.terms();
        if intent.max_credit_sats < terms.target_sats()
            || intent.max_credit_sats > MAX_ACCOUNTING_UNITS
        {
            bail!("invalid immutable intent credit limit");
        }
        let (num, den) = intent.quote.rate().ratio();
        let valuation = Valuation {
            id: xmr_id(intent.id),
            asset: "XMR".into(),
            network: intent.network.as_str().into(),
            provider: intent.provider.clone(),
            account_scope: intent.account_scope.clone(),
            receive_scope: intent.receive_scope.clone(),
            address_user: intent.address_user.clone(),
            credit_pool: intent.credit_pool.clone(),
            expected_atomic: to_i64(terms.expected_atomic(), "quoted amount")?,
            target_sats: to_i64(terms.target_sats(), "quoted credit")?,
            max_credit_sats: to_i64(intent.max_credit_sats, "credit limit")?,
            rate_numerator: Some(num.to_string()),
            rate_denominator: Some(den.to_string()),
            rate_source: Some(intent.quote.rate().source().into()),
            rate_observed_at: Some(intent.quote.rate().observed_at()),
            issued_at: Some(intent.quote.issued_at()),
            expires_at: Some(intent.quote.expires_at()),
            policy_version: "xmr-unlocked-quote-v1".into(),
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<Valuation> =
            sqlx::query_as("SELECT * FROM credit_valuations WHERE id=?")
                .bind(&valuation.id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(existing) = existing {
            if existing != valuation {
                bail!("credit intent already has different immutable terms");
            }
        } else {
            valuation.insert(&mut tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn xmr_credit_intent(&self, intent_id: Uuid) -> Result<XmrCreditIntent> {
        let v: Valuation =
            sqlx::query_as("SELECT * FROM credit_valuations WHERE id=? AND asset='XMR'")
                .bind(xmr_id(intent_id))
                .fetch_optional(&self.pool)
                .await?
                .context("unknown XMR credit intent")?;
        if v.policy_version != "xmr-unlocked-quote-v1" {
            bail!("unsupported stored quote policy");
        }
        let rate = XmrBtcRate::new(
            v.rate_numerator.context("missing rate")?.parse()?,
            v.rate_denominator.context("missing rate")?.parse()?,
            &v.rate_source.context("missing rate source")?,
            v.rate_observed_at.context("missing rate time")?,
        )?;
        // Reconstruct previously validated evidence; never fetch or reprice here.
        let quote = rate.quote(
            to_u64(v.target_sats, "target")?,
            v.issued_at.context("missing issue time")?,
            v.expires_at.context("missing expiry")?,
            u64::MAX,
        )?;
        if quote.terms().expected_atomic() != to_u64(v.expected_atomic, "quoted amount")? {
            bail!("stored quote ratio does not match its rate evidence");
        }
        let network = match v.network.as_str() {
            "mainnet" => XmrNetwork::Mainnet,
            "testnet" => XmrNetwork::Testnet,
            "stagenet" => XmrNetwork::Stagenet,
            _ => bail!("unsupported stored XMR network"),
        };
        Ok(XmrCreditIntent {
            id: intent_id,
            provider: v.provider,
            network,
            account_scope: v.account_scope,
            receive_scope: v.receive_scope,
            address_user: v.address_user,
            credit_pool: v.credit_pool,
            quote,
            max_credit_sats: to_u64(v.max_credit_sats, "credit limit")?,
        })
    }

    /// Persist and value ONE canonical receipt. The upstream reconciler must also
    /// validate completeness/aggregate totals and call hold_xmr_credit_intent on
    /// missing/regressed history. This method cannot establish chain finality.
    pub async fn record_xmr_receipt(
        &self,
        intent_id: Uuid,
        observation: &XmrReceiptObservation,
    ) -> Result<CreditReceiptOutcome> {
        super::validate_external_id(&observation.receipt_key, "receipt key", 256)?;
        if observation.amount.asset() != Asset::Xmr
            || observation.amount.atomic_units() == 0
            || observation.first_seen_at < 0
        {
            bail!("invalid XMR receipt observation");
        }
        let amount = to_i64(observation.amount.atomic_units(), "receipt amount")?;
        let id = xmr_id(intent_id);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let valuation: Valuation =
            sqlx::query_as("SELECT * FROM credit_valuations WHERE id=? AND asset='XMR'")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?
                .context("unknown XMR credit intent")?;
        if valuation.policy_version != "xmr-unlocked-quote-v1" {
            bail!("unsupported stored quote policy");
        }
        if observation.provider != valuation.provider
            || observation.network.as_str() != valuation.network
            || observation.account_scope != valuation.account_scope
            || observation.receive_scope != valuation.receive_scope
        {
            bail!("receipt does not match the intent receive binding");
        }
        let allocation = allocation_in_tx(&mut tx, &id).await?;
        let existing = sqlx::query("SELECT id,amount_atomic,first_seen_at,unlocked FROM asset_receipts WHERE valuation_id=? AND receipt_key=?")
            .bind(&id).bind(&observation.receipt_key).fetch_optional(&mut *tx).await?;
        if let Some(row) = &existing {
            let changed = row.get::<i64, _>("amount_atomic") != amount
                || row.get::<Option<i64>, _>("first_seen_at") != Some(observation.first_seen_at);
            let regressed = row.get::<i64, _>("unlocked") == 1 && !observation.unlocked;
            if changed || regressed || observation.double_spend_seen {
                let reason = if changed {
                    CreditHold::ReceiptConflict
                } else if regressed {
                    CreditHold::FinalityRegression
                } else {
                    CreditHold::DoubleSpend
                };
                hold_observation(&mut tx, &id, observation, reason).await?;
                tx.commit().await?;
                return Ok(CreditReceiptOutcome::Held);
            }
        }
        let timely = observation.first_seen_at
            >= valuation.issued_at.context("quote missing issue time")?
            && observation.first_seen_at < valuation.expires_at.context("quote missing expiry")?;
        let receipt_id = if let Some(row) = existing {
            row.get::<i64, _>("id")
        } else {
            sqlx::query("INSERT INTO asset_receipts (valuation_id,receipt_key,amount_atomic,first_seen_at,unlocked,eligible) VALUES (?,?,?,?,?,?)")
                .bind(&id).bind(&observation.receipt_key).bind(amount).bind(observation.first_seen_at)
                .bind(observation.unlocked).bind(timely && !observation.double_spend_seen)
                .execute(&mut *tx).await?.last_insert_rowid()
        };
        if observation.unlocked {
            sqlx::query("UPDATE asset_receipts SET unlocked=1 WHERE id=?")
                .bind(receipt_id)
                .execute(&mut *tx)
                .await?;
        }
        if observation.double_spend_seen {
            hold_observation(&mut tx, &id, observation, CreditHold::DoubleSpend).await?;
        }
        if allocation.hold_reason.is_some() || observation.double_spend_seen {
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Held);
        }
        if !timely {
            // A late top-up does not invalidate timely receipts waiting to unlock.
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Held);
        }
        if !observation.unlocked {
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Pending);
        }
        let terms = CreditTerms::quoted_xmr(
            to_u64(valuation.expected_atomic, "quote amount")?,
            to_u64(valuation.target_sats, "quote credit")?,
        )?;
        // Check persisted watermarks against actual grant history before any new grant.
        let totals = sqlx::query("SELECT COALESCE(SUM(r.amount_atomic),0) AS atomic,COALESCE(SUM(g.delta_sats),0) AS sats FROM credit_grants g JOIN asset_receipts r ON r.id=g.receipt_id WHERE r.valuation_id=?")
            .bind(&id).fetch_one(&mut *tx).await?;
        if totals.get::<i64, _>("atomic") != to_i64(allocation.eligible_atomic, "allocation")?
            || totals.get::<i64, _>("sats") != to_i64(allocation.credited_sats, "allocation")?
            || terms
                .cumulative_sats(AssetAmount::new(Asset::Xmr, allocation.eligible_atomic)?)
                .ok()
                != Some(allocation.credited_sats)
        {
            hold_observation(&mut tx, &id, observation, CreditHold::AllocationMismatch).await?;
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Held);
        }
        let granted: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credit_grants WHERE receipt_id=?)")
                .bind(receipt_id)
                .fetch_one(&mut *tx)
                .await?;
        if granted {
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Duplicate);
        }
        let current = allocation
            .eligible_atomic
            .checked_add(observation.amount.atomic_units())
            .filter(|n| *n <= MAX_ACCOUNTING_UNITS);
        let update = current.and_then(|current| {
            terms
                .assess_update(
                    AssetAmount::new(Asset::Xmr, allocation.eligible_atomic).ok()?,
                    allocation.credited_sats,
                    AssetAmount::new(Asset::Xmr, current).ok()?,
                )
                .ok()
        });
        let Some(update) =
            update.filter(|u| u.cumulative_credit_sats <= valuation.max_credit_sats as u64)
        else {
            hold_observation(&mut tx, &id, observation, CreditHold::CreditLimit).await?;
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Held);
        };
        let before = feed_credit_in_transaction(&mut tx).await?;
        let Some(feed_credit_sats) = before
            .checked_add(update.delta_sats)
            .filter(|v| *v <= MAX_ACCOUNTING_UNITS)
        else {
            hold_observation(&mut tx, &id, observation, CreditHold::CreditLimit).await?;
            tx.commit().await?;
            return Ok(CreditReceiptOutcome::Held);
        };
        let (ledger_id, event_seq) = if update.delta_sats > 0 {
            // No wallet/provider/private identifiers enter the public event.
            let result = sqlx::query("INSERT INTO ledger_entries (entry_type,source_key,delta_sats,address_user,credit_pool) VALUES ('HERD_RECEIPT',?,?,?,?)")
                .bind(format!("asset-receipt:{receipt_id}")).bind(to_i64(update.delta_sats,"credit delta")?)
                .bind(&valuation.address_user).bind(&valuation.credit_pool).execute(&mut *tx).await?;
            let event_seq = append_event_in_transaction(
                &mut tx,
                "payment_received",
                &PublicCreditEvent {
                    amount_sats: update.delta_sats,
                    feed_credit_sats,
                    address_user: &valuation.address_user,
                    credit_pool: &valuation.credit_pool,
                },
            )
            .await?;
            (
                Some(result.last_insert_rowid()),
                Some(to_i64(event_seq, "event sequence")?),
            )
        } else {
            (None, None)
        };
        sqlx::query("INSERT INTO credit_grants (receipt_id,delta_sats,cumulative_atomic,cumulative_sats,ledger_entry_id,event_seq) VALUES (?,?,?,?,?,?)")
            .bind(receipt_id).bind(to_i64(update.delta_sats,"delta")?).bind(to_i64(update.cumulative_eligible.atomic_units(),"cumulative amount")?)
            .bind(to_i64(update.cumulative_credit_sats,"cumulative credit")?).bind(ledger_id).bind(event_seq).execute(&mut *tx).await?;
        sqlx::query("UPDATE credit_allocations SET eligible_atomic=?,credited_sats=?,updated_at=unixepoch() WHERE valuation_id=?")
            .bind(to_i64(update.cumulative_eligible.atomic_units(),"cumulative amount")?).bind(to_i64(update.cumulative_credit_sats,"cumulative credit")?)
            .bind(&id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(CreditReceiptOutcome::Recorded {
            delta_sats: update.delta_sats,
            feed_credit_sats,
        })
    }

    /// Used by authoritative reconciliation for missing/regressed provider history.
    /// A hold never reduces already granted credit or clears itself after recovery.
    pub async fn hold_xmr_credit_intent(&self, intent_id: Uuid) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query("UPDATE credit_allocations SET hold_reason=COALESCE(hold_reason,?),updated_at=unixepoch() WHERE valuation_id=?")
            .bind(CreditHold::ProviderInconsistency.as_str()).bind(xmr_id(intent_id)).execute(&mut *tx).await?;
        if result.rows_affected() != 1 {
            bail!("unknown XMR credit intent");
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn xmr_credit_allocation(&self, intent_id: Uuid) -> Result<CreditAllocation> {
        let mut tx = self.pool.begin().await?;
        let state = allocation_in_tx(&mut tx, &xmr_id(intent_id)).await?;
        tx.commit().await?;
        Ok(state)
    }
}

fn xmr_id(id: Uuid) -> String {
    format!("xmr:{id}")
}
async fn allocation_in_tx(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<CreditAllocation> {
    let row = sqlx::query("SELECT eligible_atomic,credited_sats,hold_reason FROM credit_allocations WHERE valuation_id=?")
        .bind(id).fetch_optional(&mut **tx).await?.context("missing credit allocation")?;
    Ok(CreditAllocation {
        eligible_atomic: to_u64(row.try_get("eligible_atomic")?, "allocation amount")?,
        credited_sats: to_u64(row.try_get("credited_sats")?, "allocation credit")?,
        hold_reason: row.try_get("hold_reason")?,
    })
}
async fn hold_observation(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    o: &XmrReceiptObservation,
    reason: CreditHold,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO credit_conflicts (valuation_id,receipt_key,amount_atomic,first_seen_at,unlocked,reason) VALUES (?,?,?,?,?,?)")
        .bind(id).bind(&o.receipt_key).bind(to_i64(o.amount.atomic_units(),"conflict amount")?).bind(o.first_seen_at)
        .bind(o.unlocked).bind(reason.as_str()).execute(&mut **tx).await?;
    sqlx::query("UPDATE credit_allocations SET hold_reason=COALESCE(hold_reason,?),updated_at=unixepoch() WHERE valuation_id=?")
        .bind(reason.as_str()).bind(id).execute(&mut **tx).await?;
    Ok(())
}

/// Called ONLY inside record_payment's settlement/ledger/event transaction.
/// One BTC identity valuation per pre-existing source identity; no extra credit.
pub(super) async fn record_native_btc(
    tx: &mut Transaction<'_, Sqlite>,
    settlement_id: i64,
    ledger_id: i64,
    event_seq: u64,
) -> Result<()> {
    let row = sqlx::query("SELECT source,source_id,address_user,credit_pool,amount_msat FROM settled_payments WHERE id=?")
        .bind(settlement_id).fetch_one(&mut **tx).await?;
    let msat: i64 = row.try_get("amount_msat")?;
    if msat <= 0 || msat % 1000 != 0 {
        bail!("invalid native BTC settlement");
    }
    let sats = AssetAmount::new(Asset::Btc, (msat / 1000) as u64)?.native_sats()?;
    let terms = CreditTerms::native_btc();
    let delta = terms.assess_update(
        AssetAmount::new(Asset::Btc, 0)?,
        0,
        AssetAmount::new(Asset::Btc, sats)?,
    )?;
    let id = format!("btc:{settlement_id}");
    let source_id: String = row.try_get("source_id")?;
    Valuation {
        id: id.clone(),
        asset: "BTC".into(),
        network: "legacy-unspecified".into(),
        provider: row.try_get("source")?,
        account_scope: "legacy-settled-payment".into(),
        receive_scope: source_id.clone(),
        address_user: row.try_get("address_user")?,
        credit_pool: row.try_get("credit_pool")?,
        expected_atomic: 1,
        target_sats: 1,
        max_credit_sats: i64::MAX,
        rate_numerator: None,
        rate_denominator: None,
        rate_source: None,
        rate_observed_at: None,
        issued_at: None,
        expires_at: None,
        policy_version: "btc-identity-v1".into(),
    }
    .insert(tx)
    .await?;
    let receipt = sqlx::query("INSERT INTO asset_receipts (valuation_id,receipt_key,amount_atomic,first_seen_at,unlocked,eligible,btc_settlement_id) VALUES (?,?,?,NULL,1,1,?)")
        .bind(&id).bind(source_id).bind(to_i64(sats,"native sats")?).bind(settlement_id).execute(&mut **tx).await?.last_insert_rowid();
    sqlx::query("INSERT INTO credit_grants (receipt_id,delta_sats,cumulative_atomic,cumulative_sats,ledger_entry_id,event_seq) VALUES (?,?,?,?,?,?)")
        .bind(receipt).bind(to_i64(delta.delta_sats,"delta")?).bind(to_i64(sats,"amount")?).bind(to_i64(sats,"credit")?)
        .bind(ledger_id).bind(to_i64(event_seq,"event sequence")?).execute(&mut **tx).await?;
    sqlx::query(
        "UPDATE credit_allocations SET eligible_atomic=?,credited_sats=? WHERE valuation_id=?",
    )
    .bind(to_i64(sats, "amount")?)
    .bind(to_i64(sats, "credit")?)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn verify_native_btc(
    tx: &mut Transaction<'_, Sqlite>,
    settlement_id: i64,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM settled_payments p JOIN asset_receipts r ON r.btc_settlement_id=p.id JOIN credit_valuations v ON v.id=r.valuation_id JOIN credit_allocations a ON a.valuation_id=v.id JOIN credit_grants g ON g.receipt_id=r.id JOIN ledger_entries l ON l.id=g.ledger_entry_id WHERE p.id=? AND p.amount_msat%1000=0 AND v.asset='BTC' AND v.provider=p.source AND v.receive_scope=p.source_id AND v.address_user=p.address_user AND v.credit_pool=p.credit_pool AND v.expected_atomic=1 AND v.target_sats=1 AND r.receipt_key=p.source_id AND r.amount_atomic=p.amount_msat/1000 AND r.unlocked=1 AND r.eligible=1 AND a.eligible_atomic=r.amount_atomic AND a.credited_sats=r.amount_atomic AND a.hold_reason IS NULL AND g.delta_sats=r.amount_atomic AND g.cumulative_atomic=r.amount_atomic AND g.cumulative_sats=r.amount_atomic AND l.delta_sats=r.amount_atomic AND l.source_key='payment:'||p.source||':'||p.source_id AND l.entry_type='HERD_RECEIPT')")
        .bind(settlement_id).fetch_one(&mut **tx).await?;
    if !valid {
        bail!("native BTC receipt provenance is inconsistent");
    }
    Ok(())
}
