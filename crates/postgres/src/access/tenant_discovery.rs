use fleetiq_access::{
    DiscoveredTenant, TenantDiscoveryPage, TenantDiscoveryQuery, TenantDiscoveryRepository,
};
use fleetiq_domain::{ActorId, TenantId};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, action_codec::parse_tenant_action, error::corrupt};

/// Current actor-scoped tenant discovery using one bounded statement per page.
#[derive(Clone, Debug)]
pub struct PostgresTenantDiscoveryRepository {
    pool: PgPool,
}
impl PostgresTenantDiscoveryRepository {
    /// Creates a read adapter sharing the application's PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantDiscoveryRepository for PostgresTenantDiscoveryRepository {
    type Error = PostgresRepositoryError;

    /// Reads tenant identities and their complete action sets in one snapshot.
    ///
    /// Grouping precedes LIMIT: a tenant with several grants consumes one page
    /// slot, and its actions are never truncated at a page boundary. The extra
    /// tenant is only lookahead. Existing actor/grant and tenant indexes support
    /// the joins; no per-tenant query or grant cache is introduced.
    async fn find_page(
        &self,
        actor: &ActorId,
        query: &TenantDiscoveryQuery,
    ) -> Result<TenantDiscoveryPage, Self::Error> {
        let after = query.after().map(TenantId::as_str);
        let fetch_limit = i64::from(query.limit()) + 1;
        let rows = sqlx::query!(
            r#"
            SELECT grants.tenant_id,
                   array_agg(grants.action ORDER BY grants.action) AS "actions!"
            FROM identity.tenant_action_grants AS grants
            JOIN identity.actors AS actors ON actors.actor_id = grants.actor_id
            JOIN metadata.tenants AS tenants ON tenants.tenant_id = grants.tenant_id
            WHERE grants.actor_id = $1 AND actors.is_active AND tenants.is_active
              AND ($2::text IS NULL OR grants.tenant_id > $2)
            GROUP BY grants.tenant_id
            ORDER BY grants.tenant_id ASC
            LIMIT $3
            "#,
            actor.as_str(),
            after,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;
        let mut entries = rows
            .into_iter()
            .map(|row| {
                let tenant = TenantId::new(row.tenant_id)
                    .map_err(|error| corrupt("identity.tenant_action_grants.tenant_id", error))?;
                let actions = row
                    .actions
                    .iter()
                    .map(|action| {
                        parse_tenant_action(action, "identity.tenant_action_grants.action")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(DiscoveredTenant::new(tenant, actions))
            })
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;
        let has_more = entries.len() > usize::from(query.limit());
        entries.truncate(usize::from(query.limit()));
        let next_after = if has_more {
            entries.last().map(|entry| entry.tenant_id().clone())
        } else {
            None
        };
        Ok(TenantDiscoveryPage::new(entries, next_after))
    }
}
