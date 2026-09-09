use std::{error::Error, fmt, future::Future};

use fleetiq_application::AssetRepository;
use fleetiq_domain::{
    Asset, AssetId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, ExternalIdentifier,
    ExternalIdentifierAuthority, ExternalIdentifierKind, ExternalIdentifierValue, TenantId,
};
use sqlx::PgPool;

/// PostgreSQL implementation of the application-owned asset repository port.
#[derive(Clone, Debug)]
pub struct PostgresAssetRepository {
    pool: PgPool,
}

impl PostgresAssetRepository {
    /// Constructs an asset repository using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn insert_asset(&self, asset: &Asset) -> Result<(), PostgresAssetRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let tenant_id = asset.tenant_id().as_str();
        let asset_id = asset.id().as_str();
        let type_id = asset.asset_type().id().as_str();
        let type_version = i64::from(asset.asset_type().version().get());
        let name = asset.name().as_str();

        sqlx::query!(
            r"
            INSERT INTO metadata.assets
                (tenant_id, asset_id, type_id, type_version, display_name)
            VALUES ($1, $2, $3, $4, $5)
            ",
            tenant_id,
            asset_id,
            type_id,
            type_version,
            name
        )
        .execute(&mut *transaction)
        .await
        .map_err(classify_database_error)?;

        for identifier in asset.external_identifiers() {
            let kind = identifier.kind().as_str();
            let authority = identifier.authority().map(AsRef::as_ref);
            let value = identifier.value().as_str();

            sqlx::query!(
                r"
                INSERT INTO metadata.asset_external_identifiers
                    (tenant_id, asset_id, identifier_kind, authority, identifier_value)
                VALUES ($1, $2, $3, $4, $5)
                ",
                tenant_id,
                asset_id,
                kind,
                authority,
                value
            )
            .execute(&mut *transaction)
            .await
            .map_err(classify_database_error)?;
        }

        transaction.commit().await.map_err(classify_database_error)
    }

    async fn find_asset(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
    ) -> Result<Option<Asset>, PostgresAssetRepositoryError> {
        let tenant_id = tenant_id.as_str();
        let asset_id = asset_id.as_str();
        let row = sqlx::query_as!(
            AssetRow,
            r"
            SELECT tenant_id, asset_id, type_id, type_version, display_name
            FROM metadata.assets
            WHERE tenant_id = $1 AND asset_id = $2
            ",
            tenant_id,
            asset_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let identifier_rows = sqlx::query_as!(
            ExternalIdentifierRow,
            r"
            SELECT identifier_kind, authority, identifier_value
            FROM metadata.asset_external_identifiers
            WHERE tenant_id = $1 AND asset_id = $2
            ORDER BY identifier_kind, authority NULLS FIRST, identifier_value
            ",
            tenant_id,
            asset_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.into_domain(identifier_rows).map(Some)
    }
}

impl AssetRepository for PostgresAssetRepository {
    type Error = PostgresAssetRepositoryError;

    fn insert<'a>(
        &'a self,
        asset: &'a Asset,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_asset(asset)
    }

    fn find_by_id<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<Asset>, Self::Error>> + Send + 'a {
        self.find_asset(tenant_id, asset_id)
    }
}

#[derive(Debug)]
struct AssetRow {
    tenant_id: String,
    asset_id: String,
    type_id: String,
    type_version: i64,
    display_name: String,
}

impl AssetRow {
    fn into_domain(
        self,
        identifiers: Vec<ExternalIdentifierRow>,
    ) -> Result<Asset, PostgresAssetRepositoryError> {
        let version = u32::try_from(self.type_version)
            .map_err(|error| corrupt("assets.type_version", error))?;
        let mut asset = Asset::new(
            TenantId::new(self.tenant_id).map_err(|error| corrupt("assets.tenant_id", error))?,
            AssetId::new(self.asset_id).map_err(|error| corrupt("assets.asset_id", error))?,
            AssetTypeRef::new(
                AssetTypeId::new(self.type_id).map_err(|error| corrupt("assets.type_id", error))?,
                DefinitionVersion::new(version)
                    .map_err(|error| corrupt("assets.type_version", error))?,
            ),
            AssetName::new(self.display_name)
                .map_err(|error| corrupt("assets.display_name", error))?,
        );

        for identifier in identifiers {
            asset
                .add_external_identifier(identifier.into_domain()?)
                .map_err(|error| corrupt("asset_external_identifiers", error))?;
        }

        Ok(asset)
    }
}

