//! Configuration-only four-role financial candidate check. No runtime opens.
#![forbid(unsafe_code)]

use anyhow::{Result, bail, ensure};
use lightning_goats::{
    config::AppConfig,
    cyberherd::runtime::Config as Member,
    payout::{runtime::Config as Payout, signer::Config as Signer},
};
use std::{collections::BTreeSet, os::unix::fs::MetadataExt, path::Path};

fn process_uid() -> Result<u32> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    let row = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| anyhow::anyhow!("missing process identity"))?;
    let values = row
        .split_whitespace()
        .skip(1)
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        values.len() == 4 && values.iter().all(|v| *v == values[0]),
        "stable unprivileged process identity required"
    );
    Ok(values[0])
}

fn protected_config(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && path.canonicalize()? == path,
        "canonical candidate path required"
    );
    for (index, part) in path.ancestors().enumerate() {
        let metadata = std::fs::symlink_metadata(part)?;
        ensure!(
            metadata.uid() == 0
                && metadata.mode() & 0o022 == 0
                && if index == 0 {
                    metadata.is_file() && metadata.nlink() == 1
                } else {
                    metadata.is_dir()
                },
            "root-owned protected candidate and ancestors required"
        );
    }
    Ok(())
}

fn validate_stage(
    core: &AppConfig,
    member: &Member,
    payout: &Payout,
    signer: &Signer,
) -> Result<()> {
    core.validate()?;
    member.validate()?;
    payout.validate()?;
    signer.validate()?;
    let bridge = core
        .core_bridge
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing core bridge"))?;
    let reward = bridge
        .reward_policy
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing core policy"))?;
    let planner = member
        .rewards
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing planner"))?;
    let plans = member
        .payout_bridge
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing plan export"))?;
    let zap = core
        .lnurl
        .zap
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing receive registry"))?;
    let p = &payout.policy;
    let s = &signer.policy;
    let identities = [
        member.core.core_uid,
        bridge.peer_uid,
        s.peer_uid,
        payout.signer_uid,
    ];
    ensure!(
        identities.iter().all(|v| *v > 0)
            && identities.iter().copied().collect::<BTreeSet<_>>().len() == 4,
        "four distinct original runtime identities required"
    );
    ensure!(
        member.enabled
            && payout.enabled
            && signer.enabled
            && bridge.payout_execution_enabled
            && p.execution_enabled,
        "complete candidate process and execution configuration required"
    );
    ensure!(
        bridge.payout_peer_uid == Some(s.peer_uid)
            && plans.peer_uid == s.peer_uid
            && payout.intake_uid == bridge.peer_uid
            && p.scope.core_uid == member.core.core_uid
            && bridge.socket == member.core_socket
            && payout.core_socket == bridge.socket
            && plans.socket == payout.intake_socket
            && signer.socket == payout.signer_socket,
        "candidate kernel peer or socket binding differs"
    );
    ensure!(
        bridge.payout_account_id == Some(p.scope.provider.account_id)
            && member.core.epoch == p.scope.epoch
            && p.scope.owner == member.project_pubkey
            && reward.owner == member.project_pubkey
            && s.owner == member.project_pubkey
            && core.nostr.bunker_pubkey == member.project_pubkey
            && signer.nostr.bunker_pubkey == member.project_pubkey
            && zap.receipt_pubkey == member.project_pubkey
            && zap.payee_pubkeys == member.core.payees,
        "candidate project, receive, provider or epoch binding differs"
    );
    ensure!(
        member.settings.max_members == 3
            && member.settings.member_percent == p.member_percent_ceiling
            && reward.member_percent_ceiling == p.member_percent_ceiling
            && reward.max_sats_per_feed == p.max_sats_per_feed
            && reward.max_sats_per_24h == p.max_sats_per_24h
            && p.max_sats_per_feed <= p.max_sats_per_24h
            && p.max_fee_sats <= p.max_sats_per_feed
            && s.max_net_sats == p.max_net_sats
            && s.max_net_sats <= p.max_sats_per_feed,
        "candidate monetary ceilings differ"
    );
    let required = [
        u64::from(p.member_percent_ceiling),
        p.max_sats_per_feed,
        p.max_sats_per_24h,
        s.max_net_sats,
        u64::from(s.max_new_per_hour),
    ];
    let all_zero = required.iter().all(|v| *v == 0) && p.max_fee_sats == 0;
    ensure!(
        all_zero || required.iter().all(|v| *v > 0),
        "candidate limits are only partially selected"
    );
    ensure!(
        planner.fee_mode == p.fee_mode
            && planner.fee_basis_points == 0
            && planner.minimum_fee_sats == p.max_fee_sats
            && planner.minimum_net_sats == 1
            && planner.maximum_net_sats == p.max_net_sats
            && planner.profile_max_age_seconds == 3600
            && planner.operation_addresses == p.operation_addresses
            && p.receipt_relays == s.relays,
        "candidate planning or receipt policy differs"
    );
    let users = ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        member
            .core
            .payees
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == users,
        "candidate must retain the six native payees"
    );
    for origin in std::iter::once(&core.lnurl.public_base_url).chain(zap.alias_origins.iter()) {
        let url = reqwest::Url::parse(origin)?;
        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("missing receive host"))?;
        for user in &users {
            ensure!(
                p.operation_addresses.contains(&format!("{user}@{host}")),
                "candidate omitted an operational receive alias"
            );
        }
    }
    Ok(())
}

