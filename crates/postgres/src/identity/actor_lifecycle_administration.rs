use std::{error::Error, fmt};

use fleetiq_domain::{ActorId, ActorLifecycleChangeId};
use fleetiq_identity::{
    ActorLifecycleAdministrationRepository, ActorLifecycleChange, ActorLifecycleChangeEffect,
    ActorLifecycleChangeKind, ActorLifecycleChangeOutcome,
};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic actor lifecycle changes and immutable audit.
#[derive(Clone, Debug)]
pub struct PostgresActorLifecycleAdministration {
    pool: PgPool,
}

impl PostgresActorLifecycleAdministration {
    /// Creates a repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl ActorLifecycleAdministrationRepository for PostgresActorLifecycleAdministration {
    type Error = PostgresActorLifecycleAdministrationError;

    /// Serializes actor lifecycle commands through the bootstrap singleton,
    /// changes current state, and appends audit evidence in one transaction.
    async fn apply(
        &self,
        change: &ActorLifecycleChange,
    ) -> Result<ActorLifecycleChangeOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        lock_identity_scope(&mut transaction).await?;

        if let Some(outcome) = resolve_existing(&mut transaction, change).await? {
            transaction.commit().await.map_err(repository_error)?;
            return Ok(outcome);
        }

        let state_changed = mutate_actor_state(&mut transaction, change).await?;
        sqlx::query!(
            r#"
            INSERT INTO identity.actor_lifecycle_changes
                (change_id, administrator_actor_id, target_actor_id,
                 change_kind, recorded_at_ms, state_changed)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            change.change_id().as_str(),
            change.administrator_actor_id().as_str(),
            change.target_actor_id().as_str(),
            kind_name(change.kind()),
            change.recorded_at().get(),
            state_changed,
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(ActorLifecycleChangeOutcome::Recorded(effect(state_changed)))
    }
}

