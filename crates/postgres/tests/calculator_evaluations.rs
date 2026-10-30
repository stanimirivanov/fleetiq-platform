//! Real atomic append/retry/provenance tests; existing history fixture stays intact.
use fleetiq_analytics::*;
use fleetiq_domain::*;
use fleetiq_postgres::{EvaluationStoreError, PostgresCalculatorEvaluationStore};
use sqlx::PgPool;
use std::error::Error;
fn time(v: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(v)
}
fn release() -> CalculatorRef {
    CalculatorRef::new(
        CalculatorId::new("mean").unwrap(),
        CalculatorVersion::new("1").unwrap(),
    )
}
fn definition() -> PropertyDefinition {
    PropertyDefinition::new(
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("mean.voltage").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        DefinitionName::new("Mean").unwrap(),
        PropertyValueKind::Decimal,
        Some(UnitId::new("volt").unwrap()),
    )
    .unwrap()
}
fn record(
    id: &str,
    configuration: &str,
    outcome: CalculatorExecutionOutcome,
    inputs: Vec<CalculatorInput>,
    values: &[(&str, &str)],
) -> CalculatorEvaluationRecord {
    let tenant = TenantId::new("tenant-a").unwrap();
    let scope = AssetId::new("asset-a").unwrap();
    let evaluation = CalculatorEvaluation::new(
        CalculatorEvaluationId::new(id).unwrap(),
        tenant.clone(),
        scope.clone(),
        release(),
        time(3000),
        time(3500),
    );
    let key = CalculatorExecutionKey::new(
        tenant,
        scope,
        release(),
        CalculatorConfigurationId::new(configuration).unwrap(),
        CalculatorInputManifest::new(
            CalculatorWindow::new(time(0), time(3000), time(3000)).unwrap(),
            time(3000),
            inputs,
        )
        .unwrap(),
    );
    let outputs = assemble_outputs(
        &evaluation,
        values.iter().map(|(asset, value)| {
            CalculatedProperty::new(
                AssetId::new(*asset).unwrap(),
                &definition(),
                PropertyValue::Decimal(value.parse().unwrap()),
                Some(UnitId::new("volt").unwrap()),
            )
            .unwrap()
        }),
    )
    .unwrap();
    CalculatorEvaluationRecord::new(key, evaluation, outcome, outputs, 1).unwrap()
}
async fn setup(pool: &PgPool) -> Result<CalculatorInput, Box<dyn Error>> {
    sqlx::raw_sql(include_str!("property_history/topology.sql"))
        .execute(pool)
        .await?;
    sqlx::raw_sql("INSERT INTO metadata.property_definitions (tenant_id,property_id,version,display_name,value_kind,canonical_unit) VALUES ('tenant-a','mean.voltage',1,'Mean','decimal','volt');
        INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count) VALUES ('tenant-a','calculated',1,'Calculated',1);
        INSERT INTO metadata.asset_type_properties (tenant_id,type_id,type_version,property_id,property_version,property_ordinal) VALUES ('tenant-a','calculated',1,'mean.voltage',1,1);
        INSERT INTO metadata.assets (tenant_id,asset_id,type_id,type_version,display_name) VALUES ('tenant-a','result','calculated',1,'Result');
        INSERT INTO analytics.calculator_definitions VALUES ('tenant-a','mean','1','window-mean-v1','electrical.voltage',1,'mean.voltage',1);").execute(pool).await?;
    let id:String=sqlx::query_scalar("SELECT analytics.semantic_evidence_id(tenant_id,message_id,protocol_package_id,protocol_package_version,output_index,binding_id,binding_revision,event_time_basis) FROM history.asset_property_changes WHERE event_at_ms=1000").fetch_one(pool).await?;
    Ok(CalculatorInput::new(
        CalculatorEvidenceId::new(id)?,
        time(1000),
    ))
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_retry_preserves_order_precision_and_rejects_identity_reuse(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let input = setup(&pool).await?;
    let store = PostgresCalculatorEvaluationStore::new(pool.clone());
    let r = record(
        "e1",
        "c1",
        CalculatorExecutionOutcome::Complete,
        vec![input.clone()],
        &[("result", "1.00000000000000000001"), ("result", "2")],
    );
    let (a, b) = tokio::join!(store.append(&r), store.append(&r));
    let outcomes = [a?, b?];
    assert!(outcomes.contains(&EvaluationAppendOutcome::Appended));
    assert!(outcomes.contains(&EvaluationAppendOutcome::AlreadyPresent));
    let values:(String,String)=sqlx::query_as("SELECT outputs->0->>'value',outputs->1->>'value' FROM analytics.evaluations WHERE evaluation_id='e1'").fetch_one(&pool).await?;
    assert_eq!(values, ("1.00000000000000000001".into(), "2".into()));
    assert_eq!(
        store
            .append(&record(
                "e2",
                "c1",
                CalculatorExecutionOutcome::Complete,
                vec![input.clone()],
                &[]
            ))
            .await,
        Err(EvaluationStoreError::IdentityCollision)
    );
    assert_eq!(
        store
            .append(&record(
                "e1",
                "c1",
                CalculatorExecutionOutcome::Complete,
                vec![input],
                &[("result", "2"), ("result", "1.00000000000000000001")]
            ))
            .await,
        Err(EvaluationStoreError::IdentityCollision)
    );
    assert!(
        sqlx::query("DELETE FROM metadata.assets WHERE tenant_id='tenant-a' AND asset_id='result'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        CalculatorEvaluationRecord::new(
            r.key().clone(),
            r.evaluation().clone(),
            CalculatorExecutionOutcome::Complete,
            r.outputs()[1..].to_vec(),
            1
        )
        .is_err()
    );
    let different_parameters = CalculatorEvaluationRecord::new(
        r.key().clone(),
        r.evaluation().clone(),
        r.outcome(),
        r.outputs().to_vec(),
        2,
    )?;
    assert_eq!(
        store.append(&different_parameters).await,
        Err(EvaluationStoreError::IdentityCollision)
    );
    let conflicting_id = record(
        "e1",
        "unused-config",
        CalculatorExecutionOutcome::Complete,
        vec![],
        &[],
    );
    assert_eq!(
        store.append(&conflicting_id).await,
        Err(EvaluationStoreError::IdentityCollision)
    );
    let unused:i64=sqlx::query_scalar("SELECT count(*) FROM analytics.evaluation_configurations WHERE configuration_id='unused-config'").fetch_one(&pool).await?;
    assert_eq!(unused, 0);
    for statement in [
        "UPDATE analytics.evaluations SET outcome='complete'",
        "DELETE FROM analytics.evaluations",
    ] {
        assert!(sqlx::query(statement).execute(&pool).await.is_err());
    }
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn invalid_evidence_rolls_back_all_outputs_and_zero_completion_is_explicit(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let input = setup(&pool).await?;
    let store = PostgresCalculatorEvaluationStore::new(pool.clone());
    let bad = record(
        "bad",
        "bad-config",
        CalculatorExecutionOutcome::Complete,
        vec![input.clone()],
        &[("result", "1"), ("foreign-asset", "2")],
    );
    assert_eq!(
        store.append(&bad).await,
        Err(EvaluationStoreError::InvalidEvidence)
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM analytics.evaluations")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    let unknown = CalculatorInput::new(CalculatorEvidenceId::new("missing")?, time(1000));
    assert_eq!(
        store
            .append(&record(
                "missing",
                "missing-config",
                CalculatorExecutionOutcome::Complete,
                vec![unknown],
                &[]
            ))
            .await,
        Err(EvaluationStoreError::InvalidEvidence)
    );
    for (id, outcome) in [
        ("empty", CalculatorExecutionOutcome::Complete),
        (
            "insufficient",
            CalculatorExecutionOutcome::InsufficientEvidence,
        ),
        (
            "failed",
            CalculatorExecutionOutcome::Failed(CalculatorExecutionFailure::ResourceLimit),
        ),
    ] {
        assert_eq!(
            store.append(&record(id, id, outcome, vec![], &[])).await?,
            EvaluationAppendOutcome::Appended
        );
    }
    let outcomes: Vec<String> =
        sqlx::query_scalar("SELECT outcome FROM analytics.evaluations ORDER BY evaluation_id")
            .fetch_all(&pool)
            .await?;
    assert_eq!(outcomes, vec!["complete", "resource_limit", "insufficient"]);
    let original = record(
        "context",
        "ctx",
        CalculatorExecutionOutcome::Complete,
        vec![],
        &[],
    );
    let foreign = CalculatorExecutionKey::new(
        TenantId::new("tenant-b")?,
        AssetId::new("asset-a")?,
        release(),
        CalculatorConfigurationId::new("ctx")?,
        original.key().manifest().clone(),
    );
    let evaluation = CalculatorEvaluation::new(
        CalculatorEvaluationId::new("foreign")?,
        TenantId::new("tenant-b")?,
        AssetId::new("asset-a")?,
        release(),
        time(3000),
        time(3500),
    );
    assert_eq!(
        store
            .append(&CalculatorEvaluationRecord::new(
                foreign,
                evaluation,
                CalculatorExecutionOutcome::Complete,
                vec![],
                1
            )?)
            .await,
        Err(EvaluationStoreError::InvalidEvidence)
    );
    let mismatch = CalculatorEvaluation::new(
        CalculatorEvaluationId::new("context")?,
        TenantId::new("tenant-a")?,
        AssetId::new("asset-a")?,
        release(),
        time(2999),
        time(3500),
    );
    assert!(
        CalculatorEvaluationRecord::new(
            original.key().clone(),
            mismatch,
            CalculatorExecutionOutcome::Complete,
            vec![],
            1
        )
        .is_err()
    );
    assert!(
        CalculatorEvaluationRecord::new(
            bad.key().clone(),
            bad.evaluation().clone(),
            CalculatorExecutionOutcome::InsufficientEvidence,
            bad.outputs().to_vec(),
            1
        )
        .is_err()
    );
    Ok(())
}
