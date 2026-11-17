//! Private admission/status projection. No mint calls or credit authority here.
#![forbid(unsafe_code)]
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const MAX_STATUS_BYTES: usize = 1024;
pub const MAX_ADMISSION_BYTES: usize = 1_048_576;

/// Bind a public delivery alias to the exact bounded request bytes.
/// A delivery ID is not a public status capability.
pub fn http_delivery_id(body: &[u8]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"lightning-goats/cashu-http-delivery/v1\0");
    digest.update(body);
    let digest = digest.finalize();
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest[..16]);
    uuid::Builder::from_custom_bytes(identity).into_uuid()
}

/// Private lookup capability. Deliberately no Debug/Display implementation.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Capability(String);
impl Capability {
    pub fn parse(value: String) -> Result<Self> {
        let cap = Self(value);
        cap.validate()?;
        Ok(cap)
    }
    pub fn fresh() -> Self {
        Self(format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        ))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.0.len() == 64
                && self
                    .0
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid receipt status capability"
        );
        Ok(())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Acquired,
    Credited,
    Held,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmitOperation {
    OrdinaryAdmit,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionQuery {
    pub version: u8,
    pub operation: AdmitOperation,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub delivery_id: Uuid,
    pub body: String,
}
fn registration(
    version: u8,
    instance: Uuid,
    epoch: Uuid,
    id: Uuid,
    expected_instance: Uuid,
    expected_epoch: Uuid,
) -> Result<()> {
    ensure!(
        version == 1
            && !instance.is_nil()
            && !epoch.is_nil()
            && !id.is_nil()
            && instance == expected_instance
            && epoch == expected_epoch,
        "receipt status registration mismatch"
    );
    Ok(())
}
impl AdmissionQuery {
    pub fn decode(bytes: &[u8], instance: Uuid, epoch: Uuid) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_ADMISSION_BYTES,
            "ordinary admission frame exceeds bound"
        );
        let query: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid ordinary admission frame"))?;
        registration(
            query.version,
            query.wallet_instance,
            query.wallet_epoch,
            query.delivery_id,
            instance,
            epoch,
        )?;
        ensure!(!query.body.is_empty(), "empty ordinary admission body");
        Ok(query)
    }
}
/// Rejection means this submission failed validation before new admission or
/// acquisition. It makes no claim about prior uses of its proofs. Storage and
/// unknown outcomes must be transport errors, never a guessed rejected result.
#[derive(Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdmissionOutcome {
    Admitted {
        capability: Capability,
        status: Status,
    },
    Rejected,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionReply {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub delivery_id: Uuid,
    pub result: AdmissionOutcome,
}
impl AdmissionReply {
    pub fn validate(&self, query: &AdmissionQuery) -> Result<()> {
        ensure!(
            query.version == 1,
            "ordinary admission query version mismatch"
        );
        registration(
            self.version,
            self.wallet_instance,
            self.wallet_epoch,
            self.delivery_id,
            query.wallet_instance,
            query.wallet_epoch,
        )?;
        ensure!(
            self.delivery_id == query.delivery_id,
            "ordinary admission reply mismatch"
        );
        if let AdmissionOutcome::Admitted { capability, .. } = &self.result {
            capability.validate()?;
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryOperation {
    ReceiveStatus,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub version: u8,
    pub operation: QueryOperation,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub request_id: Uuid,
    pub capability: Capability,
}
impl Query {
    pub fn decode(bytes: &[u8], instance: Uuid, epoch: Uuid) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_STATUS_BYTES,
            "receipt status frame exceeds bound"
        );
        let query: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid receipt status query"))?;
        registration(
            query.version,
            query.wallet_instance,
            query.wallet_epoch,
            query.request_id,
            instance,
            epoch,
        )?;
        query.capability.validate()?;
        Ok(query)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub request_id: Uuid,
    pub status: Option<Status>,
}
impl Reply {
    pub fn validate(&self, query: &Query) -> Result<()> {
        ensure!(query.version == 1, "receipt status query version mismatch");
        registration(
            self.version,
            self.wallet_instance,
            self.wallet_epoch,
            self.request_id,
            query.wallet_instance,
            query.wallet_epoch,
        )?;
        ensure!(
            self.request_id == query.request_id,
            "receipt status reply mismatch"
        );
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_queries_bind_exact_registration_and_nonce_without_echoing_capability() -> Result<()> {
        let query = Query {
            version: 1,
            operation: QueryOperation::ReceiveStatus,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
            capability: Capability::fresh(),
        };
        Query::decode(
            &serde_json::to_vec(&query)?,
            query.wallet_instance,
            query.wallet_epoch,
        )?;
        let reply = Reply {
            version: 1,
            wallet_instance: query.wallet_instance,
            wallet_epoch: query.wallet_epoch,
            request_id: query.request_id,
            status: Some(Status::Credited),
        };
        reply.validate(&query)?;
        assert!(!serde_json::to_string(&reply)?.contains(query.capability.as_str()));
        for field in ["wallet_instance", "wallet_epoch", "request_id"] {
            let mut value = serde_json::to_value(&reply)?;
            value[field] = serde_json::json!(Uuid::new_v4());
            assert!(
                serde_json::from_value::<Reply>(value)?
                    .validate(&query)
                    .is_err()
            );
        }
        let mut value = serde_json::to_value(&query)?;
        value["proofs"] = serde_json::json!([]);
        assert!(
            Query::decode(
                &serde_json::to_vec(&value)?,
                query.wallet_instance,
                query.wallet_epoch
            )
            .is_err()
        );
        assert!(
            Query::decode(
                &vec![0; MAX_STATUS_BYTES + 1],
                query.wallet_instance,
                query.wallet_epoch
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn capability_is_random_and_bad_values_do_not_enter_error_messages() -> Result<()> {
        let first = Capability::fresh();
        let second = Capability::fresh();
        assert!(first != second);
        first.validate()?;
        second.validate()?;
        for text in [
            "synthetic-private-status-secret",
            "",
            &"a".repeat(65),
            &"G".repeat(64),
        ] {
            let result = Capability::parse(text.to_owned());
            assert!(result.is_err());
            assert!(
                !result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("synthetic-private-status-secret")
            );
        }
        Ok(())
    }
    #[test]
    fn ordinary_admission_has_an_independent_operation_and_exact_reply_scope() -> Result<()> {
        let query = AdmissionQuery {
            version: 1,
            operation: AdmitOperation::OrdinaryAdmit,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            delivery_id: Uuid::new_v4(),
            body: "private-synthetic-proof-body".into(),
        };
        let bytes = serde_json::to_vec(&query)?;
        AdmissionQuery::decode(&bytes, query.wallet_instance, query.wallet_epoch)?;
        assert!(AdmissionQuery::decode(&bytes, query.wallet_instance, Uuid::new_v4()).is_err());
        let mut reply = AdmissionReply {
            version: 1,
            wallet_instance: query.wallet_instance,
            wallet_epoch: query.wallet_epoch,
            delivery_id: query.delivery_id,
            result: AdmissionOutcome::Admitted {
                capability: Capability::fresh(),
                status: Status::Pending,
            },
        };
        reply.validate(&query)?;
        reply.delivery_id = Uuid::new_v4();
        assert!(reply.validate(&query).is_err());
        let mut value = serde_json::to_value(&query)?;
        value["invoice"] = serde_json::json!("forbidden");
        assert!(
            AdmissionQuery::decode(
                &serde_json::to_vec(&value)?,
                query.wallet_instance,
                query.wallet_epoch
            )
            .is_err()
        );
        Ok(())
    }
}
