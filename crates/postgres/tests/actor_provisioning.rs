//! Integration tests for authorized, atomic actor provisioning.

use std::error::Error;

use fleetiq_application::{
    ActorIdentityResolver, ActorProvisioningError, ActorProvisioningOutcome,
    ActorProvisioningRequest, ActorProvisioningService, PlatformAction,
    PlatformAuthorizationService, PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ActorProvisioningId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId,
    UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresActorProvisioning, PostgresActorProvisioningError,
    PostgresPlatformAccessPolicy, PostgresPlatformBootstrap,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn creates_an_actor_mapping_and_audit_without_granting_access(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    bootstrap_operator(&pool).await?;
    let service = ActorProvisioningService::new(PostgresActorProvisioning::new(pool.clone()));
    let expected_identity = external_identity("workforce-oidc", "subject-a");
    let request = request(
        "actor-provisioning-a",
        "actor-a",
        "workforce-oidc",
        "subject-a",
        1_000,
    );

    assert_eq!(
        service
            .provision(manage_identity(&pool).await?, request.clone())
            .await?,
        ActorProvisioningOutcome::Provisioned
    );
    assert_eq!(
        service
            .provision(manage_identity(&pool).await?, request.clone())
            .await?,
        ActorProvisioningOutcome::AlreadyProvisioned
    );

    let resolved = PostgresActorIdentityResolver::new(pool.clone())
        .resolve(&expected_identity)
        .await?
        .expect("provisioned external identity should resolve");
    assert_eq!(resolved.as_str(), "actor-a");
    assert_eq!(actor_count(&pool).await?, 2);
    assert_eq!(actor_provisioning_count(&pool).await?, 1);
    assert_eq!(actor_grant_count(&pool, "actor-a").await?, 0);
    assert_eq!(actor_grant_count(&pool, "operator-a").await?, 3);

    let audit = sqlx::query!(
        r#"
        SELECT actor_id, authority_id, subject, provisioner_actor_id, recorded_at_ms
        FROM identity.actor_provisionings
        WHERE provisioning_id = $1
        "#,
        "actor-provisioning-a",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(audit.actor_id, "actor-a");
    assert_eq!(audit.authority_id, "workforce-oidc");
    assert_eq!(audit.subject, "subject-a");
    assert_eq!(audit.provisioner_actor_id, "operator-a");
    assert_eq!(audit.recorded_at_ms, 1_000);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_commands_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    bootstrap_operator(&pool).await?;
    let service = ActorProvisioningService::new(PostgresActorProvisioning::new(pool.clone()));
    let request = request(
        "actor-provisioning-a",
        "actor-a",
        "workforce-oidc",
        "subject-a",
        1_000,
    );
    let first_authorization = manage_identity(&pool).await?;
    let second_authorization = manage_identity(&pool).await?;

    let (first, second) = tokio::join!(
        service.provision(first_authorization, request.clone()),
        service.provision(second_authorization, request),
    );
    let outcomes = [first?, second?];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ActorProvisioningOutcome::Provisioned)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ActorProvisioningOutcome::AlreadyProvisioned)
            .count(),
        1
    );
    assert_eq!(actor_count(&pool).await?, 2);
    assert_eq!(actor_provisioning_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn every_unique_arbiter_rejects_different_command_content(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    bootstrap_operator(&pool).await?;
    let service = ActorProvisioningService::new(PostgresActorProvisioning::new(pool.clone()));
    service
        .provision(
            manage_identity(&pool).await?,
            request(
                "actor-provisioning-a",
                "actor-a",
                "workforce-oidc",
                "subject-a",
                1_000,
            ),
        )
        .await?;

    let conflicting = [
        request(
            "actor-provisioning-a",
            "actor-b",
            "workforce-oidc",
            "subject-b",
            1_000,
        ),
        request(
            "actor-provisioning-b",
            "actor-a",
            "workforce-oidc",
            "subject-b",
            1_000,
        ),
        request(
            "actor-provisioning-b",
            "actor-b",
            "workforce-oidc",
            "subject-a",
            1_000,
        ),
    ];

    for request in conflicting {
        let error = service
            .provision(manage_identity(&pool).await?, request)
            .await
            .expect_err("reusing any command arbiter must be a collision");
        assert!(matches!(
            error,
            ActorProvisioningError::Repository(
                PostgresActorProvisioningError::IdentityCollision { .. }
            )
        ));
    }

    assert_eq!(actor_count(&pool).await?, 2);
    assert_eq!(actor_provisioning_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn existing_external_identity_rolls_back_actor_and_audit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    bootstrap_operator(&pool).await?;
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('legacy-actor')")
        .execute(&pool)
        .await?;
    sqlx::query(
        "INSERT INTO identity.external_actor_identities \
         (authority_id, subject, actor_id) \
         VALUES ('legacy-oidc', 'legacy-subject', 'legacy-actor')",
    )
    .execute(&pool)
    .await?;

    let error = ActorProvisioningService::new(PostgresActorProvisioning::new(pool.clone()))
        .provision(
            manage_identity(&pool).await?,
            request(
                "actor-provisioning-a",
                "actor-a",
                "legacy-oidc",
                "legacy-subject",
                1_000,
            ),
        )
        .await
        .expect_err("an existing external identity must reject provisioning");
    assert!(matches!(error, ActorProvisioningError::Repository(_)));
    assert_eq!(actor_count(&pool).await?, 2);
    assert_eq!(actor_provisioning_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provisioning_evidence_is_immutable(pool: PgPool) -> Result<(), Box<dyn Error>> {
    bootstrap_operator(&pool).await?;
    ActorProvisioningService::new(PostgresActorProvisioning::new(pool.clone()))
        .provision(
            manage_identity(&pool).await?,
            request(
                "actor-provisioning-a",
                "actor-a",
                "workforce-oidc",
                "subject-a",
                1_000,
            ),
        )
        .await?;

    for statement in [
        "UPDATE identity.actor_provisionings SET recorded_at_ms = 2",
        "DELETE FROM identity.actor_provisionings",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("actor provisioning evidence must reject mutation");
        assert_constraint(&error, "ck_actor_provisionings_immutable");
    }
    Ok(())
}

async fn bootstrap_operator(pool: &PgPool) -> Result<(), Box<dyn Error>> {
    PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
        .bootstrap(&PlatformBootstrapRequest::new(
            actor_id("operator-a"),
            external_identity("bootstrap-oidc", "operator-subject"),
            UnixTimeMillis::new(100),
        ))
        .await?;
    Ok(())
}

async fn manage_identity(
    pool: &PgPool,
) -> Result<fleetiq_application::AuthorizedPlatformAction, Box<dyn Error>> {
    Ok(
        PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(pool.clone()))
            .authorize(
                support::authenticated_actor("operator-a").await,
                PlatformAction::ManageIdentity,
            )
            .await?,
    )
}

fn request(
    provisioning_id: &str,
    actor_id: &str,
    authority_id: &str,
    subject: &str,
    recorded_at_ms: i64,
) -> ActorProvisioningRequest {
    ActorProvisioningRequest::new(
        ActorProvisioningId::new(provisioning_id).expect("valid provisioning ID"),
        self::actor_id(actor_id),
        external_identity(authority_id, subject),
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

async fn actor_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.actors")
        .fetch_one(pool)
        .await
}

async fn actor_provisioning_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.actor_provisionings")
        .fetch_one(pool)
        .await
}

async fn actor_grant_count(pool: &PgPool, actor_id: &str) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.platform_action_grants WHERE actor_id = $1")
        .bind(actor_id)
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
