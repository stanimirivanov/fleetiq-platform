//! Production bootstrap uses a narrow data-writing role, never a schema owner.
use std::error::Error;

use fleetiq_identity::PlatformBootstrapOutcome;
use fleetiq_server::{PlatformBootstrapConfig, SchemaMode, bootstrap_platform_operator};
use sqlx::{AssertSqlSafe, ConnectOptions, PgPool, postgres::PgPoolOptions};

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn checked_bootstrap_uses_only_explicit_dml_grants_and_retries_without_ddl(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    // Names contain only a constant prefix and hexadecimal random data. The
    // role is unique per fixture and removed even when the scenario fails.
    let random =
        getrandom::u64().map_err(|_| std::io::Error::other("fixture randomness unavailable"))?;
    let role = format!("fiq_bootstrap_{random:x}");
    sqlx::query(AssertSqlSafe(format!(
        "CREATE ROLE {role} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT"
    )))
    .execute(&pool)
    .await?;

    let result = async {
        let grants = [
            format!("GRANT USAGE ON SCHEMA identity, public TO {role}"),
            format!("GRANT SELECT ON public._sqlx_migrations TO {role}"),
            format!(
                "GRANT SELECT, INSERT ON identity.platform_bootstrap, \
                 identity.actors, identity.external_actor_identities, \
                 identity.platform_action_grants TO {role}"
            ),
        ];
        for statement in grants {
            sqlx::query(AssertSqlSafe(statement)).execute(&pool).await?;
        }
        // Use the identical URL for the denial probe and bootstrap. SQLx's
        // lossy URL serializer does not retain role startup options.
        let mut url = pool.connect_options().to_url_lossy();
        url.query_pairs_mut()
            .append_pair("options", &format!("-c role={role}"));
        let restricted = PgPoolOptions::new()
            .max_connections(1)
            .connect(url.as_str())
            .await?;
        let ddl_denied = sqlx::query("CREATE TABLE identity.forbidden_ddl (id integer)")
            .execute(&restricted)
            .await
            .is_err();
        restricted.close().await;
        if !ddl_denied {
            return Err(std::io::Error::other("bootstrap role unexpectedly performed DDL").into());
        }
        let config = PlatformBootstrapConfig::from_values(
            url.to_string(), "operator-302", "workforce", "subject-302",
        )?
        .with_schema_mode(SchemaMode::Check);
        if bootstrap_platform_operator(config.clone()).await?
            != PlatformBootstrapOutcome::Bootstrapped
        {
            return Err(std::io::Error::other("first bootstrap did not create identity").into());
        }
        if bootstrap_platform_operator(config).await?
            != PlatformBootstrapOutcome::AlreadyBootstrapped
        {
            return Err(std::io::Error::other("bootstrap retry was not idempotent").into());
        }
        let evidence: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM identity.platform_bootstrap WHERE actor_id='operator-302')",
        )
        .fetch_one(&pool)
        .await?;
        if !evidence {
            return Err(std::io::Error::other("bootstrap evidence missing").into());
        }
        Ok::<(), Box<dyn Error>>(())
    }
    .await;

    // DROP OWNED only revokes this fixture role's privileges in this isolated
    // test database. The role never owns objects or credentials in another DB.
    sqlx::query(AssertSqlSafe(format!("DROP OWNED BY {role}")))
        .execute(&pool)
        .await?;
    sqlx::query(AssertSqlSafe(format!("DROP ROLE {role}")))
        .execute(&pool)
        .await?;
    result
}
