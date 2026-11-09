//! Operator-supplied legacy wording, with bounded simple-placeholder rendering.
//!
//! Template text never grants a publication audience or executes format code.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const LEGACY: &str = include_str!("../../templates/cyberherd-legacy.json");
const MAX_TEMPLATE_BYTES: usize = 8_192;
const MAX_RENDERED_BYTES: usize = 32_768;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog(BTreeMap<String, BTreeMap<String, String>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    OverlayOnly,
    NostrAndOverlay,
}

/// This allowlist belongs to code, never editable template metadata. Helpers
/// such as goat_names_dict are deliberately not independently publishable.
#[must_use]
pub fn audience(category: &str) -> Option<Audience> {
    match category {
        "interface_info" | "weather_status" => Some(Audience::OverlayOnly),
        "sats_received"
        | "sats_received_zap"
        | "feeder_trigger"
        | "cyber_herd"
        | "cyber_herd_treats"
        | "member_increase"
        | "member_added"
        | "member_removed"
        | "kind_6_repost"
        | "kind_7_reaction"
        | "kind_6_headbutt_failure"
        | "kind_7_headbutt_failure"
        | "headbutt_success"
        | "headbutt_failure"
        | "join_below_minimum"
        | "zapper_displaces_kind_6"
        | "zapper_displaces_kind_7"
        | "repost_displaces"
        | "existing_member_reaction"
        | "existing_member_repost"
        | "member_thread"
        | "herd_reset_message"
        | "cyber_herd_invitation"
        | "cyber_herd_pause"
        | "cyber_herd_resume"
        | "cyber_herd_evening" => Some(Audience::NostrAndOverlay),
        _ => None,
    }
}

impl Catalog {
    pub(crate) fn categories(&self) -> &BTreeMap<String, BTreeMap<String, String>> {
        &self.0
    }

    pub fn legacy() -> Result<Self> {
        Self::from_json(LEGACY)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        if json.len() > 2_000_000 {
            bail!("CyberHerd template catalog exceeds size bound");
        }
        let mut categories: BTreeMap<String, BTreeMap<String, String>> =
            serde_json::from_str(json).context("invalid CyberHerd template catalog")?;
        if categories.len() > 100 {
            bail!("CyberHerd template catalog has too many categories");
        }
        for (category, variants) in &mut categories {
            validate_identifier(category)?;
            if variants.len() > 1_000 {
                bail!("CyberHerd template category has too many variants");
            }
            for (variant, template) in variants {
                if variant.is_empty() || variant.len() > 128 {
                    bail!("invalid CyberHerd template variant identifier");
                }
                // The export has this single known formatting modifier. Values
                // are canonical strings supplied by the domain, not Python code.
                *template = template.replace("{new_amount:g}", "{new_amount}");
                fields(template)?;
            }
        }
        Ok(Self(categories))
    }

    /// Missing and intentionally empty categories both mean opt-out. Persisted
    /// editing/seeding must not replace either state with a default category.
    pub fn category(&self, category: &str) -> Option<&BTreeMap<String, String>> {
        self.0.get(category).filter(|variants| !variants.is_empty())
    }

