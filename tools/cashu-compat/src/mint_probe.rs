//! Public mint metadata inspection. Deliberately does not open a wallet or submit work.
use crate::{service_config::Config, transport::MintTransport};
use anyhow::{Context, Result, ensure};
use cdk::{
    mint_url::MintUrl,
    nuts::{CurrencyUnit, Id, KeySet, KeySetInfo, MintInfo, PaymentMethod, PublicKey},
};
use cdk_http_client::Transport;
use futures_util::{StreamExt, TryStreamExt, stream};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// Deserialize the entire inventory. CDK's ordinary response adapter may skip
// malformed entries, which would hide an active set during an operator review.
#[derive(Deserialize)]
struct Inventory<T> {
    keysets: Vec<T>,
}

#[derive(Serialize)]
struct KeysetReport {
    id: String,
    active: bool,
    input_fee_ppk: u64,
    final_expiry: Option<u64>,
    mint_denominations: usize,
    pinned_denominations: usize,
}

#[derive(Serialize)]
pub struct Report {
    mint_id: String,
    mint_url: String,
    observed_at: u64,
    bolt11_sat_min: Option<u64>,
    bolt11_sat_max: Option<u64>,
    change_advertised: bool,
    keysets: Vec<KeysetReport>,
}

pub async fn probe(config: &Config, mint_id: &str) -> Result<Report> {
    let mint = config
        .mints
        .iter()
        .find(|mint| mint.id == mint_id)
        .context("mint is not in the protected configuration")?;
    let mut pins = BTreeMap::new();
    for pinned in config
        .keysets
        .iter()
        .filter(|pinned| pinned.mint_id == mint_id)
    {
        let id: Id = pinned.id.parse()?;
        ensure!(
            pins.insert(id.to_string(), &pinned.keys).is_none(),
            "duplicate configured keyset"
        );
    }
    ensure!(!pins.is_empty(), "selected mint has no pinned public keys");
    let transport = MintTransport::new(
        mint.advertised_url.parse()?,
        Duration::from_secs(config.deadline_seconds),
        256 * 1024,
    )?;
    tokio::time::timeout(
        Duration::from_secs(60),
        observe(&transport, mint_id, &mint.advertised_url, &pins),
    )
    .await
    .context("public mint probe timed out")?
}

pub(crate) async fn sat_inventory(
    transport: &impl Transport,
    url: &MintUrl,
) -> Result<BTreeMap<String, KeySetInfo>> {
    let response: Inventory<KeySetInfo> = transport
        .http_get(url.join("v1/keysets")?, None)
        .await
        .context("mint keyset inventory unavailable")?;
    let mut sat = BTreeMap::new();
    for entry in response.keysets {
        if entry.unit == CurrencyUnit::Sat {
            ensure!(
                sat.insert(entry.id.to_string(), entry).is_none(),
                "duplicate sat keyset in the mint inventory"
            );
        }
    }
    Ok(sat)
}

async fn observe(
    transport: &impl Transport,
    mint_id: &str,
    mint_url: &str,
    pins: &BTreeMap<String, &BTreeMap<u64, String>>,
) -> Result<Report> {
    let url: MintUrl = mint_url.parse()?;
    let info: MintInfo = transport
        .http_get(url.join("v1/info")?, None)
        .await
        .context("mint capability metadata unavailable")?;
    let nuts = info.nuts;
    let melt = nuts
        .nut05
        .get_settings(&CurrencyUnit::Sat, &PaymentMethod::BOLT11)
        .context("mint does not advertise a sat BOLT11 exit")?;
    let min = melt.min_amount.map(u64::from);
    let max = melt.max_amount.map(u64::from);
    ensure!(
        !nuts.nut05.disabled
            && max != Some(0)
            && min.zip(max).is_none_or(|(lower, upper)| lower <= upper),
        "mint advertised an unusable sat BOLT11 exit"
    );
    ensure!(
        nuts.nut07.supported
            && nuts.nut09.supported
            && nuts.nut11.supported
            && nuts.nut12.supported,
        "mint does not advertise proof-state, recovery, P2PK and DLEQ support"
    );

    let inventory = sat_inventory(transport, &url).await?;
    let mut active = inventory.values().filter(|entry| entry.active).peekable();
    ensure!(
        active.peek().is_some() && active.all(|entry| pins.contains_key(&entry.id.to_string())),
        "mint has no active sat keyset or advertises an unpinned one"
    );
    ensure!(
        pins.keys().all(|id| inventory.contains_key(id)),
        "mint omitted a pinned sat keyset"
    );
    let keysets = stream::iter(pins)
        .map(|(id, pinned)| check_keyset(transport, &url, &inventory[id], pinned))
        .buffered(4)
        .try_collect::<Vec<KeysetReport>>()
        .await?;
    ensure!(
        sat_inventory(transport, &url).await? == inventory,
        "mint keyset inventory changed during inspection"
    );
    let observed_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    ensure!(
        keysets
            .iter()
            .all(|set| set.final_expiry.is_none_or(|time| time > observed_at)),
        "a pinned keyset is already past its final expiry"
    );
    Ok(Report {
        mint_id: mint_id.into(),
        mint_url: mint_url.into(),
        observed_at,
        bolt11_sat_min: min,
        bolt11_sat_max: max,
        change_advertised: nuts.nut08.supported,
        keysets,
    })
}

