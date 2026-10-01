use fleetiq_domain::{ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};
use fleetiq_identity::{
    ExternalIdentityCatalogCursor, ExternalIdentityCatalogEntry, ExternalIdentityCatalogPage,
    ExternalIdentityCatalogQuery, ExternalIdentityCatalogRepository,
};
use sqlx::PgPool;

use crate::{PostgresRepositoryError, error::corrupt};

/// PostgreSQL repository for actor-scoped current external-identity queries.
#[derive(Clone, Debug)]
pub struct PostgresExternalIdentityCatalogRepository {
    pool: PgPool,
}

impl PostgresExternalIdentityCatalogRepository {
    /// Creates a catalogue repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ExternalIdentityCatalogRepository for PostgresExternalIdentityCatalogRepository {
    type Error = PostgresRepositoryError;

    async fn find_page(
        &self,
        query: &ExternalIdentityCatalogQuery,
    ) -> Result<ExternalIdentityCatalogPage, Self::Error> {
        let after_authority = query.after().map(|cursor| cursor.authority_id().as_str());
        let after_subject = query.after().map(|cursor| cursor.subject().as_str());
        let requested = i64::from(query.page_size().get());
        let fetch_limit = requested + 1;

        let rows = sqlx::query!(
            r#"
            SELECT authority_id, subject
            FROM identity.external_actor_identities
            WHERE actor_id = $1
              AND ($2::text IS NULL OR (authority_id, subject) > ($2, $3))
            ORDER BY authority_id ASC, subject ASC
            LIMIT $4
            "#,
            query.actor_id().as_str(),
            after_authority,
            after_subject,
            fetch_limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(PostgresRepositoryError::Database)?;

        let mut entries = rows
            .into_iter()
            .map(|row| {
                Ok(ExternalIdentityCatalogEntry::new(ExternalIdentity::new(
                    IdentityAuthorityId::new(row.authority_id).map_err(|error| {
                        corrupt("identity.external_actor_identities.authority_id", error)
                    })?,
                    ExternalIdentitySubject::new(row.subject).map_err(|error| {
                        corrupt("identity.external_actor_identities.subject", error)
                    })?,
                )))
            })
            .collect::<Result<Vec<_>, PostgresRepositoryError>>()?;

        let has_more = entries.len() > usize::from(query.page_size().get());
        if has_more {
            entries.pop();
        }
        let next = has_more.then(|| {
            let identity = entries
                .last()
                .expect("a page with an extra row retains at least one entry")
                .external_identity();
            ExternalIdentityCatalogCursor::new(
                identity.authority_id().clone(),
                identity.subject().clone(),
            )
        });

        Ok(ExternalIdentityCatalogPage::new(entries, next))
    }
}

/// Error returned by PostgreSQL current external-identity queries.
pub type PostgresExternalIdentityCatalogRepositoryError = PostgresRepositoryError;
