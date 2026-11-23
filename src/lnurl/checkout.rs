//! Fixed-amount, dual-rail requests for the remaining shared feeding credit.
use super::*;
use crate::ledger::FeedingQuote;
use cashu::nuts::{CurrencyUnit, nut18::PaymentRequest};
use lightning_goats_cashu_receipt::public_request::Descriptor;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Checkout {
    Funded {
        credit_sats: u64,
        threshold_sats: u64,
    },
    PendingFeed,
    Ready {
        amount_sats: u64,
        invoice: String,
        cashu: String,
        uri: String,
    },
}

impl LnurlService {
    pub async fn finish_feeding(
        &self,
        user: &str,
        id: Uuid,
        descriptor: Descriptor,
    ) -> Result<Checkout> {
        let address = self.address(user).map_err(|e| anyhow!(e.public_reason()))?;
        let admission = self
            .ledger
            .reserve_invoice()
            .await?
            .context("invoice admission busy")?;
        let result=async {
            let quote=self.ledger.prepare_feeding_checkout(id, &address.user, &address.credit_pool,
                &address.metadata, &descriptor, address.min_sendable_msat..=address.max_sendable_msat).await?;
            let (intent, saved_invoice)=match quote {
                FeedingQuote::Funded(credit_sats,threshold_sats)=>return Ok(Checkout::Funded{credit_sats,threshold_sats}),
                FeedingQuote::Pending=>return Ok(Checkout::PendingFeed),
                FeedingQuote::Prepared(intent,invoice)=>(intent,invoice),
            };
            let amount_sats=intent["amount_sat"].as_u64().context("invalid checkout amount")?;
            let cashu=intent["checkout"]["creq"].as_str().context("checkout lost Cashu request")?.to_owned();
            let payload=json!({"request_id":id,"amount_sat":amount_sats,"user":address.user,"credit_pool":address.credit_pool,
                "description":intent["description"],"zap":false});
            let invoice=self.project_invoice(&self.collector,id,&payload,saved_invoice).await?;
            let uri=format!("bitcoin:?lightning={invoice}&creq={cashu}");
            Ok(Checkout::Ready{amount_sats,invoice,cashu,uri})
        }.await;
        self.ledger.finish_invoice_admission(admission).await?;
        result
    }
}

pub(crate) fn fixed_request(descriptor: &Descriptor, id: Uuid, amount: u64) -> Result<String> {
    descriptor.validate()?;
    let mut req: PaymentRequest = descriptor.request.parse()?;
    anyhow::ensure!(
        req.payment_id.as_deref() == Some(descriptor.id.as_str())
            && req.unit == Some(CurrencyUnit::Sat)
            && req.amount.is_none()
            && !req.mints.is_empty()
            && req.transports.len() == 1
            && req.transports[0]._type == cashu::nuts::nut18::TransportType::HttpPost
            && reqwest::Url::parse(&req.transports[0].target).is_ok_and(|url| url.scheme()
                == "https"
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()),
        "invalid underlying wallet request"
    );
    req.payment_id = Some(format!("lgf-{id}"));
    req.amount = Some(amount.into());
    req.single_use = Some(true);
    Ok(req.to_bech32_string()?.to_ascii_uppercase())
}
