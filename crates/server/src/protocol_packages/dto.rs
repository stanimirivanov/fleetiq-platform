//! Explicit HTTP shapes; raw artifacts are accepted only as bounded binary bodies.
use fleetiq_ingestion::{ProtocolAdmission, ProtocolAdmissionWorld, ProtocolPackageRelease};
use fleetiq_telemetry::{
    ProtocolArtifactDigest, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use serde::{Deserialize, Serialize};
#[derive(Serialize)]
pub(super) struct ReleaseBody {
    package_id: String,
    package_version: String,
    artifact_sha256: String,
    artifact_size_bytes: u64,
}
impl From<&ProtocolPackageRelease> for ReleaseBody {
    fn from(release: &ProtocolPackageRelease) -> Self {
        Self {
            package_id: release.package().id().to_string(),
            package_version: release.package().version().to_string(),
            artifact_sha256: release.artifact_digest().to_string(),
            artifact_size_bytes: release.artifact_size_bytes().get(),
        }
    }
}
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum World {
    #[serde(rename = "runtime-v1")]
    Runtime,
    #[serde(rename = "identity-v1")]
    Identity,
}
impl From<World> for ProtocolAdmissionWorld {
    fn from(value: World) -> Self {
        match value {
            World::Runtime => Self::Runtime,
            World::Identity => Self::Identity,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionBody {
    pub world: World,
}
#[derive(Serialize)]
pub(super) struct AdmissionResponse {
    release: ReleaseBody,
    world: &'static str,
    policy_version: i32,
    actor_id: String,
    admitted_at_ms: i64,
}
impl From<&ProtocolAdmission> for AdmissionResponse {
    fn from(receipt: &ProtocolAdmission) -> Self {
        Self {
            release: (&receipt.release).into(),
            world: receipt.world.as_str(),
            policy_version: receipt.policy_version,
            actor_id: receipt.actor_id.to_string(),
            admitted_at_ms: receipt.admitted_at.get(),
        }
    }
}
pub(super) fn package(id: &str, version: &str) -> Option<ProtocolPackageRef> {
    Some(ProtocolPackageRef::new(
        ProtocolPackageId::new(id).ok()?,
        ProtocolPackageVersion::new(version).ok()?,
    ))
}
pub(super) fn digest(text: &str) -> Option<ProtocolArtifactDigest> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(ProtocolArtifactDigest::from_sha256(bytes))
}
