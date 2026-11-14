use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{cyberherd::templates::Catalog, ledger::DurableEvent};

pub const PAYMENT_MESSAGE_MIN_SATS: u64 = 10;

#[derive(Clone)]
pub struct MessageRenderer {
    catalog: Arc<TemplateCatalog>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPresentation {
    pub nostr_content: Option<String>,
    pub overlay_type: Option<String>,
    pub overlay_message: Option<String>,
    pub overlay_goats: Vec<OverlayGoat>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayGoat {
    pub name: String,
    #[serde(rename = "imageUrl")]
    pub image_url: String,
}

#[derive(Debug)]
struct TemplateCatalog {
    sats_received: Vec<String>,
    feeder_trigger: Vec<String>,
    interface_info: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct Goat {
    user: &'static str,
    name: &'static str,
    nostr_profile: &'static str,
}

const GOATS: [Goat; 5] = [
    Goat {
        user: "dexter",
        name: "Dexter",
        nostr_profile: "nostr:nprofile1qqsw4zlzyfx43mc88psnlse8sywpfl45kuap9dy05yzkepkvu6ca5wg7qyak5",
    },
    Goat {
        user: "rowan",
        name: "Rowan",
        nostr_profile: "nostr:nprofile1qqs2w94r0fs29gepzfn5zuaupn969gu3fstj3gq8kvw3cvx9fnxmaugwur22r",
    },
    Goat {
        user: "cosmo",
        name: "Cosmo",
        nostr_profile: "nostr:nprofile1qqsq6n8u7dzrnhhy7xy78k2ee7e4wxlgrkm5g2rgjl3napr9q54n4ncvkqcsj",
    },
    Goat {
        user: "newton",
        name: "Newton",
        nostr_profile: "nostr:nprofile1qqszdsnpyzwhjcqads3hwfywt5jfmy85jvx8yup06yq0klrh93ldjxc26lmyx",
    },
    Goat {
        user: "nova",
        name: "Nova",
        nostr_profile: "nostr:nprofile1qqsrzy7clymq5xwcfhh0dfz6zfe7h63k8r0j8yr49mxu6as4yv2084s0vf035",
    },
];

impl MessageRenderer {
    pub fn embedded() -> Result<Self> {
        let legacy = Catalog::legacy()?;
        let catalog = TemplateCatalog {
            sats_received: legacy.category_templates("sats_received"),
            feeder_trigger: legacy.category_templates("feeder_trigger"),
            interface_info: legacy.category_templates("interface_info"),
        };
        validate_catalog(&catalog)?;
        Ok(Self {
            catalog: Arc::new(catalog),
        })
    }

    pub fn render(
        &self,
        event: &DurableEvent,
        threshold_sats: u64,
    ) -> Result<RenderedPresentation> {
        if threshold_sats == 0 {
            bail!("feeder threshold must be greater than zero");
        }
        let payload: Value = serde_json::from_str(&event.payload_json)
            .context("durable event contains invalid JSON for presentation rendering")?;
        let object = payload
            .as_object()
            .context("durable event payload must be a JSON object")?;

        let pause_note = feeding_pause_note(object.get("feeding"))?;
        let payment = matches!(
            event.event_type.as_str(),
            "payment_received" | "operator_credit_added"
        );
        let mut rendered = match event.event_type.as_str() {
            "payment_received" | "operator_credit_added" => {
                self.render_payment(event, object, threshold_sats, pause_note.as_deref())
            }
            "feeder_confirmed" => self.render_feeder(event, object, threshold_sats),
            "interface_info" => self.render_interface_info(event),
            "weather_status" => self.render_weather(object),
            _ => Ok(RenderedPresentation::empty()),
        }?;
        if let Some(note) = pause_note.filter(|_| !payment) {
            if let Some(message) = rendered.nostr_content.as_mut() {
                *message = format!("{note} {message}");
            }
        }
        Ok(rendered)
    }

    fn render_payment(
        &self,
        event: &DurableEvent,
        payload: &serde_json::Map<String, Value>,
        threshold_sats: u64,
        pause_note: Option<&str>,
    ) -> Result<RenderedPresentation> {
        let amount = required_u64(payload, "amount_sats")?;
        if amount < PAYMENT_MESSAGE_MIN_SATS {
            return Ok(RenderedPresentation::empty());
        }
        let credit = required_u64(payload, "feed_credit_sats")?;
        if payment_funds_ready_feeding(payload, amount, credit) {
            // Keep the payment/progress notification, but let the independently
            // confirmed feeding own the text announcement. This never claims
            // success or authorizes a feeding from presentation state.
            return Ok(RenderedPresentation {
                nostr_content: None,
                overlay_type: Some("sats_received".to_owned()),
                overlay_message: None,
                overlay_goats: Vec::new(),
            });
        }
        let address_user = payload
            .get("address_user")
            .and_then(Value::as_str)
            .unwrap_or("herd");
        let goat = goat_for_event(event, Some(address_user));
        let difference_message = difference_message(credit, threshold_sats);
        let template = select_template(&self.catalog.sats_received, event, "sats_received")?;

        let mut overlay_values = common_values(amount, &difference_message, goat.name);
        overlay_values.insert(
            "difference".to_owned(),
            remaining_sats(credit, threshold_sats).to_string(),
        );
        let nostr_difference = if let Some(note) = pause_note {
            if credit < threshold_sats {
                format!(
                    "{note} Your sats are saved, with {} more needed to fund the next feeding.",
                    threshold_sats - credit
                )
            } else if credit % threshold_sats != 0 {
                format!(
                    "{note} Another {} sats are saved toward the next feeding.",
                    credit % threshold_sats
                )
            } else {
                note.to_owned()
            }
        } else {
            difference_message
        };
        let mut nostr_values = common_values(amount, &nostr_difference, goat.nostr_profile);
        nostr_values.insert(
            "difference".to_owned(),
            remaining_sats(credit, threshold_sats).to_string(),
        );

        let mut nostr_content = safe_substitute(template, &nostr_values)?;
        if let Some(note) = pause_note.filter(|_| !template.contains("{difference_message}")) {
            // Custom templates may omit progress, but still need pause context.
            nostr_content = format!("{note} {nostr_content}");
        }
        Ok(RenderedPresentation {
            nostr_content: Some(nostr_content),
            overlay_type: Some("sats_received".to_owned()),
            overlay_message: Some(safe_substitute(template, &overlay_values)?),
            overlay_goats: vec![overlay_goat(goat)],
        })
    }

    fn render_feeder(
        &self,
        event: &DurableEvent,
        payload: &serde_json::Map<String, Value>,
        threshold_sats: u64,
    ) -> Result<RenderedPresentation> {
        if payload.get("announce_feeding").and_then(Value::as_bool) == Some(false) {
            return Ok(RenderedPresentation {
                nostr_content: None,
                overlay_type: None,
                overlay_message: None,
                overlay_goats: vec![],
            });
        }
        if payload.get("presentation_owner").is_some() {
            return Ok(RenderedPresentation {
                nostr_content: None,
                overlay_type: Some("feeder_trigger".into()),
                overlay_message: None,
                overlay_goats: vec![],
            });
        }
        let amount = payload
            .get("triggering_payment_amount_sats")
            .map(|value| {
                value
                    .as_u64()
                    .filter(|amount| *amount > 0)
                    .context("invalid triggering payment amount for feeding presentation")
            })
            .transpose()?;
        let credit = required_u64(payload, "feed_credit_sats")?;
        let goat = goat_for_event(event, None);
        let difference_message = difference_message(credit, threshold_sats);
        let template = if amount.is_some() {
            select_template(&self.catalog.feeder_trigger, event, "feeder_trigger")?
        } else {
            "⚡ The Lightning Goats have been fed! {difference_message}\n\nhttps://lightning-goats.com"
        };

        let overlay_values = common_values(amount.unwrap_or(0), &difference_message, goat.name);
        let nostr_values =
            common_values(amount.unwrap_or(0), &difference_message, goat.nostr_profile);

        Ok(RenderedPresentation {
            nostr_content: Some(safe_substitute(template, &nostr_values)?),
            overlay_type: Some("feeder_trigger".to_owned()),
            overlay_message: Some(safe_substitute(template, &overlay_values)?),
            overlay_goats: vec![overlay_goat(goat)],
        })
    }

    pub(crate) fn editable_feeding(
        catalog: &Catalog,
        event: &DurableEvent,
        proof: &crate::cyberherd::feeding_delivery::Proof,
        threshold: u64,
    ) -> Result<Option<RenderedPresentation>> {
        if catalog.category("feeder_trigger").is_none() {
            return Ok(None);
        }
        let goat = goat_for_event(event, None);
        let difference = difference_message(proof.feed_credit_sats, threshold);
        let compose = |name: &str| -> Result<Option<String>> {
            if let Some(amount) = proof.triggering_payment_amount_sats {
                catalog.render(
                    "feeder_trigger",
                    &proof.identity,
                    &common_values(amount, &difference, name)
                        .into_iter()
                        .collect(),
                )
            } else {
                catalog.render(
                    "feeder_confirmed_unattributed",
                    &proof.identity,
                    &std::collections::BTreeMap::from([
                        ("difference_message".into(), difference.clone()),
                        ("goat_name".into(), name.into()),
                    ]),
                )
            }
        };
        let (Some(nostr), Some(overlay)) = (compose(goat.nostr_profile)?, compose(goat.name)?)
        else {
            return Ok(None);
        };
        Ok(Some(RenderedPresentation {
            nostr_content: Some(nostr),
            overlay_type: Some("feeder_trigger".into()),
            overlay_message: Some(overlay),
            overlay_goats: vec![overlay_goat(goat)],
        }))
    }

    fn render_interface_info(&self, event: &DurableEvent) -> Result<RenderedPresentation> {
        let template = select_template(&self.catalog.interface_info, event, "interface_info")?;
        Ok(RenderedPresentation {
            nostr_content: None,
            overlay_type: Some("interface_info".to_owned()),
            overlay_message: Some(safe_substitute(template, &HashMap::new())?),
            overlay_goats: Vec::new(),
        })
    }

    fn render_weather(
        &self,
        payload: &serde_json::Map<String, Value>,
    ) -> Result<RenderedPresentation> {
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .context("weather_status event is missing string field message")?;
        if message.trim().is_empty() || message.len() > 2_000 {
            bail!("weather_status message must contain 1 to 2000 characters");
        }
        Ok(RenderedPresentation {
            nostr_content: None,
            overlay_type: Some("weather_status".to_owned()),
            overlay_message: Some(message.to_owned()),
            overlay_goats: Vec::new(),
        })
    }
}

fn payment_funds_ready_feeding(
    payload: &serde_json::Map<String, Value>,
    amount: u64,
    credit: u64,
) -> bool {
    let Some(feeding) = payload.get("feeding") else {
        return false;
    };
    if feeding["paused"].as_bool() != Some(false)
        || feeding["reason"].as_str() != Some("ready")
        || feeding["awaiting_confirmation"].as_bool() != Some(false)
    {
        return false;
    }
    let Some(threshold) = feeding["threshold_sats"].as_u64().filter(|v| *v > 0) else {
        return false;
    };
    let Some(previous_credit) = credit.checked_sub(amount) else {
        return false;
    };
    credit / threshold > previous_credit / threshold
}

impl RenderedPresentation {
    fn empty() -> Self {
        Self {
            nostr_content: None,
            overlay_type: None,
            overlay_message: None,
            overlay_goats: Vec::new(),
        }
    }
}

fn validate_catalog(catalog: &TemplateCatalog) -> Result<()> {
    for (name, templates) in [
        ("sats_received", &catalog.sats_received),
        ("feeder_trigger", &catalog.feeder_trigger),
        ("interface_info", &catalog.interface_info),
    ] {
        if templates.is_empty() {
            bail!("Phase 1 template category {name} is empty");
        }
        if templates.len() > 1_000 {
            bail!("Phase 1 template category {name} is unexpectedly large");
        }
        for template in templates {
            if template.trim().is_empty() || template.len() > 8_192 {
                bail!("Phase 1 template category {name} contains an invalid template length");
            }
            validate_template_syntax(template)?;
        }
    }
    Ok(())
}

fn validate_template_syntax(template: &str) -> Result<()> {
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let close = after_open
            .find('}')
            .context("template contains an unmatched opening brace")?;
        let field = &after_open[..close];
        if field.is_empty()
            || !field
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || field.as_bytes()[0].is_ascii_digit()
        {
            bail!("template contains unsafe placeholder {{{field}}}");
        }
        rest = &after_open[close + 1..];
    }
    if rest.contains('}') {
        bail!("template contains an unmatched closing brace");
    }
    Ok(())
}

fn safe_substitute(template: &str, values: &HashMap<String, String>) -> Result<String> {
    validate_template_syntax(template)?;
    let mut rendered = String::with_capacity(template.len() + 64);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        rendered.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let close = after_open
            .find('}')
            .context("template contains an unmatched opening brace")?;
        let field = &after_open[..close];
        let value = values
            .get(field)
            .with_context(|| format!("template requires unavailable field {field}"))?;
        rendered.push_str(value);
        rest = &after_open[close + 1..];
    }
    rendered.push_str(rest);
    Ok(rendered)
}

fn select_template<'a>(
    templates: &'a [String],
    event: &DurableEvent,
    purpose: &str,
) -> Result<&'a str> {
    if templates.is_empty() {
        bail!("template category {purpose} is empty");
    }
    let index = stable_index(event, purpose, templates.len());
    Ok(templates[index].as_str())
}