async fn check_keyset(
    transport: &impl Transport,
    url: &MintUrl,
    metadata: &KeySetInfo,
    pinned: &BTreeMap<u64, String>,
) -> Result<KeysetReport> {
    let response: Inventory<KeySet> = transport
        .http_get(url.join(&format!("v1/keys/{}", metadata.id))?, None)
        .await
        .context("configured mint public keys unavailable")?;
    let [set] = response.keysets.as_slice() else {
        anyhow::bail!("mint key endpoint returned an unexpected inventory");
    };
    verify_keyset(set, metadata, pinned)
}

fn verify_keyset(
    set: &KeySet,
    metadata: &KeySetInfo,
    pinned: &BTreeMap<u64, String>,
) -> Result<KeysetReport> {
    ensure!(
        set.id == metadata.id
            && set.unit == metadata.unit
            && set.active.is_none_or(|active| active == metadata.active)
            && set.input_fee_ppk == metadata.input_fee_ppk
            && set.final_expiry == metadata.final_expiry,
        "mint key endpoint metadata differs from the inventory"
    );
    set.verify_id()
        .context("mint keyset ID does not match its public data")?;
    for (amount, key) in pinned {
        ensure!(
            set.keys.amount_key((*amount).into()) == Some(PublicKey::from_hex(key)?),
            "mint public key differs from the protected pin"
        );
    }
    Ok(KeysetReport {
        id: set.id.to_string(),
        active: metadata.active,
        input_fee_ppk: set.input_fee_ppk,
        final_expiry: set.final_expiry,
        mint_denominations: set.keys.keys().len(),
        pinned_denominations: pinned.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::nuts::{Keys, SecretKey};

    #[test]
    fn verifies_the_full_keyset_id_and_expected_pins_and_fees() -> Result<()> {
        let key = SecretKey::from_slice(&[7; 32])?.public_key();
        let other = SecretKey::from_slice(&[8; 32])?.public_key();
        let keys = Keys::new(BTreeMap::from([(1.into(), key), (8.into(), other)]));
        let pins = BTreeMap::from([(1, key.to_hex())]);
        let expiry = Some(2_100_000_000);
        let mut keyset = KeySet {
            id: Id::v2_from_data(&keys, &CurrencyUnit::Sat, 15, expiry),
            unit: CurrencyUnit::Sat,
            active: Some(true),
            keys,
            input_fee_ppk: 15,
            final_expiry: expiry,
        };
        let mut listed = KeySetInfo {
            id: keyset.id,
            unit: CurrencyUnit::Sat,
            active: true,
            input_fee_ppk: 15,
            final_expiry: expiry,
        };
        let report = verify_keyset(&keyset, &listed, &pins)?;
        assert_eq!(
            (report.mint_denominations, report.pinned_denominations),
            (2, 1)
        );
        assert!(verify_keyset(&keyset, &listed, &BTreeMap::from([(1, other.to_hex())])).is_err());
        assert!(verify_keyset(&keyset, &listed, &BTreeMap::from([(2, key.to_hex())])).is_err());
        listed.active = false;
        assert!(verify_keyset(&keyset, &listed, &pins).is_err());
        listed.active = true;
        listed.input_fee_ppk = 0;
        assert!(verify_keyset(&keyset, &listed, &pins).is_err());
        // Matching metadata in both replies still cannot rebind a V2 ID.
        keyset.input_fee_ppk = 0;
        assert!(verify_keyset(&keyset, &listed, &pins).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn unknown_mint_or_missing_pins_refuse_before_network_access() -> Result<()> {
        let mut config =
            crate::relay_intake::tests::config(std::path::Path::new("/run/mint-probe-fixture"))?;
        let unknown = probe(&config, "elsewhere")
            .await
            .err()
            .context("unknown mint accepted")?;
        assert!(
            unknown
                .to_string()
                .contains("not in the protected configuration")
        );
        config.keysets.clear();
        let unpinned = probe(&config, "fixture")
            .await
            .err()
            .context("unpinned mint accepted")?;
        assert!(unpinned.to_string().contains("no pinned public keys"));
        Ok(())
    }
}
