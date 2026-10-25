use super::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolCompiler, ProtocolCompilerError,
};
use crate::{ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease};
use fleetiq_access::{AuthorizedTenant, ManageProtocols};
use fleetiq_domain::UnixTimeMillis;
use fleetiq_telemetry::{ProtocolArtifactDigest, ProtocolPackageRef};
use std::{error::Error, fmt};

/// Coordinates catalogue, immutable artifact storage and isolated compilation.
/// Catalogue writes may precede failed artifact storage; an exact retry repairs
/// that state. A catalogue-only release is never reported as stored or admitted.
pub struct ProtocolPackageService<C, S, V> {
    catalog: C,
    store: S,
    compiler: V,
}
impl<C: ProtocolPackageCatalog, S: ProtocolArtifactStore, V: ProtocolCompiler>
    ProtocolPackageService<C, S, V>
{
    /// Composes narrow ports without granting storage or compiler activation authority.
    pub const fn new(catalog: C, store: S, compiler: V) -> Self {
        Self {
            catalog,
            store,
            compiler,
        }
    }
    /// Registers and stores host-verified bytes. Exact retries repair interrupted storage.
    /// # Errors
    /// Rejects a digest mismatch before side effects or reports catalogue/storage failure.
    pub async fn register(
        &self,
        authorization: AuthorizedTenant<ManageProtocols>,
        package: ProtocolPackageRef,
        expected: ProtocolArtifactDigest,
        artifact: &ProtocolArtifact,
    ) -> Result<
        (ProtocolPackageRelease, ProtocolPackageRegistrationOutcome),
        ProtocolPackageServiceError<C::Error, S::Error>,
    > {
        if artifact.digest() != expected {
            return Err(ProtocolPackageServiceError::ContentMismatch);
        }
        let release = ProtocolPackageRelease::new(
            authorization.tenant_id().clone(),
            package,
            artifact.digest(),
            artifact.size_bytes(),
        );
        let outcome = self
            .catalog
            .register(&release)
            .await
            .map_err(ProtocolPackageServiceError::Catalog)?;
        self.store
            .put(&release, artifact)
            .await
            .map_err(ProtocolPackageServiceError::Store)?;
        Ok((release, outcome))
    }
    /// Finds only a fully stored release; catalogue registration alone is insufficient.
    /// # Errors
    /// Reports storage/reconstruction failure and rejects inconsistent content.
    pub async fn find(
        &self,
        authorization: AuthorizedTenant<ManageProtocols>,
        package: &ProtocolPackageRef,
    ) -> Result<Option<ProtocolPackageRelease>, ProtocolPackageServiceError<C::Error, S::Error>>
    {
        let Some(release) = self
            .catalog
            .find(authorization.tenant_id(), package)
            .await
            .map_err(ProtocolPackageServiceError::Catalog)?
        else {
            return Ok(None);
        };
        let Some(artifact) = self
            .store
            .find(&release)
            .await
            .map_err(ProtocolPackageServiceError::Store)?
        else {
            return Ok(None);
        };
        if !artifact.matches(&release) {
            return Err(ProtocolPackageServiceError::ContentMismatch);
        }
        Ok(Some(release))
    }
    /// Explicitly admits one stored release/world and preserves first-receipt evidence.
    /// # Errors
    /// Reports absence, corrupt bytes, compiler rejection/capacity or storage failure.
    pub async fn admit(
        &self,
        authorization: AuthorizedTenant<ManageProtocols>,
        package: &ProtocolPackageRef,
        world: ProtocolAdmissionWorld,
        now: UnixTimeMillis,
    ) -> Result<ProtocolAdmission, ProtocolPackageServiceError<C::Error, S::Error>> {
        let release = self
            .catalog
            .find(authorization.tenant_id(), package)
            .await
            .map_err(ProtocolPackageServiceError::Catalog)?
            .ok_or(ProtocolPackageServiceError::NotFound)?;
        let artifact = self
            .store
            .find(&release)
            .await
            .map_err(ProtocolPackageServiceError::Store)?
            .ok_or(ProtocolPackageServiceError::NotFound)?;
        if !artifact.matches(&release) {
            return Err(ProtocolPackageServiceError::ContentMismatch);
        }
        if let Some(receipt) = self
            .store
            .admission(&release, world)
            .await
            .map_err(ProtocolPackageServiceError::Store)?
        {
            return Ok(receipt);
        }
        self.compiler
            .validate(&artifact, world)
            .await
            .map_err(ProtocolPackageServiceError::Compiler)?;
        self.store
            .record(&ProtocolAdmission {
                release,
                world,
                policy_version: PROTOCOL_ADMISSION_POLICY,
                actor_id: authorization.actor_id().clone(),
                admitted_at: now,
            })
            .await
            .map_err(ProtocolPackageServiceError::Store)
    }
}
/// Redacted workflow outcomes. Error sources retain boundary-specific diagnostics.
#[derive(Debug)]
pub enum ProtocolPackageServiceError<C, S> {
    /// Requested release or its bytes do not exist in the authorized tenant.
    NotFound,
    /// Artifact digest/length does not match its declared immutable identity.
    ContentMismatch,
    /// Release catalogue operation failed, possibly after a durable commit.
    Catalog(C),
    /// Artifact or evidence persistence/reconstruction failed.
    Store(S),
    /// Isolated compilation did not succeed; no evidence was recorded.
    Compiler(ProtocolCompilerError),
}
impl<C, S> fmt::Display for ProtocolPackageServiceError<C, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("protocol package operation failed")
    }
}
impl<C: Error + 'static, S: Error + 'static> Error for ProtocolPackageServiceError<C, S> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Catalog(e) => Some(e),
            Self::Store(e) => Some(e),
            Self::Compiler(e) => Some(e),
            Self::NotFound | Self::ContentMismatch => None,
        }
    }
}
