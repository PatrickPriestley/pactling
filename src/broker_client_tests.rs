use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn mcp_does_not_retry_http2_refused_stream() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = base_url(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = attempts.clone();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut connection = h2::server::Builder::new()
                    .handshake::<_, &'static [u8]>(socket)
                    .await
                    .unwrap();
                while let Some(result) = connection.accept().await {
                    let (_, mut response) = result.unwrap();
                    seen.fetch_add(1, Ordering::SeqCst);
                    response.send_reset(h2::Reason::REFUSED_STREAM);
                }
            });
        }
    });
    let client = ReadClient {
        client: mcp_client_builder()
            .http2_prior_knowledge()
            .build()
            .unwrap(),
        base,
        credentials: Credentials::default(),
    };
    let result = client.get(&ReadRoute::Integrations).await;
    server.abort();
    assert_eq!(result.unwrap_err(), ClientError::Unavailable);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "one tool call must make exactly one attempt, including protocol NACKs"
    );
}
