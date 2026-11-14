//! Configuration-only contact activation check; no ledger, key, service or network.
#![forbid(unsafe_code)]

use anyhow::{Result, bail};
use lightning_goats::config::AppConfig;
use std::path::Path;

fn validate_change(original: &toml::Value, candidate: &toml::Value) -> Result<()> {
    if original.get("contact").is_some() {
        bail!("contact must be absent from the original configuration");
    }
    let old: AppConfig = original.clone().try_into()?;
    let new: AppConfig = candidate.clone().try_into()?;
    old.validate()?;
    new.validate()?;
    if new.contact.is_none() {
        bail!("candidate contact configuration is required");
    }
    let mut retained = candidate.clone();
    retained
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("configuration table required"))?
        .remove("contact");
    if &retained != original {
        bail!("only the optional contact configuration may be added");
    }
    Ok(())
}

fn check() -> Result<()> {
    if std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("configuration checker must not load credentials");
    }
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 2 {
        bail!("original and candidate configuration paths required");
    }
    let mut values = Vec::new();
    for argument in arguments {
        let path = Path::new(&argument);
        if std::fs::metadata(path)?.len() > 65536 {
            bail!("configuration exceeds bound");
        }
        let bytes = std::fs::read_to_string(path)?;
        if bytes.len() > 65536 {
            bail!("configuration changed beyond bound");
        }
        values.push(toml::from_str::<toml::Value>(&bytes)?);
    }
    validate_change(&values[0], &values[1])?;
    println!(
        "{}",
        serde_json::json!({"accepted":true,"configuration_only":true,
        "only_contact_configuration_added":true,"public_payment_policy_unchanged":true,
        "ledger_opened":false,"credentials_loaded":false,"provider_operations":0,"services_changed":false})
    );
    Ok(())
}

fn main() {
    if check().is_err() {
        eprintln!("Private contact candidate did not validate; configuration withheld.");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (toml::Value, toml::Value) {
        let mut old: toml::Value =
            toml::from_str(include_str!("../deploy/config.receive.toml.example")).unwrap();
        old["service"]["mode"] = "active".into();
        old["nostr"]["bunker_pubkey"] =
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into();
        let mut new = old.clone();
        new.as_table_mut().unwrap().insert(
            "contact".into(),
            toml::Value::try_from(
                serde_json::json!({"recipient":old["nostr"]["bunker_pubkey"].as_str().unwrap(),
                              "inbox_relays":["wss://relay.damus.io/"]}),
            )
            .unwrap(),
        );
        (old, new)
    }

    #[test]
    fn contact_only_addition_preserves_all_original_payment_and_physical_policy() {
        let (old, new) = fixture();
        validate_change(&old, &new).unwrap();
        for change in 0..7 {
            let (mut old, mut new) = fixture();
            match change {
                0 => new["feeder"]["threshold_sats"] = 2000.into(),
                1 => new["gateway"]["url"] = "http://127.0.0.1:8789/".into(),
                2 => {
                    new["contact"]["recipient"] =
                        "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".into()
                }
                3 => {
                    new["contact"]["inbox_relays"] =
                        toml::Value::Array(vec!["wss://127.0.0.1/".into()])
                }
                4 => {
                    new["contact"]
                        .as_table_mut()
                        .unwrap()
                        .insert("api_key".into(), "synthetic-forbidden".into());
                }
                5 => {
                    old["service"]["mode"] = "shadow".into();
                    new["service"]["mode"] = "shadow".into();
                }
                _ => {
                    new.as_table_mut().unwrap().remove("contact");
                }
            }
            assert!(validate_change(&old, &new).is_err());
        }
    }
}
