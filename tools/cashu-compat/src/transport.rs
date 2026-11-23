//! CDK HTTP adapter bound to one configured mint. No public RPC surface.

use crate::mint_policy::validate_connection_addresses;
use async_trait::async_trait;
use cdk::nuts::AuthToken;
use cdk_http_client::{
    HttpError, RawResponse, Transport,
    ws::{WsError, WsReceiver, WsSender},
};
use reqwest::{Client, Method};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fmt,
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use url::{Host, Url};

fn refused(message: &'static str) -> HttpError {
    HttpError::Other(message.to_owned())
}

#[async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, HttpError>;
}

struct SystemResolver;
#[async_trait]
impl Resolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, HttpError> {
        let answers = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| HttpError::Connection("mint DNS unavailable".into()))?;
        Ok(answers.take(33).collect())
    }
}

#[derive(Clone)]
pub struct MintTransport {
    base: Url,
    resolver: Arc<dyn Resolver>,
    duration: Duration,
    body_limit: usize,
}
impl fmt::Debug for MintTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MintTransport")
            .field("body_limit", &self.body_limit)
            .finish_non_exhaustive()
    }
}

impl MintTransport {
    pub fn new(base: Url, duration: Duration, body_limit: usize) -> Result<Self, HttpError> {
        if base.scheme() != "https"
            || base.host().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || duration.is_zero()
            || body_limit == 0
            || body_limit > 16 * 1024 * 1024
        {
            return Err(refused("invalid mint transport configuration"));
        }
        Ok(Self {
            base,
            resolver: Arc::new(SystemResolver),
            duration,
            body_limit,
        })
    }