#[derive(Debug)]
struct ExternalIdentifierRow {
    identifier_kind: String,
    authority: Option<String>,
    identifier_value: String,
}

impl ExternalIdentifierRow {
    fn into_domain(self) -> Result<ExternalIdentifier, PostgresAssetRepositoryError> {
        Ok(ExternalIdentifier::new(
            ExternalIdentifierKind::new(self.identifier_kind)
                .map_err(|error| corrupt("asset_external_identifiers.identifier_kind", error))?,
            self.authority
                .map(ExternalIdentifierAuthority::new)
                .transpose()
                .map_err(|error| corrupt("asset_external_identifiers.authority", error))?,
            ExternalIdentifierValue::new(self.identifier_value)
                .map_err(|error| corrupt("asset_external_identifiers.identifier_value", error))?,
        ))
    }
}

/// Describes a failed PostgreSQL asset-repository operation.
#[derive(Debug)]
pub enum PostgresAssetRepositoryError {
    /// A uniqueness constraint rejected a conflicting value.
    Conflict {
        /// The database constraint that detected the conflict, when available.
        constraint: Option<String>,
        /// The original database error.
        source: sqlx::Error,
    },
    /// A foreign key rejected a reference to missing tenant or definition data.
    InvalidReference {
        /// The database constraint that detected the invalid reference.
        constraint: Option<String>,
        /// The original database error.
        source: sqlx::Error,
    },
    /// Persisted data violated a domain invariant during reconstruction.
    CorruptData {
        /// The column or record that could not be reconstructed.
        field: &'static str,
        /// The domain validation failure.
        message: String,
    },
    /// Another database operation failed.
    Database(sqlx::Error),
}

impl fmt::Display for PostgresAssetRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { constraint, .. } => {
                write!(formatter, "database conflict")?;
                format_constraint(formatter, constraint.as_deref())
            }
            Self::InvalidReference { constraint, .. } => {
                write!(formatter, "database reference is invalid")?;
                format_constraint(formatter, constraint.as_deref())
            }
            Self::CorruptData { field, message } => {
                write!(
                    formatter,
                    "persisted value in {field} is invalid: {message}"
                )
            }
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
        }
    }
}

impl Error for PostgresAssetRepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Conflict { source, .. } | Self::InvalidReference { source, .. } => Some(source),
            Self::Database(error) => Some(error),
            Self::CorruptData { .. } => None,
        }
    }
}

fn classify_database_error(error: sqlx::Error) -> PostgresAssetRepositoryError {
    let Some(database_error) = error.as_database_error() else {
        return PostgresAssetRepositoryError::Database(error);
    };
    let code = database_error.code();
    let constraint = database_error.constraint().map(ToOwned::to_owned);

    match code.as_deref() {
        Some("23505") => PostgresAssetRepositoryError::Conflict {
            constraint,
            source: error,
        },
        Some("23503") => PostgresAssetRepositoryError::InvalidReference {
            constraint,
            source: error,
        },
        _ => PostgresAssetRepositoryError::Database(error),
    }
}

fn corrupt(field: &'static str, error: impl fmt::Display) -> PostgresAssetRepositoryError {
    PostgresAssetRepositoryError::CorruptData {
        field,
        message: error.to_string(),
    }
}

fn format_constraint(formatter: &mut fmt::Formatter<'_>, constraint: Option<&str>) -> fmt::Result {
    if let Some(constraint) = constraint {
        write!(formatter, " ({constraint})")?;
    }
    Ok(())
}
