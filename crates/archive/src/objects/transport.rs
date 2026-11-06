//! Bounded existing-Rustls transport; no redirects, proxies or ambient destinations.
use super::{ArchiveObjectError, MAX_FILE_BYTES};
use async_trait::async_trait;
use http_body_util::BodyExt;
use object_store::client::{
    ClientOptions, HttpClient, HttpConnector, HttpError, HttpErrorKind, HttpRequest, HttpResponse,
};
use std::time::Duration;
use url::Url;
#[derive(Clone)]
pub(super) struct Transport {
    client: reqwest::Client,
    origin: Url,
}
impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArchiveTransport([REDACTED])")
    }
}
impl Transport {
    pub(super) fn new(origin: Url) -> Result<Self, ArchiveObjectError> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| ArchiveObjectError::Configuration)?;
        Ok(Self { client, origin })
    }
}
fn error() -> HttpError {
    HttpError::new(
        HttpErrorKind::Unknown,
        std::io::Error::other("archive transport failed"),
    )
}
impl HttpConnector for Transport {
    fn connect(&self, _: &ClientOptions) -> object_store::Result<HttpClient> {
        Ok(HttpClient::new(self.clone()))
    }
}
#[async_trait]
impl object_store::client::HttpService for Transport {
    async fn call(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        if request.body().content_length() > MAX_FILE_BYTES {
            return Err(error());
        }
        let (parts, body) = request.into_parts();
        let url = Url::parse(&parts.uri.to_string()).map_err(|_| error())?;
        if url.origin() != self.origin.origin() {
            return Err(error());
        }
        let body = body.collect().await.map_err(|_| error())?.to_bytes();
        let head = parts.method == reqwest::Method::HEAD;
        let mut reply = self
            .client
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body)
            .send()
            .await
            .map_err(|_| error())?;
        if reply.status().is_redirection() {
            return Err(error());
        }
        let mut response = HttpResponse::new(Vec::new().into());
        *response.status_mut() = reply.status();
        *response.headers_mut() = reply.headers().clone();
        let mut bytes = Vec::new();
        if !head {
            if reply
                .content_length()
                .is_some_and(|n| n > MAX_FILE_BYTES as u64)
            {
                return Err(error());
            }
            while let Some(chunk) = reply.chunk().await.map_err(|_| error())? {
                if chunk.len() > MAX_FILE_BYTES - bytes.len() {
                    return Err(error());
                }
                bytes.extend_from_slice(&chunk);
            }
        }
        *response.body_mut() = bytes.into();
        Ok(response)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use object_store::client::{HttpRequestBody, HttpService};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn redirects_and_unbounded_chunked_bodies_are_rejected() {
        for oversized in [false, true] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin =
                Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let _ = socket.read(&mut request).await.unwrap();
                if oversized {
                    let _ = socket
                        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                        .await;
                    let chunk = vec![0_u8; MAX_FILE_BYTES];
                    for _ in 0..2 {
                        if socket
                            .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if socket.write_all(&chunk).await.is_err() {
                            break;
                        }
                        if socket.write_all(b"\r\n").await.is_err() {
                            break;
                        }
                    }
                } else {
                    let _=socket.write_all(b"HTTP/1.1 307 Temporary Redirect\r\nLocation: https://forbidden.example/\r\nContent-Length: 0\r\n\r\n").await;
                }
            });
            let transport = Transport::new(origin.clone()).unwrap();
            let mut request = HttpRequest::new(HttpRequestBody::empty());
            *request.uri_mut() = origin.as_str().parse().unwrap();
            assert!(transport.call(request).await.is_err());
            server.await.unwrap();
        }
    }
}
