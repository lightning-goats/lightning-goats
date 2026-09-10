//! Bounded reads apply to chunked bodies as well as advertised lengths.
use anyhow::{Result, bail};
pub async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response.content_length().is_some_and(|n| n > limit as u64) {
        bail!("provider response exceeds size limit");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > limit.saturating_sub(body.len()) {
            bail!("provider response exceeds size limit");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
