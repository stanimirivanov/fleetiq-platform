//! Integration tests for authorized, audited tenant lifecycle administration.

use std::error::Error;

use fleetiq_application::{
    ManageTenantLifecycle, PlatformAuthorizationService, PlatformBootstrapRequest,
    PlatformBootstrapService, ReadMetadata, TenantAuthorizationError, TenantAuthorizationService,
    TenantLifecycleAdministration, TenantLifecycleAdministrationError, TenantLifecycleChangeEffect,
    TenantLifecycleChangeKind, TenantLifecycleChangeOutcome, TenantLifecycleChangeRequest,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, TenantId,
    TenantLifecycleChangeId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresPlatformAccessPolicy, PostgresPlatformBootstrap, PostgresTenantAccessPolicy,
    PostgresTenantLifecycleAdministration, PostgresTenantLifecycleAdministrationError,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn suspend_and_activate_control_fresh_tenant_authorization(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration = TenantLifecycleAdministration::new(
        PostgresTenantLifecycleAdministration::new(pool.clone()),
    );
    let suspend = request(
        "suspend-tenant",
        "tenant-a",
        TenantLifecycleChangeKind::Suspend,
        1_000,
    );

    assert_eq!(
        administration
            .apply(manage_tenant_lifecycle(&pool).await?, suspend.clone())
            .await?,
        TenantLifecycleChangeOutcome::Recorded(TenantLifecycleChangeEffect::StateChanged)
    );
    assert_eq!(
        administration
            .apply(manage_tenant_lifecycle(&pool).await?, suspend)
            .await?,
        TenantLifecycleChangeOutcome::AlreadyRecorded(TenantLifecycleChangeEffect::StateChanged)
    );

    let denied = tenant_authorization(&pool)
        .await
        .expect_err("a suspended tenant must fail current policy evaluation");
    assert!(matches!(denied, TenantAuthorizationError::Denied));
    assert_eq!(tenant_count(&pool).await?, 1);
    assert_eq!(tenant_grant_count(&pool).await?, 1);

    assert_eq!(
        administration
            .apply(
                manage_tenant_lifecycle(&pool).await?,
                request(
                    "activate-tenant",
                    "tenant-a",
                    TenantLifecycleChangeKind::Activate,
                    1_001,
                ),
            )
            .await?,
        TenantLifecycleChangeOutcome::Recorded(TenantLifecycleChangeEffect::StateChanged)
    );
    let authorization = tenant_authorization(&pool).await?;
    assert_eq!(authorization.tenant_id().as_str(), "tenant-a");
    assert_eq!(audit_count(&pool).await?, 2);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn already_active_tenant_records_no_state_change(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let outcome = TenantLifecycleAdministration::new(PostgresTenantLifecycleAdministration::new(
        pool.clone(),
    ))
    .apply(
        manage_tenant_lifecycle(&pool).await?,
        request(
            "already-active",
            "tenant-a",
            TenantLifecycleChangeKind::Activate,
            1_000,
        ),
    )
    .await?;

    assert_eq!(
        outcome,
        TenantLifecycleChangeOutcome::Recorded(TenantLifecycleChangeEffect::NoStateChange)
    );
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn collision_and_missing_tenant_do_not_change_state(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration = TenantLifecycleAdministration::new(
        PostgresTenantLifecycleAdministration::new(pool.clone()),
    );
    administration
        .apply(
            manage_tenant_lifecycle(&pool).await?,
            request(
                "lifecycle-a",
                "tenant-a",
                TenantLifecycleChangeKind::Suspend,
                1_000,
            ),
        )
        .await?;

    let collision = administration
        .apply(
            manage_tenant_lifecycle(&pool).await?,
            request(
                "lifecycle-a",
                "tenant-a",
                TenantLifecycleChangeKind::Activate,
                1_001,
            ),
        )
        .await
        .expect_err("one change ID must not name different content");
    assert!(matches!(
        collision,
        TenantLifecycleAdministrationError::Repository(
            PostgresTenantLifecycleAdministrationError::IdentityCollision { ref change_id }
        ) if change_id.as_str() == "lifecycle-a"
    ));

    let missing = administration
        .apply(
            manage_tenant_lifecycle(&pool).await?,
            request(
                "missing-tenant",
                "tenant-missing",
                TenantLifecycleChangeKind::Suspend,
                1_002,
            ),
        )
        .await
        .expect_err("missing tenants must fail atomically");
    assert!(matches!(
        missing,
        TenantLifecycleAdministrationError::Repository(
            PostgresTenantLifecycleAdministrationError::TenantNotFound { ref tenant_id }
        ) if tenant_id.as_str() == "tenant-missing"
    ));

    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_commands_have_one_winner_and_audit_is_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration = TenantLifecycleAdministration::new(
        PostgresTenantLifecycleAdministration::new(pool.clone()),
    );
    let change = request(
        "concurrent-change",
        "tenant-a",
        TenantLifecycleChangeKind::Suspend,
        1_000,
    );
    let first_authorization = manage_tenant_lifecycle(&pool).await?;
    let second_authorization = manage_tenant_lifecycle(&pool).await?;
    let (first, second) = tokio::join!(
        administration.apply(first_authorization, change.clone()),
        administration.apply(second_authorization, change),
    );
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, TenantLifecycleChangeOutcome::Recorded(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, TenantLifecycleChangeOutcome::AlreadyRecorded(_)))
            .count(),
        1
    );

    for statement in [
        "UPDATE identity.tenant_lifecycle_changes SET state_changed = false",
        "DELETE FROM identity.tenant_lifecycle_changes",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("tenant lifecycle evidence must be immutable");
        assert_constraint(&error, "ck_tenant_lifecycle_changes_immutable");
    }
    Ok(())
}

async fn seed(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
        .bootstrap(&PlatformBootstrapRequest::new(
            actor_id("operator-a"),
            external_identity("bootstrap-oidc", "operator-subject"),
            UnixTimeMillis::new(100),
        ))
        .await?;
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('tenant-user')")
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.tenant_action_grants (tenant_id, actor_id, action) VALUES ('tenant-a', 'tenant-user', 'metadata.read')",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn tenant_authorization(
    pool: &PgPool,
) -> Result<
    fleetiq_application::AuthorizedTenant<ReadMetadata>,
    TenantAuthorizationError<fleetiq_postgres::PostgresTenantAccessPolicyError>,
> {
    TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone()))
        .authorize::<ReadMetadata>(
            support::authenticated_actor("tenant-user").await,
            tenant_id("tenant-a"),
        )
        .await
}

