//! One-shot first-party execution; clocks, selection and durability stay at ports.
use crate::{
    CalculatedProperty, CalculatorConfigurationId, CalculatorEvaluation,
    CalculatorEvaluationRecord, CalculatorEvaluationStore, CalculatorExecutionFailure,
    CalculatorExecutionKey, CalculatorExecutionOutcome, CalculatorImplementation,
    CalculatorInputManifest, CalculatorWindow, EvaluationAppendOutcome, assemble_outputs,
};
use bigdecimal::BigDecimal;
use fleetiq_domain::{AssetId, PropertyDefinition, PropertyValue, UnixTimeMillis};
use std::{error::Error, fmt};

/// Host-selected logical request. Retries retain every field, including first time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorRunRequest {
    /// Evaluation identity, tenant, scope, release and first processing time.
    pub evaluation: CalculatorEvaluation,
    /// Immutable configuration identity whose parameters are pinned by the store.
    pub configuration: CalculatorConfigurationId,
    /// Finalized half-open event-time selection.
    pub window: CalculatorWindow,
    /// Recorded-time visibility cutoff; not a wall-clock source.
    pub known_at: UnixTimeMillis,
    /// Required sample count, from 1 through 1024.
    pub minimum_samples: u16,
    /// Tenant-owned asset receiving the exact registered output property.
    pub target: AssetId,
}
/// Durable preparation returned only after committing the exact selection.
#[derive(Clone, Debug)]
pub struct PreparedCalculatorEvaluation {
    /// Original request, compared on every retry.
    pub request: CalculatorRunRequest,
    /// Reviewed ABI pinned by the immutable registered release.
    pub implementation: CalculatorImplementation,
    /// Canonical exact semantic references.
    pub manifest: CalculatorInputManifest,
    /// Values in the same canonical order; None is unusable quality/type/size.
    pub samples: Vec<Option<PropertyValue>>,
    /// Exact registered decimal output definition, including canonical unit.
    pub output: PropertyDefinition,
}
/// Consistent bounded selection and durable preparation; never reselect a retry.
pub trait CalculatorPreparation: Send + Sync {
    /// Bounded source, identity collision or storage failure.
    type Error: Error + Send + Sync + 'static;
    /// Commits at most 1024 exact inputs before returning algorithm material.
    /// # Errors
    /// Rejects overflow, incompatible targets, changed requests and storage failure.
    fn prepare<'a>(
        &'a self,
        request: &'a CalculatorRunRequest,
    ) -> impl std::future::Future<Output = Result<PreparedCalculatorEvaluation, Self::Error>> + Send + 'a;
}
/// Computes the reviewed mean: exact sum, then truncation toward zero at 18 places.
/// Numeric coefficients are bounded to 1024 digits and scale to +/-256.
/// # Errors
/// Rejects malformed host context; insufficient/unusable samples produce a
/// terminal insufficient outcome, numeric resource excess a bounded failure.
pub fn calculate_mean(
    prepared: PreparedCalculatorEvaluation,
) -> Result<CalculatorEvaluationRecord, CalculatorRunError> {
    if prepared.implementation != CalculatorImplementation::WindowMeanV1 {
        return Err(CalculatorRunError::Context);
    }
    let request = prepared.request;
    if prepared.samples.len() != prepared.manifest.inputs().len()
        || request.window != prepared.manifest.window()
        || request.known_at != prepared.manifest.known_at()
    {
        return Err(CalculatorRunError::Context);
    }
    let mut outcome = CalculatorExecutionOutcome::InsufficientEvidence;
    let mut properties = Vec::new();
    if !prepared.samples.is_empty()
        && prepared.samples.len() >= usize::from(request.minimum_samples)
        && prepared.samples.iter().all(Option::is_some)
    {
        let mut sum = BigDecimal::from(0);
        let mut limited = false;
        for sample in prepared.samples.iter().flatten() {
            let value = match sample {
                PropertyValue::Integer(value) => BigDecimal::from(*value),
                PropertyValue::Decimal(value) => value.clone(),
                _ => return Err(CalculatorRunError::Context),
            };
            let (coefficient, scale) = value.as_bigint_and_exponent();
            if !(-256..=256).contains(&scale) || coefficient.to_string().len() > 1024 {
                limited = true;
                break;
            }
            sum += value;
        }
        if limited {
            outcome = CalculatorExecutionOutcome::Failed(CalculatorExecutionFailure::ResourceLimit);
        } else {
            let (coefficient, _) = sum.with_scale(18).into_bigint_and_exponent();
            let count =
                i64::try_from(prepared.samples.len()).map_err(|_| CalculatorRunError::Context)?;
            let mean = BigDecimal::new(coefficient / count, 18);
            properties.push(
                CalculatedProperty::new(
                    request.target.clone(),
                    &prepared.output,
                    PropertyValue::Decimal(mean),
                    prepared.output.canonical_unit().cloned(),
                )
                .map_err(|_| CalculatorRunError::Context)?,
            );
            outcome = CalculatorExecutionOutcome::Complete;
        }
    }
    finish(request, prepared.manifest, outcome, properties)
}
pub(crate) fn finish(
    request: CalculatorRunRequest,
    manifest: CalculatorInputManifest,
    outcome: CalculatorExecutionOutcome,
    properties: Vec<CalculatedProperty>,
) -> Result<CalculatorEvaluationRecord, CalculatorRunError> {
    let key = CalculatorExecutionKey::new(
        request.evaluation.tenant_id().clone(),
        request.evaluation.scope_asset_id().clone(),
        request.evaluation.calculator().clone(),
        request.configuration,
        manifest,
    );
    let outputs = assemble_outputs(&request.evaluation, properties)
        .map_err(|_| CalculatorRunError::Context)?;
    CalculatorEvaluationRecord::new(
        key,
        request.evaluation,
        outcome,
        outputs,
        request.minimum_samples,
    )
    .map_err(|_| CalculatorRunError::Context)
}
/// Dispatches the exact admitted first-party ABI, preserving terminal evidence.
/// # Errors
/// Rejects incompatible host context; missing data never becomes a healthy flag.
pub fn calculate_prepared(
    prepared: PreparedCalculatorEvaluation,
) -> Result<CalculatorEvaluationRecord, CalculatorRunError> {
    match prepared.implementation {
        CalculatorImplementation::WindowMeanV1 => return calculate_mean(prepared),
        CalculatorImplementation::WindowMadV1 => {}
    }
    if prepared.samples.len() != prepared.manifest.inputs().len()
        || prepared.request.window != prepared.manifest.window()
        || prepared.request.known_at != prepared.manifest.known_at()
        || prepared.output.canonical_unit().is_some()
    {
        return Err(CalculatorRunError::Context);
    }
    let mut properties = Vec::new();
    let outcome = match crate::detect_mad(&prepared.samples, prepared.request.minimum_samples) {
        Ok(Some(evidence)) => {
            properties.push(
                CalculatedProperty::new(
                    prepared.request.target.clone(),
                    &prepared.output,
                    PropertyValue::Decimal(BigDecimal::from(u8::from(evidence.anomalous))),
                    None,
                )
                .map_err(|_| CalculatorRunError::Context)?,
            );
            CalculatorExecutionOutcome::Complete
        }
        Ok(None) => CalculatorExecutionOutcome::InsufficientEvidence,
        Err(failure) => CalculatorExecutionOutcome::Failed(failure),
    };
    finish(prepared.request, prepared.manifest, outcome, properties)
}
/// Executes one prepared evaluation and atomically appends its terminal record.
/// # Errors
/// Storage ambiguity is returned; retry the identical request to reuse preparation.
pub async fn run_calculator_once<P: CalculatorPreparation, S: CalculatorEvaluationStore>(
    preparation: &P,
    store: &S,
    request: &CalculatorRunRequest,
) -> Result<EvaluationAppendOutcome, CalculatorRunError> {
    let prepared = preparation
        .prepare(request)
        .await
        .map_err(|_| CalculatorRunError::Preparation)?;
    let record = calculate_prepared(prepared)?;
    store
        .append(&record)
        .await
        .map_err(|_| CalculatorRunError::Storage)
}
/// Safe failure classification without input values or process diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorRunError {
    /// Preparation failed or conflicted; no selection may be silently substituted.
    Preparation,
    /// Invalid/incompatible host context.
    Context,
    /// Terminal append failed; its commit may be ambiguous.
    Storage,
}
impl fmt::Display for CalculatorRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "calculator turn failed: {self:?}")
    }
}
impl Error for CalculatorRunError {}

#[cfg(test)]
mod tests;
