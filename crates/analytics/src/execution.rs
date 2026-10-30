//! Bounded, host-selected inputs and replay keys; no clock, storage or runtime.
use std::{collections::BTreeSet, error::Error, fmt};

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};

use crate::{CalculatorConfigurationId, CalculatorEvidenceId, CalculatorRef};

/// Maximum immutable evidence references in one evaluation.
pub const MAX_CALCULATOR_INPUTS: usize = 1024;
/// Maximum logical window span: seven days in milliseconds.
pub const MAX_CALCULATOR_WINDOW_MILLIS: i64 = 604_800_000;

/// A nonempty half-open event-time interval with an explicit finalization time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalculatorWindow {
    start: UnixTimeMillis,
    end: UnixTimeMillis,
    finalize_at: UnixTimeMillis,
}
impl CalculatorWindow {
    /// Pins a window and its allowed-lateness boundary.
    ///
    /// # Errors
    /// Rejects reversed/empty/overlong intervals and finalization before the end
    /// or more than seven days after it. Arithmetic never wraps.
    pub fn new(
        start: UnixTimeMillis,
        end: UnixTimeMillis,
        finalize_at: UnixTimeMillis,
    ) -> Result<Self, ExecutionContractError> {
        let span = end.get().checked_sub(start.get());
        let lateness = finalize_at.get().checked_sub(end.get());
        if !span.is_some_and(|span| (1..=MAX_CALCULATOR_WINDOW_MILLIS).contains(&span))
            || !lateness.is_some_and(|late| (0..=MAX_CALCULATOR_WINDOW_MILLIS).contains(&late))
        {
            return Err(ExecutionContractError::InvalidWindow);
        }
        Ok(Self {
            start,
            end,
            finalize_at,
        })
    }
    /// Returns the inclusive event-time lower bound.
    pub const fn start(self) -> UnixTimeMillis {
        self.start
    }
    /// Returns the exclusive event-time upper bound and logical output time.
    pub const fn end(self) -> UnixTimeMillis {
        self.end
    }
    /// Returns the earliest knowledge cutoff that may finalize this window.
    pub const fn finalize_at(self) -> UnixTimeMillis {
        self.finalize_at
    }
}

/// Reference to an immutable semantic fact, not a value or mutable latest row.
///
/// The adapter must verify exact content, tenant ownership and visibility at the
/// manifest cutoff. An opaque ID must encode the full durable fact identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorInput {
    id: CalculatorEvidenceId,
    event_at: UnixTimeMillis,
}
impl CalculatorInput {
    /// References one exact fact and its event time; the host verifies both.
    pub const fn new(id: CalculatorEvidenceId, event_at: UnixTimeMillis) -> Self {
        Self { id, event_at }
    }
    /// Returns the immutable evidence identity.
    pub const fn id(&self) -> &CalculatorEvidenceId {
        &self.id
    }
    /// Returns the fact's event time.
    pub const fn event_at(&self) -> UnixTimeMillis {
        self.event_at
    }
}

/// Complete bounded selection, canonicalized by event time then evidence ID.
///
/// Overflow is rejection, never truncation. Empty selection is valid evidence;
/// it does not imply successful calculation. The host supplies one consistent
/// recorded-time snapshot, rather than independently queried latest values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorInputManifest {
    window: CalculatorWindow,
    known_at: UnixTimeMillis,
    inputs: Vec<CalculatorInput>,
}
impl CalculatorInputManifest {
    /// Validates and canonicalizes a selected snapshot without reading storage.
    ///
    /// # Errors
    /// Rejects premature cutoffs, oversized selections, duplicate identities
    /// (even at different times), and evidence outside the half-open window.
    pub fn new(
        window: CalculatorWindow,
        known_at: UnixTimeMillis,
        mut inputs: Vec<CalculatorInput>,
    ) -> Result<Self, ExecutionContractError> {
        if known_at < window.finalize_at {
            return Err(ExecutionContractError::PrematureSnapshot);
        }
        if inputs.len() > MAX_CALCULATOR_INPUTS {
            return Err(ExecutionContractError::TooManyInputs);
        }
        let mut ids = BTreeSet::new();
        for input in &inputs {
            if input.event_at < window.start || input.event_at >= window.end {
                return Err(ExecutionContractError::OutsideWindow);
            }
            if !ids.insert(&input.id) {
                return Err(ExecutionContractError::DuplicateEvidence);
            }
        }
        inputs.sort_by(|a, b| (a.event_at, &a.id).cmp(&(b.event_at, &b.id)));
        Ok(Self {
            window,
            known_at,
            inputs,
        })
    }
    /// Returns the finalized logical interval.
    pub const fn window(&self) -> CalculatorWindow {
        self.window
    }
    /// Returns the recorded-time visibility cutoff retained across retries.
    pub const fn known_at(&self) -> UnixTimeMillis {
        self.known_at
    }
    /// Returns canonical immutable references, including an explicitly empty set.
    pub fn inputs(&self) -> &[CalculatorInput] {
        &self.inputs
    }
}

