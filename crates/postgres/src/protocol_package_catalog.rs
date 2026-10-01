use std::{error::Error, fmt, future::Future, num::NonZeroU64};

use bigdecimal::BigDecimal;
use fleetiq_domain::TenantId;
use fleetiq_ingestion::{
    ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease,
};
use fleetiq_telemetry::{
    ProtocolArtifactDigest, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// PostgreSQL implementation of the immutable protocol-package release catalogue.
#[derive(Clone, Debug)]
pub struct PostgresProtocolPackageCatalog {
    pool: PgPool,
}

impl PostgresProtocolPackageCatalog {
    /// Constructs a protocol-package catalogue using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Registers one release or resolves an exact concurrent retry.
    ///
    /// The conflict read is deliberately a second statement. At PostgreSQL
    /// `READ COMMITTED`, an insert can wait for another transaction while the
    /// original statement snapshot remains unable to see the committed winner.
    /// The fresh snapshot lets this adapter compare every immutable field rather
    /// than mistaking a release-key conflict for idempotent success.
    async fn register_release(
        &self,
        release: &ProtocolPackageRelease,
    ) -> Result<ProtocolPackageRegistrationOutcome, PostgresProtocolPackageCatalogError> {
        let digest = release.artifact_digest();
        let size = BigDecimal::from(release.artifact_size_bytes().get());
        let inserted = sqlx::query_scalar!(
            r#"
            INSERT INTO metadata.protocol_package_releases
                (tenant_id, protocol_package_id, protocol_package_version,
                 artifact_sha256, artifact_size_bytes)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT
                (tenant_id, protocol_package_id, protocol_package_version)
            DO NOTHING
            RETURNING TRUE AS "inserted!"
            "#,
            release.tenant_id().as_str(),
            release.package().id().as_str(),
            release.package().version().as_str(),
            digest.as_bytes().as_slice(),
            size,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        if inserted.is_some() {
            return Ok(ProtocolPackageRegistrationOutcome::Registered);
        }

        let existing =
            ProtocolPackageReleaseRow::find(&self.pool, release.tenant_id(), release.package())
                .await?
                .ok_or_else(|| {
                    corrupt(
                        "protocol_package_releases",
                        "conflicting immutable release disappeared before it could be read",
                    )
                })?
                .into_domain()?;

        if &existing != release {
            return Err(PostgresProtocolPackageCatalogError::IdentityCollision {
                tenant_id: release.tenant_id().clone(),
                package: release.package().clone(),
            });
        }

        Ok(ProtocolPackageRegistrationOutcome::AlreadyPresent)
    }

    async fn find_release(
        &self,
        tenant_id: &TenantId,
        package: &ProtocolPackageRef,
    ) -> Result<Option<ProtocolPackageRelease>, PostgresProtocolPackageCatalogError> {
        ProtocolPackageReleaseRow::find(&self.pool, tenant_id, package)
            .await?
            .map(ProtocolPackageReleaseRow::into_domain)
            .transpose()
            .map_err(Into::into)
    }
}

impl ProtocolPackageCatalog for PostgresProtocolPackageCatalog {
    type Error = PostgresProtocolPackageCatalogError;

    fn register<'a>(
        &'a self,
        release: &'a ProtocolPackageRelease,
    ) -> impl Future<Output = Result<ProtocolPackageRegistrationOutcome, Self::Error>> + Send + 'a
    {
        self.register_release(release)
    }

    fn find<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        package: &'a ProtocolPackageRef,
    ) -> impl Future<Output = Result<Option<ProtocolPackageRelease>, Self::Error>> + Send + 'a {
        self.find_release(tenant_id, package)
    }
}

/// Failure while registering or reconstructing a protocol-package release.
#[derive(Debug)]
pub enum PostgresProtocolPackageCatalogError {
    /// The tenant-scoped release key already names different immutable content.
    IdentityCollision {
        /// Tenant that owns the conflicting package namespace.
        tenant_id: TenantId,
        /// Package ID and version whose content identity was reused.
        package: ProtocolPackageRef,
    },
    /// PostgreSQL operation or persisted-data reconstruction failed.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresProtocolPackageCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision { tenant_id, package } => write!(
                formatter,
                "protocol-package release {tenant_id}.{}@{} already names different immutable content",
                package.id(),
                package.version()
            ),
            Self::Repository(error) => error.fmt(formatter),
        }
    }
}

impl Error for PostgresProtocolPackageCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

impl From<PostgresRepositoryError> for PostgresProtocolPackageCatalogError {
    fn from(error: PostgresRepositoryError) -> Self {
        Self::Repository(error)
    }
}

struct ProtocolPackageReleaseRow {
    tenant_id: String,
    protocol_package_id: String,
    protocol_package_version: String,
    artifact_sha256: Vec<u8>,
    artifact_size_bytes: BigDecimal,
}

impl ProtocolPackageReleaseRow {
    async fn find(
        pool: &PgPool,
        tenant_id: &TenantId,
        package: &ProtocolPackageRef,
    ) -> Result<Option<Self>, PostgresRepositoryError> {
        sqlx::query_as!(
            Self,
            r"
            SELECT tenant_id, protocol_package_id, protocol_package_version,
                   artifact_sha256, artifact_size_bytes
            FROM metadata.protocol_package_releases
            WHERE tenant_id = $1
              AND protocol_package_id = $2
              AND protocol_package_version = $3
            ",
            tenant_id.as_str(),
            package.id().as_str(),
            package.version().as_str(),
        )
        .fetch_optional(pool)
        .await
        .map_err(classify_database_error)
    }

    fn into_domain(self) -> Result<ProtocolPackageRelease, PostgresRepositoryError> {
        let digest: [u8; 32] = self.artifact_sha256.try_into().map_err(|value: Vec<u8>| {
            corrupt(
                "protocol_package_releases.artifact_sha256",
                format_args!("expected 32 bytes but found {}", value.len()),
            )
        })?;
        let size = self
            .artifact_size_bytes
            .to_string()
            .parse::<u64>()
            .map_err(|error| corrupt("protocol_package_releases.artifact_size_bytes", error))?;
        let size = NonZeroU64::new(size).ok_or_else(|| {
            corrupt(
                "protocol_package_releases.artifact_size_bytes",
                "artifact size must be nonzero",
            )
        })?;

        Ok(ProtocolPackageRelease::new(
            TenantId::new(self.tenant_id)
                .map_err(|error| corrupt("protocol_package_releases.tenant_id", error))?,
            ProtocolPackageRef::new(
                ProtocolPackageId::new(self.protocol_package_id).map_err(|error| {
                    corrupt("protocol_package_releases.protocol_package_id", error)
                })?,
                ProtocolPackageVersion::new(self.protocol_package_version).map_err(|error| {
                    corrupt("protocol_package_releases.protocol_package_version", error)
                })?,
            ),
            ProtocolArtifactDigest::from_sha256(digest),
            size,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_reconstructs_the_complete_unsigned_artifact_size_range() {
        let row = ProtocolPackageReleaseRow {
            tenant_id: "tenant-a".to_owned(),
            protocol_package_id: "custom.binary".to_owned(),
            protocol_package_version: "1".to_owned(),
            artifact_sha256: vec![7; 32],
            artifact_size_bytes: BigDecimal::from(u64::MAX),
        };

        let release = row.into_domain().expect("maximum u64 should reconstruct");

        assert_eq!(release.artifact_size_bytes().get(), u64::MAX);
        assert_eq!(release.artifact_digest().as_bytes(), &[7; 32]);
    }
}
