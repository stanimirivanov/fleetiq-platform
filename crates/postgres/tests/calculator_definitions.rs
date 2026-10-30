//! Registry retry, ownership and immutability against the complete migration chain.
use fleetiq_analytics::{
    CalculatorDefinition, CalculatorId, CalculatorImplementation, CalculatorRef,
    CalculatorRegistrationOutcome, CalculatorVersion,
};
use fleetiq_domain::{DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, TenantId};
use fleetiq_postgres::{CalculatorRegistryError, PostgresCalculatorRegistry};
use sqlx::PgPool;
use std::error::Error;
fn declaration(tenant: &str) -> CalculatorDefinition {
    let reference = |id| {
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(id).unwrap(),
            DefinitionVersion::new(1).unwrap(),
        )
    };
    CalculatorDefinition {
        tenant: TenantId::new(tenant).unwrap(),
        release: CalculatorRef::new(
            CalculatorId::new("mean").unwrap(),
            CalculatorVersion::new("1").unwrap(),
        ),
        implementation: CalculatorImplementation::WindowMeanV1,
        input: reference("input"),
        output: reference("output"),
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn registration_is_tenant_owned_immutable_and_retry_safe(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    for tenant in ["a", "b"] {
        sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
            .bind(tenant)
            .execute(&pool)
            .await?;
    }
    sqlx::query("INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('a','input',1,'Input','integer','deg'),('a','output',1,'Output','decimal','deg'),('a','other',1,'Other','decimal','deg'),('a','bad',1,'Bad','decimal','volt')").execute(&pool).await?;
    let registry = PostgresCalculatorRegistry::new(pool.clone());
    let d = declaration("a");
    let (a, b) = tokio::join!(registry.register(&d), registry.register(&d));
    let outcomes = [a?, b?];
    assert!(outcomes.contains(&CalculatorRegistrationOutcome::Registered));
    assert!(outcomes.contains(&CalculatorRegistrationOutcome::AlreadyPresent));
    assert_eq!(
        registry.register(&d).await?,
        CalculatorRegistrationOutcome::AlreadyPresent
    );
    let mut different = d.clone();
    different.output = PropertyDefinitionRef::new(
        PropertyDefinitionId::new("other")?,
        DefinitionVersion::new(1)?,
    );
    assert_eq!(
        registry.register(&different).await,
        Err(CalculatorRegistryError::IdentityCollision)
    );
    assert_eq!(
        registry.register(&declaration("b")).await,
        Err(CalculatorRegistryError::InvalidReferences)
    );
    let mut incompatible = d.clone();
    incompatible.release =
        CalculatorRef::new(CalculatorId::new("other")?, CalculatorVersion::new("1")?);
    incompatible.output = PropertyDefinitionRef::new(
        PropertyDefinitionId::new("bad")?,
        DefinitionVersion::new(1)?,
    );
    assert_eq!(
        registry.register(&incompatible).await,
        Err(CalculatorRegistryError::InvalidReferences)
    );
    for sql in [
        "UPDATE analytics.calculator_definitions SET implementation='window-mean-v1'",
        "DELETE FROM analytics.calculator_definitions",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }
    assert!(sqlx::query("INSERT INTO analytics.calculator_definitions VALUES ('b','x','1','window-mean-v1','input',1,'output',1)").execute(&pool).await.is_err());
    Ok(())
}
