//! Bounded HTTP/1.1 transport for nginx/gateway upstream connections.
use anyhow::Result;
use axum::Router;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinSet};

pub async fn serve(listener: TcpListener, app: Router) -> Result<()> {
    serve_with_shutdown(listener, app, std::future::pending()).await
}

pub async fn serve_with_shutdown(
    listener: TcpListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> Result<()> {
    serve_with_limits(
        listener,
        app,
        Arc::new(Semaphore::new(128)),
        Duration::from_secs(10),
        Duration::from_secs(180),
        shutdown,
    )
    .await
}

async fn serve_with_limits(
    listener: TcpListener,
    app: Router,
    slots: Arc<Semaphore>,
    header_timeout: Duration,
    connection_timeout: Duration,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> Result<()> {
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let Ok(permit) = slots.clone().try_acquire_owned() else { drop(stream); continue; };
                let service = TowerToHyperService::new(app.clone());
                connections.spawn(async move {
                    let _permit = permit;
                    let mut builder = hyper::server::conn::http1::Builder::new();
                    builder.timer(TokioTimer::new()).header_read_timeout(header_timeout).max_buf_size(32 * 1024);
                    // 180s exceeds gateway's 140s handler / 150s client budget.
                    // Upgrades transfer to their separately bounded WS lifetime.
                    let connection = builder.serve_connection(TokioIo::new(stream), service).with_upgrades();
                    if let Ok(Err(error)) = tokio::time::timeout(connection_timeout, connection).await {
                        tracing::debug!(%error, "HTTP connection closed");
                    }
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    while connections.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn incomplete_headers_are_capped_and_expire_before_handler_admission() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let slots = Arc::new(Semaphore::new(2));
        let server = tokio::spawn(serve_with_limits(
            listener,
            Router::new().fallback(|| async { "ok" }),
            slots.clone(),
            Duration::from_millis(300),
            Duration::from_secs(2),
            std::future::pending(),
        ));
        let mut slow = Vec::new();
        for _ in 0..2 {
            let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
            socket.write_all(b"GET / HTTP/1.1\r\nHost:").await.unwrap();
            slow.push(socket);
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while slots.available_permits() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut excess = tokio::net::TcpStream::connect(address).await.unwrap();
        let _ = excess
            .write_all(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n")
            .await;
        let mut response = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(1), excess.read_to_end(&mut response))
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&response).contains("200 OK"));
        for mut socket in slow {
            let mut response = String::new();
            let _ =
                tokio::time::timeout(Duration::from_secs(2), socket.read_to_string(&mut response))
                    .await
                    .unwrap();
            assert!(!response.contains("200 OK"));
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while slots.available_permits() != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        server.abort();
    }
}