    fn authorize(&self, url: &Url, method: &Method) -> Result<(), HttpError> {
        if url.origin() != self.base.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(refused("mint request destination rejected"));
        }
        let prefix = format!("{}/", self.base.path().trim_end_matches('/'));
        let path = url
            .path()
            .strip_prefix(&prefix)
            .ok_or_else(|| refused("mint request path rejected"))?;
        let get = matches!(path, "v1/info" | "v1/keys" | "v1/keysets")
            || path.strip_prefix("v1/keys/").is_some_and(safe_id)
            || path
                .strip_prefix("v1/melt/quote/bolt11/")
                .is_some_and(safe_id);
        let post = matches!(
            path,
            "v1/swap" | "v1/checkstate" | "v1/restore" | "v1/melt/quote/bolt11" | "v1/melt/bolt11"
        );
        if (method == Method::GET && get) || (method == Method::POST && post) {
            Ok(())
        } else {
            Err(refused("mint operation not enabled"))
        }
    }

    async fn pinned_client(&self) -> Result<Client, HttpError> {
        let port = self
            .base
            .port_or_known_default()
            .ok_or_else(|| refused("missing mint port"))?;
        let host = self
            .base
            .host_str()
            .ok_or_else(|| refused("missing mint host"))?;
        let answers = match self.base.host() {
            Some(Host::Ipv4(ip)) => vec![SocketAddr::new(IpAddr::V4(ip), port)],
            Some(Host::Ipv6(ip)) => vec![SocketAddr::new(IpAddr::V6(ip), port)],
            _ => self.resolver.resolve(host, port).await?,
        };
        let answers = validate_connection_addresses(&answers)
            .map_err(|_| refused("mint connection addresses rejected"))?;
        if answers.iter().any(|answer| answer.port() != port) {
            return Err(refused("mint DNS port mismatch"));
        }
        Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(host, &answers)
            .timeout(self.duration)
            .connect_timeout(self.duration)
            .build()
            .map_err(|_| refused("mint TLS client unavailable"))
    }

    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Vec<u8>>,
    ) -> Result<RawResponse, HttpError> {
        self.authorize(&url, &method)?;
        let operation = async {
            let client = self.pinned_client().await?;
            let mut request = client.request(method, url);
            if let Some(body) = body {
                request = request
                    .header("content-type", "application/json")
                    .body(body);
            }
            let mut response = request.send().await.map_err(|error| {
                if error.is_timeout() {
                    HttpError::Timeout
                } else {
                    HttpError::Connection("mint request outcome unavailable".into())
                }
            })?;
            let status = response.status().as_u16();
            // Never pass untrusted error-body text to CDK diagnostics.
            if !(200..300).contains(&status) {
                return Err(HttpError::Status {
                    status,
                    message: "mint request rejected".into(),
                });
            }
            if response
                .content_length()
                .is_some_and(|length| length > self.body_limit as u64)
            {
                return Err(refused("mint response exceeds limit"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| HttpError::Connection("mint reply incomplete".into()))?
            {
                if chunk.len() > self.body_limit.saturating_sub(bytes.len()) {
                    return Err(refused("mint response exceeds limit"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(RawResponse::new(status, bytes))
        };
        tokio::time::timeout(self.duration, operation)
            .await
            .map_err(|_| HttpError::Timeout)?
    }
}
fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
struct LimitedBody {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedBody {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("body limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn serialize_body<P: Serialize>(
    payload: &P,
    limit: usize,
) -> Result<Vec<u8>, HttpError> {
    let mut body = LimitedBody {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut body, payload)
        .map_err(|_| refused("invalid or oversized mint request"))?;
    Ok(body.bytes)
}

#[async_trait]
impl Transport for MintTransport {
    fn with_proxy(
        &mut self,
        _proxy: Url,
        _matcher: Option<&str>,
        _invalid_certs: bool,
    ) -> Result<(), HttpError> {
        Err(refused("mint proxies disabled"))
    }
    async fn ws_connect(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
    ) -> Result<(WsSender, WsReceiver), WsError> {
        Err(WsError::NotSupported("mint WebSockets disabled".into()))
    }
    async fn http_get<R: DeserializeOwned>(
        &self,
        url: Url,
        auth: Option<AuthToken>,
    ) -> Result<R, HttpError> {
        self.http_get_raw(url, auth)
            .await?
            .json()
            .await
            .map_err(|_| refused("invalid mint response"))
    }
    async fn http_get_raw(
        &self,
        url: Url,
        auth: Option<AuthToken>,
    ) -> Result<RawResponse, HttpError> {
        if auth.is_some() {
            return Err(refused("mint auth profile unsupported"));
        }
        self.request(Method::GET, url, None).await
    }
    async fn http_post<P: Serialize + Send + Sync, R: DeserializeOwned>(
        &self,
        url: Url,
        auth: Option<AuthToken>,
        payload: &P,
    ) -> Result<R, HttpError> {
        self.authorize(&url, &Method::POST)?;
        if auth.is_some() {
            return Err(refused("mint auth profile unsupported"));
        }
        let body = serialize_body(payload, self.body_limit)?;
        self.request(Method::POST, url, Some(body))
            .await?
            .json()
            .await
            .map_err(|_| refused("invalid mint response"))
    }
    async fn http_post_form_raw<P: Serialize + Send + Sync>(
        &self,
        _url: Url,
        _auth: Option<AuthToken>,
        _payload: &P,
    ) -> Result<RawResponse, HttpError> {
        Err(refused("mint form requests disabled"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Rebinding(AtomicUsize);
    #[async_trait]
    impl Resolver for Rebinding {
        async fn resolve(&self, _host: &str, port: u16) -> Result<Vec<SocketAddr>, HttpError> {
            let ip = if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                "1.1.1.1"
            } else {
                "127.0.0.1"
            };
            Ok(vec![SocketAddr::new(ip.parse().unwrap(), port)])
        }
    }
    fn transport() -> MintTransport {
        MintTransport::new(
            Url::parse("https://mint.example/cashu").unwrap(),
            Duration::from_secs(2),
            1024,
        )
        .unwrap()
    }
    #[tokio::test]
    async fn pinning_revalidates_each_resolution_before_connecting() {
        let mut transport = transport();
        transport.resolver = Arc::new(Rebinding(AtomicUsize::new(0)));
        assert!(transport.pinned_client().await.is_ok());
        assert!(transport.pinned_client().await.is_err());
    }
    #[tokio::test]
    async fn unsupported_routes_auth_proxy_and_websocket_never_connect() {
        let mut transport = transport();
        for value in [
            "http://mint.example/cashu/v1/info",
            "https://evil.example/cashu/v1/info",
            "https://mint.example/v1/info",
            "https://mint.example/cashu/v1/info?q=secret",
            "https://mint.example/cashu/v1/mint/quote/bolt11",
        ] {
            assert!(
                transport
                    .http_get_raw(Url::parse(value).unwrap(), None)
                    .await
                    .is_err()
            );
        }
        assert!(
            transport
                .with_proxy(Url::parse("http://127.0.0.1").unwrap(), None, true)
                .is_err()
        );
        assert!(transport.ws_connect("wss://127.0.0.1", &[]).await.is_err());
        assert!(
            transport
                .authorize(
                    &Url::parse("https://mint.example/cashu/v1/swap").unwrap(),
                    &Method::POST
                )
                .is_ok()
        );
    }
    #[test]
    fn serialized_body_is_bounded_without_echoing_payload() {
        let mut body = LimitedBody {
            bytes: Vec::new(),
            limit: 16,
        };
        let error =
            serde_json::to_writer(&mut body, &"secret-token-material-too-large").unwrap_err();
        assert!(body.bytes.len() <= 16);
        assert!(!error.to_string().contains("secret-token"));
    }
}
