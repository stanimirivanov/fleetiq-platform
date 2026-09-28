//! Integration tests for persisted exact platform action grants.

use std::error::Error;

use fleetiq_application::{
    PlatformAction, PlatformAuthorizationError, PlatformAuthorizationService,
};
use fleetiq_postgres::PostgresPlatformAccessPolicy;
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_grant_authorizes_only_its_actor_and_revocation_is_immediate(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "actor-a").await?;
    seed_actor(&pool, "actor-b").await?;
    seed_grant(&pool, "actor-a", "tenant.provision").await?;

    let service =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()));
    let authorization = service
        .authorize(
            support::authenticated_actor("actor-a").await,
            PlatformAction::ProvisionTenant,
        )
        .await?;

    assert_eq!(authorization.actor_id().as_str(), "actor-a");
    assert_eq!(authorization.action(), PlatformAction::ProvisionTenant);

    let error = service
        .authorize(
            support::authenticated_actor("actor-a").await,
            PlatformAction::ManageIdentity,
        )
        .await
        .expect_err("one platform action must not imply another");
    assert!(matches!(error, PlatformAuthorizationError::Denied));

    let error = service
        .authorize(
            support::authenticated_actor("actor-a").await,
            PlatformAction::ManagePlatformAccess,
        )
        .await
        .expect_err("one platform action must not imply grant administration");
    assert!(matches!(error, PlatformAuthorizationError::Denied));

    let error = service
        .authorize(
            support::authenticated_actor("actor-b").await,
            PlatformAction::ProvisionTenant,
        )
        .await
        .expect_err("another actor must be denied");
    assert!(matches!(error, PlatformAuthorizationError::Denied));

    sqlx::query("DELETE FROM identity.platform_action_grants WHERE actor_id = $1 AND action = $2")
        .bind("actor-a")
        .bind("tenant.provision")
        .execute(&pool)
        .await?;

    let error = service
        .authorize(
            support::authenticated_actor("actor-a").await,
            PlatformAction::ProvisionTenant,
        )
        .await
        .expect_err("revocation must affect the next policy decision");
    assert!(matches!(error, PlatformAuthorizationError::Denied));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grants_require_existing_actors_exact_actions_and_unique_tuples(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_actor(&pool, "actor-a").await?;

    let missing_actor = seed_grant(&pool, "actor-missing", "tenant.provision")
        .await
        .expect_err("a platform grant must reference an existing actor");
    assert_constraint(&missing_actor, "fk_platform_action_grants_actor");

    seed_grant(&pool, "actor-a", "identity.manage").await?;
    seed_grant(&pool, "actor-a", "platform.access.manage").await?;
    seed_grant(&pool, "actor-a", "tenant.lifecycle.manage").await?;
    let unknown_action = seed_grant(&pool, "actor-a", "identity.delete")
        .await
        .expect_err("a platform grant must use the closed action vocabulary");
    assert_constraint(&unknown_action, "ck_platform_action_grants_action");

    seed_grant(&pool, "actor-a", "tenant.provision").await?;
    let duplicate = seed_grant(&pool, "actor-a", "tenant.provision")
        .await
        .expect_err("the same exact platform grant must not be stored twice");
    assert_constraint(&duplicate, "pk_platform_action_grants");
    Ok(())
}

async fn seed_actor(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(actor_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_grant(pool: &PgPool, actor_id: &str, action: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.platform_action_grants (actor_id, action) VALUES ($1, $2)")
        .bind(actor_id)
        .bind(action)
        .execute(pool)
        .await?;
    Ok(())
}

fn assert_constraint(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some(expected)
    );
}
