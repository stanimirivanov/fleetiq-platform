use fleetiq_domain::TenantId;
use fleetiq_identity::{
    TenantCatalogCursor, TenantCatalogEntry, TenantCatalogPage, TenantCatalogQuery,
    TenantCatalogRepository, TenantLifecycleState,
};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for bounded platform tenant catalogue queries.
#[derive(Clone, Debug)]
pub struct PostgresTenantCatalogRepository {
    pool: PgPool,
}

impl PostgresTenantCatalogRepository {
    /// Creates a catalogue repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantCatalogRepository for PostgresTenantCatalogRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(
        &self,
        query: &TenantCatalogQuery,
    ) -> Result<TenantCatalogPage, Self::Error> {
        let after_tenant_id = query.after().map(|cursor| cursor.tenant_id().as_str());
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT tenant_id, is_active
            FROM metadata.tenants
            WHERE $1::text IS NULL OR tenant_id > $1
            ORDER BY tenant_id ASC
            LIMIT $2
            "#,
            after_tenant_id,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        let mut entries = rows
            .into_iter()
            .map(|row| {
                Ok(TenantCatalogEntry::new(
                    TenantId::new(row.tenant_id)
                        .map_err(|error| corrupt("metadata.tenants.tenant_id", error))?,
                    if row.is_active {
                        TenantLifecycleState::Active
                    } else {
                        TenantLifecycleState::Suspended
                    },
                ))
            })
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;

        let has_more = entries.len() > usize::from(query.page_size().get());
        if has_more {
            entries.pop();
        }
        let next = has_more.then(|| {
            let last = entries
                .last()
                .expect("a page with an extra row retains at least one entry");
            TenantCatalogCursor::new(last.tenant_id().clone())
        });

        Ok(TenantCatalogPage::new(entries, next))
    }
}

/// Error returned by PostgreSQL tenant catalogue queries.
pub type PostgresTenantCatalogRepositoryError = PostgresRepositoryError;
