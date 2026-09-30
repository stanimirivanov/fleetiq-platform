//! Integration tests for persisted actors and exact tenant action grants.

use std::error::Error;

use fleetiq_application::{
    ManageAccess, ManageMetadata, ReadMetadata, TenantAction, TenantActionMarker,
    TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_postgres::PostgresTenantAccessPolicy;
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_grant_authorizes_only_its_tuple_and_revocation_is_immediate(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    seed_tenant(&pool, "tenant-b").await?;
    seed_actor(&pool, "actor-a").await?;
    seed_actor(&pool, "actor-b").await?;
    seed_grant(&pool, "tenant-a", "actor-a", "metadata.read").await?;

    let policy = PostgresTenantAccessPolicy::new(pool.clone());
    let service = TenantAuthorizationService::new(policy.clone());
    let authorization = service
        .authorize::<ReadMetadata>(
            support::authenticated_actor("actor-a").await,
            tenant_id("tenant-a"),
        )
        .await?;

    assert_eq!(authorization.actor_id().as_str(), "actor-a");
    assert_eq!(authorization.tenant_id().as_str(), "tenant-a");
    assert_eq!(authorization.action(), TenantAction::ReadMetadata);

    for (actor, tenant) in [("actor-b", "tenant-a"), ("actor-a", "tenant-b")] {
        let error = service
            .authorize::<ReadMetadata>(support::authenticated_actor(actor).await, tenant_id(tenant))
            .await
            .expect_err("an unmatched tuple must be denied");
        assert!(matches!(error, TenantAuthorizationError::Denied));
    }
    let wrong_action = service
        .authorize::<ManageMetadata>(
            support::authenticated_actor("actor-a").await,
            tenant_id("tenant-a"),
        )
        .await
        .expect_err("a read grant must not authorize metadata management");
    assert!(matches!(wrong_action, TenantAuthorizationError::Denied));

    sqlx::query(
        "DELETE FROM identity.tenant_action_grants \
         WHERE tenant_id = $1 AND actor_id = $2 AND action = $3",
    )
    .bind("tenant-a")
    .bind("actor-a")
    .bind("metadata.read")
    .execute(&pool)
    .await?;

    let error = service
        .authorize::<ReadMetadata>(
            support::authenticated_actor("actor-a").await,
            tenant_id("tenant-a"),
        )
        .await
        .expect_err("a deleted grant must stop authorizing the next operation");
    assert!(matches!(error, TenantAuthorizationError::Denied));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn every_access_action_matches_the_persisted_vocabulary(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    seed_actor(&pool, "actor-a").await?;

    for action in ["metadata.read", "metadata.manage", "access.manage"] {
        seed_grant(&pool, "tenant-a", "actor-a", action).await?;
    }

    let service = TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool));
    assert_action::<ReadMetadata>(&service).await?;
    assert_action::<ManageMetadata>(&service).await?;
    assert_action::<ManageAccess>(&service).await?;
    Ok(())
}

async fn assert_action<A: TenantActionMarker>(
    service: &TenantAuthorizationService<PostgresTenantAccessPolicy>,
) -> Result<(), Box<dyn Error>> {
    let authorization = service
        .authorize::<A>(
            support::authenticated_actor("actor-a").await,
            tenant_id("tenant-a"),
        )
        .await?;
    assert_eq!(authorization.action(), A::ACTION);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn grants_require_valid_existing_actors_tenants_and_actions(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant(&pool, "tenant-a").await?;
    seed_actor(&pool, "actor-a").await?;

    let invalid_actor = sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(" actor-b")
        .execute(&pool)
        .await
        .expect_err("actor IDs must satisfy the shared identifier invariant");
    assert_constraint(&invalid_actor, "ck_actors_id");

    let missing_tenant = seed_grant(&pool, "tenant-missing", "actor-a", "metadata.read")
        .await
        .expect_err("grants must reference an existing tenant");
    assert_constraint(&missing_tenant, "fk_tenant_action_grants_tenant");

    let missing_actor = seed_grant(&pool, "tenant-a", "actor-missing", "metadata.read")
        .await
        .expect_err("grants must reference an existing actor");
    assert_constraint(&missing_actor, "fk_tenant_action_grants_actor");

    let unknown_action = seed_grant(&pool, "tenant-a", "actor-a", "metadata.delete")
        .await
        .expect_err("grants must use the closed action vocabulary");
    assert_constraint(&unknown_action, "ck_tenant_action_grants_action");

    seed_grant(&pool, "tenant-a", "actor-a", "metadata.read").await?;
    let duplicate = seed_grant(&pool, "tenant-a", "actor-a", "metadata.read")
        .await
        .expect_err("the same exact grant must not be stored twice");
    assert_constraint(&duplicate, "pk_tenant_action_grants");
    Ok(())
}

async fn seed_tenant(pool: &PgPool, tenant_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_actor(pool: &PgPool, actor_id: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind(actor_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn seed_grant(
    pool: &PgPool,
    tenant_id: &str,
    actor_id: &str,
    action: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) \
         VALUES ($1, $2, $3)",
    )
    .bind(tenant_id)
    .bind(actor_id)
    .bind(action)
    .execute(pool)
    .await?;
    Ok(())
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant ID is valid")
}

fn assert_constraint(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some(expected)
    );
}
