use crate::action_codec::tenant_action_name;
use fleetiq_identity::{
    TenantGrantCatalogCursor, TenantGrantCatalogEntry, TenantGrantCatalogPage,
    TenantGrantCatalogQuery, TenantGrantCatalogRepository,
};

use crate::action_codec::parse_tenant_action;

use fleetiq_domain::TenantId;
use sqlx::PgPool;

use crate::PostgresRepositoryError;

/// PostgreSQL repository for actor-scoped current tenant-grant queries.
#[derive(Clone, Debug)]
pub struct PostgresTenantGrantCatalogRepository {
    pool: PgPool,
}

impl PostgresTenantGrantCatalogRepository {
    /// Creates a catalogue repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantGrantCatalogRepository for PostgresTenantGrantCatalogRepository {
    type Error = PostgresRepositoryError;

    /// One indexed keyset query reads at most limit + 1 rows. The extra row
    /// determines continuation without a count query or a query per grant.
    async fn find_page(
        &self,
        tenant_id: &TenantId,
        query: &TenantGrantCatalogQuery,
    ) -> Result<TenantGrantCatalogPage, Self::Error> {
        let after_action = query
            .after()
            .map(|cursor| tenant_action_name(cursor.action()));
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT action
            FROM identity.tenant_action_grants
            WHERE tenant_id = $1 AND actor_id = $2
              AND ($3::text IS NULL OR action > $3)
            ORDER BY action ASC
            LIMIT $4
            "#,
            tenant_id.as_str(),
            query.actor_id().as_str(),
            after_action,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        let mut entries = rows
            .into_iter()
            .map(|row| {
                Ok(TenantGrantCatalogEntry::new(parse_tenant_action(
                    &row.action,
                    "identity.tenant_action_grants.action",
                )?))
            })
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;

        let has_more = entries.len() > usize::from(query.page_size().get());
        if has_more {
            entries.pop();
        }
        let next = has_more.then(|| {
            TenantGrantCatalogCursor::new(
                entries
                    .last()
                    .expect("a page with an extra row retains at least one entry")
                    .action(),
            )
        });

        Ok(TenantGrantCatalogPage::new(entries, next))
    }
}

/// Error returned by PostgreSQL current tenant-grant queries.
pub type PostgresTenantGrantCatalogRepositoryError = PostgresRepositoryError;
