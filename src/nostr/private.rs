//! Private gift-wrap transport, separate from the public kind-1 contract.
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Url;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

use super::{MAX_NAK_INPUT, NakClient, PUBLISH_DUMMY_SECRET, SignedNostrEvent, validate_hex};

/// Contains only a completed, verified ciphertext event. No Debug/Serialize:
/// callers must explicitly choose the bytes to persist, not log the recipient.
pub struct PrivateGiftWrap {
    event_id: String,
    json: String,
}
impl PrivateGiftWrap {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn ciphertext_json(&self) -> &str {
        &self.json
    }
}

impl NakClient {
    /// Encrypt through NIP-46 and sign a seal; do not publish or discover keys.
    pub async fn wrap_private_message(
        &self,
        recipient: &str,
        content: &str,
    ) -> Result<PrivateGiftWrap> {
        validate_hex(recipient, 64, "private recipient")?;
        if recipient != recipient.to_ascii_lowercase() || content.is_empty() || content.len() > 4096
        {
            bail!("invalid private message input");
        }
        tokio::fs::create_dir_all(&self.nak_config_path)
            .await
            .map_err(|_| anyhow::anyhow!("private signer runtime unavailable"))?;
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("private message clock invalid")?
            .as_secs();
        let partial = Zeroizing::new(serde_json::to_string(&serde_json::json!({
            "kind":14,"created_at":created_at,"tags":[["p",recipient]],"content":content
        }))?);
        let output = self
            .run_nak(
                [
                    "gift",
                    "wrap",
                    "--use-our-identity-key",
                    "--use-their-identity-key",
                    "-p",
                    recipient,
                ],
                &partial,
                Some((&self.bunker_uri, self.client_key.as_str())),
            )
            .await
            .map_err(|_| anyhow::anyhow!("private message encryption/signing failed"))?;
        self.restore_private_wrap(recipient, output.trim()).await
    }

    /// Validate restored ciphertext before retry. Never obtains a signer key.
    pub async fn restore_private_wrap(
        &self,
        recipient: &str,
        raw: &str,
    ) -> Result<PrivateGiftWrap> {
        if raw.len() > MAX_NAK_INPUT {
            bail!("private ciphertext exceeds limit");
        }
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|_| anyhow::anyhow!("invalid private ciphertext event"))?;
        let fields = [
            "id",
            "pubkey",
            "created_at",
            "kind",
            "tags",
            "content",
            "sig",
        ];
        if !value.as_object().is_some_and(|object| {
            object.len() == fields.len() && object.keys().all(|key| fields.contains(&key.as_str()))
        }) {
            bail!("unexpected private ciphertext fields");
        }
        let event: SignedNostrEvent = serde_json::from_str(raw)
            .map_err(|_| anyhow::anyhow!("invalid private ciphertext event"))?;
        validate_private(&event, recipient)?;
        if event.pubkey == self.project_pubkey {
            bail!("private wrapper must use ephemeral identity");
        }
        self.run_nak(["verify"], raw, None)
            .await
            .map_err(|_| anyhow::anyhow!("private ciphertext signature rejected"))?;
        // Keep these exact bytes for every retry, including after database reopen.
        Ok(PrivateGiftWrap {
            event_id: event.id,
            json: raw.to_owned(),
        })
    }

    /// Publish only validated saved ciphertext to the explicitly supplied DM
    /// inbox relays. There is intentionally no announcement-relay fallback.
    pub async fn publish_private_wrap(
        &self,
        recipient: &str,
        relays: &[String],
        wrap: &PrivateGiftWrap,
    ) -> Result<()> {
        validate_private_relays(relays)?;
        self.restore_private_wrap(recipient, &wrap.json).await?;
        let mut args = vec!["event", "--sec", PUBLISH_DUMMY_SECRET];
        args.extend(relays.iter().map(String::as_str));
        let output = self
            .run_nak(args, &wrap.json, None)
            .await
            .map_err(|_| anyhow::anyhow!("private ciphertext publication failed"))?;
        let echoed: SignedNostrEvent = serde_json::from_str(output.trim())
            .map_err(|_| anyhow::anyhow!("invalid private publication result"))?;
        let saved: SignedNostrEvent = serde_json::from_str(&wrap.json)
            .map_err(|_| anyhow::anyhow!("invalid saved private event"))?;
        if echoed != saved {
            bail!("private publication changed saved event");
        }
        Ok(())
    }
}

fn validate_private(event: &SignedNostrEvent, recipient: &str) -> Result<()> {
    validate_hex(recipient, 64, "private recipient")?;
    validate_hex(&event.id, 64, "private event id")?;
    validate_hex(&event.pubkey, 64, "private wrapper pubkey")?;
    validate_hex(&event.sig, 128, "private wrapper signature")?;
    if event.kind != 1059
        || event.created_at == 0
        || event.tags != vec![vec!["p".to_owned(), recipient.to_owned()]]
    {
        bail!("private event kind/recipient mismatch");
    }
    let ciphertext = STANDARD
        .decode(&event.content)
        .map_err(|_| anyhow::anyhow!("invalid private ciphertext encoding"))?;
    // NIP-44 v2: version + 32-byte nonce + padded ciphertext + 32-byte MAC.
    if ciphertext.len() < 99 || ciphertext[0] != 2 {
        bail!("private ciphertext is not NIP-44 v2");
    }
    Ok(())
}

fn validate_private_relays(relays: &[String]) -> Result<()> {
    if relays.is_empty() || relays.len() > 8 {
        bail!("private inbox relay list required");
    }
    for raw in relays {
        let url = Url::parse(raw).map_err(|_| anyhow::anyhow!("invalid private inbox relay"))?;
        let loopback = url
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if raw.len() > 2048
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
            || url.fragment().is_some()
            || url.query().is_some()
            || !(url.scheme() == "wss" || (url.scheme() == "ws" && loopback))
        {
            bail!("invalid private inbox relay");
        }
    }
    Ok(())
}
