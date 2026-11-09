//! Outbound untrusted LNURL/relay destinations never reach the trusted network.
//! Resolve once, reject every non-public answer and pin the approved addresses.
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::protocol::WebSocketConfig};

const DNS_TIMEOUT: Duration = Duration::from_secs(4);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub fn validate_destination(raw: &str, scheme: &str) -> Result<Url> {
    if raw.len() > 2048 || raw.chars().any(char::is_control) || !matches!(scheme, "https" | "wss") {
        bail!("invalid public destination");
    }
    let url = Url::parse(raw).context("invalid public destination URL")?;
    if url.scheme() != scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        bail!("public destination requires secure port 443 without credentials or fragment");
    }
    let host = url
        .host_str()
        .context("public destination is missing a host")?;
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = literal.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            bail!("destination address is not globally routable");
        }
    } else {
        if host.len() > 253
            || !host.contains('.')
            || host.ends_with('.')
            || [
                ".localhost",
                ".local",
                ".internal",
                ".home",
                ".test",
                ".invalid",
                ".onion",
            ]
            .iter()
            .any(|suffix| host.ends_with(suffix))
        {
            bail!("public destination requires a public DNS name");
        }
        for label in host.split('.') {
            if label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                bail!("invalid public destination DNS name");
            }
        }
    }
    Ok(url)
}

#[must_use]
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let bits = u32::from(ip);
            ![
                (0x00000000, 8),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ]
            .iter()
            .any(|(network, prefix)| bits & (u32::MAX << (32 - prefix)) == *network)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // Accept global unicast only. Exclude protocol/transition and
            // documentation ranges, including IPv4-mapped/NAT64 encodings.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && (segments[1] & 0xf000) == 0)
        }
    }
}

fn validate_answers(addresses: &[SocketAddr]) -> Result<()> {
    if addresses.is_empty()
        || addresses.len() > 16
        || addresses
            .iter()
            .any(|address| address.port() != 443 || !is_public_ip(address.ip()))
    {
        bail!("DNS response includes a non-public, unsupported or excessive address set");
    }
    Ok(())
}

async fn resolve(url: &Url) -> Result<Vec<SocketAddr>> {
    let host = url.host_str().context("destination is missing a host")?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<_> = timeout(DNS_TIMEOUT, tokio::net::lookup_host((host, 443)))
        .await
        .context("public DNS lookup deadline exceeded")??
        .take(17)
        .collect();
    validate_answers(&addresses)?;
    Ok(addresses)
}

pub async fn get_json(raw: &str, max_bytes: usize) -> Result<Vec<u8>> {
    if max_bytes == 0 || max_bytes > 65_536 {
        bail!("invalid public response bound");
    }
    timeout(Duration::from_secs(12), async {
        let url = validate_destination(raw, "https")?;
        let addresses = resolve(&url).await?;
        let host = url.host_str().context("destination is missing a host")?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(8))
            .resolve_to_addrs(host, &addresses)
            .build()?;
        let response = client
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("public JSON endpoint did not succeed");
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .context("public JSON endpoint omitted content type")?;
        if !content_type
            .split(';')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
        {
            bail!("public endpoint did not return JSON");
        }
        crate::http::bounded_body(response, max_bytes).await
    })
    .await
    .context("public JSON fetch deadline exceeded")?
}

pub async fn connect_relay(raw: &str) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    timeout(Duration::from_secs(12), async {
        let url = validate_destination(raw, "wss")?;
        let addresses = resolve(&url).await?;
        // TCP receives the approved IP set directly; TLS/WebSocket retain the
        // original hostname. No second DNS lookup can rebind to a private host.
        let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(addresses.as_slice()))
            .await
            .context("relay TCP connection deadline exceeded")??;
        let config = WebSocketConfig::default()
            .max_message_size(Some(131_072))
            .max_frame_size(Some(131_072));
        let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
            url.as_str(),
            stream,
            Some(config),
            None,
        )
        .await?;
        Ok(socket)
    })
    .await
    .context("relay connection deadline exceeded")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_private_documentation_special_mapped_and_mixed_dns_answers() {
        for ip in [
            "0.1.2.3",
            "10.8.0.6",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.31.0.1",
            "192.0.0.9",
            "192.0.2.1",
            "192.168.1.217",
            "198.18.1.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b::a08:6",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2001::1",
            "2002:808:808::1",
            "3fff::1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "93.184.216.34",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
        ] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(
            validate_answers(&[
                "8.8.8.8:443".parse().unwrap(),
                "10.8.0.6:443".parse().unwrap()
            ])
            .is_err()
        );
        assert!(validate_answers(&[]).is_err());
        assert!(validate_answers(&["8.8.8.8:8789".parse().unwrap()]).is_err());
    }

    #[test]
    fn urls_cannot_select_credentials_ports_local_aliases_or_alternate_ip_encodings() {
        for raw in [
            "http://example.com/",
            "https://example.com:8789/",
            "https://user:pass@example.com/",
            "https://example.com/#fragment",
            "https://10.8.0.6/",
            "https://127.1/",
            "https://2130706433/",
            "https://[::1]/",
            "https://home.local/",
            "https://localhost/",
            "https://a.internal/",
            "https://example.com./",
        ] {
            assert!(validate_destination(raw, "https").is_err(), "{raw}");
        }
        assert!(validate_destination("https://pay.example.com/lnurl?amount=1000", "https").is_ok());
        assert!(validate_destination("wss://relay.example.com/path", "wss").is_ok());
        assert!(validate_destination("wss://8.8.8.8/", "wss").is_ok());
    }
}
