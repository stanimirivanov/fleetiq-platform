//! Real TLS verifies trust; no fixture key is committed or read from deployment files.
use super::*;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair, KeyUsagePurpose};
use std::{error::Error, fs, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinSet,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{self, pki_types::PrivatePkcs8KeyDer},
};

struct Fixture {
    issuer: String,
    pem: String,
    task: tokio::task::JoinHandle<()>,
    directory: PathBuf,
}
impl Fixture {
    async fn start() -> Result<Self, Box<dyn Error>> {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca = CertifiedIssuer::self_signed(params, KeyPair::generate()?)?;
        let key = KeyPair::generate()?;
        let leaf = CertificateParams::new(vec!["127.0.0.1".to_owned()])?.signed_by(&key, &ca)?;
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )?;
        let acceptor = TlsAcceptor::from(Arc::new(tls));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let issuer = format!("https://{address}");
        let served_issuer = issuer.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let Ok((stream, _)) = result else { break; };
                        let acceptor = acceptor.clone(); let issuer = served_issuer.clone();
                        connections.spawn(async move {
                            let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(5), acceptor.accept(stream)).await else { return; };
                            let mut bytes = [0; 8192];
                            let Ok(Ok(length)) = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut bytes)).await else { return; };
                            let request = String::from_utf8_lossy(&bytes[..length]);
                            let (status, extra, body) = if request.contains("/.well-known/openid-configuration ") {
                                ("200 OK", "", serde_json::json!({"issuer":issuer,"authorization_endpoint":format!("{issuer}/authorize"),"token_endpoint":format!("{issuer}/token"),"jwks_uri":format!("{issuer}/keys"),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["EdDSA"]}).to_string())
                            } else if request.contains("/redirect ") {
                                ("302 Found", "Location: https://127.0.0.1:1/forbidden\r\n", String::new())
                            } else { ("200 OK", "", "{\"keys\":[]}".to_owned()) };
                            let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}", body.len());
                            let _ = stream.write_all(response.as_bytes()).await;
                            let _ = stream.shutdown().await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        let random = getrandom::u64()
            .map_err(|_| std::io::Error::other("fixture randomness unavailable"))?;
        let directory = std::env::temp_dir().join(format!(
            "fleetiq-oidc-trust-{}-{random:x}",
            std::process::id()
        ));
        fs::create_dir(&directory)?;
        Ok(Self {
            issuer,
            pem: ca.pem(),
            task,
            directory,
        })
    }
    fn config(&self) -> BrowserIdentityConfig {
        BrowserIdentityConfig::from_values(
            "https://fleet.example",
            &self.issuer,
            "client",
            "private-client-value",
            "workforce",
        )
        .unwrap()
    }
    fn bundle(&self, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.directory.join("ca.pem");
        fs::write(&path, contents).unwrap();
        path
    }
    async fn close(mut self) {
        self.task.abort();
        // Normal teardown joins cancellation; Drop also aborts on assertion failure.
        let stopped = (&mut self.task).await;
        assert!(
            matches!(stopped, Err(error) if error.is_cancelled()),
            "TLS fixture exited unexpectedly"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
        // Only a freshly created, positively identified direct temp child is removed.
        assert_eq!(
            self.directory.parent(),
            Some(std::env::temp_dir().as_path())
        );
        assert!(
            self.directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("fleetiq-oidc-trust-")
        );
        fs::remove_dir_all(&self.directory).expect("remove owned public trust fixture");
    }
}

#[tokio::test]
async fn real_discovery_and_requests_use_additional_trust_but_reject_missing_trust_and_wrong_host()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::start().await?;
    assert!(OidcClient::discover(fixture.config()).await.is_err());
    let config = fixture
        .config()
        .with_additional_ca_bundle(fixture.bundle(&fixture.pem))?;
    let client = OidcClient::discover(config.clone()).await?;
    assert!(
        client
            .begin_login()?
            .authorization_url
            .starts_with(&fixture.issuer)
    );
    let token = openidconnect::http::Request::builder()
        .method("POST")
        .uri(format!("{}/token", fixture.issuer))
        .body(Vec::new())?;
    assert_eq!(bounded_request(&client.http, token).await?.status(), 200);
    let wrong = fixture.issuer.replace("127.0.0.1", "localhost");
    assert!(
        trust::http_client(&config)?
            .get(format!("{wrong}/keys"))
            .send()
            .await
            .is_err()
    );
    let redirect = openidconnect::http::Request::builder()
        .uri(format!("{}/redirect", fixture.issuer))
        .body(Vec::new())?;
    assert_eq!(bounded_request(&client.http, redirect).await?.status(), 302);
    let debug = format!("{config:?}");
    assert!(!debug.contains("private-client-value"));
    assert!(!debug.contains(&fixture.directory.to_string_lossy().to_string()));
    fixture.close().await;
    Ok(())
}

#[tokio::test]
async fn invalid_missing_empty_mixed_and_oversized_ca_files_fail_closed()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::start().await?;
    let absent = fixture
        .config()
        .with_additional_ca_bundle(fixture.directory.join("absent.pem"))?;
    assert!(trust::http_client(&absent).is_err());
    for text in [
        String::new(),
        "not a certificate".to_owned(),
        "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----".to_owned(),
        "-----BEGIN CERTIFICATE-----\nZm9v\n-----END CERTIFICATE-----".to_owned(),
        "-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----".to_owned(),
        format!("{}\ntrailing junk", fixture.pem),
        "x".repeat(256 * 1024 + 1),
        fixture.pem.repeat(33),
    ] {
        let config = fixture
            .config()
            .with_additional_ca_bundle(fixture.bundle(text))?;
        assert!(trust::http_client(&config).is_err());
    }
    assert!(
        fixture
            .config()
            .with_additional_ca_bundle(PathBuf::new())
            .is_err()
    );
    // LF and CRLF and multiple roots are accepted without replacing default trust.
    for text in [fixture.pem.replace('\n', "\r\n"), fixture.pem.repeat(2)] {
        let config = fixture
            .config()
            .with_additional_ca_bundle(fixture.bundle(text))?;
        assert!(OidcClient::discover(config).await.is_ok());
    }
    fixture.close().await;
    Ok(())
}
