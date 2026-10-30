//! Atomic terminal evaluation evidence, independent of persistence technology.
use crate::{
    CalculatorEvaluation, CalculatorExecutionKey, CalculatorExecutionOutcome, CalculatorOutput,
};
use std::{error::Error, fmt};

/// Maximum ordered outputs in one atomic evaluation envelope.
pub const MAX_CALCULATOR_OUTPUTS: usize = 1024;
/// Validated terminal record; persistence must verify all durable references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorEvaluationRecord {
    key: CalculatorExecutionKey,
    evaluation: CalculatorEvaluation,
    outcome: CalculatorExecutionOutcome,
    outputs: Vec<CalculatorOutput>,
    minimum_samples: u16,
}
impl CalculatorEvaluationRecord {
    /// Validates host context and deterministic output order before persistence.
    /// # Errors
    /// Rejects mismatched context, premature processing, partial failed results,
    /// oversized output batches and noncontiguous or foreign output identities.
    pub fn new(
        key: CalculatorExecutionKey,
        evaluation: CalculatorEvaluation,
        outcome: CalculatorExecutionOutcome,
        outputs: Vec<CalculatorOutput>,
        minimum_samples: u16,
    ) -> Result<Self, InvalidCalculatorEvaluation> {
        if !(1..=1024).contains(&minimum_samples)
            || evaluation.tenant_id() != key.tenant()
            || evaluation.scope_asset_id() != key.scope()
            || evaluation.calculator() != key.calculator()
            || evaluation.event_at() != key.manifest().window().end()
            || evaluation.evaluated_at() < key.manifest().known_at()
            || outputs.len() > MAX_CALCULATOR_OUTPUTS
            || (outcome != CalculatorExecutionOutcome::Complete && !outputs.is_empty())
        {
            return Err(InvalidCalculatorEvaluation);
        }
        for (index, output) in outputs.iter().enumerate() {
            if output.evaluation() != &evaluation
                || usize::try_from(output.id().output_index()).ok() != Some(index)
            {
                return Err(InvalidCalculatorEvaluation);
            }
        }
        Ok(Self {
            key,
            evaluation,
            outcome,
            outputs,
            minimum_samples,
        })
    }
    /// Returns the pinned mean configuration minimum sample count (1..=1024).
    pub const fn minimum_samples(&self) -> u16 {
        self.minimum_samples
    }
    /// Returns the complete logical replay key, excluding attempt time.
    pub const fn key(&self) -> &CalculatorExecutionKey {
        &self.key
    }
    /// Returns the first host-owned evaluation context, retained on retries.
    pub const fn evaluation(&self) -> &CalculatorEvaluation {
        &self.evaluation
    }
    /// Returns terminal classification; empty completion is not insufficient data.
    pub const fn outcome(&self) -> CalculatorExecutionOutcome {
        self.outcome
    }
    /// Returns outputs whose identities are evaluation/release/array position.
    pub fn outputs(&self) -> &[CalculatorOutput] {
        &self.outputs
    }
}
/// Host context or ordered output batch is inconsistent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidCalculatorEvaluation;
impl fmt::Display for InvalidCalculatorEvaluation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid calculator evaluation record")
    }
}
impl Error for InvalidCalculatorEvaluation {}
/// Result of atomically appending all terminal evidence and outputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationAppendOutcome {
    /// The complete envelope was appended.
    Appended,
    /// Identical immutable content already exists under the same identity.
    AlreadyPresent,
}
/// Durable append boundary; a retry must retain exact evidence and context.
pub trait CalculatorEvaluationStore: Send + Sync {
    /// Storage, reference-validation or identity-collision failure.
    type Error: Error + Send + Sync + 'static;
    /// Atomically appends the envelope or compares every immutable field.
    /// # Errors
    /// Rejects missing/foreign provenance, identity reuse and storage failures.
    fn append<'a>(
        &'a self,
        record: &'a CalculatorEvaluationRecord,
    ) -> impl std::future::Future<Output = Result<EvaluationAppendOutcome, Self::Error>> + Send + 'a;
}
