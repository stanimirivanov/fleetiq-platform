use std::future::Future;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};
use fleetiq_domain::{
    Asset, AssetId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, ExternalIdentifier,
    ExternalIdentifierAuthority, ExternalIdentifierKind, ExternalIdentifierValue, TenantId,
};
use fleetiq_metadata::AssetRepository;
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

    /// Inserts an asset aggregate in one atomic database statement.
    ///
    /// The data-modifying common table expression inserts the asset first and
    /// feeds its identity into one set-based insert for all external
    /// identifiers. Parallel arrays are expanded with `UNNEST`, eliminating a
    /// database round trip per identifier. Empty arrays still insert the asset
    /// and naturally produce no identifier rows.
    async fn insert_asset(&self, asset: &Asset) -> Result<(), PostgresRepositoryError> {
        let tenant_id = asset.tenant_id().as_str();
        let asset_id = asset.id().as_str();
        let type_id = asset.asset_type().id().as_str();
        let type_version = i64::from(asset.asset_type().version().get());
        let name = asset.name().as_str();
        let identifiers = asset.external_identifiers();
        let kinds: Vec<_> = identifiers
            .iter()
            .map(|identifier| identifier.kind().as_str())
            .collect();
        let authorities: Vec<_> = identifiers
            .iter()
            .map(|identifier| identifier.authority().map(AsRef::as_ref))
            .collect();
        let values: Vec<_> = identifiers
            .iter()
            .map(|identifier| identifier.value().as_str())
            .collect();

        sqlx::query!(
            r"
            WITH inserted_asset AS (
                INSERT INTO metadata.assets
                    (tenant_id, asset_id, type_id, type_version, display_name)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING tenant_id, asset_id
            )
            INSERT INTO metadata.asset_external_identifiers
                (tenant_id, asset_id, identifier_kind, authority, identifier_value)
            SELECT
                inserted_asset.tenant_id,
                inserted_asset.asset_id,
                identifier.identifier_kind,
                identifier.authority,
                identifier.identifier_value
            FROM inserted_asset
            CROSS JOIN UNNEST($6::text[], $7::text[], $8::text[])
                AS identifier(identifier_kind, authority, identifier_value)
            ",
            tenant_id,
            asset_id,
            type_id,
            type_version,
            name,
            &kinds as &[&str],
            &authorities as &[Option<&str>],
            &values as &[&str]
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;

        Ok(())
    }

    /// Reconstructs an asset aggregate with two deliberately separate reads.
    ///
    /// Reading the parent first distinguishes an absent asset without decoding
    /// an aggregated child column. The second query uses the child-table index
    /// and preserves compile-time column checking for every identifier field.
    async fn find_asset(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
    ) -> Result<Option<Asset>, PostgresRepositoryError> {
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
    type Error = PostgresRepositoryError;

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
    ) -> Result<Asset, PostgresRepositoryError> {
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
    fn into_domain(self) -> Result<ExternalIdentifier, PostgresRepositoryError> {
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
