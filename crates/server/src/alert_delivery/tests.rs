use super::*;
use axum::{
    Router,
    body::{Body, Bytes},
    http::{Response, StatusCode},
    routing::{get, post},
};
use fleetiq_analytics::{
    AlertComparison, AlertPhase, AlertResolution, AlertRuleDefinition, AlertRuleId, CalculatorId,
    CalculatorRef, CalculatorVersion,
};
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, TenantId, UnitId,
    UnixTimeMillis,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
fn lease(destination: [u8; 32]) -> AlertNotificationLease {
    AlertNotificationLease {
        destination,
        tenant: TenantId::new("tenant-a").unwrap(),
        rule: AlertRuleId::new("high").unwrap(),
        revision: 1,
        firing_version: 1,
        version: 1,
        generation: 1,
        phase: AlertPhase::Firing,
        starts_at: UnixTimeMillis::new(1000),
        ends_at: UnixTimeMillis::new(121_000),
        definition: AlertRuleDefinition {
            calculator: CalculatorRef::new(
                CalculatorId::new("mean").unwrap(),
                CalculatorVersion::new("1").unwrap(),
            ),
            scope: AssetId::new("scope").unwrap(),
            target: AssetId::new("target").unwrap(),
            output: PropertyDefinitionRef::new(
                PropertyDefinitionId::new("voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
            output_index: 0,
            comparison: AlertComparison::Greater,
            threshold: "50".parse().unwrap(),
            unit: Some(UnitId::new("volt").unwrap()),
            hold_ms: 0,
            resolution: AlertResolution::OnFalse,
            resolve_after_ms: 0,
        },
    }
}
#[test]
fn endpoint_validation_redacts_credentials_and_preserves_canonical_destination_identity() {
    for url in [
        "file:///api/v2/alerts",
        "http://user:password@localhost/api/v2/alerts",
        "https://example/api/v2/alerts?q=x",
        "https://example/api/v2/alerts#token",
        "https://example/other",
    ] {
        assert!(AlertmanagerConfig::new(url).is_err());
    }
    let a = AlertmanagerConfig::new("https://EXAMPLE:443/api/v2/alerts").unwrap();
    let b = AlertmanagerConfig::new("https://example/api/v2/alerts").unwrap();
    assert_eq!(a.destination, b.destination);
    assert!(!format!("{a:?}").contains("example"));
}
#[test]
fn active_resolved_refresh_and_new_incident_have_explicit_stable_wire_identity() {
    let a = lease([1; 32]);
    let first: serde_json::Value = serde_json::from_slice(&payload::encode(&a).unwrap()).unwrap();
    let mut resolved = a.clone();
    resolved.phase = AlertPhase::Resolved;
    resolved.version = 2;
    resolved.ends_at = UnixTimeMillis::new(2000);
    let r: serde_json::Value =
        serde_json::from_slice(&payload::encode(&resolved).unwrap()).unwrap();
    assert_eq!(first[0]["labels"], r[0]["labels"]);
    assert_eq!(first[0]["startsAt"], "1970-01-01T00:00:01Z");
    assert_eq!(r[0]["endsAt"], "1970-01-01T00:00:02Z");
    let mut next = a;
    next.firing_version = 3;
    next.version = 3;
    let n: serde_json::Value = serde_json::from_slice(&payload::encode(&next).unwrap()).unwrap();
    assert_ne!(first[0]["labels"], n[0]["labels"]);
    next.phase = AlertPhase::Inactive;
    assert_eq!(
        payload::encode(&next),
        Err(AlertDeliveryFailure::InvalidPayload)
    );
    next.phase = AlertPhase::Firing;
    next.starts_at = UnixTimeMillis::new(i64::MAX);
    next.ends_at = next.starts_at;
    assert!(payload::encode(&next).is_err());
}
#[tokio::test]
async fn actual_http_retry_ambiguous_acceptance_redirect_refusal_and_response_limit() {
    let requests = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let seen = requests.clone();
    let mode = Arc::new(AtomicUsize::new(0));
    let current = mode.clone();
    let redirected = Arc::new(AtomicUsize::new(0));
    let hit = redirected.clone();
    let app = Router::new()
        .route(
            "/api/v2/alerts",
            post(move |body: Bytes| {
                let seen = seen.clone();
                let mode = current.load(Ordering::SeqCst);
                async move {
                    seen.lock().unwrap().push(body.to_vec());
                    match mode {
                        0 => Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(Body::empty())
                            .unwrap(),
                        1 => Response::builder()
                            .status(StatusCode::FOUND)
                            .header("Location", "/redirected")
                            .body(Body::empty())
                            .unwrap(),
                        2 => Response::builder()
                            .body(Body::from(vec![b'x'; 8193]))
                            .unwrap(),
                        4 => Response::builder()
                            .body(Body::from_stream(futures_util::stream::iter([
                                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"accepted")),
                                Err(std::io::Error::other("synthetic truncated response")),
                            ])))
                            .unwrap(),
                        _ => Response::builder().body(Body::empty()).unwrap(),
                    }
                }
            }),
        )
        .route(
            "/redirected",
            get(move || {
                hit.fetch_add(1, Ordering::SeqCst);
                async { StatusCode::OK }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = AlertmanagerConfig::new(&format!("http://{address}/api/v2/alerts")).unwrap();
    let l = lease(config.destination);
    let client = AlertmanagerClient::new(config).unwrap();
    assert_eq!(client.send(&l).await, Err(AlertDeliveryFailure::Rejected));
    mode.store(3, Ordering::SeqCst);
    assert_eq!(client.send(&l).await, Ok(()));
    {
        let seen = requests.lock().unwrap();
        assert_eq!(seen[0], seen[1]);
    }
    mode.store(4, Ordering::SeqCst);
    assert_eq!(client.send(&l).await, Err(AlertDeliveryFailure::Transport));
    mode.store(3, Ordering::SeqCst);
    assert_eq!(client.send(&l).await, Ok(()));
    mode.store(1, Ordering::SeqCst);
    assert_eq!(client.send(&l).await, Err(AlertDeliveryFailure::Rejected));
    assert_eq!(redirected.load(Ordering::SeqCst), 0);
    mode.store(2, Ordering::SeqCst);
    assert_eq!(
        client.send(&l).await,
        Err(AlertDeliveryFailure::ResponseLimit)
    );
    task.abort();
}
#[tokio::test]
async fn stalled_receiver_hits_the_owned_transport_timeout() {
    let (entered, received) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered)));
    let app = Router::new().route(
        "/api/v2/alerts",
        post(move || {
            if let Some(tx) = entered.lock().unwrap().take() {
                let _ = tx.send(());
            }
            async { std::future::pending::<StatusCode>().await }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = AlertmanagerConfig::new(&format!("http://{address}/api/v2/alerts")).unwrap();
    let l = lease(config.destination);
    let client = AlertmanagerClient::new(config).unwrap();
    let send = tokio::spawn(async move { client.send(&l).await });
    tokio::time::timeout(Duration::from_secs(10), received)
        .await
        .unwrap()
        .unwrap();
    // Connect using real time before advancing the owned response timeout.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    assert_eq!(send.await.unwrap(), Err(AlertDeliveryFailure::Transport));
    server.abort();
}
