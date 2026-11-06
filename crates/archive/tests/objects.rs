//! Archive object scope/configuration and real provider qualification.
use fleetiq_archive::objects::{ArchiveObjectRef, S3ArchiveConfig};
use fleetiq_domain::TenantId;
#[test]
fn object_scope_and_operator_configuration_are_closed() {
    let tenant = TenantId::new("tenant-a").unwrap();
    assert!(ArchiveObjectRef::new(tenant.clone(), [1; 32], 0).is_err());
    assert!(ArchiveObjectRef::new(tenant, [1; 32], u64::MAX).is_err());
    for endpoint in [
        "http://s3.example.test",
        "https://user:secret@s3.example.test",
        "https://s3.example.test/?secret=x",
    ] {
        assert!(
            S3ArchiveConfig::new(
                endpoint,
                "archive-test",
                "us-east-1",
                "fixture-access".into(),
                "fixture-secret".into()
            )
            .is_err()
        );
    }
    let config = S3ArchiveConfig::new(
        "https://s3.example.test",
        "archive-test",
        "us-east-1",
        "fixture-access".into(),
        "fixture-secret".into(),
    )
    .unwrap();
    assert!(!format!("{config:?}").contains("fixture-secret"));
}
#[path = "support/evidence.rs"]
mod evidence;
#[path = "support/provider.rs"]
mod provider;
#[tokio::test]
async fn real_s3_objects_are_immutable_verified_and_retryable_after_interruption() {
    use fleetiq_archive::{
        encode,
        objects::{ArchiveObjectError, ImmutableArchiveObjects},
    };
    use fleetiq_domain::PropertyValue;
    let fixture = provider::Provider::start().await;
    let store = ImmutableArchiveObjects::connect(fixture.config()).unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    let segment = encode(
        &tenant,
        &[evidence::evidence(0, PropertyValue::Integer(42))],
    )
    .unwrap();
    let (a, b) = tokio::join!(store.put(&tenant, &segment), store.put(&tenant, &segment));
    assert!(
        a.is_ok() && b.is_ok(),
        "real conditional create must converge"
    );
    let reference = a.unwrap();
    assert_eq!(reference, b.unwrap());
    assert_eq!(store.get(&tenant, &reference).await.unwrap(), segment);
    assert_eq!(
        store
            .get(&TenantId::new("tenant-b").unwrap(), &reference)
            .await,
        Err(ArchiveObjectError::Scope)
    );
    let missing = ArchiveObjectRef::new(tenant.clone(), [3; 32], 10).unwrap();
    assert_eq!(
        store.get(&tenant, &missing).await,
        Err(ArchiveObjectError::Missing)
    );
    let wrong_length = ArchiveObjectRef::new(
        tenant.clone(),
        reference.sha256(),
        reference.byte_length() + 1,
    )
    .unwrap();
    assert_eq!(
        store.get(&tenant, &wrong_length).await,
        Err(ArchiveObjectError::Integrity)
    );
    let second = encode(
        &tenant,
        &[evidence::evidence(1, PropertyValue::Integer(99))],
    )
    .unwrap();
    fixture.pause();
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        store.put(&tenant, &second),
    )
    .await;
    fixture.resume();
    assert!(
        cancelled.is_err(),
        "paused provider cannot acknowledge an upload"
    );
    let retried = store.put(&tenant, &second).await;
    assert!(
        retried.is_ok(),
        "exact retry after interrupted upload must verify"
    );
    verify_corruption(&fixture, &store, &tenant, &reference, &segment).await;
    fixture.close();
}
async fn verify_corruption(
    fixture: &provider::Provider,
    store: &fleetiq_archive::objects::ImmutableArchiveObjects,
    tenant: &TenantId,
    reference: &ArchiveObjectRef,
    segment: &[u8],
) {
    use fleetiq_archive::{MAX_FILE_BYTES, objects::ArchiveObjectError};
    let mut corrupt = segment.to_vec();
    corrupt[0] ^= 1;
    assert!(
        fixture
            .raw(reqwest::Method::PUT, reference.key(), corrupt.clone())
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        store.get(tenant, reference).await,
        Err(ArchiveObjectError::Integrity)
    );
    assert_eq!(
        store.put(tenant, segment).await,
        Err(ArchiveObjectError::Conflict)
    );
    assert_eq!(
        fixture
            .raw(reqwest::Method::GET, reference.key(), Vec::new())
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        corrupt
    );
    assert!(
        fixture
            .raw(
                reqwest::Method::PUT,
                reference.key(),
                vec![0; MAX_FILE_BYTES + 1]
            )
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        store.get(tenant, reference).await,
        Err(ArchiveObjectError::Integrity)
    );
}
#[tokio::test]
async fn invalid_segments_fail_before_network_access() {
    use fleetiq_archive::objects::{ArchiveObjectError, ImmutableArchiveObjects};
    let config = S3ArchiveConfig::new(
        "http://127.0.0.1:9",
        "archive-test",
        "us-east-1",
        "fixture-access".into(),
        "fixture-secret".into(),
    )
    .unwrap();
    let store = ImmutableArchiveObjects::connect(config).unwrap();
    let tenant = TenantId::new("tenant-a").unwrap();
    assert_eq!(
        store.put(&tenant, &[]).await,
        Err(ArchiveObjectError::Limit)
    );
    assert_eq!(
        store.put(&tenant, b"not a segment").await,
        Err(ArchiveObjectError::Segment)
    );
}
