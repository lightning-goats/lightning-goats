//! Protected, bounded wallet service configuration. Parsing never opens state or a mint.
use crate::{
    intake::{Limits, PinnedKeys},
    mint_policy::{MintPolicy, MintRegistry},
};
use anyhow::{Result, ensure};
use cdk::nuts::{Id, PublicKey};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MAX_CONFIG_BYTES: usize = 65_536;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub runtime_uid: u32,
    pub core_uid: u32,
    pub ipc_gid: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub state_directory: PathBuf,
    pub intake_socket: PathBuf,
    #[serde(default)]
    pub receive_cashu: bool,
    #[serde(default)]
    pub receive_nutzap: bool,
    pub receiving_key: String,
    #[serde(default)]
    pub receiving_key_anchor: Option<String>,
    #[serde(default)]
    pub retained_receiving_keys: Vec<RetainedReceiver>,
    pub recipient: String,
    pub descriptors: Vec<String>,
    #[serde(default)]
    pub retired_descriptors: Vec<crate::descriptor::RetiredDescriptor>,
    #[serde(default)]
    pub public_request: Option<PublicRequestConfig>,
    pub mints: Vec<MintPolicy>,
    pub keysets: Vec<Keyset>,
    pub body_bytes: usize,
    pub proof_count: usize,
    pub receipt_sats: u64,
    pub deadline_seconds: u64,
    pub connections_per_second: u32,
    #[serde(default)]
    pub execution: crate::scheduler::Execution,
    #[serde(default)]
    pub receipt_delivery: Option<crate::receipt_outbox::Config>,
    #[serde(default)]
    pub relay_intake: Option<crate::relay_intake::Config>,
    #[serde(default)]
    pub publication_signing: Option<crate::publication_signing::Config>,
    #[serde(default)]
    pub publication_delivery: Option<crate::publication_delivery::Config>,
    #[serde(default)]
    pub treasury_intake: Option<crate::treasury_config::Config>,
    #[serde(default)]
    pub treasury_execution: crate::treasury_scheduler::Execution,
}
/// Select public receiving instructions from current protected wallet policy.
/// No caller-supplied encoded request is accepted here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicRequestConfig {
    pub endpoint: String,
    pub encoding: crate::descriptor::Encoding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyset {
    pub mint_id: String,
    pub id: String,
    pub keys: BTreeMap<u64, String>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedReceiver {
    pub public_key: String,
    pub nutzap: Option<NutzapWindow>,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct NutzapWindow {
    pub starts_at: u64,
    pub accept_before: u64,
    pub mint_ids: BTreeSet<String>,
}

pub struct Prepared {
    pub registry: MintRegistry,
    pub keys: PinnedKeys,
    pub receiver: PublicKey,
    pub key_anchor: PublicKey,
    pub retained_receivers: Vec<PublicKey>,
    pub limits: Limits,
    pub public_request: Option<crate::descriptor::Descriptor>,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        crate::private_ipc::protected_config(path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take((MAX_CONFIG_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_CONFIG_BYTES,
            "wallet configuration exceeds bound"
        );
        let config: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid wallet configuration"))?;
        config.prepare()?;
        Ok(config)
    }
    /// Validate the configured identity separately from a harmless offline check.
    pub fn require_runtime_identity(&self) -> Result<()> {
        ensure!(
            rustix::process::geteuid().as_raw() == self.runtime_uid && self.runtime_uid > 0,
            "wallet runtime identity differs from registration"
        );
        crate::private_ipc::private_state(&self.state_directory)
    }
    /// Startup-only key loading, after offline policy and runtime identity checks.
    /// Offline configuration validation deliberately does not require live keys.
    pub fn load_keys(&self) -> Result<crate::service_keys::Keys> {
        let prepared = self.prepare()?;
        self.require_runtime_identity()?;
        crate::writer_lock::check_recovery_marker(&self.state_directory)?;
        crate::service_keys::Keys::load_configured(
            &self.state_directory,
            prepared.receiver,
            prepared.key_anchor,
            &prepared.retained_receivers,
        )
    }
    /// Public receiving instructions only: no runtime identity, key or journal access.
    pub fn prepare_request(
        &self,
        endpoint: &str,
        encoding: crate::descriptor::Encoding,
    ) -> Result<crate::descriptor::Descriptor> {
        let prepared = self.prepare()?;
        crate::descriptor::Descriptor::prepare(
            &prepared.registry,
            endpoint,
            prepared.receiver,
            encoding,
        )
    }
    pub fn prepare(&self) -> Result<Prepared> {
        self.treasury_execution.validate()?;
        ensure!(
            !(self.treasury_execution.execute || self.treasury_execution.reconcile)
                || self.treasury_intake.is_some(),
            "treasury execution requires protected bindings"
        );
        if let Some(treasury) = &self.treasury_intake {
            treasury.bindings(self)?;
        }
        if self
            .publication_signing
            .as_ref()
            .is_some_and(|c| c.announcements)
            || self
                .publication_delivery
                .as_ref()
                .is_some_and(|c| c.announcements)
        {
            ensure!(
                self.receive_nutzap && self.relay_intake.as_ref().is_some_and(|c| c.enabled),
                "announcement workers require configured nutzap receiving and relay intake"
            );
        }
        if let Some(delivery) = &self.publication_delivery {
            delivery.validate()?;
        }
        if let Some(signing) = &self.publication_signing {
            signing.validate(self.runtime_uid, self.core_uid)?;
            ensure!(
                signing.socket != self.intake_socket
                    && !signing.socket.starts_with(&self.state_directory)
                    && self
                        .receipt_delivery
                        .as_ref()
                        .is_none_or(|delivery| delivery.socket != signing.socket),
                "signer must use a separate protected socket"
            );
        }
        self.execution.validate()?;
        if let Some(relays) = &self.relay_intake {
            relays.validate(self.receive_nutzap)?;
        }
        if let Some(delivery) = &self.receipt_delivery {
            delivery.validate(self.core_uid)?;
            ensure!(
                delivery.socket != self.intake_socket
                    && !delivery.socket.starts_with(&self.state_directory),
                "receipt delivery must use a separate core socket"
            );
        }
        ensure!(
            self.version == 1
                && self.runtime_uid > 0
                && self.core_uid > 0
                && self.runtime_uid != self.core_uid
                && self.ipc_gid > 0
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil(),
            "invalid wallet service registration"
        );
        crate::private_ipc::socket_path(&self.intake_socket)?;
        ensure!(
            self.state_directory.is_absolute()
                && self.state_directory != Path::new("/")
                && self.state_directory.components().all(|c| matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                ))
                && !self.intake_socket.starts_with(&self.state_directory),
            "wallet state and IPC paths must be distinct and canonical"
        );
        ensure!(
            self.body_bytes > 0
                && self.body_bytes <= 1_048_576
                && self.proof_count > 0
                && self.proof_count <= 1024
                && self.receipt_sats > 0
                && self.receipt_sats <= i64::MAX as u64
                && (1..=60).contains(&self.deadline_seconds)
                && (1..=64).contains(&self.connections_per_second),
            "invalid wallet intake limits"
        );
        ensure!(
            self.mints.len() <= 16
                && self.keysets.len() <= 128
                && self.descriptors.len() + self.retired_descriptors.len() <= 16
                && self
                    .descriptors
                    .iter()
                    .all(|d| !d.is_empty() && d.len() <= 128 && !d.chars().any(char::is_control)),
            "wallet policy inventory exceeds bounds"
        );
        let registry = MintRegistry::new(self.mints.clone())?;
        let receiver = PublicKey::from_hex(&self.receiving_key)?;
        let key_anchor = self
            .receiving_key_anchor
            .as_deref()
            .map(PublicKey::from_hex)
            .transpose()?
            .unwrap_or(receiver);
        ensure!(
            self.retained_receiving_keys.len() <= 16
                && (key_anchor == receiver
                    || key_anchor.x_only_public_key() != receiver.x_only_public_key()),
            "invalid retained receiver inventory"
        );
        if self.receiving_key_anchor.is_some() || !self.retained_receiving_keys.is_empty() {
            ensure!(
                self.receiving_key == receiver.to_hex()
                    && self
                        .receiving_key_anchor
                        .as_ref()
                        .is_none_or(|raw| *raw == key_anchor.to_hex()),
                "rotated receiving identities must use canonical compressed encoding"
            );
        }
        let mut retained_receivers = Vec::new();
        let mut receiving_identities = BTreeSet::from([receiver.x_only_public_key()]);
        for retained in &self.retained_receiving_keys {
            let key = PublicKey::from_hex(&retained.public_key)?;
            ensure!(
                key.to_hex() == retained.public_key
                    && (key == key_anchor
                        || key.x_only_public_key() != key_anchor.x_only_public_key())
                    && receiving_identities.insert(key.x_only_public_key()),
                "duplicate or noncanonical retained receiver"
            );
            if let Some(window) = &retained.nutzap {
                ensure!(
                    window.starts_at > 0
                        && window.starts_at < window.accept_before
                        && window.accept_before <= i64::MAX as u64
                        && !window.mint_ids.is_empty()
                        && window
                            .mint_ids
                            .iter()
                            .all(|id| self.mints.iter().any(|mint| mint.id == *id)),
                    "invalid retained nutzap receiving window"
                );
            }
            retained_receivers.push(key);
        }
        let mut descriptor_ids = self.descriptors.iter().collect::<BTreeSet<_>>();
        for retired in &self.retired_descriptors {
            ensure!(
                descriptor_ids.insert(&retired.id),
                "retired descriptor still active or listed twice"
            );
            let bound = retired.receiver(receiver)?;
            ensure!(
                bound == receiver || bound == key_anchor || retained_receivers.contains(&bound),
                "retired descriptor receiver is not retained"
            );
            retired.verify(bound)?;
        }
        lightning_goats_nostr_event::validate_public_key(&self.recipient)?;
        ensure!(
            receiving_identities
                .iter()
                .all(|key| key.to_string() != self.recipient)
                && key_anchor.x_only_public_key().to_string() != self.recipient,
            "Cashu receiving key must be separate from project Nostr identity"
        );
        let mut keys = PinnedKeys::default();
        let mut qualified = BTreeSet::new();
        let mut keyset_ids = BTreeSet::new();
        let mut count = 0usize;
        for keyset in &self.keysets {
            ensure!(
                self.mints.iter().any(|m| m.id == keyset.mint_id)
                    && keyset_ids.insert((&keyset.mint_id, &keyset.id))
                    && !keyset.keys.is_empty()
                    && keyset.keys.len() <= 64,
                "invalid or duplicate configured keyset"
            );
            let id: Id = keyset.id.parse()?;
            for (amount, key) in &keyset.keys {
                count += 1;
                ensure!(
                    count <= 1024 && *amount <= i64::MAX as u64,
                    "configured key inventory exceeds bounds"
                );
                keys.insert(&keyset.mint_id, id, *amount, PublicKey::from_hex(key)?)?;
            }
            qualified.insert(keyset.mint_id.as_str());
        }
        let active_cashu = self
            .mints
            .iter()
            .any(|m| m.receive_cashu && m.bolt11_exit_qualified);
        let active_nutzap = self
            .mints
            .iter()
            .any(|m| m.receive_nutzap && m.bolt11_exit_qualified);
        ensure!(
            !self.receive_cashu || (active_cashu && !self.descriptors.is_empty()),
            "ordinary Cashu receiving has no qualified descriptor/mint"
        );
        ensure!(
            !self.receive_nutzap || active_nutzap,
            "nutzap receiving has no qualified mint"
        );
        for mint in &self.mints {
            if mint.bolt11_exit_qualified
                && ((self.receive_cashu && mint.receive_cashu)
                    || (self.receive_nutzap && mint.receive_nutzap))
            {
                ensure!(
                    qualified.contains(mint.id.as_str()),
                    "receiving mint has no pinned keys"
                );
            }
        }
        if let Some(relays) = &self.relay_intake {
            if relays.enabled {
                crate::relay::Filter {
                    recipient: self.recipient.clone(),
                    mints: self
                        .mints
                        .iter()
                        .filter(|m| m.receive_nutzap && m.bolt11_exit_qualified)
                        .map(|m| m.advertised_url.clone())
                        .collect(),
                    since: 0,
                    until: 0,
                    limit: relays.event_limit,
                }
                .validate()?;
            }
        }
        let public_request = self
            .public_request
            .as_ref()
            .map(|display| {
                let descriptor = crate::descriptor::Descriptor::prepare(
                    &registry,
                    &display.endpoint,
                    receiver,
                    display.encoding,
                )?;
                ensure!(
                    self.descriptors.iter().any(|id| id == descriptor.id()),
                    "public request is not in the accepted descriptor inventory"
                );
                ensure!(
                    descriptor.encoded().len() <= 2048,
                    "public request exceeds display bound"
                );
                Ok::<_, anyhow::Error>(descriptor)
            })
            .transpose()?;
        Ok(Prepared {
            public_request: public_request.filter(|_| self.receive_cashu),
            registry,
            keys,
            receiver,
            key_anchor,
            retained_receivers,
            limits: Limits {
                body_bytes: self.body_bytes,
                proof_count: self.proof_count,
                receipt_sats: self.receipt_sats,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn fixture() -> Value {
        let receiver = cdk::nuts::SecretKey::from_slice(&[1; 32])
            .unwrap()
            .public_key();
        let recipient = cdk::nuts::SecretKey::from_slice(&[3; 32])
            .unwrap()
            .public_key()
            .x_only_public_key()
            .to_string();
        json!({"version":1,"runtime_uid":12345,"core_uid":12346,"ipc_gid":12347,
            "wallet_instance":Uuid::new_v4(),"wallet_epoch":Uuid::new_v4(),
            "state_directory":"/var/lib/lightning-goats-cashu",
            "intake_socket":"/run/lightning-goats-cashu/intake.sock",
            "receiving_key":receiver.to_hex(),"recipient":recipient,
            "descriptors":[],"mints":[],"keysets":[],
            "body_bytes":65536,"proof_count":64,"receipt_sats":10000,
            "deadline_seconds":5,"connections_per_second":8})
    }
    #[test]
    fn request_preparation_is_offline_reusable_and_does_not_register_or_enable_receiving()
    -> Result<()> {
        use crate::descriptor::Encoding;
        use cdk::nuts::nut18::PaymentRequest;
        use std::str::FromStr;
        let directory = tempfile::tempdir()?;
        let absent = directory.path().join("uninitialized-wallet");
        let mut value = fixture();
        value["state_directory"] = serde_json::to_value(&absent)?;
        value["mints"] = json!([{"id":"fixture", "advertised_url":"https://mint.example",
            "policy_version":"receive-v1", "receive_cashu":true,"receive_nutzap":false,"bolt11_exit_qualified":true}]);
        let config = Config::decode(&serde_json::to_vec(&value)?)?;
        let mut original = None;
        for encoding in [Encoding::CreqA, Encoding::CreqB] {
            let descriptor =
                config.prepare_request("https://lightning-goats.example/c", encoding)?;
            let request = PaymentRequest::from_str(descriptor.encoded())?;
            assert_eq!(request.single_use, Some(false));
            assert_eq!(request.amount, None);
            assert_eq!(request.payment_id.as_deref(), Some(descriptor.id()));
            assert_eq!(
                request.transports[0].target,
                "https://lightning-goats.example/c"
            );
            if let Some(id) = &original {
                assert_eq!(descriptor.id(), id);
            } else {
                original = Some(descriptor.id().to_owned());
            }
        }
        for endpoint in [
            "http://example.com/c",
            "https://user:secret@example.com/c",
            "https://example.com/c?secret=value",
        ] {
            assert!(config.prepare_request(endpoint, Encoding::CreqA).is_err());
        }
        assert!(
            !config.receive_cashu
                && !config.execution.acquire
                && !config.treasury_execution.execute
        );
        assert!(config.descriptors.is_empty());
        assert!(
            !absent.exists(),
            "request preparation must not create state or keys"
        );
        Ok(())
    }

    #[test]
    fn public_request_is_generated_from_current_registered_policy_and_hidden_while_paused()
    -> Result<()> {
        use crate::descriptor::Encoding;
        let mut value = fixture();
        value["mints"] = json!([{"id":"fixture","advertised_url":"https://mint.example",
            "policy_version":"receive-v1","receive_cashu":true,"receive_nutzap":false,"bolt11_exit_qualified":true}]);
        let config = Config::decode(&serde_json::to_vec(&value)?)?;
        assert!(config.prepare()?.public_request.is_none());
        let endpoint = "https://lightning-goats.example/api/v1/cashu";
        let original = config.prepare_request(endpoint, Encoding::CreqA)?;
        value["public_request"] = json!({"endpoint":endpoint,"encoding":"creqA"});
        assert!(Config::decode(&serde_json::to_vec(&value)?).is_err());
        value["descriptors"] = json!([original.id(), "retained-old-descriptor"]);
        // Keeping an old descriptor admitted does not make it the active QR.
        assert!(
            Config::decode(&serde_json::to_vec(&value)?)?
                .prepare()?
                .public_request
                .is_none()
        );
        let key = cdk::nuts::SecretKey::from_slice(&[7; 32])?.public_key();
        let keys = cdk::nuts::Keys::new(BTreeMap::from([(8u64.into(), key)]));
        value["keysets"] = json!([{"mint_id":"fixture","id":Id::v1_from_keys(&keys).to_string(),"keys":{"8":key.to_hex()}}]);
        value["receive_cashu"] = json!(true);
        let configured = Config::decode(&serde_json::to_vec(&value)?)?
            .prepare()?
            .public_request
            .unwrap();
        assert_eq!(configured.id(), original.id());
        assert_eq!(configured.encoded(), original.encoded());
        value["public_request"]["encoding"] = json!("creqB");
        let efficient = Config::decode(&serde_json::to_vec(&value)?)?
            .prepare()?
            .public_request
            .unwrap();
        assert_eq!(efficient.id(), original.id());
        assert!(efficient.encoded().starts_with("CREQB1"));
        for (field, replacement) in [
            ("policy_version", "receive-v2"),
            ("advertised_url", "https://other.example"),
        ] {
            let mut changed = value.clone();
            changed["mints"][0][field] = json!(replacement);
            assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        }
        let mut changed = value.clone();
        changed["public_request"]["endpoint"] = json!("https://lightning-goats.example/other");
        assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        let mut changed = value.clone();
        changed["receiving_key"] = json!(key.to_hex());
        assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        let mut changed = value.clone();
        changed["public_request"]["request"] = json!(original.encoded());
        assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        Ok(())
    }

    #[test]
    fn retirement_binds_original_request_mint_policy_endpoint_and_receiver() -> Result<()> {
        use crate::descriptor::Encoding;
        let mut value = fixture();
        let old_mint = json!({"id":"fixture","advertised_url":"https://mint.example/Bitcoin",
            "policy_version":"receive-v1","receive_cashu":true,"receive_nutzap":false,"bolt11_exit_qualified":true});
        value["mints"] = json!([old_mint]);
        let endpoint = "https://lightning-goats.example/api/v1/cashu";
        let old = Config::decode(&serde_json::to_vec(&value)?)?
            .prepare_request(endpoint, Encoding::CreqA)?;
        value["mints"][0]["policy_version"] = json!("receive-v2");
        let current = Config::decode(&serde_json::to_vec(&value)?)?
            .prepare_request(endpoint, Encoding::CreqB)?;
        value["descriptors"] = json!([current.id()]);
        value["public_request"] = json!({"endpoint":endpoint,"encoding":"creqB"});
        value["retired_descriptors"] = json!([{
            "id":old.id(),"endpoint":endpoint,"retired_at":100,"accept_before":200,
            "mints":[{"id":"fixture","advertised_url":"https://mint.example/Bitcoin","policy_version":"receive-v1"}]
        }]);
        assert_eq!(
            Config::decode(&serde_json::to_vec(&value)?)?.retired_descriptors[0].id,
            old.id()
        );
        for (pointer, replacement) in [
            ("/retired_descriptors/0/id", json!(current.id())),
            (
                "/retired_descriptors/0/endpoint",
                json!("https://other.example/c"),
            ),
            ("/retired_descriptors/0/accept_before", json!(100)),
            (
                "/retired_descriptors/0/mints/0/policy_version",
                json!("receive-v2"),
            ),
            (
                "/retired_descriptors/0/mints/0/advertised_url",
                json!("https://mint.example/bitcoin"),
            ),
            (
                "/receiving_key",
                json!(
                    cdk::nuts::SecretKey::from_slice(&[7; 32])?
                        .public_key()
                        .to_hex()
                ),
            ),
            ("/descriptors", json!([current.id(), old.id()])),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                Config::decode(&serde_json::to_vec(&changed)?).is_err(),
                "{pointer}"
            );
        }
        Ok(())
    }

    #[test]
    fn treasury_endpoint_is_separate_default_off_and_can_pause_new_admission() -> Result<()> {
        let mut value = fixture();
        value["treasury_execution"] = json!({"reconcile":true,"maximum_attempts":2});
        // Recovery still needs protected destination/epoch bindings, even with
        // no listener and no authority to create a new mint operation.
        assert!(Config::decode(&serde_json::to_vec(&value)?).is_err());
        value.as_object_mut().unwrap().remove("treasury_execution");
        assert!(
            Config::decode(&serde_json::to_vec(&value)?)?
                .treasury_intake
                .is_none()
        );
        value["mints"] = json!([{"id":"fixture","advertised_url":"https://mint.example",
            "policy_version":"receive-v1","receive_cashu":false,"receive_nutzap":false,"bolt11_exit_qualified":false}]);
        value["treasury_intake"] = json!({"socket":"/run/lightning-goats-cashu/treasury.sock",
            "core_epoch":Uuid::from_u128(42), "destination":{"origin":"https://api.example.com/",
                "account_id":Uuid::from_u128(43),"credential_fingerprint":"ab".repeat(32),"network":"bitcoin"},
            "policies":{"fixture":"treasury-v1"}});
        let config = Config::decode(&serde_json::to_vec(&value)?)?;
        let treasury = config.treasury_intake.as_ref().unwrap();
        assert!(!treasury.enabled && !treasury.accept_new);
        assert_eq!(
            treasury.bindings(&config)?["fixture"].wallet_epoch,
            config.wallet_epoch
        );
        assert!(!config.treasury_execution.execute && !config.treasury_execution.reconcile);
        value["treasury_execution"] = json!({"reconcile":true,"maximum_attempts":2});
        let recovery = Config::decode(&serde_json::to_vec(&value)?)?;
        assert!(!recovery.treasury_intake.as_ref().unwrap().enabled);
        assert!(recovery.treasury_execution.reconcile);
        assert!(!recovery.treasury_execution.execute);
        assert_eq!(recovery.treasury_execution.maximum_input_sats, 0);
        value["treasury_intake"]["enabled"] = json!(true);
        Config::decode(&serde_json::to_vec(&value)?)?; // Status survives paused exit qualification.
        value["treasury_intake"]["accept_new"] = json!(true);
        assert!(Config::decode(&serde_json::to_vec(&value)?).is_err());
        value["mints"][0]["bolt11_exit_qualified"] = json!(true);
        Config::decode(&serde_json::to_vec(&value)?)?;
        for (field, invalid) in [
            ("enabled", json!(false)),
            ("socket", value["intake_socket"].clone()),
            (
                "socket",
                json!("/var/lib/lightning-goats-cashu/treasury.sock"),
            ),
            ("core_epoch", json!(Uuid::nil())),
            ("policies", json!({})),
            ("policies", json!({"unknown":"v1"})),
            ("policies", json!({"fixture":""})),
        ] {
            let mut changed = value.clone();
            changed["treasury_intake"][field] = invalid;
            assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        }
        value["receipt_delivery"] = json!({"socket":"/run/core/receipts.sock", "core_epoch":Uuid::from_u128(99), "deadline_seconds":5});
        assert!(Config::decode(&serde_json::to_vec(&value)?).is_err());
        value["receipt_delivery"]["core_epoch"] = value["treasury_intake"]["core_epoch"].clone();
        Config::decode(&serde_json::to_vec(&value)?)?;
        value["receipt_delivery"]["socket"] = value["treasury_intake"]["socket"].clone();
        assert!(Config::decode(&serde_json::to_vec(&value)?).is_err());
        Ok(())
    }

    #[test]
    fn absent_toggles_are_off_and_invalid_scope_or_bounds_fail_offline() -> Result<()> {
        let original = fixture();
        let config = Config::decode(&serde_json::to_vec(&original)?)?;
        assert!(!config.receive_cashu && !config.receive_nutzap);
        assert!(config.prepare()?.registry.cashu_mints().next().is_none());
        for (key, value) in [
            ("runtime_uid", json!(0)),
            ("core_uid", json!(12345)),
            ("ipc_gid", json!(0)),
            ("wallet_epoch", json!(Uuid::nil())),
            ("proof_count", json!(1025)),
            ("body_bytes", json!(0)),
            ("deadline_seconds", json!(61)),
            ("connections_per_second", json!(65)),
            ("state_directory", json!("relative")),
            (
                "intake_socket",
                json!("/var/lib/lightning-goats-cashu/intake.sock"),
            ),
            ("receive_cashu", json!(true)),
            ("receive_nutzap", json!(true)),
            ("spend_any_invoice", json!(true)),
            (
                "publication_delivery",
                json!({"announcements":true,"base_relays":["wss://relay.example/"],"maximum_attempts":1}),
            ),
            (
                "publication_signing",
                json!({"announcements":true,"socket":"/run/signer/sign.sock","signer_uid":12347,"maximum_attempts":1}),
            ),
        ] {
            let mut changed = original.clone();
            changed[key] = value;
            assert!(
                Config::decode(&serde_json::to_vec(&changed)?).is_err(),
                "accepted {key}"
            );
        }
        let mut changed = original.clone();
        changed["recipient"] = json!(config.prepare()?.receiver.x_only_public_key().to_string());
        assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        assert!(Config::decode(&vec![0; MAX_CONFIG_BYTES + 1]).is_err());
        Ok(())
    }
    #[test]
    fn enabled_receiving_requires_qualified_mints_descriptors_and_pinned_keys() -> Result<()> {
        let mut config = fixture();
        config["receive_cashu"] = json!(true);
        config["receive_nutzap"] = json!(true);
        config["descriptors"] = json!(["fixture-reusable"]);
        config["mints"] = json!([{"id":"fixture","advertised_url":"https://fixture.example",
            "policy_version":"fixture-v1","receive_cashu":true,"receive_nutzap":true,"bolt11_exit_qualified":true}]);
        assert!(Config::decode(&serde_json::to_vec(&config)?).is_err());
        let key = cdk::nuts::SecretKey::from_slice(&[7; 32])?.public_key();
        let keys = cdk::nuts::Keys::new(BTreeMap::from([(8u64.into(), key)]));
        config["keysets"] = json!([{"mint_id":"fixture","id":Id::v1_from_keys(&keys).to_string(),"keys":{"8":key.to_hex()}}]);
        Config::decode(&serde_json::to_vec(&config)?)?;
        let mut unqualified = config.clone();
        unqualified["mints"][0]["bolt11_exit_qualified"] = json!(false);
        assert!(Config::decode(&serde_json::to_vec(&unqualified)?).is_err());
        let mut mismatched = config.clone();
        mismatched["keysets"][0]["mint_id"] = json!("other");
        assert!(Config::decode(&serde_json::to_vec(&mismatched)?).is_err());
        let duplicate = config["keysets"][0].clone();
        config["keysets"].as_array_mut().unwrap().push(duplicate);
        assert!(Config::decode(&serde_json::to_vec(&config)?).is_err());
        Ok(())
    }
}
