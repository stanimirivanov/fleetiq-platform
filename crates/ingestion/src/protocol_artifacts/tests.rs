use super::*;
#[path = "test_support.rs"]
mod authentication;
use crate::{ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease};
use fleetiq_access::{
    AccessDecision, AuthorizedTenant, ManageProtocols, TenantAccessPolicy, TenantAccessRequest,
    TenantAuthorizationService,
};
use fleetiq_domain::{ActorId, TenantId, UnixTimeMillis};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use std::{
    convert::Infallible,
    future::ready,
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Clone, Default)]
struct Memory {
    release: Arc<Mutex<Option<ProtocolPackageRelease>>>,
    bytes: Arc<Mutex<Option<ProtocolArtifact>>>,
    receipt: Arc<Mutex<Option<ProtocolAdmission>>>,
    calls: Arc<AtomicUsize>,
    fail_store: bool,
}
impl ProtocolPackageCatalog for Memory {
    type Error = std::io::Error;
    fn register(
        &self,
        release: &ProtocolPackageRelease,
    ) -> impl Future<Output = Result<ProtocolPackageRegistrationOutcome, Self::Error>> + Send {
        let mut existing = self.release.lock().unwrap();
        let outcome = match existing.as_ref() {
            Some(old) if old == release => Ok(ProtocolPackageRegistrationOutcome::AlreadyPresent),
            Some(_) => Err(std::io::Error::other("collision")),
            None => {
                *existing = Some(release.clone());
                Ok(ProtocolPackageRegistrationOutcome::Registered)
            }
        };
        ready(outcome)
    }
    fn find(
        &self,
        tenant: &TenantId,
        package: &ProtocolPackageRef,
    ) -> impl Future<Output = Result<Option<ProtocolPackageRelease>, Self::Error>> + Send {
        ready(Ok(self.release.lock().unwrap().clone().filter(|r| {
            r.tenant_id() == tenant && r.package() == package
        })))
    }
}
impl ProtocolArtifactStore for Memory {
    type Error = std::io::Error;
    fn put(
        &self,
        _: &ProtocolPackageRelease,
        artifact: &ProtocolArtifact,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        if self.fail_store {
            return ready(Err(std::io::Error::other("interrupted")));
        }
        *self.bytes.lock().unwrap() = Some(artifact.clone());
        ready(Ok(()))
    }
    fn find(
        &self,
        _: &ProtocolPackageRelease,
    ) -> impl Future<Output = Result<Option<ProtocolArtifact>, Self::Error>> + Send {
        ready(Ok(self.bytes.lock().unwrap().clone()))
    }
    fn admission(
        &self,
        _: &ProtocolPackageRelease,
        world: ProtocolAdmissionWorld,
    ) -> impl Future<Output = Result<Option<ProtocolAdmission>, Self::Error>> + Send {
        ready(Ok(self
            .receipt
            .lock()
            .unwrap()
            .clone()
            .filter(|r| r.world == world)))
    }
    fn record(
        &self,
        r: &ProtocolAdmission,
    ) -> impl Future<Output = Result<ProtocolAdmission, Self::Error>> + Send {
        let mut value = self.receipt.lock().unwrap();
        ready(Ok(value.get_or_insert_with(|| r.clone()).clone()))
    }
}
struct Compiler(Memory, Option<ProtocolCompilerError>);
impl ProtocolCompiler for Compiler {
    fn validate(
        &self,
        _: &ProtocolArtifact,
        _: ProtocolAdmissionWorld,
    ) -> impl Future<Output = Result<(), ProtocolCompilerError>> + Send {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        ready(self.1.map_or(Ok(()), Err))
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
async fn grant(tenant: &str) -> AuthorizedTenant<ManageProtocols> {
    TenantAuthorizationService::new(Allow)
        .authorize::<ManageProtocols>(
            authentication::authenticated_actor(ActorId::new("operator-a").unwrap()).await,
            TenantId::new(tenant).unwrap(),
        )
        .await
        .unwrap()
}
fn package() -> ProtocolPackageRef {
    ProtocolPackageRef::new(
        ProtocolPackageId::new("custom.binary").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    )
}
fn artifact() -> ProtocolArtifact {
    ProtocolArtifact::new(b"opaque-fixture-private-bytes".to_vec()).unwrap()
}
fn service(
    memory: &Memory,
    error: Option<ProtocolCompilerError>,
) -> ProtocolPackageService<Memory, Memory, Compiler> {
    ProtocolPackageService::new(
        memory.clone(),
        memory.clone(),
        Compiler(memory.clone(), error),
    )
}
#[test]
fn raw_bounds_hash_and_redaction_are_enforced() {
    assert!(ProtocolArtifact::new(vec![]).is_err());
    assert!(ProtocolArtifact::new(vec![0; MAX_PROTOCOL_ARTIFACT_BYTES + 1]).is_err());
    let a = artifact();
    assert!(!format!("{a:?}").contains("opaque-fixture"));
    assert_eq!(a.digest(), artifact().digest());
    let release = ProtocolPackageRelease::new(
        TenantId::new("tenant-a").unwrap(),
        package(),
        a.digest(),
        NonZeroU64::new(1).unwrap(),
    );
    assert!(!a.matches(&release));
}
#[tokio::test]
async fn mismatch_has_no_catalogue_or_storage_side_effect() {
    let m = Memory::default();
    assert!(matches!(
        service(&m, None)
            .register(
                grant("tenant-a").await,
                package(),
                ProtocolArtifactDigest::from_sha256([0; 32]),
                &artifact()
            )
            .await,
        Err(ProtocolPackageServiceError::ContentMismatch)
    ));
    assert!(m.release.lock().unwrap().is_none());
    assert!(m.bytes.lock().unwrap().is_none());
}
#[tokio::test]
async fn interrupted_storage_is_repaired_by_exact_retry_without_implicit_admission() {
    let mut m = Memory {
        fail_store: true,
        ..Memory::default()
    };
    let a = artifact();
    assert!(
        service(&m, None)
            .register(grant("tenant-a").await, package(), a.digest(), &a)
            .await
            .is_err()
    );
    assert!(
        service(&m, None)
            .find(grant("tenant-a").await, &package())
            .await
            .unwrap()
            .is_none()
    );
    m.fail_store = false;
    let (_, outcome) = service(&m, None)
        .register(grant("tenant-a").await, package(), a.digest(), &a)
        .await
        .unwrap();
    assert_eq!(outcome, ProtocolPackageRegistrationOutcome::AlreadyPresent);
    assert_eq!(m.calls.load(Ordering::SeqCst), 0);
    assert!(m.receipt.lock().unwrap().is_none());
}
#[tokio::test]
async fn admission_is_exact_and_retries_preserve_first_time_without_recompiling() {
    let m = Memory::default();
    let s = service(&m, None);
    let a = artifact();
    s.register(grant("tenant-a").await, package(), a.digest(), &a)
        .await
        .unwrap();
    let receipt = s
        .admit(
            grant("tenant-a").await,
            &package(),
            ProtocolAdmissionWorld::Runtime,
            UnixTimeMillis::new(10),
        )
        .await
        .unwrap();
    assert_eq!(
        receipt,
        s.admit(
            grant("tenant-a").await,
            &package(),
            ProtocolAdmissionWorld::Runtime,
            UnixTimeMillis::new(99)
        )
        .await
        .unwrap()
    );
    assert_eq!(m.calls.load(Ordering::SeqCst), 1);
    assert!(
        s.find(grant("tenant-b").await, &package())
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        s.admit(
            grant("tenant-b").await,
            &package(),
            ProtocolAdmissionWorld::Runtime,
            UnixTimeMillis::new(99)
        )
        .await,
        Err(ProtocolPackageServiceError::NotFound)
    ));
}
#[tokio::test]
async fn all_compiler_failures_leave_no_successful_evidence() {
    for e in [
        ProtocolCompilerError::Rejected,
        ProtocolCompilerError::Unavailable,
        ProtocolCompilerError::Busy,
        ProtocolCompilerError::Timeout,
    ] {
        let m = Memory::default();
        let s = service(&m, Some(e));
        let a = artifact();
        s.register(grant("tenant-a").await, package(), a.digest(), &a)
            .await
            .unwrap();
        assert!(
            matches!(s.admit(grant("tenant-a").await,&package(),ProtocolAdmissionWorld::Identity,UnixTimeMillis::new(1)).await,Err(ProtocolPackageServiceError::Compiler(actual)) if actual==e)
        );
        assert!(m.receipt.lock().unwrap().is_none());
    }
}

#[tokio::test]
async fn corrupted_stored_bytes_never_reach_the_compiler() {
    let m = Memory::default();
    let s = service(&m, None);
    let a = artifact();
    s.register(grant("tenant-a").await, package(), a.digest(), &a)
        .await
        .unwrap();
    *m.bytes.lock().unwrap() = Some(ProtocolArtifact::new(vec![9]).unwrap());
    assert!(matches!(
        s.admit(
            grant("tenant-a").await,
            &package(),
            ProtocolAdmissionWorld::Runtime,
            UnixTimeMillis::new(1)
        )
        .await,
        Err(ProtocolPackageServiceError::ContentMismatch)
    ));
    assert_eq!(m.calls.load(Ordering::SeqCst), 0);
}
