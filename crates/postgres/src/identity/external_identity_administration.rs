use std::{error::Error, fmt};

use fleetiq_domain::{ActorId, ExternalIdentityChangeId};
use fleetiq_identity::{
    ExternalIdentityAdministrationRepository, ExternalIdentityChange, ExternalIdentityChangeEffect,
    ExternalIdentityChangeKind, ExternalIdentityChangeOutcome,
};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic external-identity changes and audit.
#[derive(Clone, Debug)]
pub struct PostgresExternalIdentityAdministration {
    pool: PgPool,
}

impl PostgresExternalIdentityAdministration {
    /// Creates a repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ExternalIdentityAdministrationRepository for PostgresExternalIdentityAdministration {
    type Error = PostgresExternalIdentityAdministrationError;

    /// Serializes platform identity changes, mutates the exact current mapping,
    /// and appends immutable audit evidence in one transaction. The global lock
    /// makes change identities deterministic across target actors and prevents
    /// concurrent detach commands from removing an actor's final identities.
    /// Resolve existing audit before mutation: delivery retries return the original
    /// effect and receipt time without reapplying an earlier mapping state.
    async fn apply(
        &self,
        change: &ExternalIdentityChange,
    ) -> Result<ExternalIdentityChangeOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        lock_identity_scope(&mut transaction).await?;

        if let Some(outcome) = resolve_existing(&mut transaction, change).await? {
            transaction.commit().await.map_err(repository_error)?;
            return Ok(outcome);
        }

        let state_changed = mutate_mapping(&mut transaction, change).await?;
        let identity = change.external_identity();

        sqlx::query!(
            r#"
            INSERT INTO identity.external_identity_changes
                (change_id, administrator_actor_id, target_actor_id,
                 authority_id, subject, change_kind, recorded_at_ms,
                 state_changed)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            change.change_id().as_str(),
            change.administrator_actor_id().as_str(),
            change.target_actor_id().as_str(),
            identity.authority_id().as_str(),
            identity.subject().as_str(),
            kind_name(change.kind()),
            change.recorded_at().get(),
            state_changed,
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(ExternalIdentityChangeOutcome::Recorded(effect(
            state_changed,
        )))
    }
}

