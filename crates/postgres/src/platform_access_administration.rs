use std::{error::Error, fmt};

use fleetiq_application::{
    PlatformAccessAdministrationRepository, PlatformAccessChange, PlatformAccessChangeEffect,
    PlatformAccessChangeKind, PlatformAccessChangeOutcome, PlatformAction,
};
use fleetiq_domain::{ActorId, PlatformAccessChangeId};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic platform grant changes and immutable audit.
#[derive(Clone, Debug)]
pub struct PostgresPlatformAccessAdministration {
    pool: PgPool,
}

impl PostgresPlatformAccessAdministration {
    /// Creates a repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl PlatformAccessAdministrationRepository for PostgresPlatformAccessAdministration {
    type Error = PostgresPlatformAccessAdministrationError;

    /// Serializes platform changes through the immutable bootstrap singleton,
    /// mutates current grants, and appends audit evidence in one transaction.
    /// The global lock also makes a platform-scoped command identity safe when
    /// concurrent requests target different actors.
    async fn apply(
        &self,
        change: &PlatformAccessChange,
    ) -> Result<PlatformAccessChangeOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        lock_platform_scope(&mut transaction).await?;

        if let Some(outcome) = resolve_existing(&mut transaction, change).await? {
            transaction.commit().await.map_err(repository_error)?;
            return Ok(outcome);
        }

        let state_changed = mutate_current_grant(&mut transaction, change).await?;

        sqlx::query!(
            r#"
            INSERT INTO identity.platform_access_changes
                (change_id, administrator_actor_id, target_actor_id, action,
                 change_kind, recorded_at_ms, state_changed)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            change.change_id().as_str(),
            change.administrator_actor_id().as_str(),
            change.target_actor_id().as_str(),
            action_name(change.action()),
            kind_name(change.kind()),
            change.recorded_at().get(),
            state_changed,
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(PlatformAccessChangeOutcome::Recorded(effect(state_changed)))
    }
}

async fn lock_platform_scope(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), PostgresPlatformAccessAdministrationError> {
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
    change: &PlatformAccessChange,
) -> Result<Option<PlatformAccessChangeOutcome>, PostgresPlatformAccessAdministrationError> {
    let Some(existing) = sqlx::query!(
        r#"
        SELECT administrator_actor_id, target_actor_id, action, change_kind,
               recorded_at_ms, state_changed
        FROM identity.platform_access_changes
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
        || existing.action != action_name(change.action())
        || existing.change_kind != kind_name(change.kind())
        || existing.recorded_at_ms != change.recorded_at().get()
    {
        return Err(
            PostgresPlatformAccessAdministrationError::IdentityCollision {
                change_id: change.change_id().clone(),
            },
        );
    }

    Ok(Some(PlatformAccessChangeOutcome::AlreadyRecorded(effect(
        existing.state_changed,
    ))))
}

async fn mutate_current_grant(
    transaction: &mut Transaction<'_, Postgres>,
    change: &PlatformAccessChange,
) -> Result<bool, PostgresPlatformAccessAdministrationError> {
    let result = match change.kind() {
        PlatformAccessChangeKind::Grant => {
            sqlx::query!(
                r#"
                INSERT INTO identity.platform_action_grants (actor_id, action)
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
                "#,
                change.target_actor_id().as_str(),
                action_name(change.action()),
            )
            .execute(&mut **transaction)
            .await
        }
        PlatformAccessChangeKind::Revoke => {
            sqlx::query!(
                r#"
                DELETE FROM identity.platform_action_grants
                WHERE actor_id = $1 AND action = $2
                "#,
                change.target_actor_id().as_str(),
                action_name(change.action()),
            )
            .execute(&mut **transaction)
            .await
        }
    }
    .map_err(repository_error)?;

    let state_changed = result.rows_affected() == 1;
    if state_changed
        && change.kind() == PlatformAccessChangeKind::Revoke
        && change.action() == PlatformAction::ManagePlatformAccess
    {
        preserve_final_administrator(transaction, change.target_actor_id()).await?;
    }
    Ok(state_changed)
}

async fn preserve_final_administrator(
    transaction: &mut Transaction<'_, Postgres>,
    target_actor_id: &ActorId,
) -> Result<(), PostgresPlatformAccessAdministrationError> {
    let remaining_administrators = sqlx::query_scalar!(
        r#"
        SELECT COUNT(*) AS "count!"
        FROM identity.platform_action_grants
        WHERE action = 'platform.access.manage'
        "#,
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(repository_error)?;

    if remaining_administrators == 0 {
        return Err(
            PostgresPlatformAccessAdministrationError::LastPlatformAdministrator {
                actor_id: target_actor_id.clone(),
            },
        );
    }
    Ok(())
}

const fn action_name(action: PlatformAction) -> &'static str {
    match action {
        PlatformAction::ManagePlatformAccess => "platform.access.manage",
        PlatformAction::ManageIdentity => "identity.manage",
        PlatformAction::ProvisionTenant => "tenant.provision",
    }
}

const fn kind_name(kind: PlatformAccessChangeKind) -> &'static str {
    match kind {
        PlatformAccessChangeKind::Grant => "grant",
        PlatformAccessChangeKind::Revoke => "revoke",
    }
}

const fn effect(state_changed: bool) -> PlatformAccessChangeEffect {
    if state_changed {
        PlatformAccessChangeEffect::StateChanged
    } else {
        PlatformAccessChangeEffect::NoStateChange
    }
}

fn repository_error(error: sqlx::Error) -> PostgresPlatformAccessAdministrationError {
    PostgresPlatformAccessAdministrationError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve a platform access administration command.
#[derive(Debug)]
pub enum PostgresPlatformAccessAdministrationError {
    /// A platform-scoped command identity already names different content.
    IdentityCollision {
        /// Reused command identity.
        change_id: PlatformAccessChangeId,
    },
    /// Revocation would remove the final platform access administrator.
    LastPlatformAdministrator {
        /// Actor whose protected grant would have been removed.
        actor_id: ActorId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresPlatformAccessAdministrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision { change_id } => {
                write!(
                    formatter,
                    "platform access change identity collision for {change_id}"
                )
            }
            Self::LastPlatformAdministrator { actor_id } => write!(
                formatter,
                "cannot revoke the final platform access administrator {actor_id}"
            ),
            Self::Repository(_) => {
                formatter.write_str("PostgreSQL platform access administration failed")
            }
        }
    }
}

impl Error for PostgresPlatformAccessAdministrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::IdentityCollision { .. } | Self::LastPlatformAdministrator { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}
