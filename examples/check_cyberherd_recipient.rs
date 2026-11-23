//! Read-only native recipient/DNS qualification. Never initialize application
//! state, load credentials, follow a callback, request an invoice or send money.
#![forbid(unsafe_code)]

use anyhow::{Result, bail};
use lightning_goats::{payout::recipient::Recipient, public_network};
use std::{collections::BTreeSet, net::IpAddr, time::Duration};

const FIXTURE_HOSTS: [(&str, &[&str]); 3] = [
    ("private-v4.cyberherd.example.net", &["127.0.0.1"]),
    ("private-v6.cyberherd.example.net", &["fd00:abcd::1"]),
    ("mixed.cyberherd.example.net", &["1.1.1.1", "127.0.0.1"]),
];

fn private_dns_refusal(error: &anyhow::Error) -> bool {
    error.chain().any(|part| {
        part.to_string()
            == "DNS response includes a non-public, unsupported or excessive address set"
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("recipient qualification must not load any systemd credential");
    }
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [mode, address] if mode == "public" => {
            let recipient = Recipient::fetch(address)
                .await
                .map_err(|_| anyhow::anyhow!("public recipient discovery did not qualify"))?;
            println!(
                "{}",
                serde_json::json!({"recipient_validated":true,
                    "nostr_receipts_advertised":recipient.receipt_pubkey.is_some(),
                    "min_msat":recipient.min_msat,"max_msat":recipient.max_msat,
                    "invoice_requests":0,"provider_operations":0,"credentials_loaded":false})
            );
        }
        [mode] if mode == "private-dns-fixture" => {
            // The temporary unit must bind its own reviewed hosts file. This
            // never changes host DNS; NXDOMAIN/connection errors cannot pass.
            for (host, expected) in FIXTURE_HOSTS {
                let answers: BTreeSet<IpAddr> = tokio::time::timeout(
                    Duration::from_secs(4),
                    tokio::net::lookup_host((host, 443)),
                )
                .await??
                .take(17)
                .map(|address| address.ip())
                .collect();
                let expected: BTreeSet<IpAddr> = expected
                    .iter()
                    .map(|address| address.parse())
                    .collect::<std::result::Result<_, _>>()?;
                if answers != expected {
                    bail!("fixture DNS answers differ from the reviewed hosts file");
                }
                let url = format!("https://{host}/.well-known/lnurlp/member");
                public_network::validate_destination(&url, "https")?;
                match public_network::get_json(&url, 4096).await {
                    Err(error) if private_dns_refusal(&error) => {}
                    _ => bail!("fixture did not prove native private-DNS refusal"),
                }
            }
            println!(
                "{}",
                serde_json::json!({"private_dns_fixtures_rejected":3,"fixture_answer_sets_verified":true,
                    "native_dns_refusal_before_http":true,"credentials_loaded":false,
                    "invoice_requests":0,"provider_operations":0})
            );
        }
        _ => bail!("use public LIGHTNING_ADDRESS or private-dns-fixture"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_native_dns_admission_refusal_can_accept_a_negative_fixture() {
        let refusal = anyhow::anyhow!(
            "DNS response includes a non-public, unsupported or excessive address set"
        )
        .context("outer fetch context");
        assert!(private_dns_refusal(&refusal));
        for other in [
            "NXDOMAIN",
            "connection refused",
            "public DNS lookup deadline exceeded",
            "destination address is not globally routable",
        ] {
            assert!(!private_dns_refusal(&anyhow::anyhow!(other.to_owned())));
        }
    }
}
