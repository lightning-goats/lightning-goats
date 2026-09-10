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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, extract::Path, response::Response, routing::get};

    #[tokio::test]
    async fn chunked_response_is_bounded_at_exact_limit_without_content_length() {
        let app = Router::new().route(
            "/{size}",
            get(|Path(size): Path<usize>| async move {
                Response::new(Body::from_stream(futures_util::stream::iter(vec![
                    Ok::<_, std::convert::Infallible>(vec![b'x'; 8]),
                    Ok(vec![b'y'; size - 8]),
                ])))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for size in [15, 16, 17] {
            let response = client
                .get(format!("http://{address}/{size}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.content_length(), None);
            let body = bounded_body(response, 16).await;
            if size <= 16 {
                assert_eq!(body.unwrap().len(), size);
            } else {
                assert!(body.is_err());
            }
        }
        server.abort();
    }
}
