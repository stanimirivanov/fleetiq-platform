use super::*;
use crate::ProtocolArtifact;
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageVersion};
use std::{
    convert::Infallible,
    future::ready,
    sync::{Arc, Mutex},
};
#[derive(Clone, Default)]
struct Repository(Arc<Mutex<Option<ProtocolSessionRoute>>>);
impl ProtocolRoutingRepository for Repository {
    type Error = Infallible;
    fn find(
        &self,
        _: &TenantId,
        _: &AssetId,
        _: &ProtocolChannelId,
    ) -> impl Future<Output = Result<Option<ProtocolDeviceRoute>, Self::Error>> + Send {
        ready(Ok(None))
    }
    fn change(
        &self,
        _: &ProtocolRouteCommand,
    ) -> impl Future<Output = Result<ProtocolRouteWrite, Self::Error>> + Send {
        ready(Ok(ProtocolRouteWrite::Unavailable))
    }
    fn resolve(
        &self,
        _: &RegisteredIdentityChannel,
        _: &DeviceIdentityClaim,
    ) -> impl Future<Output = Result<Option<ProtocolSessionRoute>, Self::Error>> + Send {
        ready(Ok(self.0.lock().unwrap().clone()))
    }
}
fn snapshot() -> ProtocolSessionRoute {
    let tenant = TenantId::new("tenant-a").unwrap();
    let artifact = ProtocolArtifact::new(vec![1]).unwrap();
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("custom").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    );
    ProtocolSessionRoute {
        release: ProtocolPackageRelease::new(
            tenant.clone(),
            package.clone(),
            artifact.digest(),
            artifact.size_bytes(),
        ),
        route: ProtocolDeviceRoute {
            tenant_id: tenant,
            device_asset_id: AssetId::new("device-a").unwrap(),
            channel_id: ProtocolChannelId::new("tcp-a").unwrap(),
            revision: NonZeroU32::new(1).unwrap(),
            policy_version: Some(crate::PROTOCOL_ADMISSION_POLICY),
            selection: Some(ProtocolRouteSelection {
                package,
                world: ProtocolAdmissionWorld::Identity,
            }),
            actor_id: ActorId::new("operator").unwrap(),
            recorded_at: UnixTimeMillis::new(1),
        },
    }
}
#[test]
fn channel_names_are_bounded_host_identifiers() {
    assert!(ProtocolChannelId::new("tcp.test_1-A").is_ok());
    assert!(ProtocolChannelId::new("a".repeat(128)).is_ok());
    for value in [
        String::new(),
        " a".into(),
        "a/b".into(),
        "é".into(),
        "a".repeat(129),
    ] {
        assert!(ProtocolChannelId::new(value).is_err());
    }
}
#[tokio::test]
async fn handoff_retains_exact_snapshot_and_revalidates_every_generation() {
    let repository = Repository::default();
    let first = snapshot();
    *repository.0.lock().unwrap() = Some(first.clone());
    let service = ProtocolRoutingService::new(repository.clone());
    let channel = RegisteredIdentityChannel {
        tenant_id: first.route.tenant_id.clone(),
        channel_id: first.route.channel_id.clone(),
        authority: None,
        selection: first.route.selection.clone().unwrap(),
    };
    let claim = DeviceIdentityClaim::new("serial".into(), "Private-Claim".into()).unwrap();
    let session = service.resolve(channel, claim).await.unwrap().unwrap();
    assert_eq!(session.snapshot(), &first);
    assert!(!format!("{session:?}").contains("Private-Claim"));
    assert!(service.revalidate(&session).await.unwrap());
    let mut next = first.clone();
    next.route.revision = NonZeroU32::new(2).unwrap();
    *repository.0.lock().unwrap() = Some(next);
    assert!(!service.revalidate(&session).await.unwrap());
    *repository.0.lock().unwrap() = None;
    assert!(!service.revalidate(&session).await.unwrap());
}
#[tokio::test]
async fn missing_or_rejected_route_never_creates_a_handoff() {
    let first = snapshot();
    let channel = RegisteredIdentityChannel {
        tenant_id: first.route.tenant_id.clone(),
        channel_id: first.route.channel_id.clone(),
        authority: None,
        selection: first.route.selection.unwrap(),
    };
    assert!(
        ProtocolRoutingService::new(Repository::default())
            .resolve(
                channel,
                DeviceIdentityClaim::new("serial".into(), "unknown".into()).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_port_cannot_substitute_another_tenant_into_the_host_handoff() {
    let repository = Repository::default();
    let first = snapshot();
    let channel = RegisteredIdentityChannel {
        tenant_id: first.route.tenant_id.clone(),
        channel_id: first.route.channel_id.clone(),
        authority: None,
        selection: first.route.selection.clone().unwrap(),
    };
    let mut foreign = first;
    foreign.route.tenant_id = TenantId::new("tenant-b").unwrap();
    *repository.0.lock().unwrap() = Some(foreign);
    assert!(
        ProtocolRoutingService::new(repository)
            .resolve(
                channel,
                DeviceIdentityClaim::new("serial".into(), "test".into()).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
}
