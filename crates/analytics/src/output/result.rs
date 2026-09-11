use crate::{CalculatedProperty, CalculatorEvaluation, CalculatorEvaluationId, CalculatorRef};

/// Deterministically identifies one output within a calculator evaluation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CalculatorOutputId {
    evaluation_id: CalculatorEvaluationId,
    produced_by: CalculatorRef,
    output_index: u32,
}

impl CalculatorOutputId {
    pub(crate) fn new(
        evaluation_id: CalculatorEvaluationId,
        produced_by: CalculatorRef,
        output_index: u32,
    ) -> Self {
        Self {
            evaluation_id,
            produced_by,
            output_index,
        }
    }

    /// Returns the logical evaluation that produced this output.
    pub const fn evaluation_id(&self) -> &CalculatorEvaluationId {
        &self.evaluation_id
    }

    /// Returns the exact calculator release that produced this output.
    pub const fn produced_by(&self) -> &CalculatorRef {
        &self.produced_by
    }

    /// Returns the output's zero-based position within the evaluation.
    pub const fn output_index(&self) -> u32 {
        self.output_index
    }
}

/// One derived asset-property value with host-controlled evaluation provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorOutput {
    id: CalculatorOutputId,
    evaluation: CalculatorEvaluation,
    property: CalculatedProperty,
}

impl CalculatorOutput {
    pub(crate) fn new(
        id: CalculatorOutputId,
        evaluation: CalculatorEvaluation,
        property: CalculatedProperty,
    ) -> Self {
        Self {
            id,
            evaluation,
            property,
        }
    }

    /// Returns this output's deterministic identity.
    pub const fn id(&self) -> &CalculatorOutputId {
        &self.id
    }

    /// Returns the host-owned evaluation context.
    pub const fn evaluation(&self) -> &CalculatorEvaluation {
        &self.evaluation
    }

    /// Returns the calculated semantic property.
    pub const fn property(&self) -> &CalculatedProperty {
        &self.property
    }
}
