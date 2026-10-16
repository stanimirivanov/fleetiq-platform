//! Bounded summary traversal avoids loading child memberships for every version.
use super::{PostgresDefinitionRepository, common::definition_version};
use crate::{
    PostgresRepositoryError,
    error::{classify_database_error, corrupt},
};
use fleetiq_domain::{DefinitionName, TenantId};
use fleetiq_metadata::{
    DefinitionCataloguePage, DefinitionCatalogueQuery, DefinitionCatalogueRepository,
    DefinitionIdentity, DefinitionKind, DefinitionPosition, DefinitionSummary,
};
impl DefinitionCatalogueRepository for PostgresDefinitionRepository {
    type Error = PostgresRepositoryError;

    /// One checked UNION selects the requested namespace inside one snapshot.
    ///
    /// Constant branch guards and tenant predicates keep each namespace scoped.
    /// The existing tenant/ID/version keys serve ordering and exact-ID filtering.
    /// A single lookahead row supplies continuation without count or per-row SQL.
    async fn list_page(
        &self,
        tenant: &TenantId,
        query: &DefinitionCatalogueQuery,
    ) -> Result<DefinitionCataloguePage, Self::Error> {
        let kind = match query.kind() {
            DefinitionKind::AssetType => "asset_type",
            DefinitionKind::Property => "property",
            DefinitionKind::RelationshipType => "relationship_type",
        };
        let identity = query.identity().map(DefinitionIdentity::as_str);
        let after_id = query.after().map(|p| p.identity().as_str());
        let after_version = query.after().map(|p| i64::from(p.version().get()));
        let fetch_limit = i64::from(query.limit()) + 1;
        let rows = sqlx::query!(
            r#"
            SELECT id AS "id!", version AS "version!", display_name AS "name!"
            FROM (
                SELECT type_id AS id, version, display_name
                FROM metadata.asset_type_definitions
                WHERE tenant_id = $1 AND $2::text = 'asset_type'
                UNION ALL
                SELECT property_id AS id, version, display_name
                FROM metadata.property_definitions
                WHERE tenant_id = $1 AND $2::text = 'property'
                UNION ALL
                SELECT relationship_type_id AS id, version, display_name
                FROM metadata.relationship_type_definitions
                WHERE tenant_id = $1 AND $2::text = 'relationship_type'
            ) AS definitions
            WHERE ($3::text IS NULL OR id = $3)
              AND ($4::text IS NULL OR (id, version) > ($4, $5::bigint))
            ORDER BY id ASC, version ASC
            LIMIT $6
            "#,
            tenant.as_str(),
            kind,
            identity,
            after_id,
            after_version,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;
        let mut entries = rows
            .into_iter()
            .map(|row| {
                let identity = DefinitionIdentity::new(query.kind(), row.id)
                    .map_err(|e| corrupt("metadata.definition.id", e))?;
                let version = definition_version("metadata.definition.version", row.version)?;
                let name = DefinitionName::new(row.name)
                    .map_err(|e| corrupt("metadata.definition.display_name", e))?;
                Ok(DefinitionSummary::new(
                    DefinitionPosition::new(identity, version),
                    name,
                ))
            })
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;
        let more = entries.len() > usize::from(query.limit());
        if more {
            entries.pop();
        }
        let next = more.then(|| {
            entries
                .last()
                .expect("lookahead leaves a nonempty bounded page")
                .position()
                .clone()
        });
        Ok(DefinitionCataloguePage::new(entries, next))
    }
}