fn stable_index(event: &DurableEvent, purpose: &str, len: usize) -> usize {
    let mut hasher = Sha256::new();
    hasher.update(event.seq.to_be_bytes());
    hasher.update(event.event_type.as_bytes());
    hasher.update([0]);
    hasher.update(purpose.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    (u64::from_be_bytes(bytes) % len as u64) as usize
}

fn goat_for_event(event: &DurableEvent, address_user: Option<&str>) -> &'static Goat {
    if let Some(user) = address_user
        && let Some(goat) = GOATS.iter().find(|goat| goat.user == user)
    {
        return goat;
    }
    &GOATS[stable_index(event, "goat", GOATS.len())]
}

fn overlay_goat(goat: &Goat) -> OverlayGoat {
    OverlayGoat {
        name: goat.name.to_owned(),
        image_url: format!("images/{}.png", goat.user),
    }
}

fn common_values(
    amount: u64,
    difference_message: &str,
    goat_name: &str,
) -> HashMap<String, String> {
    HashMap::from([
        ("new_amount".to_owned(), amount.to_string()),
        (
            "difference_message".to_owned(),
            difference_message.to_owned(),
        ),
        ("goat_name".to_owned(), goat_name.to_owned()),
    ])
}

fn required_u64(payload: &serde_json::Map<String, Value>, field: &str) -> Result<u64> {
    payload
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("durable event is missing unsigned integer field {field}"))
}

