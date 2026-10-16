use super::*;
use crate::{AccessDecision, TenantAccessPolicy, TenantAccessRequest, TenantAuthorizationService};
use fleetiq_domain::{
    AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, DeviceEndpoint, DeviceEndpointId,
    DeviceEndpointKindId, DeviceEndpointName, ExternalIdentifier, ExternalIdentifierKind,
    ExternalIdentifierValue,
};
use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
};

use std::future::{Future, ready};

type Call = (MetadataRegistration, ActorId, UnixTimeMillis);
#[derive(Clone, Default)]
struct RecordingRepository {
    calls: Arc<Mutex<Vec<Call>>>,
    fail: bool,
}
impl RegistrationRepository for RecordingRepository {
    type Error = std::io::Error;
    fn register(
        &self,
        value: &MetadataRegistration,
        actor: &ActorId,
        time: UnixTimeMillis,
    ) -> impl Future<Output = Result<RegistrationOutcome, Self::Error>> + Send {
        self.calls
            .lock()
            .unwrap()
            .push((value.clone(), actor.clone(), time));
        if self.fail {
            return ready(Err(std::io::Error::other("storage failed")));
        }
        ready(Ok(RegistrationOutcome::Registered(
            RegistrationReceipt::new(actor.clone(), time),
        )))
    }
}
struct Allow;
impl TenantAccessPolicy for Allow {
    type Error = Infallible;
    fn evaluate(
        &self,
        _: &TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Infallible>> + Send {
        ready(Ok(AccessDecision::Allow))
    }
}
async fn grant() -> AuthorizedTenant<ManageMetadata> {
    TenantAuthorizationService::new(Allow)
        .authorize::<ManageMetadata>(
            crate::test_support::authenticated_actor(ActorId::new("operator-a").unwrap()).await,
            TenantId::new("tenant-a").unwrap(),
        )
        .await
        .unwrap()
}
fn asset(tenant: &str) -> Asset {
    Asset::new(
        TenantId::new(tenant).unwrap(),
        AssetId::new("asset-a").unwrap(),
        AssetTypeRef::new(
            AssetTypeId::new("equipment").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        AssetName::new("Machine").unwrap(),
    )
}
#[tokio::test]
async fn consumes_exact_tenant_grant_and_derives_actor_attribution() {
    for registration in [
        MetadataRegistration::Asset(asset("tenant-a")),
        MetadataRegistration::Device(CommunicationDevice::new(
            TenantId::new("tenant-a").unwrap(),
            AssetId::new("asset-a").unwrap(),
        )),
    ] {
        let repository = RecordingRepository::default();
        let time = UnixTimeMillis::new(123);
        let result = RegistrationService::new(repository.clone())
            .register(grant().await, &registration, time)
            .await
            .unwrap();
        let receipt = RegistrationReceipt::new(ActorId::new("operator-a").unwrap(), time);
        assert_eq!(result, RegistrationOutcome::Registered(receipt));
        assert_eq!(
            *repository.calls.lock().unwrap(),
            vec![(registration, ActorId::new("operator-a").unwrap(), time)]
        );
    }
}
#[tokio::test]
async fn rejects_foreign_assets_and_devices_before_persistence() {
    let repository = RecordingRepository::default();
    let service = RegistrationService::new(repository.clone());
    for registration in [
        MetadataRegistration::Asset(asset("tenant-b")),
        MetadataRegistration::Device(CommunicationDevice::new(
            TenantId::new("tenant-b").unwrap(),
            AssetId::new("asset-a").unwrap(),
        )),
    ] {
        assert!(matches!(
            service
                .register(grant().await, &registration, UnixTimeMillis::new(0))
                .await,
            Err(RegistrationError::TenantMismatch)
        ));
    }
    assert!(repository.calls.lock().unwrap().is_empty());
}
#[tokio::test]
async fn preserves_unresolved_storage_failure_and_its_source() {
    let service = RegistrationService::new(RecordingRepository {
        fail: true,
        ..RecordingRepository::default()
    });
    let error = service
        .register(
            grant().await,
            &MetadataRegistration::Asset(asset("tenant-a")),
            UnixTimeMillis::new(0),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, RegistrationError::Repository(_)));
    assert!(error.source().is_some());
}
#[test]
fn identifier_order_is_not_semantic_but_endpoint_order_is() {
    let mut left = asset("tenant-a");
    let ids: Vec<_> = ["a", "b"]
        .map(|value| {
            ExternalIdentifier::new(
                ExternalIdentifierKind::new("serial").unwrap(),
                None,
                ExternalIdentifierValue::new(value).unwrap(),
            )
        })
        .into();
    for id in &ids {
        left.add_external_identifier(id.clone()).unwrap();
    }
    let mut right = asset("tenant-a");
    for id in ids.into_iter().rev() {
        right.add_external_identifier(id).unwrap();
    }
    assert!(MetadataRegistration::Asset(left).same_content(&MetadataRegistration::Asset(right)));
    let mut left = CommunicationDevice::new(
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("asset-a").unwrap(),
    );
    let mut right = left.clone();
    let endpoints = ["one", "two"].map(|id| {
        DeviceEndpoint::new(
            DeviceEndpointId::new(id).unwrap(),
            DeviceEndpointKindId::new("bus.can").unwrap(),
            DeviceEndpointName::new(id).unwrap(),
        )
    });
    for endpoint in &endpoints {
        left.add_endpoint(endpoint.clone()).unwrap();
    }
    for endpoint in endpoints.into_iter().rev() {
        right.add_endpoint(endpoint).unwrap();
    }
    assert!(!MetadataRegistration::Device(left).same_content(&MetadataRegistration::Device(right)));
}
