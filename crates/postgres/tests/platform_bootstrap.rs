//! Integration tests for one-time atomic platform bootstrap.

use std::error::Error;

use fleetiq_application::{
    ActorIdentityResolver, PlatformAction, PlatformAuthorizationService, PlatformBootstrapError,
    PlatformBootstrapOutcome, PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresPlatformAccessPolicy, PostgresPlatformBootstrap,
    PostgresPlatformBootstrapError,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn establishes_a_resolvable_operator_grant_and_immutable_evidence(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let service = PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()));
    let initial_request = request("operator-a", "workforce-oidc", "subject-a", 1_000);

    assert_eq!(
        service.bootstrap(&initial_request).await?,
        PlatformBootstrapOutcome::Bootstrapped
    );
    assert_eq!(
        service
            .bootstrap(&request("operator-a", "workforce-oidc", "subject-a", 2_000))
            .await?,
        PlatformBootstrapOutcome::AlreadyBootstrapped
    );

    let resolved = PostgresActorIdentityResolver::new(pool.clone())
        .resolve(initial_request.external_identity())
        .await?
        .expect("bootstrapped external identity should resolve");
    assert_eq!(resolved, initial_request.actor_id().clone());

    let authorization =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("operator-a").await,
                PlatformAction::ProvisionTenant,
            )
            .await?;
    assert_eq!(authorization.actor_id(), initial_request.actor_id());

    let identity_authorization =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("operator-a").await,
                PlatformAction::ManageIdentity,
            )
            .await?;
    assert_eq!(
        identity_authorization.actor_id(),
        initial_request.actor_id()
    );

    let access_authorization =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("operator-a").await,
                PlatformAction::ManagePlatformAccess,
            )
            .await?;
    assert_eq!(access_authorization.actor_id(), initial_request.actor_id());

    let tenant_lifecycle_authorization =
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("operator-a").await,
                PlatformAction::ManageTenantLifecycle,
            )
            .await?;
    assert_eq!(
        tenant_lifecycle_authorization.actor_id(),
        initial_request.actor_id()
    );

    assert_eq!(actor_count(&pool).await?, 1);
    assert_eq!(external_identity_count(&pool).await?, 1);
    assert_eq!(platform_grant_count(&pool).await?, 4);
    assert_eq!(bootstrap_count(&pool).await?, 1);

    let audit = sqlx::query!(
        r#"
        SELECT actor_id, authority_id, subject, recorded_at_ms
        FROM identity.platform_bootstrap
        WHERE singleton = TRUE
        "#,
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(audit.actor_id, "operator-a");
    assert_eq!(audit.authority_id, "workforce-oidc");
    assert_eq!(audit.subject, "subject-a");
    assert_eq!(audit.recorded_at_ms, 1_000);

    sqlx::query(
        "DELETE FROM identity.platform_action_grants \
         WHERE actor_id = 'operator-a' AND action = 'tenant.provision'",
    )
    .execute(&pool)
    .await?;
    assert_eq!(
        service
            .bootstrap(&request("operator-a", "workforce-oidc", "subject-a", 3_000,))
            .await?,
        PlatformBootstrapOutcome::AlreadyBootstrapped
    );
    assert_eq!(platform_grant_count(&pool).await?, 3);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_bootstraps_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let service = PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()));
    let request = request("operator-a", "workforce-oidc", "subject-a", 1_000);

    let (first, second) = tokio::join!(service.bootstrap(&request), service.bootstrap(&request));
    let outcomes = [first?, second?];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PlatformBootstrapOutcome::Bootstrapped)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PlatformBootstrapOutcome::AlreadyBootstrapped)
            .count(),
        1
    );
    assert_eq!(actor_count(&pool).await?, 1);
    assert_eq!(external_identity_count(&pool).await?, 1);
    assert_eq!(platform_grant_count(&pool).await?, 4);
    assert_eq!(bootstrap_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn a_different_second_request_cannot_replace_the_initial_operator(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let service = PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()));
    service
        .bootstrap(&request("operator-a", "workforce-oidc", "subject-a", 1_000))
        .await?;

    for different in [
        request("operator-b", "workforce-oidc", "subject-b", 1_000),
        request("operator-a", "other-oidc", "subject-a", 1_000),
    ] {
        let error = service
            .bootstrap(&different)
            .await
            .expect_err("a different bootstrap request must be rejected");
        assert!(matches!(
            error,
            PlatformBootstrapError::Repository(PostgresPlatformBootstrapError::AlreadyInitialized)
        ));
    }

    assert_eq!(actor_count(&pool).await?, 1);
    assert_eq!(external_identity_count(&pool).await?, 1);
    assert_eq!(platform_grant_count(&pool).await?, 4);
    assert_eq!(bootstrap_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn dependent_write_failure_rolls_back_the_bootstrap_claim(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
        .bind("operator-a")
        .execute(&pool)
        .await?;
    let service = PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()));

    service
        .bootstrap(&request("operator-a", "workforce-oidc", "subject-a", 1_000))
        .await
        .expect_err("the duplicate actor must fail the complete bootstrap transaction");

    assert_eq!(actor_count(&pool).await?, 1);
    assert_eq!(external_identity_count(&pool).await?, 0);
    assert_eq!(platform_grant_count(&pool).await?, 0);
    assert_eq!(bootstrap_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn bootstrap_evidence_is_immutable(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let service = PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()));
    service
        .bootstrap(&request("operator-a", "workforce-oidc", "subject-a", 1_000))
        .await?;

    for statement in [
        "UPDATE identity.platform_bootstrap SET recorded_at_ms = 2",
        "DELETE FROM identity.platform_bootstrap",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("bootstrap evidence must reject mutation");
        assert_constraint(&error, "ck_platform_bootstrap_immutable");
    }

    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn bootstrap_evidence_must_reference_the_same_actor_as_the_identity_mapping(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    for actor in ["operator-a", "operator-b"] {
        sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ($1)")
            .bind(actor)
            .execute(&pool)
            .await?;
    }
    sqlx::query(
        "INSERT INTO identity.external_actor_identities \
         (authority_id, subject, actor_id) VALUES ('workforce-oidc', 'subject-b', 'operator-b')",
    )
    .execute(&pool)
    .await?;

    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO identity.platform_bootstrap \
         (singleton, actor_id, authority_id, subject, recorded_at_ms) \
         VALUES (TRUE, 'operator-a', 'workforce-oidc', 'subject-b', 1)",
    )
    .execute(&mut *transaction)
    .await?;
    let error = transaction
        .commit()
        .await
        .expect_err("deferred constraint must reject a mismatched actor mapping");
    assert_constraint(&error, "fk_platform_bootstrap_external_identity");
    Ok(())
}

fn request(
    actor_id: &str,
    authority_id: &str,
    subject: &str,
    recorded_at_ms: i64,
) -> PlatformBootstrapRequest {
    PlatformBootstrapRequest::new(
        ActorId::new(actor_id).expect("valid actor ID"),
        ExternalIdentity::new(
            IdentityAuthorityId::new(authority_id).expect("valid authority ID"),
            ExternalIdentitySubject::new(subject).expect("valid subject"),
        ),
        UnixTimeMillis::new(recorded_at_ms),
    )
}

async fn actor_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.actors")
        .fetch_one(pool)
        .await
}

async fn external_identity_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.external_actor_identities")
        .fetch_one(pool)
        .await
}

async fn platform_grant_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.platform_action_grants")
        .fetch_one(pool)
        .await
}

async fn bootstrap_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.platform_bootstrap")
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
