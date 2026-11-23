//! Reserve the purpose and exact tender identity before private wallet admission.
use super::*;
use ::cashu::nuts::{CurrencyUnit, nut18::PaymentRequest};
use lightning_goats_cashu_receipt::{
    contact_fee::{ContactTender, FEE_SATS},
    public_request::Descriptor,
};

pub enum ContactAction {
    NeedsDescriptor,
    Fulfilled,
    Pending,
    Invalid,
    Challenge(String),
    Submit(ContactTender),
}

impl LedgerStore {
    pub async fn contact_tender(
        &self,
        intent: &Intent,
        descriptor: Option<&Descriptor>,
        delivery: Option<Uuid>,
    ) -> Result<ContactAction> {
        self.create_contact_intent(intent).await?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id = intent.id.to_string();
        if sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM private_contact_entitlements WHERE contact_id=?)",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?
        {
            tx.commit().await?;
            return Ok(ContactAction::Fulfilled);
        }
        let saved: Option<(String, String)> = sqlx::query_as(
            "SELECT request,descriptor_id FROM private_contact_requests WHERE contact_id=?",
        )
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?;
        let (request, descriptor_id) = match saved {
            Some(saved) => saved,
            None => {
                let Some(descriptor) = descriptor else {
                    tx.commit().await?;
                    return Ok(ContactAction::NeedsDescriptor);
                };
                descriptor.validate()?;
                let mut request: PaymentRequest = descriptor.request.parse()?;
                anyhow::ensure!(
                    request.unit == Some(CurrencyUnit::Sat)
                        && !request.mints.is_empty()
                        && request.payment_id.as_deref() == Some(descriptor.id.as_str()),
                    "invalid wallet descriptor"
                );
                request.amount = Some(FEE_SATS.into());
                request.single_use = Some(true);
                request.payment_id = Some(format!("lgc-{id}"));
                request.description = Some("Lightning Goats private contact".into());
                request.transports.clear();
                request.supported_methods.clear();
                let value = request.to_bech32_string()?.to_ascii_uppercase();
                sqlx::query("INSERT INTO private_contact_requests(contact_id,request,descriptor_id) VALUES(?,?,?)")
                    .bind(&id).bind(&value).bind(&descriptor.id).execute(&mut *tx).await?;
                (value, descriptor.id.clone())
            }
        };
        let action = if let Some(delivery) = delivery {
            anyhow::ensure!(delivery.get_version_num() == 8, "invalid tender identity");
            let previous: Option<(String, String)> = sqlx::query_as(
                "SELECT contact_id,state FROM private_contact_tenders WHERE delivery_id=?",
            )
            .bind(delivery.to_string())
            .fetch_optional(&mut *tx)
            .await?;
            let terms = ContactTender {
                contact_id: intent.id,
                descriptor_id,
            };
            match previous {
                Some((contact, state)) if contact == id && state == "reserved" => {
                    ContactAction::Submit(terms)
                }
                Some(_) => ContactAction::Invalid,
                None => {
                    let inserted=sqlx::query("INSERT INTO private_contact_tenders(delivery_id,contact_id) VALUES(?,?) ON CONFLICT DO NOTHING")
                        .bind(delivery.to_string()).bind(&id).execute(&mut *tx).await?.rows_affected();
                    if inserted == 1 {
                        ContactAction::Submit(terms)
                    } else {
                        ContactAction::Pending
                    }
                }
            }
        } else if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM private_contact_tenders WHERE contact_id=? AND state='reserved')")
            .bind(&id).fetch_one(&mut *tx).await? {
            ContactAction::Pending
        } else {
            ContactAction::Challenge(request)
        };
        tx.commit().await?;
        Ok(action)
    }
    pub async fn reject_contact_tender(&self, id: Uuid, delivery: Uuid) -> Result<()> {
        sqlx::query("UPDATE private_contact_tenders SET state='rejected' WHERE contact_id=? AND delivery_id=? AND state='reserved'")
            .bind(id.to_string()).bind(delivery.to_string()).execute(&self.pool).await?;
        Ok(())
    }
}
