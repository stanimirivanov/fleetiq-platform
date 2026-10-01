use fleetiq_access::PlatformAction;
use fleetiq_identity::{
    PlatformGrantCatalogCursor, PlatformGrantCatalogEntry, PlatformGrantCatalogPage,
    PlatformGrantCatalogQuery, PlatformGrantCatalogRepository,
};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for actor-scoped current platform-grant queries.
#[derive(Clone, Debug)]
pub struct PostgresPlatformGrantCatalogRepository {
    pool: PgPool,
}

impl PostgresPlatformGrantCatalogRepository {
    /// Creates a catalogue repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PlatformGrantCatalogRepository for PostgresPlatformGrantCatalogRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(
        &self,
        query: &PlatformGrantCatalogQuery,
    ) -> Result<PlatformGrantCatalogPage, Self::Error> {
        let after_action = query.after().map(|cursor| action_name(cursor.action()));
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT action
            FROM identity.platform_action_grants
            WHERE actor_id = $1
              AND ($2::text IS NULL OR action > $2)
            ORDER BY action ASC
            LIMIT $3
            "#,
            query.actor_id().as_str(),
            after_action,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        let mut entries = rows
            .into_iter()
            .map(|row| Ok(PlatformGrantCatalogEntry::new(action(&row.action)?)))
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;

        let has_more = entries.len() > usize::from(query.page_size().get());
        if has_more {
            entries.pop();
        }
        let next = has_more.then(|| {
            PlatformGrantCatalogCursor::new(
                entries
                    .last()
                    .expect("a page with an extra row retains at least one entry")
                    .action(),
            )
        });

        Ok(PlatformGrantCatalogPage::new(entries, next))
    }
}

const fn action_name(action: PlatformAction) -> &'static str {
    match action {
        PlatformAction::ManagePlatformAccess => "platform.access.manage",
        PlatformAction::ManageIdentity => "identity.manage",
        PlatformAction::ProvisionTenant => "tenant.provision",
        PlatformAction::ManageTenantLifecycle => "tenant.lifecycle.manage",
    }
}

fn action(value: &str) -> Result<PlatformAction, PostgresRepositoryError> {
    match value {
        "platform.access.manage" => Ok(PlatformAction::ManagePlatformAccess),
        "identity.manage" => Ok(PlatformAction::ManageIdentity),
        "tenant.provision" => Ok(PlatformAction::ProvisionTenant),
        "tenant.lifecycle.manage" => Ok(PlatformAction::ManageTenantLifecycle),
        _ => Err(corrupt("identity.platform_action_grants.action", value)),
    }
}

/// Error returned by PostgreSQL current platform-grant queries.
pub type PostgresPlatformGrantCatalogRepositoryError = PostgresRepositoryError;
