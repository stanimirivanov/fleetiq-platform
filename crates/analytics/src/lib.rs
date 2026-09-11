//! Calculator contracts and analytics policies for `FleetIQ`.
//!
//! The crate defines versioned calculator provenance and deterministic outputs
//! independently of a worker runtime, storage adapter, or particular algorithm.

mod assembly;
mod identifier;
mod output;

pub use assembly::{CalculatorAssemblyError, assemble_output, assemble_outputs};
pub use identifier::{
    AnalyticsIdentifierError, CalculatorEvaluationId, CalculatorId, CalculatorVersion,
};
pub use output::{
    CalculatedProperty, CalculatedPropertyError, CalculatorEvaluation, CalculatorOutput,
    CalculatorOutputId, CalculatorRef,
};