async fn lock_identity_scope(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), PostgresExternalIdentityAdministrationError> {
    sqlx::query!(
        r#"
        SELECT singleton
        FROM identity.platform_bootstrap
        WHERE singleton = TRUE
        FOR UPDATE
        "#,
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(repository_error)?;
    Ok(())
}

async fn resolve_existing(
    transaction: &mut Transaction<'_, Postgres>,
    change: &ExternalIdentityChange,
) -> Result<Option<ExternalIdentityChangeOutcome>, PostgresExternalIdentityAdministrationError> {
    let Some(existing) = sqlx::query!(
        r#"
        SELECT administrator_actor_id, target_actor_id, authority_id, subject,
               change_kind, recorded_at_ms, state_changed
        FROM identity.external_identity_changes
        WHERE change_id = $1
        "#,
        change.change_id().as_str(),
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(repository_error)?
    else {
        return Ok(None);
    };

    let identity = change.external_identity();
    if existing.administrator_actor_id != change.administrator_actor_id().as_str()
        || existing.target_actor_id != change.target_actor_id().as_str()
        || existing.authority_id != identity.authority_id().as_str()
        || existing.subject != identity.subject().as_str()
        || existing.change_kind != kind_name(change.kind())
        || !change.accepts_recorded_at(fleetiq_domain::UnixTimeMillis::new(existing.recorded_at_ms))
    {
        return Err(
            PostgresExternalIdentityAdministrationError::IdentityCollision {
                change_id: change.change_id().clone(),
            },
        );
    }

    Ok(Some(ExternalIdentityChangeOutcome::AlreadyRecorded(
        effect(existing.state_changed),
    )))
}

async fn mutate_mapping(
    transaction: &mut Transaction<'_, Postgres>,
    change: &ExternalIdentityChange,
) -> Result<bool, PostgresExternalIdentityAdministrationError> {
    match change.kind() {
        ExternalIdentityChangeKind::Attach => attach_mapping(transaction, change).await,
        ExternalIdentityChangeKind::Detach => detach_mapping(transaction, change).await,
    }
}

async fn attach_mapping(
    transaction: &mut Transaction<'_, Postgres>,
    change: &ExternalIdentityChange,
) -> Result<bool, PostgresExternalIdentityAdministrationError> {
    let identity = change.external_identity();
    let result = sqlx::query!(
        r#"
        INSERT INTO identity.external_actor_identities
            (authority_id, subject, actor_id)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
        identity.authority_id().as_str(),
        identity.subject().as_str(),
        change.target_actor_id().as_str(),
    )
    .execute(&mut **transaction)
    .await
    .map_err(repository_error)?;

    if result.rows_affected() == 1 {
        return Ok(true);
    }

    let assigned_actor = find_assigned_actor(transaction, change).await?;
    if assigned_actor.as_deref() == Some(change.target_actor_id().as_str()) {
        Ok(false)
    } else {
        Err(PostgresExternalIdentityAdministrationError::IdentityAssigned)
    }
}

async fn detach_mapping(
    transaction: &mut Transaction<'_, Postgres>,
    change: &ExternalIdentityChange,
) -> Result<bool, PostgresExternalIdentityAdministrationError> {
    let assigned_actor = find_assigned_actor(transaction, change).await?;
    match assigned_actor.as_deref() {
        None => return Ok(false),
        Some(actor_id) if actor_id != change.target_actor_id().as_str() => {
            return Err(PostgresExternalIdentityAdministrationError::IdentityAssigned);
        }
        Some(_) => {}
    }

    let identity = change.external_identity();
    sqlx::query!(
        r#"
        DELETE FROM identity.external_actor_identities
        WHERE authority_id = $1 AND subject = $2 AND actor_id = $3
        "#,
        identity.authority_id().as_str(),
        identity.subject().as_str(),
        change.target_actor_id().as_str(),
    )
    .execute(&mut **transaction)
    .await
    .map_err(repository_error)?;

    preserve_final_identity(transaction, change.target_actor_id()).await?;
    Ok(true)
}

async fn find_assigned_actor(
    transaction: &mut Transaction<'_, Postgres>,
    change: &ExternalIdentityChange,
) -> Result<Option<String>, PostgresExternalIdentityAdministrationError> {
    let identity = change.external_identity();
    sqlx::query_scalar!(
        r#"
        SELECT actor_id
        FROM identity.external_actor_identities
        WHERE authority_id = $1 AND subject = $2
        "#,
        identity.authority_id().as_str(),
        identity.subject().as_str(),
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(repository_error)
}

async fn preserve_final_identity(
    transaction: &mut Transaction<'_, Postgres>,
    target_actor_id: &ActorId,
) -> Result<(), PostgresExternalIdentityAdministrationError> {
    let remaining_identities = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM identity.external_actor_identities
        WHERE actor_id = $1
        "#,
        target_actor_id.as_str(),
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(repository_error)?;

    if remaining_identities == 0 {
        return Err(
            PostgresExternalIdentityAdministrationError::LastExternalIdentity {
                actor_id: target_actor_id.clone(),
            },
        );
    }
    Ok(())
}

const fn kind_name(kind: ExternalIdentityChangeKind) -> &'static str {
    match kind {
        ExternalIdentityChangeKind::Attach => "attach",
        ExternalIdentityChangeKind::Detach => "detach",
    }
}

const fn effect(state_changed: bool) -> ExternalIdentityChangeEffect {
    if state_changed {
        ExternalIdentityChangeEffect::StateChanged
    } else {
        ExternalIdentityChangeEffect::NoStateChange
    }
}

fn repository_error(error: sqlx::Error) -> PostgresExternalIdentityAdministrationError {
    PostgresExternalIdentityAdministrationError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve an external-identity administration command.
#[derive(Debug)]
pub enum PostgresExternalIdentityAdministrationError {
    /// A platform-scoped command identity already names different content.
    IdentityCollision {
        /// Reused command identity.
        change_id: ExternalIdentityChangeId,
    },
    /// The exact external identity is assigned to a different actor.
    IdentityAssigned,
    /// Detachment would remove the target actor's final external identity.
    LastExternalIdentity {
        /// Actor whose final mapping would have been removed.
        actor_id: ActorId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresExternalIdentityAdministrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision { change_id } => write!(
                formatter,
                "external identity change collision for {change_id}"
            ),
            Self::IdentityAssigned => {
                formatter.write_str("external identity is assigned to another actor")
            }
            Self::LastExternalIdentity { actor_id } => write!(
                formatter,
                "cannot detach the final external identity from actor {actor_id}"
            ),
            Self::Repository(_) => {
                formatter.write_str("PostgreSQL external identity administration failed")
            }
        }
    }
}

impl Error for PostgresExternalIdentityAdministrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. }
            | Self::IdentityAssigned
            | Self::LastExternalIdentity { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}
