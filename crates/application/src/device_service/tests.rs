use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{
    ActorId, AssetId, CommunicationDevice, DeviceEndpoint, DeviceEndpointId, DeviceEndpointKindId,
    DeviceEndpointName, TenantId,
};

use crate::{
    AccessDecision, AuthorizedTenant, DeviceRepository, ManageMetadata, ReadMetadata,
    TenantAccessPolicy, TenantAccessRequest, TenantAuthorizationService,
};

use super::{DeviceApplicationError, DeviceApplicationService};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RepositoryCall {
    Insert(CommunicationDevice),
    FindByAsset(TenantId, AssetId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestRepositoryError;

impl fmt::Display for TestRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test repository failed")
    }
}

impl Error for TestRepositoryError {}

#[derive(Clone, Debug, Default)]
struct RecordingDeviceRepository {
    calls: Arc<Mutex<Vec<RepositoryCall>>>,
    fail: bool,
    found: Option<CommunicationDevice>,
}

impl RecordingDeviceRepository {
    fn returning(device: CommunicationDevice) -> Self {
        Self {
            found: Some(device),
            ..Self::default()
        }
    }

    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<RepositoryCall> {
        self.calls.lock().expect("calls lock poisoned").clone()
    }

    fn record(&self, call: RepositoryCall) {
        self.calls.lock().expect("calls lock poisoned").push(call);
    }
}

impl DeviceRepository for RecordingDeviceRepository {
    type Error = TestRepositoryError;

    fn insert<'a>(
        &'a self,
        device: &'a CommunicationDevice,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.record(RepositoryCall::Insert(device.clone()));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(())
        })
    }

    fn find_by_asset<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<CommunicationDevice>, Self::Error>> + Send + 'a {
        self.record(RepositoryCall::FindByAsset(
            tenant_id.clone(),
            asset_id.clone(),
        ));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(self.found.clone())
        })
    }
}

struct AllowAllPolicy;

impl TenantAccessPolicy for AllowAllPolicy {
    type Error = Infallible;

    fn evaluate<'a>(
        &'a self,
        _request: &'a TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
        ready(Ok(AccessDecision::Allow))
    }
}

#[tokio::test]
async fn registers_device_owned_by_the_authorized_tenant() {
    let repository = RecordingDeviceRepository::default();
    let service = DeviceApplicationService::new(repository.clone());
    let device = device("tenant-a");

    service
        .register(manage_authorization("tenant-a").await, &device)
        .await
        .expect("matching tenant device should be registered");

    assert_eq!(repository.calls(), vec![RepositoryCall::Insert(device)]);
}

#[tokio::test]
async fn rejects_wrong_tenant_before_registration() {
    let repository = RecordingDeviceRepository::default();
    let service = DeviceApplicationService::new(repository.clone());
    let device = device("tenant-b");

    let wrong_tenant = service
        .register(manage_authorization("tenant-a").await, &device)
        .await
        .expect_err("cross-tenant device must be rejected");
    assert!(matches!(
        wrong_tenant,
        DeviceApplicationError::TenantMismatch {
            authorized_tenant_id,
            device_tenant_id,
        } if authorized_tenant_id.as_str() == "tenant-a"
            && device_tenant_id.as_str() == "tenant-b"
    ));
    assert!(repository.calls().is_empty());
}

#[tokio::test]
async fn lookup_derives_tenant_scope_from_read_authorization() {
    let device = device("tenant-a");
    let repository = RecordingDeviceRepository::returning(device.clone());
    let service = DeviceApplicationService::new(repository.clone());
    let asset_id = device.asset_id().clone();

    let found = service
        .find_by_asset(read_authorization("tenant-a").await, &asset_id)
        .await
        .expect("device lookup should succeed");

    assert_eq!(found, Some(device));
    assert_eq!(
        repository.calls(),
        vec![RepositoryCall::FindByAsset(tenant_id("tenant-a"), asset_id,)]
    );
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let service = DeviceApplicationService::new(RecordingDeviceRepository::failing());

    let error = service
        .find_by_asset(
            read_authorization("tenant-a").await,
            &AssetId::new("gateway-1").expect("valid asset ID"),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        DeviceApplicationError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

async fn manage_authorization(tenant: &str) -> AuthorizedTenant<ManageMetadata> {
    authorization::<ManageMetadata>(tenant).await
}

async fn read_authorization(tenant: &str) -> AuthorizedTenant<ReadMetadata> {
    authorization::<ReadMetadata>(tenant).await
}

async fn authorization<A: crate::TenantActionMarker>(tenant: &str) -> AuthorizedTenant<A> {
    TenantAuthorizationService::new(AllowAllPolicy)
        .authorize::<A>(
            crate::test_support::authenticated_actor(
                ActorId::new("actor-a").expect("valid actor ID"),
            )
            .await,
            tenant_id(tenant),
        )
        .await
        .expect("test policy should authorize request")
}

fn device(tenant: &str) -> CommunicationDevice {
    let mut device = CommunicationDevice::new(
        tenant_id(tenant),
        AssetId::new("gateway-1").expect("valid asset ID"),
    );
    device
        .add_endpoint(DeviceEndpoint::new(
            DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
            DeviceEndpointKindId::new("bus.can").expect("valid endpoint kind"),
            DeviceEndpointName::new("Primary CAN interface").expect("valid endpoint name"),
        ))
        .expect("unique endpoint should be accepted");
    device
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("valid tenant ID")
}
