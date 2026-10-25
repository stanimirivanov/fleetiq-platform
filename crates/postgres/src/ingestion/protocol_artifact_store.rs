//! PostgreSQL keeps each bounded artifact atomic with no external file lifecycle.
use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_domain::{ActorId, UnixTimeMillis};
use fleetiq_ingestion::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolPackageRelease,
};
use sqlx::PgPool;

/// Tenant-scoped immutable artifact bytes and policy-versioned admission receipts.
#[derive(Clone, Debug)]
pub struct PostgresProtocolArtifactStore {
    pool: PgPool,
}
impl PostgresProtocolArtifactStore {
    /// Uses the existing application pool; no filesystem or object-store authority.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}
impl ProtocolArtifactStore for PostgresProtocolArtifactStore {
    type Error = PostgresRepositoryError;
    async fn put(
        &self,
        release: &ProtocolPackageRelease,
        artifact: &ProtocolArtifact,
    ) -> Result<(), Self::Error> {
        if !artifact.matches(release) {
            return Err(corrupt("protocol_artifacts", "content identity mismatch"));
        }
        let digest = release.artifact_digest();
        let size = bigdecimal::BigDecimal::from(release.artifact_size_bytes().get());
        // INSERT SELECT verifies the catalogue identity in the same statement.
        // A no-op conflict needs a fresh READ COMMITTED snapshot: a concurrent
        // winner may have been invisible to the original INSERT snapshot.
        let inserted = sqlx::query!(
            r"
            INSERT INTO ingestion.protocol_artifacts
                (tenant_id, artifact_sha256, artifact_bytes)
            SELECT tenant_id, artifact_sha256, $4
            FROM metadata.protocol_package_releases
            WHERE tenant_id = $1 AND protocol_package_id = $2 AND protocol_package_version = $3
              AND artifact_sha256 = $5 AND artifact_size_bytes = $6
            ON CONFLICT (tenant_id, artifact_sha256) DO NOTHING
        ",
            release.tenant_id().as_str(),
            release.package().id().as_str(),
            release.package().version().as_str(),
            artifact.bytes(),
            digest.as_bytes().as_slice(),
            size
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;
        if inserted.rows_affected() == 1 {
            return Ok(());
        }
        if self.find(release).await?.as_ref() != Some(artifact) {
            return Err(corrupt(
                "protocol_artifacts",
                "missing or conflicting immutable bytes",
            ));
        }
        Ok(())
    }
    async fn find(
        &self,
        release: &ProtocolPackageRelease,
    ) -> Result<Option<ProtocolArtifact>, Self::Error> {
        let digest = release.artifact_digest();
        let size = bigdecimal::BigDecimal::from(release.artifact_size_bytes().get());
        let bytes = sqlx::query_scalar!(
            r"
            SELECT artifact.artifact_bytes
            FROM ingestion.protocol_artifacts AS artifact
            INNER JOIN metadata.protocol_package_releases AS release
                ON release.tenant_id = artifact.tenant_id AND release.artifact_sha256 = artifact.artifact_sha256
            WHERE release.tenant_id = $1 AND release.protocol_package_id = $2 AND release.protocol_package_version = $3
              AND artifact.artifact_sha256 = $4 AND release.artifact_size_bytes = $5
        ",
            release.tenant_id().as_str(),
            release.package().id().as_str(),
            release.package().version().as_str(),
            digest.as_bytes().as_slice(),
            size
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        bytes
            .map(|bytes| {
                let artifact = ProtocolArtifact::new(bytes)
                    .map_err(|_| corrupt("protocol_artifacts", "invalid raw-byte bounds"))?;
                if !artifact.matches(release) {
                    return Err(corrupt(
                        "protocol_artifacts",
                        "catalogue digest or length differs",
                    ));
                }
                Ok(artifact)
            })
            .transpose()
    }
    async fn admission(
        &self,
        release: &ProtocolPackageRelease,
        world: ProtocolAdmissionWorld,
    ) -> Result<Option<ProtocolAdmission>, Self::Error> {
        let digest = release.artifact_digest();
        let world_name = world.as_str();
        let row = sqlx::query!(
            r"
            SELECT actor_id, admitted_at_ms FROM ingestion.protocol_admissions
            WHERE tenant_id = $1 AND protocol_package_id = $2 AND protocol_package_version = $3
              AND wit_world = $4 AND policy_version = $5 AND artifact_sha256 = $6
        ",
            release.tenant_id().as_str(),
            release.package().id().as_str(),
            release.package().version().as_str(),
            world_name,
            PROTOCOL_ADMISSION_POLICY,
            digest.as_bytes().as_slice()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        row.map(|row| {
            Ok(ProtocolAdmission {
                release: release.clone(),
                world,
                policy_version: PROTOCOL_ADMISSION_POLICY,
                actor_id: ActorId::new(row.actor_id)
                    .map_err(|_| corrupt("protocol_admissions.actor_id", "invalid actor"))?,
                admitted_at: UnixTimeMillis::new(row.admitted_at_ms),
            })
        })
        .transpose()
    }
    async fn record(&self, receipt: &ProtocolAdmission) -> Result<ProtocolAdmission, Self::Error> {
        if receipt.policy_version != PROTOCOL_ADMISSION_POLICY
            || self.find(&receipt.release).await?.is_none()
        {
            return Err(corrupt(
                "protocol_admissions",
                "unverified policy or missing artifact",
            ));
        }
        let release = &receipt.release;
        let digest = release.artifact_digest();
        let world = receipt.world.as_str();
        let time = receipt.admitted_at.get();
        // No update: concurrent operators must retain the first actor and time.
        sqlx::query!(r"
            INSERT INTO ingestion.protocol_admissions
                (tenant_id, protocol_package_id, protocol_package_version, wit_world, policy_version, actor_id, admitted_at_ms, artifact_sha256)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (tenant_id, protocol_package_id, protocol_package_version, wit_world, policy_version) DO NOTHING
        ", release.tenant_id().as_str(), release.package().id().as_str(), release.package().version().as_str(), world, PROTOCOL_ADMISSION_POLICY, receipt.actor_id.as_str(), time, digest.as_bytes().as_slice())
            .execute(&self.pool).await.map_err(classify_database_error)?;
        self.admission(release, receipt.world)
            .await?
            .ok_or_else(|| corrupt("protocol_admissions", "committed evidence disappeared"))
    }
}
