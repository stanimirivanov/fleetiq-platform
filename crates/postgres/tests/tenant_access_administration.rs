//! Integration tests for authorized, audited tenant grant administration.

use std::error::Error;

use fleetiq_application::{
    TenantAccessAdministration, TenantAccessAdministrationError, TenantAccessChangeEffect,
    TenantAccessChangeKind, TenantAccessChangeOutcome, TenantAccessChangeRequest, TenantAction,
    TenantAuthorizationService,
};
use fleetiq_domain::{ActorId, TenantAccessChangeId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{
    PostgresTenantAccessAdministration, PostgresTenantAccessAdministrationError,
    PostgresTenantAccessPolicy,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grant_revoke_and_exact_retry_are_atomic_and_audited(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        TenantAccessAdministration::new(PostgresTenantAccessAdministration::new(pool.clone()));

    let grant = request(
        "change-grant",
        "actor-target",
        TenantAction::ReadMetadata,
        TenantAccessChangeKind::Grant,
        100,
    );
    assert_eq!(
        administration
            .apply(manage_access(&pool).await?, grant.clone())
            .await?,
        TenantAccessChangeOutcome::Recorded(TenantAccessChangeEffect::StateChanged)
    );
    assert_eq!(
        administration
            .apply(manage_access(&pool).await?, grant)
            .await?,
        TenantAccessChangeOutcome::AlreadyRecorded(TenantAccessChangeEffect::StateChanged)
    );

    assert_eq!(
        administration
            .apply(
                manage_access(&pool).await?,
                request(
                    "change-grant-again",
                    "actor-target",
                    TenantAction::ReadMetadata,
                    TenantAccessChangeKind::Grant,
                    101,
                ),
            )
            .await?,
        TenantAccessChangeOutcome::Recorded(TenantAccessChangeEffect::NoStateChange)
    );

    assert_eq!(
        administration
            .apply(
                manage_access(&pool).await?,
                request(
                    "change-revoke",
                    "actor-target",
                    TenantAction::ReadMetadata,
                    TenantAccessChangeKind::Revoke,
                    102,
                ),
            )
            .await?,
        TenantAccessChangeOutcome::Recorded(TenantAccessChangeEffect::StateChanged)
    );

    let grant_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.tenant_action_grants \
         WHERE tenant_id = 'tenant-a' AND actor_id = 'actor-target' \
           AND action = 'metadata.read'",
    )
    .fetch_one(&pool)
    .await?;
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.tenant_access_changes")
            .fetch_one(&pool)
            .await?;
    assert_eq!(grant_count, 0);
    assert_eq!(audit_count, 3);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn non_administrative_authorization_cannot_mutate_or_audit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    seed_grant(&pool, "actor-admin", "metadata.read").await?;
    let read_authorization =
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("actor-admin").await,
                TenantId::new("tenant-a")?,
                TenantAction::ReadMetadata,
            )
            .await?;
    let administration =
        TenantAccessAdministration::new(PostgresTenantAccessAdministration::new(pool.clone()));

    let error = administration
        .apply(
            read_authorization,
            request(
                "unauthorized-change",
                "actor-target",
                TenantAction::ReadMetadata,
                TenantAccessChangeKind::Grant,
                100,
            ),
        )
        .await
        .expect_err("read authorization must not administer access");

    assert!(matches!(
        error,
        TenantAccessAdministrationError::ManageAccessRequired {
            actual: TenantAction::ReadMetadata
        }
    ));
    assert_eq!(audit_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reused_change_identity_with_different_content_is_a_collision(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    seed_actor(&pool, "actor-other").await?;
    let administration =
        TenantAccessAdministration::new(PostgresTenantAccessAdministration::new(pool.clone()));

    administration
        .apply(
            manage_access(&pool).await?,
            request(
                "change-a",
                "actor-target",
                TenantAction::ReadMetadata,
                TenantAccessChangeKind::Grant,
                100,
            ),
        )
        .await?;

    let error = administration
        .apply(
            manage_access(&pool).await?,
            request(
                "change-a",
                "actor-other",
                TenantAction::ReadMetadata,
                TenantAccessChangeKind::Grant,
                100,
            ),
        )
        .await
        .expect_err("one command identity must not name different content");

    assert!(matches!(
        error,
        TenantAccessAdministrationError::Repository(
            PostgresTenantAccessAdministrationError::IdentityCollision {
                ref tenant_id,
                ref change_id,
            }
        ) if tenant_id.as_str() == "tenant-a" && change_id.as_str() == "change-a"
    ));
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_mutation_leaves_no_audit_and_audit_rows_are_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration =
        TenantAccessAdministration::new(PostgresTenantAccessAdministration::new(pool.clone()));

    let error = administration
        .apply(
            manage_access(&pool).await?,
            request(
                "missing-target",
                "actor-missing",
                TenantAction::ReadMetadata,
                TenantAccessChangeKind::Grant,
                100,
            ),
        )
        .await
        .expect_err("missing target actor must fail atomically");
    assert!(matches!(
        error,
        TenantAccessAdministrationError::Repository(
            PostgresTenantAccessAdministrationError::Repository(_)
        )
    ));
    assert_eq!(audit_count(&pool).await?, 0);

    administration
        .apply(
            manage_access(&pool).await?,
            request(
                "immutable-change",
                "actor-target",
                TenantAction::ReadMetadata,
                TenantAccessChangeKind::Grant,
                101,
            ),
        )
        .await?;
    for statement in [
        "UPDATE identity.tenant_access_changes SET state_changed = false",
        "DELETE FROM identity.tenant_access_changes",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("audit evidence must be immutable");
        assert!(error.as_database_error().is_some());
    }
    Ok(())
}

async fn seed(pool: &PgPool) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(pool)
        .await?;
    seed_actor(pool, "actor-admin").await?;
    seed_actor(pool, "actor-target").await?;
    seed_grant(pool, "actor-admin", "access.manage").await
}

async fn seed_actor(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(actor_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_grant(pool: &PgPool, actor_id: &str, action: &str) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) \
         VALUES ('tenant-a', $1, $2)",
    )
    .bind(actor_id)
    .bind(action)
    .execute(pool)
    .await?;
    Ok(())
}

async fn manage_access(
    pool: &PgPool,
) -> Result<fleetiq_application::AuthorizedTenantAction, Box<dyn Error>> {
    Ok(
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("actor-admin").await,
                TenantId::new("tenant-a")?,
                TenantAction::ManageAccess,
            )
            .await?,
    )
}

fn request(
    change_id: &str,
    target_actor_id: &str,
    action: TenantAction,
    kind: TenantAccessChangeKind,
    recorded_at_ms: i64,
) -> TenantAccessChangeRequest {
    TenantAccessChangeRequest::new(
        TenantAccessChangeId::new(change_id).expect("test change ID is valid"),
        ActorId::new(target_actor_id).expect("test target actor ID is valid"),
        action,
        kind,
        UnixTimeMillis::new(recorded_at_ms),
    )
}

async fn audit_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM identity.tenant_access_changes")
        .fetch_one(pool)
        .await
}