/// Logical replay key; processing time and execution attempt are excluded.
///
/// Durable orchestration maps this exact key to one evaluation ID. A corrected
/// configuration, release, cutoff or selected fact requires a new evaluation;
/// an existing key must never be assigned a second identity on retry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorExecutionKey {
    tenant: TenantId,
    scope: AssetId,
    calculator: CalculatorRef,
    configuration: CalculatorConfigurationId,
    manifest: CalculatorInputManifest,
}
impl CalculatorExecutionKey {
    /// Binds immutable host-selected execution context without choosing an ID.
    /// Configuration identity must reference immutable content, including input
    /// roles, exact property definitions, units and algorithm parameters.
    pub const fn new(
        tenant: TenantId,
        scope: AssetId,
        calculator: CalculatorRef,
        configuration: CalculatorConfigurationId,
        manifest: CalculatorInputManifest,
    ) -> Self {
        Self {
            tenant,
            scope,
            calculator,
            configuration,
            manifest,
        }
    }
    /// Returns the owning tenant; adapters must enforce all evidence references.
    pub const fn tenant(&self) -> &TenantId {
        &self.tenant
    }
    /// Returns the asset that scopes evaluation.
    pub const fn scope(&self) -> &AssetId {
        &self.scope
    }
    /// Returns the exact implementation release.
    pub const fn calculator(&self) -> &CalculatorRef {
        &self.calculator
    }
    /// Returns the immutable configuration identity.
    pub const fn configuration(&self) -> &CalculatorConfigurationId {
        &self.configuration
    }
    /// Returns exact selected evidence and logical time boundaries.
    pub const fn manifest(&self) -> &CalculatorInputManifest {
        &self.manifest
    }
}

/// Terminal evaluation classification, stored separately from ordered outputs.
/// Only complete evaluations may publish outputs, including zero outputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorExecutionOutcome {
    /// Calculation completed; output IDs still use ADR 0007 assembly.
    Complete,
    /// The pinned selection cannot support the calculation; not a healthy value.
    InsufficientEvidence,
    /// Calculation failed without publishing partial output.
    Failed(CalculatorExecutionFailure),
}
/// Bounded failure categories; never persist raw process errors or input values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalculatorExecutionFailure {
    /// Configuration or selected evidence is incompatible with the release.
    IncompatibleInput,
    /// The result cannot be represented by the exact output definition.
    InvalidOutput,
    /// A host-enforced resource budget was exhausted.
    ResourceLimit,
}
/// Invalid execution context; no partially valid manifest is returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionContractError {
    /// Invalid interval or allowed lateness.
    InvalidWindow,
    /// Knowledge cutoff precedes window finalization.
    PrematureSnapshot,
    /// Selection exceeds the reference budget.
    TooManyInputs,
    /// One immutable identity appears more than once.
    DuplicateEvidence,
    /// Event time is outside the half-open interval.
    OutsideWindow,
}
impl fmt::Display for ExecutionContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid calculator execution context: {self:?}")
    }
}
impl Error for ExecutionContractError {}

#[cfg(test)]
mod tests;
