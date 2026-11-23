//! Read-only public receiving metadata over the existing authenticated wallet IPC.
//! No proof, invoice, acquisition or spending fields are accepted by this query.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_QUERY_BYTES: usize = 1024;
pub const MAX_REPLY_BYTES: usize = 4096;
pub const MAX_REQUEST_BYTES: usize = 2048;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub version: u8,
    pub operation: Operation,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub request_id: Uuid,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    PublicRequest,
}
impl Query {
    pub fn decode(bytes: &[u8], instance: Uuid, epoch: Uuid) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_QUERY_BYTES,
            "public request query exceeds bound"
        );
        let query: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid public request query"))?;
        ensure!(
            query.version == 1
                && !instance.is_nil()
                && !epoch.is_nil()
                && query.wallet_instance == instance
                && query.wallet_epoch == epoch
                && !query.request_id.is_nil(),
            "public request query registration mismatch"
        );
        Ok(query)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u8,
    pub id: String,
    pub encoding: Encoding,
    pub request: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mints: Vec<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Encoding {
    #[serde(rename = "creqA")]
    CreqA,
    #[serde(rename = "creqB")]
    CreqB,
}
impl Descriptor {
    /// Transport validation only. The trusted wallet must generate the actual
    /// descriptor with CDK from its protected, registered receiving policy.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1
                && self.id.len() == 36
                && self.id.starts_with("lg1-")
                && self.id.as_bytes()[4..]
                    .iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)),
            "invalid public descriptor identity"
        );
        ensure!(
            self.request.len() <= MAX_REQUEST_BYTES,
            "public descriptor exceeds bound"
        );
        ensure!(
            self.mints.len() <= 16
                && self
                    .mints
                    .iter()
                    .all(|s| url::Url::parse(s).is_ok_and(|u| u.scheme() == "https"
                        && u.host().is_some()
                        && u.username().is_empty()
                        && u.password().is_none()
                        && u.fragment().is_none()
                        && u.query().is_none())),
            "invalid advertised mint list"
        );
        let valid = match self.encoding {
            Encoding::CreqA => self.request.strip_prefix("creqA").is_some_and(|body| {
                let payload = body.trim_end_matches('=');
                !payload.is_empty()
                    && body.len() - payload.len() <= 2
                    && payload
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            }),
            Encoding::CreqB => self.request.strip_prefix("CREQB1").is_some_and(|body| {
                !body.is_empty()
                    && body
                        .bytes()
                        .all(|b| b"023456789ACDEFGHJKLMNPQRSTUVWXYZ".contains(&b))
            }),
        };
        ensure!(valid, "invalid public descriptor encoding");
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub request_id: Uuid,
    pub descriptor: Option<Descriptor>,
}
impl Reply {
    pub fn decode(bytes: &[u8], query: &Query) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_REPLY_BYTES,
            "public request reply exceeds bound"
        );
        let reply: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid public request reply"))?;
        ensure!(
            query.version == 1
                && !query.wallet_instance.is_nil()
                && !query.wallet_epoch.is_nil()
                && !query.request_id.is_nil()
                && reply.version == 1
                && reply.wallet_instance == query.wallet_instance
                && reply.wallet_epoch == query.wallet_epoch
                && reply.request_id == query.request_id,
            "public request reply registration mismatch"
        );
        if let Some(descriptor) = &reply.descriptor {
            descriptor.validate()?;
        }
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_reply_is_bound_to_registration_and_exact_query() -> Result<()> {
        let query = Query {
            version: 1,
            operation: Operation::PublicRequest,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            request_id: Uuid::new_v4(),
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
            descriptor: None,
        };
        Reply::decode(&serde_json::to_vec(&reply)?, &query)?;
        for field in ["wallet_instance", "wallet_epoch", "request_id"] {
            let mut value = serde_json::to_value(&reply)?;
            value[field] = serde_json::json!(Uuid::new_v4());
            assert!(Reply::decode(&serde_json::to_vec(&value)?, &query).is_err());
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
                &vec![0; MAX_QUERY_BYTES + 1],
                query.wallet_instance,
                query.wallet_epoch
            )
            .is_err()
        );
        assert!(Reply::decode(&vec![0; MAX_REPLY_BYTES + 1], &query).is_err());
        Ok(())
    }
    #[test]
    fn metadata_preserves_native_request_bytes_and_rejects_tokens_or_encoding_mismatch()
    -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/website/fixtures/cashu-requests.json"
        ))?;
        for value in fixture["requests"].as_object().unwrap().values() {
            let descriptor = Descriptor {
                version: 1,
                id: value["id"].as_str().unwrap().into(),
                encoding: serde_json::from_value(value["encoding"].clone())?,
                mints: Vec::new(),
                request: value["request"].as_str().unwrap().into(),
            };
            descriptor.validate()?;
            let roundtrip: Descriptor = serde_json::from_slice(&serde_json::to_vec(&descriptor)?)?;
            assert_eq!(roundtrip, descriptor);
            let mut wrong = descriptor.clone();
            wrong.request = "cashuAtoken".into();
            assert!(wrong.validate().is_err());
            wrong.request = "creqA".to_owned() + &"A".repeat(MAX_REQUEST_BYTES);
            assert!(wrong.validate().is_err());
            wrong = descriptor.clone();
            wrong.encoding = match descriptor.encoding {
                Encoding::CreqA => Encoding::CreqB,
                Encoding::CreqB => Encoding::CreqA,
            };
            assert!(wrong.validate().is_err());
        }
        Ok(())
    }
}
