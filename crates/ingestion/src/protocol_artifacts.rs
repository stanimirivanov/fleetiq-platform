//! Artifact registration and admission are separate from activation authority.
mod service;
#[cfg(test)]
mod tests;
use crate::ProtocolPackageRelease;
use fleetiq_domain::{ActorId, UnixTimeMillis};
use fleetiq_telemetry::ProtocolArtifactDigest;
pub use service::{ProtocolPackageService, ProtocolPackageServiceError};
use sha2::{Digest, Sha256};
use std::{error::Error, fmt, future::Future, num::NonZeroU64};

/// Raw-byte ceiling, shared with the persistence constraint and worker protocol.
pub const MAX_PROTOCOL_ARTIFACT_BYTES: usize = 8 * 1024 * 1024;
/// Version of the complete admission policy; changes require fresh admission.
pub const PROTOCOL_ADMISSION_POLICY: i32 = 1;

/// Host-hashed nonempty, bounded bytes; Debug never prints untrusted code.
#[derive(Clone, Eq, PartialEq)]
pub struct ProtocolArtifact {
    bytes: Vec<u8>,
    digest: ProtocolArtifactDigest,
    size: NonZeroU64,
}
impl ProtocolArtifact {
    /// Validates the raw-byte ceiling before hashing. This does not compile code.
    /// # Errors
    /// Rejects empty and oversized content before creating a trusted identity.
    pub fn new(bytes: Vec<u8>) -> Result<Self, ProtocolArtifactError> {
        if bytes.is_empty() || bytes.len() > MAX_PROTOCOL_ARTIFACT_BYTES {
            return Err(ProtocolArtifactError);
        }
        let digest = ProtocolArtifactDigest::from_sha256(Sha256::digest(&bytes).into());
        let size = NonZeroU64::new(bytes.len() as u64).ok_or(ProtocolArtifactError)?;
        Ok(Self {
            bytes,
            digest,
            size,
        })
    }
    /// Exact bytes that the host hashed; callers cannot mutate their identity.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// SHA-256 computed by the host, never accepted on trust from a manifest.
    pub const fn digest(&self) -> ProtocolArtifactDigest {
        self.digest
    }
    /// Returns the validated nonzero raw byte length.
    pub const fn size_bytes(&self) -> NonZeroU64 {
        self.size
    }
    /// Checks the complete release identity before storage or compilation.
    pub fn matches(&self, release: &ProtocolPackageRelease) -> bool {
        self.digest == release.artifact_digest() && self.size == release.artifact_size_bytes()
    }
}
impl fmt::Debug for ProtocolArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProtocolArtifact")
            .field("digest", &self.digest)
            .field("size", &self.bytes.len())
            .finish()
    }
}
/// Raw artifact failed the nonempty byte bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolArtifactError;
impl fmt::Display for ProtocolArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid protocol artifact size")
    }
}
impl Error for ProtocolArtifactError {}

/// Exact supported WIT world; optional identity remains an explicit selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolAdmissionWorld {
    /// Original protocol runtime world.
    Runtime,
    /// Runtime plus opt-in untrusted identity handoff.
    Identity,
}
impl ProtocolAdmissionWorld {
    /// Stable persistence and HTTP spelling, independent of Rust enum layout.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime-v1",
            Self::Identity => "identity-v1",
        }
    }
}

/// Immutable evidence of successful admission, never an activation grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolAdmission {
    /// Complete registered release that was checked.
    pub release: ProtocolPackageRelease,
    /// Exact WIT world checked in the isolated compiler.
    pub world: ProtocolAdmissionWorld,
    /// Reviewed host-policy version; previous versions do not imply current admission.
    pub policy_version: i32,
    /// First authorized actor; exact retries preserve this attribution.
    pub actor_id: ActorId,
    /// Host command-receipt time of the first successful admission, in Unix milliseconds.
    pub admitted_at: UnixTimeMillis,
}

/// Persistence for immutable bytes and successful admission evidence.
///
/// Release keys are tenant-scoped. Put resolves exact retries, never replaces
/// content, and returns only after durable storage. Admission recording preserves
/// the first actor/time under concurrency; failed/interrupted compiles create no receipt.
pub trait ProtocolArtifactStore: Send + Sync {
    /// Storage, reconstruction or immutable content collision failure.
    type Error: Error + Send + Sync + 'static;
    /// Stores the exact bytes, checking digest/size against the registered release.
    /// # Errors
    /// Returns an error on mismatched content, collision or unconfirmed durability.
    fn put(
        &self,
        release: &ProtocolPackageRelease,
        artifact: &ProtocolArtifact,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
    /// Retrieves only the selected release's tenant-owned bytes.
    /// # Errors
    /// Returns an error on storage or persisted-data validation failure.
    fn find(
        &self,
        release: &ProtocolPackageRelease,
    ) -> impl Future<Output = Result<Option<ProtocolArtifact>, Self::Error>> + Send;
    /// Finds evidence for the exact current policy and world; absence is not admission.
    /// # Errors
    /// Returns an error if storage or reconstruction fails.
    fn admission(
        &self,
        release: &ProtocolPackageRelease,
        world: ProtocolAdmissionWorld,
    ) -> impl Future<Output = Result<Option<ProtocolAdmission>, Self::Error>> + Send;
    /// Atomically retains the first successful evidence or resolves its exact retry.
    /// # Errors
    /// Returns an error if the release/content differs or durability is unconfirmed.
    fn record(
        &self,
        receipt: &ProtocolAdmission,
    ) -> impl Future<Output = Result<ProtocolAdmission, Self::Error>> + Send;
}
/// Redacted isolated compiler outcomes; none may be converted into admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolCompilerError {
    /// Component does not implement the chosen import-free WIT world.
    Rejected,
    /// The isolated worker or required resource isolation is unavailable.
    Unavailable,
    /// The bounded worker slot is occupied; callers must retry explicitly.
    Busy,
    /// The wall-clock deadline expired and the child was killed/reaped.
    Timeout,
}
impl fmt::Display for ProtocolCompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("protocol compilation did not succeed")
    }
}
impl Error for ProtocolCompilerError {}
/// Consumer-owned compilation port. Implementations must isolate compilation,
/// bound its capacity and terminate/reap interrupted workers; never invoke guests.
pub trait ProtocolCompiler: Send + Sync {
    /// Checks the exact artifact/world; success grants no runtime authority.
    /// # Errors
    /// Reports rejection, missing isolation, bounded capacity or timeout.
    fn validate(
        &self,
        artifact: &ProtocolArtifact,
        world: ProtocolAdmissionWorld,
    ) -> impl Future<Output = Result<(), ProtocolCompilerError>> + Send;
}
