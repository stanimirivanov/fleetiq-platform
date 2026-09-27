//! Integration tests for authorized, audited external-identity administration.

use std::error::Error;

use fleetiq_application::{
    ActorProvisioningRequest, ActorProvisioningService, ExternalIdentityAdministration,
    ExternalIdentityAdministrationError, ExternalIdentityChangeEffect, ExternalIdentityChangeKind,
    ExternalIdentityChangeOutcome, ExternalIdentityChangeRequest, PlatformAction,
    PlatformAuthorizationService, PlatformBootstrapRequest, PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ActorProvisioningId, ExternalIdentity, ExternalIdentityChangeId,
    ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresActorProvisioning, PostgresExternalIdentityAdministration,
    PostgresExternalIdentityAdministrationError, PostgresPlatformAccessPolicy,
    PostgresPlatformBootstrap,
};
use sqlx::PgPool;

mod support;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn attaches_replaces_and_retries_with_atomic_audit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration = ExternalIdentityAdministration::new(
        PostgresExternalIdentityAdministration::new(pool.clone()),
    );
    let attach = request(
        "identity-change-attach",
        "actor-target",
        "recovery-oidc",
        "recovery-subject",
        ExternalIdentityChangeKind::Attach,
        1_000,
    );

    assert_eq!(
        administration
            .apply(manage_identity(&pool).await?, attach.clone())
            .await?,
        ExternalIdentityChangeOutcome::Recorded(ExternalIdentityChangeEffect::StateChanged)
    );
    assert_eq!(
        administration
            .apply(manage_identity(&pool).await?, attach)
            .await?,
        ExternalIdentityChangeOutcome::AlreadyRecorded(ExternalIdentityChangeEffect::StateChanged)
    );
    assert_eq!(
        administration
            .apply(
                manage_identity(&pool).await?,
                request(
                    "identity-change-attach-again",
                    "actor-target",
                    "recovery-oidc",
                    "recovery-subject",
                    ExternalIdentityChangeKind::Attach,
                    1_001,
                ),
            )
            .await?,
        ExternalIdentityChangeOutcome::Recorded(ExternalIdentityChangeEffect::NoStateChange)
    );

    // The initial identity is immutable audit evidence, but it is no longer a
    // permanently pinned current mapping after a replacement has been attached.
    assert_eq!(
        administration
            .apply(
                manage_identity(&pool).await?,
                request(
                    "identity-change-detach-initial",
                    "actor-target",
                    "workforce-oidc",
                    "initial-subject",
                    ExternalIdentityChangeKind::Detach,
                    1_002,
                ),
            )
            .await?,
        ExternalIdentityChangeOutcome::Recorded(ExternalIdentityChangeEffect::StateChanged)
    );

    assert_eq!(
        mapping_count(&pool, "workforce-oidc", "initial-subject").await?,
        0
    );
    assert_eq!(
        mapping_count(&pool, "recovery-oidc", "recovery-subject").await?,
        1
    );
    assert_eq!(audit_count(&pool).await?, 3);

    let audit = sqlx::query!(
        r#"
        SELECT administrator_actor_id, target_actor_id, authority_id, subject,
               change_kind, recorded_at_ms, state_changed
        FROM identity.external_identity_changes
        WHERE change_id = 'identity-change-detach-initial'
        "#,
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(audit.administrator_actor_id, "operator-a");
    assert_eq!(audit.target_actor_id, "actor-target");
    assert_eq!(audit.authority_id, "workforce-oidc");
    assert_eq!(audit.subject, "initial-subject");
    assert_eq!(audit.change_kind, "detach");
    assert_eq!(audit.recorded_at_ms, 1_002);
    assert!(audit.state_changed);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn final_identity_is_preserved_without_an_audit_row(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let error = ExternalIdentityAdministration::new(PostgresExternalIdentityAdministration::new(
        pool.clone(),
    ))
    .apply(
        manage_identity(&pool).await?,
        request(
            "detach-final",
            "actor-target",
            "workforce-oidc",
            "initial-subject",
            ExternalIdentityChangeKind::Detach,
            1_000,
        ),
    )
    .await
    .expect_err("an actor's final external identity must be preserved");

    assert!(matches!(
        error,
        ExternalIdentityAdministrationError::Repository(
            PostgresExternalIdentityAdministrationError::LastExternalIdentity {
                ref actor_id
            }
        ) if actor_id.as_str() == "actor-target"
    ));
    assert_eq!(
        mapping_count(&pool, "workforce-oidc", "initial-subject").await?,
        1
    );
    assert_eq!(audit_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn identity_assigned_to_another_actor_is_not_rebound_or_audited(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    provision_actor(
        &pool,
        "provision-other",
        "actor-other",
        "other-oidc",
        "other-subject",
    )
    .await?;

    let error = ExternalIdentityAdministration::new(PostgresExternalIdentityAdministration::new(
        pool.clone(),
    ))
    .apply(
        manage_identity(&pool).await?,
        request(
            "rebind-other",
            "actor-target",
            "other-oidc",
            "other-subject",
            ExternalIdentityChangeKind::Attach,
            1_000,
        ),
    )
    .await
    .expect_err("an identity assigned to another actor must not be rebound");

    assert!(matches!(
        error,
        ExternalIdentityAdministrationError::Repository(
            PostgresExternalIdentityAdministrationError::IdentityAssigned
        )
    ));
    assert_eq!(
        assigned_actor(&pool, "other-oidc", "other-subject").await?,
        "actor-other"
    );
    assert_eq!(audit_count(&pool).await?, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reused_change_identity_is_a_collision_and_exact_commands_have_one_winner(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    let administration = ExternalIdentityAdministration::new(
        PostgresExternalIdentityAdministration::new(pool.clone()),
    );
    let exact = request(
        "identity-change-a",
        "actor-target",
        "recovery-oidc",
        "recovery-subject",
        ExternalIdentityChangeKind::Attach,
        1_000,
    );
    let first_authorization = manage_identity(&pool).await?;
    let second_authorization = manage_identity(&pool).await?;

    let (first, second) = tokio::join!(
        administration.apply(first_authorization, exact.clone()),
        administration.apply(second_authorization, exact),
    );
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                **outcome
                    == ExternalIdentityChangeOutcome::Recorded(
                        ExternalIdentityChangeEffect::StateChanged,
                    )
            })
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| {
                **outcome
                    == ExternalIdentityChangeOutcome::AlreadyRecorded(
                        ExternalIdentityChangeEffect::StateChanged,
                    )
            })
            .count(),
        1
    );

    let error = administration
        .apply(
            manage_identity(&pool).await?,
            request(
                "identity-change-a",
                "actor-target",
                "another-oidc",
                "another-subject",
                ExternalIdentityChangeKind::Attach,
                1_001,
            ),
        )
        .await
        .expect_err("one change identity must not name different content");
    assert!(matches!(
        error,
        ExternalIdentityAdministrationError::Repository(
            PostgresExternalIdentityAdministrationError::IdentityCollision { ref change_id }
        ) if change_id.as_str() == "identity-change-a"
    ));
    assert_eq!(audit_count(&pool).await?, 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provisioning_assertion_and_identity_audit_remain_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed(&pool).await?;
    sqlx::query("INSERT INTO identity.actors (actor_id) VALUES ('actor-direct')")
        .execute(&pool)
        .await?;

    let mut transaction = pool.begin().await?;
    sqlx::query(
        "INSERT INTO identity.actor_provisionings \
         (provisioning_id, actor_id, authority_id, subject, \
          provisioner_actor_id, recorded_at_ms) \
         VALUES ('direct-provisioning', 'actor-direct', 'missing-oidc', \
                 'missing-subject', 'operator-a', 1000)",
    )
    .execute(&mut *transaction)
    .await?;
    let error = transaction
        .commit()
        .await
        .expect_err("provisioning must still require its exact initial mapping");
    assert_constraint(&error, "fk_actor_provisionings_external_identity");

    ExternalIdentityAdministration::new(PostgresExternalIdentityAdministration::new(pool.clone()))
        .apply(
            manage_identity(&pool).await?,
            request(
                "immutable-change",
                "actor-target",
                "recovery-oidc",
                "recovery-subject",
                ExternalIdentityChangeKind::Attach,
                1_001,
            ),
        )
        .await?;
    for statement in [
        "UPDATE identity.external_identity_changes SET state_changed = false",
        "DELETE FROM identity.external_identity_changes",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("external-identity audit evidence must reject mutation");
        assert_constraint(&error, "ck_external_identity_changes_immutable");
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
    provision_actor(
        pool,
        "provision-target",
        "actor-target",
        "workforce-oidc",
        "initial-subject",
    )
    .await
}

async fn provision_actor(
    pool: &PgPool,
    provisioning_id: &str,
    actor_id: &str,
    authority_id: &str,
    subject: &str,
) -> Result<(), Box<dyn Error>> {
    ActorProvisioningService::new(PostgresActorProvisioning::new(pool.clone()))
        .provision(
            manage_identity(pool).await?,
            ActorProvisioningRequest::new(
                ActorProvisioningId::new(provisioning_id).expect("valid provisioning ID"),
                self::actor_id(actor_id),
                external_identity(authority_id, subject),
                UnixTimeMillis::new(500),
            ),
        )
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
    change_id: &str,
    actor_id: &str,
    authority_id: &str,
    subject: &str,
    kind: ExternalIdentityChangeKind,
    recorded_at_ms: i64,
) -> ExternalIdentityChangeRequest {
    ExternalIdentityChangeRequest::new(
        ExternalIdentityChangeId::new(change_id).expect("valid change ID"),
        self::actor_id(actor_id),
        external_identity(authority_id, subject),
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

async fn audit_count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM identity.external_identity_changes")
        .fetch_one(pool)
        .await
}

async fn mapping_count(pool: &PgPool, authority_id: &str, subject: &str) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM identity.external_actor_identities \
         WHERE authority_id = $1 AND subject = $2",
    )
    .bind(authority_id)
    .bind(subject)
    .fetch_one(pool)
    .await
}

async fn assigned_actor(pool: &PgPool, authority_id: &str, subject: &str) -> sqlx::Result<String> {
    sqlx::query_scalar(
        "SELECT actor_id FROM identity.external_actor_identities \
         WHERE authority_id = $1 AND subject = $2",
    )
    .bind(authority_id)
    .bind(subject)
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
