//! Single-snapshot semantic input selection, pinned before execution.
use fleetiq_analytics::{
    CalculatorEvidenceId, CalculatorImplementation, CalculatorInput, CalculatorInputManifest,
    CalculatorPreparation, CalculatorRunRequest, PreparedCalculatorEvaluation,
};
use fleetiq_domain::{
    DefinitionName, DefinitionVersion, PropertyDefinition, PropertyDefinitionId,
    PropertyDefinitionRef, PropertyValue, PropertyValueKind, UnitId, UnixTimeMillis,
};
use serde_json::{Value, json};
use sqlx::PgPool;

/// Durable first-party preparation adapter; no calculated-output recursion.
pub struct PostgresCalculatorPreparation(PgPool);
impl PostgresCalculatorPreparation {
    /// Uses an already migrated pool with bounded statement deadlines.
    pub const fn new(pool: PgPool) -> Self {
        Self(pool)
    }
    async fn saved(
        &self,
        tenant: &str,
        id: &str,
    ) -> Result<Option<(Value, Value)>, PreparationError> {
        let row=sqlx::query!("SELECT request, selection FROM analytics.prepared_evaluations WHERE tenant_id=$1 AND evaluation_id=$2",tenant,id).fetch_optional(&self.0).await.map_err(|_|PreparationError)?;
        Ok(row.map(|row| (row.request, row.selection)))
    }
}
impl CalculatorPreparation for PostgresCalculatorPreparation {
    type Error = PreparationError;
    async fn prepare(
        &self,
        request: &CalculatorRunRequest,
    ) -> Result<PreparedCalculatorEvaluation, Self::Error> {
        CalculatorInputManifest::new(request.window, request.known_at, vec![])
            .map_err(|_| PreparationError)?;
        if !(1..=1024).contains(&request.minimum_samples)
            || request.evaluation.event_at() != request.window.end()
            || request.evaluation.evaluated_at() < request.known_at
        {
            return Err(PreparationError);
        }
        let tenant = request.evaluation.tenant_id().as_str();
        let id = request.evaluation.id().as_str();
        let encoded = request_json(request);
        if let Some((original, selection)) = self.saved(tenant, id).await? {
            if original != encoded {
                return Err(PreparationError);
            }
            return decode(request, &selection);
        }
        sqlx::query!(r#"
            WITH candidate AS (
              SELECT jsonb_build_object('implementation',d.implementation,'output',jsonb_build_object('id',p.property_id,'version',p.version,'name',p.display_name,'unit',p.canonical_unit),
                'samples', COALESCE((
                  SELECT jsonb_agg(jsonb_build_array(s.evidence,s.event_at_ms,s.value) ORDER BY s.event_at_ms,s.evidence COLLATE "C") FROM (
                    SELECT analytics.semantic_evidence_id(h.tenant_id,h.message_id,h.protocol_package_id,h.protocol_package_version,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis) AS evidence,
                      h.event_at_ms, CASE WHEN o.quality='good' AND o.value_kind IN ('integer','decimal')
                        AND length(COALESCE(o.value_integer::text,o.value_decimal::text))<=4096
                        THEN jsonb_build_array(o.value_kind,COALESCE(o.value_integer::text,o.value_decimal::text)) ELSE NULL END AS value
                    FROM history.asset_property_changes h
                    JOIN metadata.signal_binding_revisions b ON b.tenant_id=h.tenant_id AND b.binding_id=h.binding_id AND b.revision=h.binding_revision
                    JOIN ingestion.decoded_observations o ON o.tenant_id=h.tenant_id AND o.message_id=h.message_id AND o.protocol_package_id=h.protocol_package_id AND o.protocol_package_version=h.protocol_package_version AND o.output_index=h.output_index
                    WHERE h.tenant_id=$1 AND b.target_asset_id=($3::jsonb)->>'scope' AND b.target_property_id=d.input_id AND b.target_property_version=d.input_version
                      AND h.event_at_ms>=(($3::jsonb)->>'start')::bigint AND h.event_at_ms<(($3::jsonb)->>'end')::bigint AND h.resolution_known_at_ms<=(($3::jsonb)->>'known')::bigint
                    ORDER BY h.event_at_ms,h.message_id,h.output_index,h.binding_id,h.binding_revision,h.event_time_basis LIMIT 1025
                  ) s
                ),'[]'::jsonb)) AS selection
              FROM analytics.calculator_definitions d
              JOIN metadata.property_definitions p ON p.tenant_id=d.tenant_id AND p.property_id=d.output_id AND p.version=d.output_version
              JOIN metadata.assets a ON a.tenant_id=d.tenant_id AND a.asset_id=($3::jsonb)->>'target'
              JOIN metadata.asset_type_properties ap ON ap.tenant_id=a.tenant_id AND ap.type_id=a.type_id AND ap.type_version=a.type_version AND ap.property_id=p.property_id AND ap.property_version=p.version
              WHERE d.tenant_id=$1 AND d.calculator_id=($3::jsonb)->>'calculator' AND d.calculator_version=($3::jsonb)->>'version' AND d.implementation IN ('window-mean-v1','window-mad-v1')
                AND EXISTS(SELECT 1 FROM metadata.assets scope WHERE scope.tenant_id=$1 AND scope.asset_id=($3::jsonb)->>'scope')
            )
            INSERT INTO analytics.prepared_evaluations(tenant_id,evaluation_id,request,selection)
            SELECT $1,$2,$3,selection FROM candidate WHERE jsonb_array_length(selection->'samples')<=1024
            ON CONFLICT DO NOTHING
        "#,tenant,id,encoded).execute(&self.0).await.map_err(|_|PreparationError)?;
        let (original, selection) = self.saved(tenant, id).await?.ok_or(PreparationError)?;
        if original != encoded {
            return Err(PreparationError);
        }
        decode(request, &selection)
    }
}
fn decode(
    request: &CalculatorRunRequest,
    selection: &Value,
) -> Result<PreparedCalculatorEvaluation, PreparationError> {
    let parse = || -> Result<PreparedCalculatorEvaluation, Box<dyn std::error::Error>> {
        let output = &selection["output"];
        let property = PropertyDefinition::new(
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new(output["id"].as_str().ok_or(PreparationError)?)?,
                DefinitionVersion::new(u32::try_from(
                    output["version"].as_u64().ok_or(PreparationError)?,
                )?)?,
            ),
            DefinitionName::new(output["name"].as_str().ok_or(PreparationError)?)?,
            PropertyValueKind::Decimal,
            output["unit"].as_str().map(UnitId::new).transpose()?,
        )?;
        let mut inputs = Vec::new();
        let mut samples = Vec::new();
        for sample in selection["samples"].as_array().ok_or(PreparationError)? {
            inputs.push(CalculatorInput::new(
                CalculatorEvidenceId::new(sample[0].as_str().ok_or(PreparationError)?)?,
                UnixTimeMillis::new(sample[1].as_i64().ok_or(PreparationError)?),
            ));
            samples.push(match sample[2][0].as_str() {
                Some("integer") => Some(PropertyValue::Integer(
                    sample[2][1].as_str().ok_or(PreparationError)?.parse()?,
                )),
                Some("decimal") => Some(PropertyValue::Decimal(
                    sample[2][1].as_str().ok_or(PreparationError)?.parse()?,
                )),
                _ => None,
            });
        }
        Ok(PreparedCalculatorEvaluation {
            request: request.clone(),
            // All preparations written before MAD admission were window-mean-v1.
            implementation: CalculatorImplementation::parse(
                match selection.get("implementation") {
                    None => "window-mean-v1",
                    Some(value) => value.as_str().ok_or(PreparationError)?,
                },
            )?,
            manifest: CalculatorInputManifest::new(request.window, request.known_at, inputs)?,
            samples,
            output: property,
        })
    };
    parse().map_err(|_| PreparationError)
}
/// Bounded invalid/overflow/conflicting preparation or database failure.
#[derive(Clone, Copy, Debug)]
pub struct PreparationError;
impl std::fmt::Display for PreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("calculator preparation rejected")
    }
}
impl std::error::Error for PreparationError {}

pub(crate) fn request_json(request: &CalculatorRunRequest) -> Value {
    json!({"scope":request.evaluation.scope_asset_id().as_str(),"calculator":request.evaluation.calculator().id().as_str(),"version":request.evaluation.calculator().version().as_str(),"configuration":request.configuration.as_str(),"start":request.window.start().get(),"end":request.window.end().get(),"finalize":request.window.finalize_at().get(),"known":request.known_at.get(),"evaluated":request.evaluation.evaluated_at().get(),"minimum":request.minimum_samples,"target":request.target.as_str()})
}
