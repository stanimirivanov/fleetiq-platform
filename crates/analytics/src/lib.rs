//! Calculator contracts and analytics policies for `FleetIQ`.
//!
//! The crate defines versioned calculator provenance and deterministic outputs
//! independently of a worker runtime, storage adapter, or particular algorithm.

mod execution;
pub use execution::{
    CalculatorExecutionFailure, CalculatorExecutionKey, CalculatorExecutionOutcome,
    CalculatorInput, CalculatorInputManifest, CalculatorWindow, ExecutionContractError,
    MAX_CALCULATOR_INPUTS, MAX_CALCULATOR_WINDOW_MILLIS,
};

mod assembly;
mod identifier;
mod output;

pub use assembly::{CalculatorAssemblyError, assemble_output, assemble_outputs};
pub use identifier::{
    AnalyticsIdentifierError, CalculatorConfigurationId, CalculatorEvaluationId,
    CalculatorEvidenceId, CalculatorId, CalculatorVersion,
};
pub use output::{
    CalculatedProperty, CalculatedPropertyError, CalculatorEvaluation, CalculatorOutput,
    CalculatorOutputId, CalculatorRef,
};

mod definitions;
pub use definitions::{
    CalculatorDefinition, CalculatorDefinitionRegistry, CalculatorImplementation,
    CalculatorRegistrationOutcome, UnknownCalculatorImplementation,
};

mod evaluation_store;
pub use evaluation_store::{
    CalculatorEvaluationRecord, CalculatorEvaluationStore, EvaluationAppendOutcome,
    InvalidCalculatorEvaluation, MAX_CALCULATOR_OUTPUTS,
};

mod runner;
pub use runner::{
    CalculatorPreparation, CalculatorRunError, CalculatorRunRequest, PreparedCalculatorEvaluation,
    calculate_mean, run_calculator_once,
};

mod work;
pub use work::{CalculatorSchedule, CalculatorWorkLease};
