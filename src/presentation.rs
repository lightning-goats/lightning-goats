use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ledger::DurableEvent;

const EMBEDDED_TEMPLATES: &str = include_str!("../templates/phase1.toml");

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverlayGoat {
    pub name: String,
    #[serde(rename = "imageUrl")]
    pub image_url: String,
}

#[derive(Debug, Deserialize)]
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
        let catalog: TemplateCatalog = toml::from_str(EMBEDDED_TEMPLATES)
            .context("invalid embedded Phase 1 message templates")?;
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

        match event.event_type.as_str() {
            "payment_received" => self.render_payment(event, object, threshold_sats),
            "feeder_confirmed" => self.render_feeder(event, object, threshold_sats),
            "interface_info" => self.render_interface_info(event),
            "weather_status" => self.render_weather(object),
            _ => Ok(RenderedPresentation::empty()),
        }
    }

    fn render_payment(
        &self,
        event: &DurableEvent,
        payload: &serde_json::Map<String, Value>,
        threshold_sats: u64,
    ) -> Result<RenderedPresentation> {
        let amount = required_u64(payload, "amount_sats")?;
        let credit = required_u64(payload, "feed_credit_sats")?;
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
        let mut nostr_values = common_values(amount, &difference_message, goat.nostr_profile);
        nostr_values.insert(
            "difference".to_owned(),
            remaining_sats(credit, threshold_sats).to_string(),
        );

        Ok(RenderedPresentation {
            nostr_content: Some(safe_substitute(template, &nostr_values)?),
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
        let amount = required_u64(payload, "threshold_sats")?;
        let credit = required_u64(payload, "feed_credit_sats")?;
        let goat = goat_for_event(event, None);
        let difference_message = difference_message(credit, threshold_sats);
        let template = select_template(&self.catalog.feeder_trigger, event, "feeder_trigger")?;

        let overlay_values = common_values(amount, &difference_message, goat.name);
        let nostr_values = common_values(amount, &difference_message, goat.nostr_profile);

        Ok(RenderedPresentation {
            nostr_content: Some(safe_substitute(template, &nostr_values)?),
            overlay_type: Some("feeder_trigger".to_owned()),
            overlay_message: Some(safe_substitute(template, &overlay_values)?),
            overlay_goats: vec![overlay_goat(goat)],
        })
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

fn remaining_sats(credit: u64, threshold: u64) -> u64 {
    if credit < threshold {
        threshold - credit
    } else {
        threshold - (credit % threshold)
    }
}

fn difference_message(credit: u64, threshold: u64) -> String {
    if credit < threshold {
        return format!("{} sats until feeder activation.", threshold - credit);
    }
    let feeds_due = credit / threshold;
    let remainder = credit % threshold;
    if remainder == 0 {
        format!("Feeder ready! {feeds_due} feeding(s) are funded.")
    } else {
        format!(
            "Feeder ready! {feeds_due} feeding(s) are funded, with {remainder} sats already toward the following feeding."
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
    fn individual_address_uses_that_goat_for_overlay_and_nostr() {
        let renderer = MessageRenderer::embedded().unwrap();
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
    fn safe_formatter_rejects_attribute_and_index_access() {
        assert!(validate_template_syntax("{x.__class__}").is_err());
        assert!(validate_template_syntax("{x[0]}").is_err());
    }
}