    pub fn category_templates(&self, category: &str) -> Vec<String> {
        self.category(category)
            .map(|variants| variants.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn render(
        &self,
        category: &str,
        interaction_id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Option<String>> {
        let Some(variants) = self.category(category) else {
            return Ok(None);
        };
        if interaction_id.is_empty() || interaction_id.len() > 512 {
            bail!("invalid template interaction identifier");
        }
        let mut digest = Sha256::new();
        digest.update(category.as_bytes());
        digest.update([0]);
        digest.update(interaction_id.as_bytes());
        let bytes: [u8; 8] = digest.finalize()[..8].try_into()?;
        let index = (u64::from_be_bytes(bytes) % variants.len() as u64) as usize;
        let template = variants
            .values()
            .nth(index)
            .context("missing template variant")?;
        Ok(Some(substitute(template, values)?))
    }
}

fn validate_identifier(field: &str) -> Result<()> {
    if field.is_empty()
        || field.len() > 128
        || field.as_bytes()[0].is_ascii_digit()
        || !field
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        bail!("only simple identifier placeholders are supported");
    }
    Ok(())
}

pub fn fields(template: &str) -> Result<BTreeSet<String>> {
    if template.trim().is_empty() || template.len() > MAX_TEMPLATE_BYTES {
        bail!("invalid CyberHerd template length");
    }
    let mut result = BTreeSet::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if rest[..open].contains('}') {
            bail!("unmatched closing brace in template");
        }
        let after_open = &rest[open + 1..];
        let close = after_open
            .find('}')
            .context("unmatched opening brace in template")?;
        let field = &after_open[..close];
        validate_identifier(field)?;
        result.insert(field.to_owned());
        rest = &after_open[close + 1..];
    }
    if rest.contains('}') {
        bail!("unmatched closing brace in template");
    }
    Ok(result)
}

pub fn substitute(template: &str, values: &BTreeMap<String, String>) -> Result<String> {
    fields(template)?;
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let close = after_open.find('}').context("unmatched opening brace")?;
        let value = values
            .get(&after_open[..close])
            .context("missing CyberHerd template value")?;
        if rendered
            .len()
            .saturating_add(open)
            .saturating_add(value.len())
            > MAX_RENDERED_BYTES
        {
            bail!("CyberHerd rendered message exceeds size bound");
        }
        rendered.push_str(&rest[..open]);
        rendered.push_str(value);
        rest = &after_open[close + 1..];
    }
    if rendered.len().saturating_add(rest.len()) > MAX_RENDERED_BYTES {
        bail!("CyberHerd rendered message exceeds size bound");
    }
    rendered.push_str(rest);
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_uploaded_template_renders_and_preserves_literal_wording() {
        let raw: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(LEGACY).unwrap();
        let catalog = Catalog::legacy().unwrap();
        assert_eq!(raw.len(), 50);
        assert_eq!(raw.values().map(BTreeMap::len).sum::<usize>(), 653);
        for (category, variants) in raw {
            for (id, original) in variants {
                let normalized = original.replace("{new_amount:g}", "{new_amount}");
                let template = &catalog.0[&category][&id];
                assert_eq!(*template, normalized);
                let values = fields(template)
                    .unwrap()
                    .into_iter()
                    .map(|field| (field, "Alice".to_owned()))
                    .collect();
                assert!(substitute(template, &values).is_ok(), "{category}/{id}");
            }
        }
    }

    #[test]
    fn expressions_formats_unmatched_braces_and_missing_values_fail_closed() {
        for template in [
            "{user.name}",
            "{user[0]}",
            "{x!r}",
            "{x:g}",
            "{x()}",
            "{0}",
            "{x",
            "} {x}",
            "{x}}",
            "{{x}}",
        ] {
            assert!(fields(template).is_err(), "{template}");
        }
        assert!(substitute("{name}", &BTreeMap::new()).is_err());
        let values = BTreeMap::from([("name".into(), "{not_executed}".into())]);
        assert_eq!(
            substitute("Hello {name}", &values).unwrap(),
            "Hello {not_executed}"
        );
        assert!(
            substitute(
                "{name}",
                &BTreeMap::from([("name".into(), "x".repeat(32_769))])
            )
            .is_err()
        );
    }

    #[test]
    fn deterministic_variants_opt_out_and_audience_are_independent() {
        let catalog = Catalog::from_json(r#"{"member_added":{"one":"Hello {name}","two":"Welcome {name}"},"cyber_herd_invitation":{}}"#).unwrap();
        let values = BTreeMap::from([("name".into(), "Alice".into())]);
        assert_eq!(
            catalog
                .render("member_added", "receipt:42", &values)
                .unwrap(),
            catalog
                .render("member_added", "receipt:42", &values)
                .unwrap()
        );
        assert_eq!(
            catalog
                .render("cyber_herd_invitation", "receipt:42", &values)
                .unwrap(),
            None
        );
        assert_eq!(
            catalog
                .render("cyber_herd_pause", "receipt:42", &values)
                .unwrap(),
            None
        );
        assert_eq!(audience("interface_info"), Some(Audience::OverlayOnly));
        assert_eq!(audience("weather_status"), Some(Audience::OverlayOnly));
        assert_eq!(audience("member_added"), Some(Audience::NostrAndOverlay));
        assert_eq!(audience("goat_names_dict"), None);
        assert_eq!(audience("arbitrary_custom_category"), None);
    }
}
