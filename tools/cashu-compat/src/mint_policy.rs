//! Strict configured mint identities and connection-address admission.
//! A transport must pin these validated addresses on each connection, disable
//! redirects/proxies and retain TLS hostname verification. URL checks alone do
//! not establish SSRF protection or qualify a mint's financial behavior.

use anyhow::{Result, ensure};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};
use url::Url;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MintPolicy {
    pub id: String,
    pub advertised_url: String,
    pub policy_version: String,
    pub receive_cashu: bool,
    pub receive_nutzap: bool,
    /// Receiving is disabled until the fixture/live capability qualification exists.
    pub bolt11_exit_qualified: bool,
}

pub struct MintRegistry {
    by_url: BTreeMap<String, MintPolicy>,
}

fn canonical_url(value: &str) -> Result<String> {
    ensure!(
        value.len() <= 2048 && !value.contains('\\'),
        "invalid mint URL"
    );
    let url = Url::parse(value).map_err(|_| anyhow::anyhow!("invalid mint URL"))?;
    ensure!(
        url.scheme() == "https" && url.host_str().is_some(),
        "mint must use HTTPS"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "unsupported mint URL components"
    );
    // Only the standard root slash is an implicit spelling variation. Other
    // aliases require explicit configuration, never inference from user input.
    let canonical = url.as_str().trim_end_matches('/').to_owned();
    ensure!(
        value == canonical || value == format!("{canonical}/"),
        "noncanonical mint URL"
    );
    Ok(canonical)
}

impl MintRegistry {
    pub fn cashu_mints(&self) -> impl Iterator<Item = &MintPolicy> {
        self.by_url
            .values()
            .filter(|policy| policy.receive_cashu && policy.bolt11_exit_qualified)
    }

    pub fn new(policies: Vec<MintPolicy>) -> Result<Self> {
        let mut by_url = BTreeMap::new();
        let mut ids = std::collections::BTreeSet::new();
        for policy in policies {
            ensure!(
                !policy.id.is_empty()
                    && policy.id.len() <= 128
                    && !policy.policy_version.is_empty()
                    && policy.policy_version.len() <= 128,
                "invalid mint policy identity"
            );
            ensure!(ids.insert(policy.id.clone()), "duplicate mint identity");
            let url = canonical_url(&policy.advertised_url)?;
            ensure!(
                by_url.insert(url, policy).is_none(),
                "mint URL has multiple identities"
            );
        }
        Ok(Self { by_url })
    }

    /// No DNS or network request occurs here. Unknown and disabled mints fail first.
    pub fn accepting(&self, advertised_url: &str, nutzap: bool) -> Result<&MintPolicy> {
        let canonical = canonical_url(advertised_url)?;
        let policy = self
            .by_url
            .get(&canonical)
            .ok_or_else(|| anyhow::anyhow!("mint not accepted"))?;
        ensure!(policy.bolt11_exit_qualified, "mint exit not qualified");
        ensure!(
            if nutzap {
                policy.receive_nutzap
            } else {
                policy.receive_cashu
            },
            "mint receiving path disabled"
        );
        Ok(policy)
    }
}

/// Fail closed on mixed public/private DNS answers. The connector must use this
/// exact set rather than asking its HTTP stack to resolve the hostname again.
pub fn validate_connection_addresses(addresses: &[SocketAddr]) -> Result<Vec<SocketAddr>> {
    ensure!(
        !addresses.is_empty() && addresses.len() <= 32,
        "invalid mint DNS response"
    );
    ensure!(
        addresses.iter().all(|address| public_address(address.ip())),
        "mint DNS includes prohibited address"
    );
    let mut addresses = addresses.to_vec();
    addresses.sort_unstable();
    addresses.dedup();
    Ok(addresses)
}

fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192
                    && (b == 168 || (b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            // Global unicast only; reject mapped/compatible IPv4, ULA, scoped,
            // link-local, multicast, NAT64, special-use and transition ranges.
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] <= 0x01ff || s[1] == 0x0db8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && (s[1] & 0xf000) == 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy(id: &str, url: &str) -> MintPolicy {
        MintPolicy {
            id: id.into(),
            advertised_url: url.into(),
            policy_version: "v1".into(),
            receive_cashu: true,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        }
    }
    #[test]
    fn two_mints_are_explicit_and_aliases_do_not_create_issuers() -> Result<()> {
        let registry = MintRegistry::new(vec![
            policy("one", "https://one.example"),
            policy("two", "https://two.example/cashu"),
        ])?;
        assert_eq!(registry.accepting("https://one.example/", false)?.id, "one");
        assert_eq!(
            registry.accepting("https://two.example/cashu", true)?.id,
            "two"
        );
        for url in [
            "https://unknown.example",
            "http://one.example",
            "https://one.example@evil.example",
            "https://one.example?mint=1",
            "https://one.example/#fragment",
            "https://one.example/../",
            "https://ONE.example",
        ] {
            assert!(registry.accepting(url, false).is_err(), "accepted {url}");
        }
        assert!(
            MintRegistry::new(vec![
                policy("one", "https://one.example"),
                policy("two", "https://one.example/")
            ])
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn every_dns_answer_is_checked_including_ipv6_and_rebinding() -> Result<()> {
        let public: SocketAddr = "1.1.1.1:443".parse()?;
        for host in [
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "192.0.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:8.8.8.8",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:0808:0808::1",
            "64:ff9b::7f00:1",
            "3fff::1",
        ] {
            let private = SocketAddr::new(host.parse()?, 443);
            assert!(
                validate_connection_addresses(&[public, private]).is_err(),
                "accepted {host}"
            );
        }
        assert!(validate_connection_addresses(&[]).is_err());
        assert!(validate_connection_addresses(&[public]).is_ok());
        assert!(validate_connection_addresses(&["[2606:4700:4700::1111]:443".parse()?]).is_ok());
        // A later connection must revalidate even after a successful resolution.
        assert!(validate_connection_addresses(&["127.0.0.1:443".parse()?]).is_err());
        Ok(())
    }
    #[test]
    fn exit_and_each_transport_are_independently_gated() -> Result<()> {
        let mut unavailable = policy("one", "https://one.example");
        unavailable.bolt11_exit_qualified = false;
        assert!(
            MintRegistry::new(vec![unavailable])?
                .accepting("https://one.example", false)
                .is_err()
        );
        let mut cashu_only = policy("two", "https://two.example");
        cashu_only.receive_nutzap = false;
        let registry = MintRegistry::new(vec![cashu_only])?;
        assert!(registry.accepting("https://two.example", false).is_ok());
        assert!(registry.accepting("https://two.example", true).is_err());
        Ok(())
    }
}
