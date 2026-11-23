//! Private receiving protocol. Admission is not acquired value or core credit.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_FRAME_BYTES: usize = 1_048_576;

// Deliberately no Debug: payloads may contain spendable bearer proofs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub delivery_id: Uuid,
    pub payload: Payload,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "transport",
    content = "body",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Payload {
    Cashu(String),
    Nutzap(String),
}
impl Request {
    pub fn decode(bytes: &[u8], instance: Uuid, epoch: Uuid) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_FRAME_BYTES,
            "private intake frame exceeds bound"
        );
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid private intake frame"))?;
        ensure!(
            value.version == 1
                && !instance.is_nil()
                && !epoch.is_nil()
                && value.wallet_instance == instance
                && value.wallet_epoch == epoch
                && !value.delivery_id.is_nil(),
            "private intake registration mismatch"
        );
        Ok(value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u8,
    pub delivery_id: Uuid,
    pub acquisition_id: Uuid,
    pub disposition: Disposition,
}
impl Reply {
    pub fn validate_delivery(&self, delivery_id: Uuid) -> Result<()> {
        ensure!(
            self.version == 1
                && !delivery_id.is_nil()
                && self.delivery_id == delivery_id
                && !self.acquisition_id.is_nil(),
            "private intake reply does not match delivery"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Admitted,
    Existing,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replies_cannot_acknowledge_another_delivery_or_empty_acquisition() -> Result<()> {
        let id = Uuid::new_v4();
        let mut reply = Reply {
            version: 1,
            delivery_id: id,
            acquisition_id: Uuid::new_v4(),
            disposition: Disposition::Admitted,
        };
        reply.validate_delivery(id)?;
        assert!(reply.validate_delivery(Uuid::new_v4()).is_err());
        reply.acquisition_id = Uuid::nil();
        assert!(reply.validate_delivery(id).is_err());
        reply.acquisition_id = Uuid::new_v4();
        reply.version = 2;
        assert!(reply.validate_delivery(id).is_err());
        Ok(())
    }

    #[test]
    fn registration_and_strict_schema_precede_intake() -> Result<()> {
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let request = Request {
            version: 1,
            wallet_instance: instance,
            wallet_epoch: epoch,
            delivery_id: Uuid::new_v4(),
            payload: Payload::Cashu("private-fixture".into()),
        };
        let bytes = serde_json::to_vec(&request)?;
        Request::decode(&bytes, instance, epoch)?;
        assert!(Request::decode(&bytes, instance, Uuid::new_v4()).is_err());
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("delivery_id", serde_json::json!(Uuid::nil())),
            ("claimed_uid", serde_json::json!(1000)),
        ] {
            let mut mutated = serde_json::to_value(&request)?;
            mutated[field] = value;
            assert!(Request::decode(&serde_json::to_vec(&mutated)?, instance, epoch).is_err());
        }
        assert!(Request::decode(&vec![0; MAX_FRAME_BYTES + 1], instance, epoch).is_err());
        Ok(())
    }
}
