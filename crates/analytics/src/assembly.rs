use std::{error::Error, fmt};

use crate::{CalculatedProperty, CalculatorEvaluation, CalculatorOutput, CalculatorOutputId};

/// Combines one calculated property with host-owned evaluation provenance.
pub fn assemble_output(
    evaluation: &CalculatorEvaluation,
    output_index: u32,
    property: CalculatedProperty,
) -> CalculatorOutput {
    let id = CalculatorOutputId::new(
        evaluation.id().clone(),
        evaluation.calculator().clone(),
        output_index,
    );

    CalculatorOutput::new(id, evaluation.clone(), property)
}

/// Combines ordered calculated properties with host-owned evaluation provenance.
///
/// Sequence order is part of deterministic replay: the same logical evaluation
/// must emit equivalent outputs in the same order.
///
/// # Errors
///
/// Returns [`CalculatorAssemblyError::OutputIndexOverflow`] if the sequence
/// contains more outputs than can be indexed by `u32`.
pub fn assemble_outputs(
    evaluation: &CalculatorEvaluation,
    properties: impl IntoIterator<Item = CalculatedProperty>,
) -> Result<Vec<CalculatorOutput>, CalculatorAssemblyError> {
    properties
        .into_iter()
        .enumerate()
        .map(|(index, property)| {
            let output_index = u32::try_from(index)
                .map_err(|_| CalculatorAssemblyError::OutputIndexOverflow { index })?;
            Ok(assemble_output(evaluation, output_index, property))
        })
        .collect()
}

/// Failure while the host assembles calculator outputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorAssemblyError {
    /// The calculator emitted more outputs than the output ID can index.
    OutputIndexOverflow {
        /// Zero-based output position that cannot be represented.
        index: usize,
    },
}

impl fmt::Display for CalculatorAssemblyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputIndexOverflow { index } => {
                write!(
                    formatter,
                    "calculator output index {index} exceeds u32::MAX"
                )
            }
        }
    }
}

impl Error for CalculatorAssemblyError {}

#[cfg(test)]
mod tests;
