use std::{error::Error, fmt};

use fleetiq_domain::{TenantId, TenantLifecycleChangeId, UnixTimeMillis};
use fleetiq_identity::{
    TenantLifecycleAdministrationRepository, TenantLifecycleChange, TenantLifecycleChangeEffect,
    TenantLifecycleChangeKind, TenantLifecycleChangeOutcome,
};
use sqlx::{PgPool, Postgres, Transaction};

use crate::{PostgresRepositoryError, error::classify_database_error};

/// PostgreSQL repository for atomic tenant lifecycle changes and immutable audit.
#[derive(Clone, Debug)]
pub struct PostgresTenantLifecycleAdministration {
    pool: PgPool,
}

impl PostgresTenantLifecycleAdministration {
    /// Creates a repository using the supplied PostgreSQL pool.
    pub const fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl TenantLifecycleAdministrationRepository for PostgresTenantLifecycleAdministration {
    type Error = PostgresTenantLifecycleAdministrationError;

    /// Serializes platform tenant lifecycle commands, changes current state,
    /// and appends immutable audit evidence in one transaction.
    async fn apply(
        &self,
        change: &TenantLifecycleChange,
    ) -> Result<TenantLifecycleChangeOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(repository_error)?;
        lock_platform_scope(&mut transaction).await?;

        if let Some(outcome) = resolve_existing(&mut transaction, change).await? {
            transaction.commit().await.map_err(repository_error)?;
            return Ok(outcome);
        }

        let state_changed = mutate_tenant_state(&mut transaction, change).await?;
        sqlx::query!(
            r#"
            INSERT INTO identity.tenant_lifecycle_changes
                (change_id, administrator_actor_id, tenant_id,
                 change_kind, recorded_at_ms, state_changed)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            change.change_id().as_str(),
            change.administrator_actor_id().as_str(),
            change.tenant_id().as_str(),
            kind_name(change.kind()),
            change.recorded_at().get(),
            state_changed,
        )
        .execute(&mut *transaction)
        .await
        .map_err(repository_error)?;

        transaction.commit().await.map_err(repository_error)?;
        Ok(TenantLifecycleChangeOutcome::Recorded(effect(
            state_changed,
        )))
    }
}

async fn lock_platform_scope(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), PostgresTenantLifecycleAdministrationError> {
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
    change: &TenantLifecycleChange,
) -> Result<Option<TenantLifecycleChangeOutcome>, PostgresTenantLifecycleAdministrationError> {
    let Some(existing) = sqlx::query!(
        r#"
        SELECT administrator_actor_id, tenant_id, change_kind,
               recorded_at_ms, state_changed
        FROM identity.tenant_lifecycle_changes
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
        || existing.tenant_id != change.tenant_id().as_str()
        || existing.change_kind != kind_name(change.kind())
        || !change.accepts_recorded_at(UnixTimeMillis::new(existing.recorded_at_ms))
    {
        return Err(
            PostgresTenantLifecycleAdministrationError::IdentityCollision {
                change_id: change.change_id().clone(),
            },
        );
    }

    Ok(Some(TenantLifecycleChangeOutcome::AlreadyRecorded(effect(
        existing.state_changed,
    ))))
}

async fn mutate_tenant_state(
    transaction: &mut Transaction<'_, Postgres>,
    change: &TenantLifecycleChange,
) -> Result<bool, PostgresTenantLifecycleAdministrationError> {
    let current = sqlx::query_scalar!(
        r#"
        SELECT is_active
        FROM metadata.tenants
        WHERE tenant_id = $1
        "#,
        change.tenant_id().as_str(),
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(repository_error)?
    .ok_or_else(
        || PostgresTenantLifecycleAdministrationError::TenantNotFound {
            tenant_id: change.tenant_id().clone(),
        },
    )?;

    let desired = change.kind() == TenantLifecycleChangeKind::Activate;
    if current == desired {
        return Ok(false);
    }

    sqlx::query!(
        r#"
        UPDATE metadata.tenants
        SET is_active = $2
        WHERE tenant_id = $1
        "#,
        change.tenant_id().as_str(),
        desired,
    )
    .execute(&mut **transaction)
    .await
    .map_err(repository_error)?;
    Ok(true)
}

const fn kind_name(kind: TenantLifecycleChangeKind) -> &'static str {
    match kind {
        TenantLifecycleChangeKind::Activate => "activate",
        TenantLifecycleChangeKind::Suspend => "suspend",
    }
}

const fn effect(state_changed: bool) -> TenantLifecycleChangeEffect {
    if state_changed {
        TenantLifecycleChangeEffect::StateChanged
    } else {
        TenantLifecycleChangeEffect::NoStateChange
    }
}

fn repository_error(error: sqlx::Error) -> PostgresTenantLifecycleAdministrationError {
    PostgresTenantLifecycleAdministrationError::Repository(classify_database_error(error))
}

/// Failure to persist or resolve a tenant lifecycle administration command.
#[derive(Debug)]
pub enum PostgresTenantLifecycleAdministrationError {
    /// A platform-scoped command identity already names different content.
    IdentityCollision {
        /// Reused command identity.
        change_id: TenantLifecycleChangeId,
    },
    /// The target tenant does not exist.
    TenantNotFound {
        /// Missing stable tenant identity.
        tenant_id: TenantId,
    },
    /// Lower-level PostgreSQL repository failure.
    Repository(PostgresRepositoryError),
}

impl fmt::Display for PostgresTenantLifecycleAdministrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityCollision { change_id } => {
                write!(
                    formatter,
                    "tenant lifecycle change collision for {change_id}"
                )
            }
            Self::TenantNotFound { tenant_id } => {
                write!(formatter, "tenant {tenant_id} does not exist")
            }
            Self::Repository(_) => {
                formatter.write_str("PostgreSQL tenant lifecycle administration failed")
            }
        }
    }
}

impl Error for PostgresTenantLifecycleAdministrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
            Self::IdentityCollision { .. } | Self::TenantNotFound { .. } => None,
        }
    }
}
