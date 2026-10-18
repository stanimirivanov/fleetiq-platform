//! Bounded loopback probe for shell-free container health checks.

use std::{error::Error, fmt, net::SocketAddr, time::Duration};

use openidconnect::reqwest;

/// Checks the image's fixed HTTP port without reading secrets or startup configuration.
pub(crate) async fn check() -> Result<(), ProbeError> {
    probe(SocketAddr::from(([127, 0, 0, 1], 8080))).await
}

async fn probe(address: SocketAddr) -> Result<(), ProbeError> {
    // Reuse the existing maintained HTTP client. Ignore ambient proxy settings,
    // reject redirects, and bound connection plus response time together.
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| ProbeError)?
        .get(format!("http://{address}/health/ready"))
        .send()
        .await
        .map_err(|_| ProbeError)?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        Ok(())
    } else {
        Err(ProbeError)
    }
}

/// Deliberately excludes network errors, response bodies and environment values.
#[derive(Debug)]
pub(crate) struct ProbeError;

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("local readiness check failed")
    }
}

impl Error for ProbeError {}

#[cfg(test)]
mod tests {
    use axum::{Router, http::StatusCode, routing::get};
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn only_exact_readiness_success_passes() {
        for status in [
            StatusCode::NO_CONTENT,
            StatusCode::OK,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = Router::new().route("/health/ready", get(move || async move { status }));
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            assert_eq!(
                probe(address).await.is_ok(),
                status == StatusCode::NO_CONTENT
            );
            server.abort();
        }
    }

    #[tokio::test]
    async fn redirects_are_not_followed_and_closed_ports_fail() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route(
                "/health/ready",
                get(|| async { axum::response::Redirect::temporary("/healthy") }),
            )
            .route("/healthy", get(|| async { StatusCode::NO_CONTENT }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(probe(address).await.is_err());
        server.abort();
        server.await.expect_err("fixture should be cancelled");
        assert!(probe(address).await.is_err());
        assert_eq!(ProbeError.to_string(), "local readiness check failed");
    }

    #[tokio::test]
    async fn an_unresponsive_endpoint_is_bounded() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/health/ready",
            get(|| async { std::future::pending::<StatusCode>().await }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = tokio::time::timeout(Duration::from_secs(5), probe(address)).await;
        server.abort();
        assert!(
            result
                .expect("probe must have a bounded request deadline")
                .is_err()
        );
    }
}
