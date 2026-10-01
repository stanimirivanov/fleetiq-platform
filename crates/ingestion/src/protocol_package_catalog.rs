use std::{error::Error, future::Future, num::NonZeroU64};

use fleetiq_domain::TenantId;
use fleetiq_telemetry::{ProtocolArtifactDigest, ProtocolPackageRef};

/// Immutable tenant-owned catalogue identity for one protocol artifact release.
///
/// A release key is `(tenant, package ID, package version)`. Its artifact digest
/// and byte length cannot change under that key. This record describes content;
/// it is not proof of runtime admission or permission to activate the package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageRelease {
    tenant_id: TenantId,
    package: ProtocolPackageRef,
    artifact_digest: ProtocolArtifactDigest,
    artifact_size_bytes: NonZeroU64,
}

impl ProtocolPackageRelease {
    /// Creates immutable catalogue metadata using already validated identities.
    ///
    /// The host must derive the digest and length from the same exact artifact.
    /// Nonzero length excludes empty uploads. Runtime admission must separately
    /// validate component compatibility and configured resource policy.
    pub const fn new(
        tenant_id: TenantId,
        package: ProtocolPackageRef,
        artifact_digest: ProtocolArtifactDigest,
        artifact_size_bytes: NonZeroU64,
    ) -> Self {
        Self {
            tenant_id,
            package,
            artifact_digest,
            artifact_size_bytes,
        }
    }

    /// Returns the tenant that owns this catalogue entry.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the immutable package ID and version within that tenant.
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    /// Returns the SHA-256 identity of the exact artifact bytes.
    pub const fn artifact_digest(&self) -> ProtocolArtifactDigest {
        self.artifact_digest
    }

    /// Returns the nonzero raw artifact length, independent of compiled size.
    pub const fn artifact_size_bytes(&self) -> NonZeroU64 {
        self.artifact_size_bytes
    }
}

/// Durable disposition of a protocol package release registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolPackageRegistrationOutcome {
    /// The immutable catalogue record was registered by this call.
    Registered,
    /// The complete identical record was already registered.
    AlreadyPresent,
}

/// Tenant-scoped append-only catalogue for immutable protocol package releases.
///
/// Exact retries must compare both artifact digest and byte length. Reusing a
/// release key for different content is a collision, never an update. Errors
/// may be ambiguous about commit; callers retain and retry the borrowed record.
/// Registration does not authorize activation, admit code, or store artifact
/// bytes. Implementations must not perform those side effects implicitly.
pub trait ProtocolPackageCatalog: Send + Sync {
    /// Storage, reconstruction, or immutable-release identity-collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Durably registers a release or resolves an exact registration retry.
    ///
    /// # Errors
    ///
    /// Returns the implementation error if durability cannot be confirmed or
    /// the release identity already names a different digest or byte length.
    fn register<'a>(
        &'a self,
        release: &'a ProtocolPackageRelease,
    ) -> impl Future<Output = Result<ProtocolPackageRegistrationOutcome, Self::Error>> + Send + 'a;

    /// Finds an immutable release within an explicit tenant boundary.
    ///
    /// `None` means the exact release is absent in this tenant. An identical
    /// package ID and version in another tenant must never satisfy this lookup.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when storage or reconstruction fails.
    fn find<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        package: &'a ProtocolPackageRef,
    ) -> impl Future<Output = Result<Option<ProtocolPackageRelease>, Self::Error>> + Send + 'a;
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageVersion};

    #[test]
    fn release_preserves_tenant_identity_and_exact_artifact_metadata() {
        let release = release("tenant-a", [1; 32], 500);
        assert_eq!(release.tenant_id().as_str(), "tenant-a");
        assert_eq!(release.package().id().as_str(), "custom.binary");
        assert_eq!(release.package().version().as_str(), "2026.09.1");
        assert_eq!(release.artifact_digest().as_bytes(), &[1; 32]);
        assert_eq!(release.artifact_size_bytes().get(), 500);
    }

    #[test]
    fn equality_distinguishes_tenant_and_every_immutable_artifact_field() {
        let original = release("tenant-a", [1; 32], 500);
        assert_eq!(original, original.clone());
        assert_ne!(original, release("tenant-b", [1; 32], 500));
        assert_ne!(original, release("tenant-a", [2; 32], 500));
        assert_ne!(original, release("tenant-a", [1; 32], 501));
    }

    fn release(tenant: &str, digest: [u8; 32], size: u64) -> ProtocolPackageRelease {
        ProtocolPackageRelease::new(
            TenantId::new(tenant).unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("custom.binary").unwrap(),
                ProtocolPackageVersion::new("2026.09.1").unwrap(),
            ),
            ProtocolArtifactDigest::from_sha256(digest),
            NonZeroU64::new(size).unwrap(),
        )
    }
}