async fn manage_tenant_lifecycle(
    pool: &PgPool,
) -> Result<fleetiq_application::AuthorizedPlatform<ManageTenantLifecycle>, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize::<ManageTenantLifecycle>(support::authenticated_actor("operator-a").await)
            .await?,
    )
}

fn request(
    change_id: &str,
    tenant: &str,
    kind: TenantLifecycleChangeKind,
    recorded_at_ms: i64,
) -> TenantLifecycleChangeRequest {
    TenantLifecycleChangeRequest::new(
        TenantLifecycleChangeId::new(change_id).expect("valid tenant lifecycle change ID"),
        tenant_id(tenant),
        kind,
        UnixTimeMillis::new(recorded_at_ms),
    )
}

fn external_identity(authority_id: &str, subject: &str) -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new(authority_id).expect("valid authority ID"),
        ExternalIdentitySubject::new(subject).expect("valid subject"),
    )
}

fn actor_id(value: &str) -> ActorId {
    ActorId::new(value).expect("valid actor ID")
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("valid tenant ID")
}

async fn tenant_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM metadata.tenants")
        .fetch_one(pool)
        .await
}

async fn tenant_grant_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.tenant_action_grants")
        .fetch_one(pool)
        .await
}

async fn audit_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.tenant_lifecycle_changes")
        .fetch_one(pool)
        .await
}

fn assert_constraint(error: &sqlx::Error, expected: &str) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some(expected)
    );
}
