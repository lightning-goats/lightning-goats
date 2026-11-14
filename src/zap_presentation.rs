//! Presentation identity comes from a settled, invoice-bound NIP-57 request.
//! Neither a wallet label nor arbitrary event text identifies the zapper.
use anyhow::{Context, Result, bail};

use crate::{core_bridge::Payment, zap::ZapRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ZapAttribution {
    sender: Option<String>,
    amount_sats: u64,
    identity: String,
}

impl ZapAttribution {
    pub(crate) fn from_payment(payment: &Payment) -> Result<Option<Self>> {
        let Some(zap) = &payment.zap else {
            return Ok(None);
        };
        if payment.source != "strike" {
            bail!("zap presentation requires authoritative Strike settlement");
        }
        let request = ZapRequest::validate(
            &zap.request_json,
            &zap.payee_pubkey,
            &zap.receipt_pubkey,
            payment.amount_msat,
        )?;
        if request.description_hash() != zap.description_hash {
            bail!("zap presentation request differs from the invoice description");
        }
        let invoice: lightning_invoice::Bolt11Invoice = zap.invoice.parse()?;
        crate::bolt11::verify(
            &zap.invoice,
            payment.amount_msat,
            &invoice.payment_hash().to_string(),
            &zap.description_hash,
        )?;
        if payment
            .payment_hash
            .as_ref()
            .is_some_and(|h| h != &invoice.payment_hash().to_string())
        {
            bail!("zap presentation settlement hash differs from its invoice");
        }
        let anonymous = request
            .event()
            .event()
            .tags
            .iter()
            .any(|tag| tag.first().is_some_and(|name| name == "anon"));
        Ok(Some(Self {
            sender: (!anonymous).then(|| request.event().event().pubkey.clone()),
            amount_sats: payment.amount_msat / 1000,
            identity: crate::cyberherd::ingress::hash(&(
                "zap-presentation",
                &payment.source,
                &payment.source_id,
            ))?,
        }))
    }

    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }
    pub(crate) fn amount_sats(&self) -> u64 {
        self.amount_sats
    }
    pub(crate) fn sender(&self) -> Option<&str> {
        self.sender.as_deref()
    }
    pub(crate) fn anonymous(&self) -> bool {
        self.sender.is_none()
    }
    pub(crate) fn nostr_name(&self) -> Result<String> {
        self.sender()
            .map(crate::cyberherd::delivery::mention)
            .unwrap_or_else(|| Ok("an anonymous zapper".into()))
    }
    pub(crate) fn overlay_name(
        &self,
        profile: Option<&crate::cyberherd::membership::Profile>,
    ) -> Result<String> {
        if self.anonymous() {
            return Ok("an anonymous zapper".into());
        }
        if let Some(profile) = profile.filter(|p| !p.name.trim().is_empty()) {
            return Ok(crate::cyberherd::delivery::display_name(profile));
        }
        let mention = self.nostr_name()?;
        let npub = mention
            .strip_prefix("nostr:")
            .context("missing public mention prefix")?;
        Ok(format!("{}…{}", &npub[..12], &npub[npub.len() - 6..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core_bridge::SettledZap, nostr_event::test_event};

    fn payment(anonymous: bool) -> Payment {
        let payee = test_event(2, 1, 100, "", vec![]).pubkey;
        let signer = test_event(3, 1, 100, "", vec![]).pubkey;
        let mut tags = vec![
            vec!["p".into(), payee.clone()],
            vec!["amount".into(), "21000".into()],
            vec!["relays".into(), "wss://relay.damus.io".into()],
        ];
        if anonymous {
            tags.push(vec!["anon".into()]);
        }
        let raw =
            serde_json::to_string(&test_event(1, 9734, 100, "I am somebody else", tags)).unwrap();
        let request = ZapRequest::validate(&raw, &payee, &signer, 21000).unwrap();
        let hash = request.description_hash();
        Payment {
            settlement_id: 1,
            source: "strike".into(),
            source_id: "synthetic-settlement-one".into(),
            payment_hash: Some("22".repeat(32)),
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 21000,
            paid_at: 100,
            zap: Some(SettledZap {
                request_json: raw,
                payee_pubkey: payee,
                receipt_pubkey: signer,
                invoice: crate::test_invoices::invoice(21000, &hash),
                description_hash: hash,
            }),
        }
    }

    #[test]
    fn invoice_bound_author_not_comment_identifies_the_zapper() {
        let payment = payment(false);
        let attribution = ZapAttribution::from_payment(&payment).unwrap().unwrap();
        let expected = test_event(1, 1, 100, "", vec![]).pubkey;
        assert_eq!(attribution.sender(), Some(expected.as_str()));
        assert_eq!(attribution.amount_sats(), 21);
        assert!(attribution.nostr_name().unwrap().starts_with("nostr:npub1"));
        assert!(!attribution.nostr_name().unwrap().contains("somebody"));
        let mut second = payment.clone();
        second.source_id = "another-paid-invoice".into();
        assert_ne!(
            attribution.identity(),
            ZapAttribution::from_payment(&second)
                .unwrap()
                .unwrap()
                .identity()
        );
        for field in ["source", "amount", "hash", "description", "signature"] {
            let mut forged = payment.clone();
            match field {
                "source" => forged.source = "operator".into(),
                "amount" => forged.amount_msat = 22000,
                "hash" => forged.payment_hash = Some("33".repeat(32)),
                "description" => forged.zap.as_mut().unwrap().description_hash = "44".repeat(32),
                "signature" => {
                    forged.zap.as_mut().unwrap().request_json = forged
                        .zap
                        .as_ref()
                        .unwrap()
                        .request_json
                        .replace("I am somebody else", "I am the operator")
                }
                _ => unreachable!(),
            }
            assert!(ZapAttribution::from_payment(&forged).is_err(), "{field}");
        }
    }

    #[test]
    fn anonymous_zaps_never_identify_the_outer_signing_key() {
        let attribution = ZapAttribution::from_payment(&payment(true))
            .unwrap()
            .unwrap();
        assert!(attribution.anonymous());
        assert_eq!(attribution.sender(), None);
        assert_eq!(attribution.nostr_name().unwrap(), "an anonymous zapper");
        let profile = crate::cyberherd::membership::Profile {
            name: "Do not disclose".into(),
            ..Default::default()
        };
        assert_eq!(
            attribution.overlay_name(Some(&profile)).unwrap(),
            "an anonymous zapper"
        );
        let mut ordinary = payment(false);
        ordinary.zap = None;
        assert!(ZapAttribution::from_payment(&ordinary).unwrap().is_none());
    }

    #[test]
    fn payment_copy_mentions_actual_zap_and_preserves_announcement_suppression() {
        let attribution = ZapAttribution::from_payment(&payment(false))
            .unwrap()
            .unwrap();
        let renderer = crate::presentation::MessageRenderer::embedded().unwrap();
        let mut event = crate::ledger::DurableEvent {
            seq: 1,
            event_type: "payment_received".into(),
            payload_json:
                serde_json::json!({"amount_sats":21,"feed_credit_sats":21,"address_user":"herd"})
                    .to_string(),
        };
        let rendered = renderer
            .render_with_zap(&event, 1000, Some(&attribution))
            .unwrap();
        let text = rendered.nostr_content.unwrap();
        assert!(text.contains(&attribution.nostr_name().unwrap()));
        assert!(text.contains("21"));
        assert!(text.to_lowercase().contains("zap"));
        assert!(!rendered.overlay_message.unwrap().contains("nostr:npub"));
        event.payload_json = serde_json::json!({"amount_sats":21,"feed_credit_sats":1000,"feeding":{"paused":false,"reason":"ready","awaiting_confirmation":false,"threshold_sats":1000}}).to_string();
        assert!(
            renderer
                .render_with_zap(&event, 1000, Some(&attribution))
                .unwrap()
                .nostr_content
                .is_none()
        );
        let mut small: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
        small["amount_sats"] = 1.into();
        event.payload_json = small.to_string();
        assert!(
            renderer
                .render_with_zap(&event, 1000, None)
                .unwrap()
                .nostr_content
                .is_none()
        );
    }

    #[tokio::test]
    async fn editable_zap_feeding_templates_keep_custom_words_and_durable_opt_out() {
        use crate::cyberherd::{
            feeding_delivery::Proof,
            membership::{Profile, Settings},
            store::{Store, TemplateEdit},
            templates::Catalog,
        };
        use crate::presentation::MessageRenderer;
        let root = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("templates.db").display());
        let store = Store::connect(&url).await.unwrap();
        let owner = test_event(3, 1, 100, "", vec![]).pubkey;
        store
            .initialize(
                &owner,
                &Settings {
                    minimum_sats: 10,
                    max_members: 3,
                    member_percent: 10,
                },
                100,
            )
            .await
            .unwrap();
        store
            .seed_templates(&owner, &Catalog::legacy().unwrap())
            .await
            .unwrap();
        store
            .seed_templates(&owner, &Catalog::delivery_defaults().unwrap())
            .await
            .unwrap();
        let event = crate::ledger::DurableEvent {
            seq: 5,
            event_type: "feeder_confirmed".into(),
            payload_json: "{}".into(),
        };
        let proof = Proof {
            owner: owner.clone(),
            identity: "55".repeat(32),
            source_seq: 5,
            triggering_payment_amount_sats: Some(21),
            feed_credit_sats: 9,
        };
        let profile = Profile {
            name: "Alice".into(),
            ..Default::default()
        };
        for anonymous in [false, true] {
            let attribution = ZapAttribution::from_payment(&payment(anonymous))
                .unwrap()
                .unwrap();
            let rendered = MessageRenderer::editable_feeding_with_zap(
                &store.templates(&owner).await.unwrap(),
                &event,
                &proof,
                1000,
                Some(&attribution),
                Some(&profile),
            )
            .unwrap()
            .unwrap();
            assert!(rendered.nostr_content.as_ref().unwrap().contains("21"));
            assert!(
                !rendered
                    .nostr_content
                    .as_ref()
                    .unwrap()
                    .contains("1000 sats")
            );
            let overlay = rendered.overlay_message.unwrap();
            assert_eq!(
                rendered.overlay_goats.len(),
                usize::from(overlay.contains("Goats like"))
            );
            if anonymous {
                assert!(overlay.contains("anonymous zapper"));
                assert!(!overlay.contains("Alice"));
                let public_sender = ZapAttribution::from_payment(&payment(false))
                    .unwrap()
                    .unwrap()
                    .nostr_name()
                    .unwrap();
                assert!(!rendered.nostr_content.unwrap().contains(&public_sender));
            } else {
                assert!(overlay.contains("Alice"));
                assert!(
                    rendered
                        .nostr_content
                        .unwrap()
                        .contains(&attribution.nostr_name().unwrap())
                );
            }
        }
        let edit = TemplateEdit::Set {
            category: "feeder_trigger_zap".into(),
            variant: "0".into(),
            text: "Custom thanks to {name} for {new_amount} sats!".into(),
        };
        store
            .edit_template(&owner, "custom-zap-template", &edit)
            .await
            .unwrap();
        store
            .seed_templates(&owner, &Catalog::delivery_defaults().unwrap())
            .await
            .unwrap();
        assert_eq!(
            store
                .templates(&owner)
                .await
                .unwrap()
                .category("feeder_trigger_zap")
                .unwrap()["0"],
            "Custom thanks to {name} for {new_amount} sats!"
        );
        store
            .edit_template(
                &owner,
                "disable-zap-template",
                &TemplateEdit::DisableCategory {
                    category: "feeder_trigger_zap".into(),
                },
            )
            .await
            .unwrap();
        drop(store);
        let reopened = Store::connect(&url).await.unwrap();
        reopened
            .seed_templates(&owner, &Catalog::delivery_defaults().unwrap())
            .await
            .unwrap();
        let attribution = ZapAttribution::from_payment(&payment(false))
            .unwrap()
            .unwrap();
        assert!(
            MessageRenderer::editable_feeding_with_zap(
                &reopened.templates(&owner).await.unwrap(),
                &event,
                &proof,
                1000,
                Some(&attribution),
                None
            )
            .unwrap()
            .is_none()
        );
    }
}