fn check() -> Result<()> {
    if process_uid()? == 0 || std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("configuration-only checker requires a non-root credential-free process");
    }
    let paths = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(paths.len() == 4, "four candidate paths required");
    for raw in &paths {
        let path = Path::new(raw);
        protected_config(path)?;
        ensure!(
            std::fs::symlink_metadata(path)?.len() <= 65536,
            "candidate exceeds bound"
        );
    }
    let core = AppConfig::load(Path::new(&paths[0]))?;
    let member = Member::load(Path::new(&paths[1]))?;
    let payout = Payout::load(Path::new(&paths[2]))?;
    let signer = Signer::load(Path::new(&paths[3]))?;
    ensure!(
        process_uid()? == member.core.core_uid,
        "original core UID required for configuration validation"
    );
    validate_stage(&core, &member, &payout, &signer)?;
    println!(
        "{}",
        serde_json::json!({"accepted":true,"configuration_only":true,
        "candidate_limits_valid":true,"journals_opened":false,"credentials_loaded":false,
        "provider_operations":0,"services_changed":false})
    );
    Ok(())
}

fn main() {
    if check().is_err() {
        eprintln!(
            "Four-role financial candidate did not validate; private configuration withheld."
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> [toml::Value; 4] {
        let owner = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        let epoch = "11111111-1111-4111-8111-111111111111";
        let account = "22222222-2222-4222-8222-222222222222";
        let mut core: toml::Value =
            toml::from_str(include_str!("../deploy/config.receive.toml.example")).unwrap();
        let mut member: toml::Value =
            toml::from_str(include_str!("../deploy/cyberherd/config.toml.example")).unwrap();
        let mut payout: toml::Value =
            toml::from_str(include_str!("../deploy/cyberherd/payout.toml.example")).unwrap();
        let mut signer: toml::Value =
            toml::from_str(include_str!("../deploy/cyberherd/signer.toml.example")).unwrap();
        core["nostr"]["bunker_pubkey"] = owner.into();
        let payees = json!({"herd":owner,"dexter":owner,"rowan":owner,"cosmo":owner,"newton":owner,"nova":owner});
        core["lnurl"].as_table_mut().unwrap().insert(
            "zap".into(),
            toml::Value::try_from(json!({"receipt_pubkey":owner,"payee_pubkeys":payees,
            "alias_origins":["https://lnb.bolverker.com/"]}))
            .unwrap(),
        );
        core.as_table_mut().unwrap().insert("core_bridge".into(), toml::Value::try_from(json!({"socket":"/run/lightning-goats-bridge/core.sock",
            "peer_uid":10002,"group_gid":10005,"payout_peer_uid":10003,"payout_account_id":account,
            "payout_execution_enabled":true,"reward_policy":{"owner":owner,"member_percent_ceiling":10,
            "max_sats_per_feed":100,"max_sats_per_24h":1000}})).unwrap());
        member["enabled"] = true.into();
        member["project_pubkey"] = owner.into();
        member["core"]["core_uid"] = 10001.into();
        member["core"]["epoch"] = epoch.into();
        member["core"]["payees"] = toml::Value::try_from(payees).unwrap();
        let operations = ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
            .into_iter()
            .flat_map(|u| {
                [
                    format!("{u}@lightning-goats.com"),
                    format!("{u}@lnb.bolverker.com"),
                ]
            })
            .collect::<Vec<_>>();
        member.as_table_mut().unwrap().insert(
            "rewards".into(),
            toml::Value::try_from(json!({"fee_basis_points":0,"minimum_fee_sats":2,
            "minimum_net_sats":1,"maximum_net_sats":98,"profile_max_age_seconds":3600,"operation_addresses":operations}))
            .unwrap(),
        );
        member.as_table_mut().unwrap().insert(
            "payout_bridge".into(),
            toml::Value::try_from(json!({"socket":"/run/cyberherd-plans/plans.sock",
            "peer_uid":10003,"group_gid":10006}))
            .unwrap(),
        );
        payout["enabled"] = true.into();
        payout["intake_uid"] = 10002.into();
        payout["signer_uid"] = 10004.into();
        payout["policy"]["scope"]["core_uid"] = 10001.into();
        payout["policy"]["scope"]["epoch"] = epoch.into();
        payout["policy"]["scope"]["owner"] = owner.into();
        payout["policy"]["scope"]["provider"]["account_id"] = account.into();
        payout["policy"]["scope"]["provider"]["credential_fingerprint"] = "11".repeat(32).into();
        for (name, value) in [
            ("member_percent_ceiling", 10),
            ("max_sats_per_feed", 100),
            ("max_sats_per_24h", 1000),
            ("max_fee_sats", 2),
            ("max_net_sats", 98),
        ] {
            payout["policy"][name] = value.into();
        }
        payout["policy"]["execution_enabled"] = true.into();
        payout["policy"]["operation_addresses"] = toml::Value::try_from(operations).unwrap();
        signer["enabled"] = true.into();
        signer["group_gid"] = 10007.into();
        signer["policy"]["owner"] = owner.into();
        signer["policy"]["client_pubkey"] =
            "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".into();
        signer["policy"]["peer_uid"] = 10003.into();
        signer["policy"]["max_net_sats"] = 98.into();
        signer["policy"]["max_new_per_hour"] = 10.into();
        signer["nostr"]["bunker_pubkey"] = owner.into();
        [core, member, payout, signer]
    }

    fn accepts(values: [toml::Value; 4]) -> Result<()> {
        validate_stage(
            &values[0].clone().try_into()?,
            &values[1].clone().try_into()?,
            &values[2].clone().try_into()?,
            &values[3].clone().try_into()?,
        )
    }

    #[test]
    fn four_native_configs_accept_complete_limits_and_refuse_cross_role_drift() {
        accepts(fixture()).unwrap();
        for (index, path, value) in [
            (
                0,
                vec!["core_bridge", "payout_peer_uid"],
                toml::Value::Integer(10004),
            ),
            (
                0,
                vec!["core_bridge", "payout_account_id"],
                toml::Value::String("33333333-3333-4333-8333-333333333333".into()),
            ),
            (
                2,
                vec!["policy", "max_sats_per_feed"],
                toml::Value::Integer(101),
            ),
            (3, vec!["policy", "max_net_sats"], toml::Value::Integer(101)),
            (2, vec!["policy", "max_net_sats"], toml::Value::Integer(97)),
            (1, vec!["settings", "max_members"], toml::Value::Integer(10)),
            (
                1,
                vec!["rewards", "maximum_net_sats"],
                toml::Value::Integer(97),
            ),
            (
                3,
                vec!["policy", "max_new_per_hour"],
                toml::Value::Integer(0),
            ),
            (2, vec!["signer_uid"], toml::Value::Integer(10002)),
            (
                1,
                vec!["core", "epoch"],
                toml::Value::String("33333333-3333-4333-8333-333333333333".into()),
            ),
            (
                1,
                vec!["rewards", "minimum_fee_sats"],
                toml::Value::Integer(1),
            ),
        ] {
            let mut values = fixture();
            let mut target = &mut values[index];
            for part in path {
                target = &mut target[part];
            }
            *target = value;
            assert!(accepts(values).is_err());
        }
    }

    #[test]
    fn inclusive_fees_require_matching_modes_and_no_fixed_fee_floor_or_cap() {
        let mut values = fixture();
        for (index, section) in [(1, "rewards"), (2, "policy")] {
            values[index][section]
                .as_table_mut()
                .unwrap()
                .insert("fee_mode".into(), "deduct_from_share".into());
        }
        values[1]["rewards"]["minimum_fee_sats"] = 0.into();
        values[2]["policy"]["max_fee_sats"] = 0.into();
        accepts(values.clone()).unwrap();
        let mut wrong = values.clone();
        wrong[2]["policy"]["fee_mode"] = "fixed_allowance".into();
        assert!(accepts(wrong).is_err());
        let mut wrong = values.clone();
        wrong[2]["policy"]["max_fee_sats"] = 1.into();
        assert!(accepts(wrong).is_err());
        values[1]["rewards"]["minimum_fee_sats"] = 1.into();
        assert!(accepts(values).is_err());
    }

    #[test]
    fn all_zero_review_is_valid_but_self_transfer_exclusions_and_no_secret_fields_are_required() {
        let mut values = fixture();
        values[0]["core_bridge"]["reward_policy"]["member_percent_ceiling"] = 0.into();
        values[0]["core_bridge"]["reward_policy"]["max_sats_per_feed"] = 0.into();
        values[0]["core_bridge"]["reward_policy"]["max_sats_per_24h"] = 0.into();
        values[1]["settings"]["member_percent"] = 0.into();
        values[1]["rewards"]["minimum_fee_sats"] = 0.into();
        values[1]["rewards"]["maximum_net_sats"] = 0.into();
        for name in [
            "member_percent_ceiling",
            "max_sats_per_feed",
            "max_sats_per_24h",
            "max_fee_sats",
            "max_net_sats",
        ] {
            values[2]["policy"][name] = 0.into();
        }
        values[3]["policy"]["max_net_sats"] = 0.into();
        values[3]["policy"]["max_new_per_hour"] = 0.into();
        accepts(values).unwrap();
        let mut values = fixture();
        values[1]["rewards"]["operation_addresses"] = toml::Value::Array(vec![]);
        values[2]["policy"]["operation_addresses"] = toml::Value::Array(vec![]);
        assert!(accepts(values).is_err());
        let mut values = fixture();
        values[2]
            .as_table_mut()
            .unwrap()
            .insert("api_key".into(), "unaccepted secret field".into());
        assert!(accepts(values).is_err());
    }
}
