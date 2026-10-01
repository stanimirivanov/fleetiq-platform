use fleetiq_domain::ActorId;
use fleetiq_identity::{
    ActorCatalogCursor, ActorCatalogEntry, ActorCatalogPage, ActorCatalogQuery,
    ActorCatalogRepository, ActorLifecycleState,
};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for bounded platform actor catalogue queries.
#[derive(Clone, Debug)]
pub struct PostgresActorCatalogRepository {
    pool: PgPool,
}

impl PostgresActorCatalogRepository {
    /// Creates a catalogue repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ActorCatalogRepository for PostgresActorCatalogRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(&self, query: &ActorCatalogQuery) -> Result<ActorCatalogPage, Self::Error> {
        let after_actor_id = query.after().map(|cursor| cursor.actor_id().as_str());
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT actor_id, is_active
            FROM identity.actors
            WHERE $1::text IS NULL OR actor_id > $1
            ORDER BY actor_id ASC
            LIMIT $2
            "#,
            after_actor_id,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        let mut entries = rows
            .into_iter()
            .map(|row| {
                Ok(ActorCatalogEntry::new(
                    ActorId::new(row.actor_id)
                        .map_err(|error| corrupt("identity.actors.actor_id", error))?,
                    if row.is_active {
                        ActorLifecycleState::Active
                    } else {
                        ActorLifecycleState::Deactivated
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
            ActorCatalogCursor::new(last.actor_id().clone())
        });

        Ok(ActorCatalogPage::new(entries, next))
    }
}

/// Error returned by PostgreSQL actor catalogue queries.
pub type PostgresActorCatalogRepositoryError = PostgresRepositoryError;
