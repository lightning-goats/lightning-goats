//! Freeze both payment options against one atomic snapshot of feeder credit.
use super::LedgerStore;
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::{public_request::Descriptor, receive_status::FeedingCheckout};
use serde_json::{Value, json};
use std::ops::RangeInclusive;
use uuid::Uuid;

pub(crate) enum FeedingQuote {
    Funded(u64, u64),
    Pending,
    Prepared(Value, Option<String>),
}

impl LedgerStore {
    pub(crate) async fn prepare_feeding_checkout(
        &self,
        id: Uuid,
        user: &str,
        pool: &str,
        metadata: &str,
        descriptor: &Descriptor,
        bounds_msat: RangeInclusive<u64>,
    ) -> Result<FeedingQuote> {
        ensure!(id.get_version_num() == 4, "invalid checkout ID");
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<(String, Option<String>)> =
            sqlx::query_as("SELECT intent,invoice FROM breez_project_invoices WHERE id=?")
                .bind(id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some((terms, invoice)) = existing {
            let intent: Value = serde_json::from_str(&terms)?;
            ensure!(
                intent["checkout"].is_object()
                    && intent["user"] == user
                    && intent["credit_pool"] == pool,
                "checkout identity conflict"
            );
            tx.commit().await?;
            return Ok(FeedingQuote::Prepared(intent, invoice));
        }
        let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_attempts WHERE status IN ('intent_committed','unknown'))").fetch_one(&mut *tx).await?;
        if pending {
            tx.commit().await?;
            return Ok(FeedingQuote::Pending);
        }
        let (credit, threshold):(i64,i64)=sqlx::query_as("SELECT (SELECT COALESCE(SUM(delta_sats),0) FROM ledger_entries), threshold_sats FROM feeder_credit_policy WHERE singleton=1").fetch_one(&mut *tx).await?;
        let credit = u64::try_from(credit)?;
        let threshold = u64::try_from(threshold)?;
        if credit >= threshold {
            tx.commit().await?;
            return Ok(FeedingQuote::Funded(credit, threshold));
        }
        let amount = threshold - credit;
        ensure!(
            bounds_msat.contains(
                &amount
                    .checked_mul(1000)
                    .context("checkout amount overflow")?
            ),
            "remaining amount outside invoice limits"
        );
        let request = crate::lnurl::checkout::fixed_request(descriptor, id, amount)?;
        let intent = json!({"request_id":id,"user":user,"credit_pool":pool,"amount_sat":amount,
            "description":metadata,"zap":false,"checkout":{"descriptor_id":descriptor.id,"creq":request,
            "credit_sats":credit,"threshold_sats":threshold}});
        sqlx::query("INSERT INTO breez_project_invoices(id,intent) VALUES (?,?)")
            .bind(id.to_string())
            .bind(intent.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(FeedingQuote::Prepared(intent, None))
    }

    pub(crate) async fn checkout_terms(&self, id: Uuid) -> Result<Option<FeedingCheckout>> {
        let row:Option<(i64,String)>=sqlx::query_as("SELECT json_extract(intent,'$.amount_sat'),json_extract(intent,'$.checkout.descriptor_id') FROM breez_project_invoices WHERE id=? AND json_type(intent,'$.checkout')='object'")
            .bind(id.to_string()).fetch_optional(&self.pool).await?;
        row.map(|(amount, descriptor_id)| {
            let terms = FeedingCheckout {
                id,
                amount_sats: u64::try_from(amount)?,
                descriptor_id,
            };
            terms.validate()?;
            Ok(terms)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cashu::nuts::{
        CurrencyUnit,
        nut18::{PaymentRequest, Transport, TransportType},
    };
    use lightning_goats_cashu_receipt::public_request::Encoding;

    #[tokio::test]
    async fn raced_quotes_freeze_the_remainder_without_reserving_credit() -> Result<()> {
        let root = tempfile::tempdir()?;
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("checkout.db").display()
        ))
        .await?;
        ledger.configure_feeder_credit_cap(1000).await?;
        let grant = |amount| crate::ledger::OperatorCreditGrant {
            id: format!("test-{amount}"),
            amount_sats: amount,
            address_user: "herd".into(),
            reason: "fixture".into(),
        };
        ledger.grant_operator_credit(&grant(760), false).await?;
        let id = format!("lg1-{}", "fa".repeat(16));
        let mint = "https://mint.lightning-goats.com";
        let original = PaymentRequest::builder()
            .payment_id(id.clone())
            .unit(CurrencyUnit::Sat)
            .mints(vec![mint.parse()?])
            .add_transport(Transport {
                _type: TransportType::HttpPost,
                target: "https://lightning-goats.com/api/v1/cashu".into(),
                tags: vec![],
            })
            .build();
        let descriptor = Descriptor {
            version: 1,
            id,
            request: original.to_bech32_string()?.to_ascii_uppercase(),
            encoding: Encoding::CreqB,
            mints: vec![mint.into()],
        };
        let open = |id, user| {
            ledger.prepare_feeding_checkout(
                id,
                user,
                "herd",
                "metadata",
                &descriptor,
                1000..=1000000000,
            )
        };
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let (a, b) = tokio::try_join!(open(first, "dexter"), open(second, "nova"))?;
        for (quote, id) in [(a, first), (b, second)] {
            let FeedingQuote::Prepared(intent, None) = quote else {
                anyhow::bail!("expected both offers")
            };
            let req: PaymentRequest = intent["checkout"]["creq"].as_str().unwrap().parse()?;
            assert_eq!(req.amount, Some(240.into()));
            assert_eq!(req.payment_id, Some(format!("lgf-{id}")));
            assert_eq!(req.single_use, Some(true));
            assert_eq!(req.mints, original.mints);
            assert_eq!(req.transports, original.transports);
            assert_eq!(ledger.checkout_terms(id).await?.unwrap().amount_sats, 240);
        }
        assert_eq!(ledger.feed_credit_sats().await?, 760);
        assert!(open(first, "nova").await.is_err());
        ledger.grant_operator_credit(&grant(240), false).await?;
        assert!(matches!(
            open(Uuid::new_v4(), "dexter").await?,
            FeedingQuote::Funded(1000, 1000)
        ));
        assert!(matches!(
            open(first, "dexter").await?,
            FeedingQuote::Prepared(_, None)
        ));
        Ok(())
    }
}