async fn lock_identity_scope(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), PostgresActorLifecycleAdministrationError> {
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
    change: &ActorLifecycleChange,
) -> Result<Option<ActorLifecycleChangeOutcome>, PostgresActorLifecycleAdministrationError> {
    let Some(existing) = sqlx::query!(
        r#"
        SELECT administrator_actor_id, target_actor_id, change_kind,
               recorded_at_ms, state_changed
        FROM identity.actor_lifecycle_changes
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

    if existing.administrator_actor_id != change.administrator_actor_id().as_str()
        || existing.target_actor_id != change.target_actor_id().as_str()
        || existing.change_kind != kind_name(change.kind())
        || !change.accepts_recorded_at(fleetiq_domain::UnixTimeMillis::new(existing.recorded_at_ms))
    {
        return Err(
            PostgresActorLifecycleAdministrationError::IdentityCollision {
                change_id: change.change_id().clone(),
            },
        );
    }

    Ok(Some(ActorLifecycleChangeOutcome::AlreadyRecorded(effect(
        existing.state_changed,
    ))))
}

async fn mutate_actor_state(
    transaction: &mut Transaction<'_, Postgres>,
    change: &ActorLifecycleChange,
) -> Result<bool, PostgresActorLifecycleAdministrationError> {
    let current = sqlx::query_scalar!(
        r#"
        SELECT is_active
        FROM identity.actors
        WHERE actor_id = $1
        "#,
        change.target_actor_id().as_str(),
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(repository_error)?
    .ok_or_else(
        || PostgresActorLifecycleAdministrationError::ActorNotFound {
            actor_id: change.target_actor_id().clone(),
        },
    )?;

    let desired = change.kind() == ActorLifecycleChangeKind::Activate;
    if current == desired {
        return Ok(false);
    }

    if !desired {
        preserve_recovery_authority(transaction, change.target_actor_id()).await?;
    }

    sqlx::query!(
        r#"
        UPDATE identity.actors
        SET is_active = $2
        WHERE actor_id = $1
        "#,
        change.target_actor_id().as_str(),
        desired,
    )
    .execute(&mut **transaction)
    .await
    .map_err(repository_error)?;
    Ok(true)
}

async fn preserve_recovery_authority(
    transaction: &mut Transaction<'_, Postgres>,
    target_actor_id: &ActorId,
) -> Result<(), PostgresActorLifecycleAdministrationError> {
    let authority = sqlx::query!(
        r#"
        SELECT
            EXISTS (
                SELECT 1 FROM identity.platform_action_grants
                WHERE actor_id = $1 AND action = 'identity.manage'
            ) AS "target_manages_identity!",
            (
                SELECT COUNT(*) FROM identity.platform_action_grants AS grant_state
                INNER JOIN identity.actors AS actor_state
                    ON actor_state.actor_id = grant_state.actor_id
                WHERE grant_state.action = 'identity.manage'
                  AND actor_state.is_active
            ) AS "active_identity_managers!",
            EXISTS (
                SELECT 1 FROM identity.platform_action_grants
                WHERE actor_id = $1 AND action = 'platform.access.manage'
            ) AS "target_manages_platform_access!",
            (
                SELECT COUNT(*) FROM identity.platform_action_grants AS grant_state
                INNER JOIN identity.actors AS actor_state
                    ON actor_state.actor_id = grant_state.actor_id
                WHERE grant_state.action = 'platform.access.manage'
                  AND actor_state.is_active
            ) AS "active_platform_access_managers!"
        "#,
        target_actor_id.as_str(),
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(repository_error)?;

    if authority.target_manages_identity && authority.active_identity_managers == 1 {
        return Err(
            PostgresActorLifecycleAdministrationError::LastActiveIdentityAdministrator {
                actor_id: target_actor_id.clone(),
            },
        );
    }
    if authority.target_manages_platform_access && authority.active_platform_access_managers == 1 {
        return Err(
            PostgresActorLifecycleAdministrationError::LastActivePlatformAdministrator {
                actor_id: target_actor_id.clone(),
            },
        );
    }
    Ok(())
}

const fn kind_name(kind: ActorLifecycleChangeKind) -> &'static str {
    match kind {
        ActorLifecycleChangeKind::Activate => "activate",
        ActorLifecycleChangeKind::Deactivate => "deactivate",
    }
}

const fn effect(state_changed: bool) -> ActorLifecycleChangeEffect {
    if state_changed {
        ActorLifecycleChangeEffect::StateChanged
    } else {
        ActorLifecycleChangeEffect::NoStateChange
    }
}

fn repository_error(error: sqlx::Error) -> PostgresActorLifecycleAdministrationError {
    PostgresActorLifecycleAdministrationError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve an actor lifecycle administration command.
#[derive(Debug)]
pub enum PostgresActorLifecycleAdministrationError {
    /// A platform-scoped command identity already names different content.
    IdentityCollision {
        /// Reused command identity.
        change_id: ActorLifecycleChangeId,
    },
    /// The target actor does not exist.
    ActorNotFound {
        /// Missing stable actor identity.
        actor_id: ActorId,
    },
    /// Deactivation would remove the final active identity administrator.
    LastActiveIdentityAdministrator {
        /// Actor whose protected authority would become unusable.
        actor_id: ActorId,
    },
    /// Deactivation would remove the final active platform-access administrator.
    LastActivePlatformAdministrator {
        /// Actor whose protected authority would become unusable.
        actor_id: ActorId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresActorLifecycleAdministrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision { change_id } => {
                write!(
                    formatter,
                    "actor lifecycle change collision for {change_id}"
                )
            }
            Self::ActorNotFound { actor_id } => {
                write!(formatter, "actor {actor_id} does not exist")
            }
            Self::LastActiveIdentityAdministrator { actor_id } => write!(
                formatter,
                "cannot deactivate the final active identity administrator {actor_id}"
            ),
            Self::LastActivePlatformAdministrator { actor_id } => write!(
                formatter,
                "cannot deactivate the final active platform access administrator {actor_id}"
            ),
            Self::Repository(_) => {
                formatter.write_str("PostgreSQL actor lifecycle administration failed")
            }
        }
    }
}

impl Error for PostgresActorLifecycleAdministrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::IdentityCollision { .. }
            | Self::ActorNotFound { .. }
            | Self::LastActiveIdentityAdministrator { .. }
            | Self::LastActivePlatformAdministrator { .. } => None,
        }
    }
}
