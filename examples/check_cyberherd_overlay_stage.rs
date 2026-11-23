//! Configuration-only staging check. No journal, credential, service or network.
#![forbid(unsafe_code)]

use anyhow::{Result, bail};
use lightning_goats::{config::AppConfig, cyberherd::runtime::Config};
use std::path::Path;

fn validate_pair(core: &AppConfig, member: &Config) -> Result<()> {
    core.validate()?;
    member.validate()?;
    let Some(bridge) = &core.core_bridge else {
        bail!("missing staged bridge");
    };
    let Some(reward) = &bridge.reward_policy else {
        bail!("missing overlay owner");
    };
    let Some(zap) = &core.lnurl.zap else {
        bail!("missing native incoming zap registry");
    };
    let Some(messages) = &member.messaging else {
        bail!("missing overlay messages");
    };
    let Some(view) = &member.view else {
        bail!("missing loopback view");
    };
    if !member.enabled
        || member.rewards.is_some()
        || member.payout_bridge.is_some()
        || bridge.socket != member.core_socket
        || bridge.peer_uid == member.core.core_uid
        || bridge.payout_peer_uid.is_some()
        || bridge.payout_account_id.is_some()
        || bridge.payout_execution_enabled
        || bridge.delegated_feeder_messages
        || reward.owner != member.project_pubkey
        || reward.member_percent_ceiling != 0
        || reward.max_sats_per_feed != 0
        || reward.max_sats_per_24h != 0
        || zap.receipt_pubkey != member.project_pubkey
        || zap.payee_pubkeys != member.core.payees
        || zap.payee_pubkeys.len() != 6
        || !messages.enabled
        || messages.lifecycle
        || messages.signer.is_some()
        || !view.enabled
        || !view.overlay
        || view.listen.to_string() != "127.0.0.1:3340"
        || view.auth.is_some()
    {
        bail!("stage must preserve zero spending and overlay-only membership");
    }
    Ok(())
}

fn check() -> Result<()> {
    if std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("configuration checker must not load credentials");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        bail!("two candidate configuration paths required");
    }
    for path in &args {
        if std::fs::metadata(path)?.len() > 65536 {
            bail!("candidate exceeds configuration bound");
        }
    }
    let core = AppConfig::load(Path::new(&args[0]))?;
    let member = Config::load(Path::new(&args[1]))?;
    validate_pair(&core, &member)?;
    println!(
        "{}",
        serde_json::json!({"accepted":true,"configuration_only":true,
            "ledger_opened":false,"credentials_loaded":false,"provider_operations":0,
            "services_changed":false,"payout_capabilities_enabled":false})
    );
    Ok(())
}

fn main() {
    if check().is_err() {
        eprintln!("Candidate overlay stage did not validate; private configuration withheld.");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (AppConfig, Config) {
        let owner = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        let mut core: toml::Value =
            toml::from_str(include_str!("../deploy/config.receive.toml.example")).unwrap();
        core["nostr"]["bunker_pubkey"] = owner.into();
        let payees = serde_json::Value::Object(
            ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
                .into_iter()
                .map(|user| (user.to_owned(), json!(owner)))
                .collect(),
        );
        let zap = json!({
            "receipt_pubkey":owner,"payee_pubkeys":payees,
            "alias_origins":["https://lnb.bolverker.com/"]
        });
        core["lnurl"]
            .as_table_mut()
            .unwrap()
            .insert("zap".into(), toml::Value::try_from(zap).unwrap());
        let bridge = json!({
            "socket":"/run/lightning-goats-bridge/core.sock","peer_uid":10002,
            "group_gid":10003,"reward_policy":{"owner":owner,
            "member_percent_ceiling":0,"max_sats_per_feed":0,"max_sats_per_24h":0}
        });
        core.as_table_mut()
            .unwrap()
            .insert("core_bridge".into(), toml::Value::try_from(bridge).unwrap());
        let mut member: toml::Value =
            toml::from_str(include_str!("../deploy/cyberherd/config.toml.example")).unwrap();
        member["enabled"] = true.into();
        member["project_pubkey"] = owner.into();
        member["core"]["core_uid"] = 10001.into();
        member["core"]["epoch"] = "11111111-1111-4111-8111-111111111111".into();
        member["core"]["payees"] = toml::Value::try_from(payees).unwrap();
        let messaging = json!({
            "enabled":true,"start_after":0,"poll_millis":1000,
            "relays":["wss://relay.primal.net"],"lifecycle":false
        });
        member.as_table_mut().unwrap().insert(
            "messaging".into(),
            toml::Value::try_from(messaging).unwrap(),
        );
        member.as_table_mut().unwrap().insert(
            "view".into(),
            toml::Value::try_from(json!({
                "enabled":true,"listen":"127.0.0.1:3340","overlay":true
            }))
            .unwrap(),
        );
        (core.try_into().unwrap(), member.try_into().unwrap())
    }

    #[test]
    fn stage_accepts_native_types_and_refuses_financial_or_publication_capabilities() {
        let (core, member) = fixture();
        validate_pair(&core, &member).unwrap();
        for changed in 0..5 {
            let (mut core, mut member) = fixture();
            match changed {
                0 => {
                    core.core_bridge
                        .as_mut()
                        .unwrap()
                        .reward_policy
                        .as_mut()
                        .unwrap()
                        .max_sats_per_feed = 1
                }
                1 => core.core_bridge.as_mut().unwrap().delegated_feeder_messages = true,
                2 => member.messaging.as_mut().unwrap().lifecycle = true,
                3 => member.view.as_mut().unwrap().listen = "127.0.0.1:3341".parse().unwrap(),
                _ => {
                    core.lnurl
                        .zap
                        .as_mut()
                        .unwrap()
                        .payee_pubkeys
                        .remove("nova");
                }
            }
            assert!(validate_pair(&core, &member).is_err());
        }
    }
}