fn feeding_pause_note(value: Option<&Value>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let paused = value["paused"]
        .as_bool()
        .context("invalid feeding pause context")?;
    if !paused {
        return Ok(None);
    }
    let queued = value["queued_feedings"]
        .as_u64()
        .context("invalid queued feeding count")?;
    let pending = value["awaiting_confirmation"]
        .as_bool()
        .context("invalid pending feeding context")?;
    let prefix = if value["reason"] == "unavailable" {
        "Feeding is paused while feeder status is unavailable."
    } else {
        "Feeding is paused."
    };
    let queue = if queued == 0 {
        String::new()
    } else {
        format!(
            " We have {queued} feeding{} lined up for when feeding resumes.",
            if queued == 1 { "" } else { "s" }
        )
    };
    let pending_note = if pending {
        " We're waiting for confirmation of an earlier feeding before the queue can continue."
    } else {
        ""
    };
    Ok(Some(format!("{prefix}{queue}{pending_note}")))
}

fn remaining_sats(credit: u64, threshold: u64) -> u64 {
    if credit < threshold {
        threshold - credit
    } else {
        threshold - (credit % threshold)
    }
}

fn difference_message(credit: u64, threshold: u64) -> String {
    if credit < threshold {
        return format!(
            "{} sats until the next feeding is funded.",
            threshold - credit
        );
    }
    let feeds_due = credit / threshold;
    let remainder = credit % threshold;
    let feeding = if feeds_due == 1 {
        "feeding is"
    } else {
        "feedings are"
    };
    if remainder == 0 {
        format!("{feeds_due} {feeding} funded and queued.")
    } else {
        format!(
            "{feeds_due} {feeding} funded and queued, with {remainder} sats already toward the following feeding."
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(seq: u64, event_type: &str, payload: Value) -> DurableEvent {
        DurableEvent {
            seq,
            event_type: event_type.to_owned(),
            payload_json: payload.to_string(),
        }
    }

    #[test]
    fn confirmed_feeding_uses_payment_amount_and_legacy_events_do_not_claim_threshold_payment() {
        let renderer = MessageRenderer::embedded().unwrap();
        for amount in [5, 10, 2340] {
            let rendered = renderer
                .render(
                    &event(
                        43,
                        "feeder_confirmed",
                        json!({"threshold_sats":1000,"feed_credit_sats":5,
                "triggering_payment_amount_sats":amount}),
                    ),
                    1000,
                )
                .unwrap();
            assert!(
                rendered
                    .nostr_content
                    .unwrap()
                    .contains(&format!("{amount} sats"))
            );
            assert!(
                rendered
                    .overlay_message
                    .unwrap()
                    .contains(&format!("{amount} sats"))
            );
        }
        let legacy = renderer
            .render(
                &event(
                    43,
                    "feeder_confirmed",
                    json!({"threshold_sats":1000,"feed_credit_sats":5}),
                ),
                1000,
            )
            .unwrap();
        for message in [
            legacy.nostr_content.unwrap(),
            legacy.overlay_message.unwrap(),
        ] {
            assert!(!message.contains("1000 sats"));
            assert!(message.contains("995 sats"));
        }
        assert!(
            renderer
                .render(
                    &event(
                        43,
                        "feeder_confirmed",
                        json!({"threshold_sats":1000,"feed_credit_sats":5,
            "triggering_payment_amount_sats":0})
                    ),
                    1000
                )
                .is_err()
        );
    }

    #[test]
    fn ready_funded_payment_defers_text_to_confirmation_but_keeps_overlay_payment() {
        let renderer = MessageRenderer::embedded().unwrap();
        for kind in ["payment_received", "operator_credit_added"] {
            for (amount, credit) in [(1000, 1305), (10, 1000), (2345, 2650)] {
                let payload = json!({"amount_sats":amount,"feed_credit_sats":credit,
                    "feeding":{"paused":false,"reason":"ready","threshold_sats":1000,
                    "awaiting_confirmation":false}});
                let source = event(42, kind, payload);
                let rendered = renderer.render(&source, 1000).unwrap();
                assert!(rendered.nostr_content.is_none());
                assert!(rendered.overlay_message.is_none());
                assert_eq!(rendered.overlay_type.as_deref(), Some("sats_received"));
                // Use the frozen funding threshold, even after a config change.
                assert_eq!(renderer.render(&source, 2000).unwrap(), rendered);
            }
        }
        let confirmed = renderer
            .render(
                &event(
                    43,
                    "feeder_confirmed",
                    json!({"threshold_sats":1000,"feed_credit_sats":305}),
                ),
                1000,
            )
            .unwrap();
        assert!(confirmed.nostr_content.is_some());
        assert!(confirmed.overlay_message.is_some());
    }

    #[test]
    fn paused_uncertain_and_nonfunding_payments_keep_receipt_announcements() {
        let renderer = MessageRenderer::embedded().unwrap();
        let ready = json!({"paused":false,"reason":"ready","threshold_sats":1000,
            "awaiting_confirmation":false});
        for (amount, credit, feeding) in [
            (10, 999, ready.clone()),
            (10, 1315, ready.clone()),
            (
                1000,
                1305,
                json!({"paused":true,"reason":"override","threshold_sats":1000,
                "queued_feedings":1,"awaiting_confirmation":false}),
            ),
            (
                1000,
                1305,
                json!({"paused":true,"reason":"unavailable","threshold_sats":1000,
                "queued_feedings":1,"awaiting_confirmation":false}),
            ),
            (
                1000,
                1305,
                json!({"paused":false,"reason":"ready","threshold_sats":1000,
                "awaiting_confirmation":true}),
            ),
            (1000, 1305, Value::Null),
            (1000, 1305, json!({"paused":false,"reason":"ready"})),
            (
                1000,
                1305,
                json!({"paused":false,"reason":"ready","threshold_sats":0,
                "awaiting_confirmation":false}),
            ),
            (1000, 305, ready),
        ] {
            let mut payload = json!({"amount_sats":amount,"feed_credit_sats":credit});
            if !feeding.is_null() {
                payload["feeding"] = feeding;
            }
            let rendered = renderer
                .render(&event(42, "payment_received", payload), 1000)
                .unwrap();
            assert!(rendered.nostr_content.is_some());
            assert!(rendered.overlay_message.is_some());
        }
    }

    #[test]
    fn same_event_renders_identically_across_retries() {
        let renderer = MessageRenderer::embedded().unwrap();
        let event = event(
            42,
            "payment_received",
            json!({
                "amount_sats": 250,
                "feed_credit_sats": 750,
                "address_user": "herd"
            }),
        );
        assert_eq!(
            renderer.render(&event, 1_000).unwrap(),
            renderer.render(&event, 1_000).unwrap()
        );
    }

    #[test]
    fn imported_variants_decide_whether_to_name_a_goat_and_preserve_audiences() {
        let legacy = Catalog::legacy().unwrap();
        let mut named = 0;
        let mut unnamed = 0;
        for template in legacy.category_templates("sats_received") {
            let names_goat = template.contains("{goat_name}");
            if names_goat {
                named += 1;
            } else {
                unnamed += 1;
            }
            let expected_overlay = safe_substitute(
                &template,
                &common_values(25, &difference_message(142, 1000), "Dexter"),
            )
            .unwrap();
            let renderer = MessageRenderer {
                catalog: Arc::new(TemplateCatalog {
                    sats_received: vec![template],
                    feeder_trigger: legacy.category_templates("feeder_trigger"),
                    interface_info: legacy.category_templates("interface_info"),
                }),
            };
            let rendered = renderer
                .render(
                    &event(
                        42,
                        "payment_received",
                        json!({"amount_sats":25,"feed_credit_sats":142,"address_user":"dexter"}),
                    ),
                    1000,
                )
                .unwrap();
            assert_eq!(rendered.overlay_message.unwrap(), expected_overlay);
            assert_eq!(
                rendered
                    .nostr_content
                    .unwrap()
                    .contains(GOATS[0].nostr_profile),
                names_goat
            );
            assert_eq!(rendered.overlay_goats[0].name, "Dexter");
            assert!(
                renderer
                    .render(
                        &event(
                            43,
                            "payment_received",
                            json!({"amount_sats":9,"feed_credit_sats":151,"address_user":"dexter"})
                        ),
                        1000
                    )
                    .unwrap()
                    .nostr_content
                    .is_none()
            );
        }
        assert!(named > 0);
        assert!(unnamed > 0);
    }

    #[test]
    fn individual_address_uses_that_goat_for_overlay_and_nostr() {
        let legacy = Catalog::legacy().unwrap();
        let named_variant = legacy
            .category_templates("sats_received")
            .into_iter()
            .find(|text| text.contains("{goat_name}"))
            .unwrap();
        let renderer = MessageRenderer {
            catalog: Arc::new(TemplateCatalog {
                sats_received: vec![named_variant],
                feeder_trigger: legacy.category_templates("feeder_trigger"),
                interface_info: legacy.category_templates("interface_info"),
            }),
        };
        let rendered = renderer
            .render(
                &event(
                    7,
                    "payment_received",
                    json!({
                        "amount_sats": 100,
                        "feed_credit_sats": 100,
                        "address_user": "dexter"
                    }),
                ),
                1_000,
            )
            .unwrap();
        assert_eq!(rendered.overlay_goats[0].name, "Dexter");
        assert!(rendered.overlay_message.unwrap().contains("Dexter"));
        assert!(rendered.nostr_content.unwrap().contains("nostr:nprofile1"));
    }

    #[test]
    fn payment_minimum_applies_to_provider_and_operator_messages_without_test_labels() {
        let renderer = MessageRenderer::embedded().unwrap();
        for kind in ["payment_received", "operator_credit_added"] {
            for amount in [1, 9, 10, 11] {
                let rendered = renderer
                    .render(
                        &event(
                            42,
                            kind,
                            json!({
                                "amount_sats":amount,"feed_credit_sats":1000,"address_user":"dexter"
                            }),
                        ),
                        1000,
                    )
                    .unwrap();
                assert_eq!(rendered.overlay_message.is_some(), amount >= 10);
                assert_eq!(rendered.nostr_content.is_some(), amount >= 10);
                if let Some(message) = rendered.overlay_message {
                    assert!(!message.contains("TEST"));
                    assert!(!message.contains("operator"));
                    assert_eq!(rendered.overlay_goats[0].name, "Dexter");
                }
            }
        }
    }

    #[test]
    fn informational_and_weather_messages_are_overlay_only() {
        let renderer = MessageRenderer::embedded().unwrap();
        let info = renderer
            .render(&event(3, "interface_info", json!({})), 1_000)
            .unwrap();
        assert!(info.nostr_content.is_none());
        assert_eq!(info.overlay_type.as_deref(), Some("interface_info"));
        assert!(info.overlay_message.is_some());

        let weather = renderer
            .render(
                &event(4, "weather_status", json!({"message": "Sunny and 72°F"})),
                1_000,
            )
            .unwrap();
        assert!(weather.nostr_content.is_none());
        assert_eq!(weather.overlay_message.as_deref(), Some("Sunny and 72°F"));
    }

    #[test]
    fn paused_context_does_not_prefix_overlay_or_expand_nostr_audiences() {
        let renderer = MessageRenderer::embedded().unwrap();
        for kind in [
            "payment_received",
            "operator_credit_added",
            "feeder_confirmed",
            "interface_info",
            "weather_status",
        ] {
            let mut payload = json!({
                "amount_sats":25,"threshold_sats":1000,"feed_credit_sats":2340,
                "address_user":"herd","message":"Current weather"
            });
            let normal = renderer
                .render(&event(42, kind, payload.clone()), 1000)
                .unwrap();
            payload["feeding"] = json!({"paused":true,"reason":"override",
                "queued_feedings":2,"awaiting_confirmation":false});
            let paused = renderer.render(&event(42, kind, payload), 1000).unwrap();
            assert_eq!(paused.overlay_message, normal.overlay_message, "{kind}");
            if matches!(kind, "interface_info" | "weather_status") {
                assert!(paused.nostr_content.is_none(), "{kind}");
            } else {
                assert!(paused.nostr_content.unwrap().contains("Feeding is paused."));
            }
        }
    }

    #[test]
    fn paused_payments_thank_first_and_describe_the_frozen_queue_once() {
        let renderer = MessageRenderer {
            catalog: Arc::new(TemplateCatalog {
                sats_received: vec!["Thank you for {new_amount} sats! {difference_message}".into()],
                feeder_trigger: vec![],
                interface_info: vec![],
            }),
        };
        for (amount, credit, queued, pending, expected) in [
            (
                2100,
                2221,
                2,
                false,
                "Thank you for 2100 sats! Feeding is paused. We have 2 feedings lined up for when feeding resumes. Another 221 sats are saved toward the next feeding.",
            ),
            (
                50,
                121,
                0,
                false,
                "Thank you for 50 sats! Feeding is paused. Your sats are saved, with 879 more needed to fund the next feeding.",
            ),
            (
                1000,
                1000,
                1,
                false,
                "Thank you for 1000 sats! Feeding is paused. We have 1 feeding lined up for when feeding resumes.",
            ),
            (
                10,
                1000,
                0,
                true,
                "Thank you for 10 sats! Feeding is paused. We're waiting for confirmation of an earlier feeding before the queue can continue.",
            ),
        ] {
            let payload = json!({"amount_sats":amount,"feed_credit_sats":credit,
                "address_user":"herd","feeding":{"paused":true,"reason":"override",
                "queued_feedings":queued,"awaiting_confirmation":pending}});
            let rendered = renderer
                .render(&event(42, "payment_received", payload), 1000)
                .unwrap();
            assert_eq!(rendered.nostr_content.as_deref(), Some(expected));
            assert_eq!(
                rendered.overlay_message,
                Some(format!(
                    "Thank you for {amount} sats! {}",
                    difference_message(credit, 1000)
                ))
            );
            assert!(!expected.contains("feeding(s)"));
            assert!(!expected.contains("0 feedings"));
        }
    }

    #[test]
    fn safe_formatter_rejects_attribute_and_index_access() {
        assert!(validate_template_syntax("{x.__class__}").is_err());
        assert!(validate_template_syntax("{x[0]}").is_err());
    }
}
