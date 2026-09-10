use fleetiq_domain::{
    PropertyDefinitionRef, PropertyValueKind, RecordRevision, SignalBindingId, SignalTarget,
    UnitId, UnixTimeMillis,
};
use fleetiq_telemetry::{DecodedObservation, DecodedObservationId};

/// Selects the physical and recorded-time view used for semantic attribution.
///
/// The caller supplies effective time because choosing between device and
/// receipt time is an operational policy. Knowledge time remains explicit so
/// replay can reproduce either an original or corrected historical view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolutionContext {
    effective_at: UnixTimeMillis,
    known_at: UnixTimeMillis,
}

impl ResolutionContext {
    /// Constructs a bitemporal semantic-resolution context.
    pub const fn new(effective_at: UnixTimeMillis, known_at: UnixTimeMillis) -> Self {
        Self {
            effective_at,
            known_at,
        }
    }

    /// Returns the physical-time instant used to select a binding.
    pub const fn effective_at(self) -> UnixTimeMillis {
        self.effective_at
    }

    /// Returns the inclusive recorded-time cutoff used to select a binding.
    pub const fn known_at(self) -> UnixTimeMillis {
        self.known_at
    }
}

/// Identifies the exact signal-binding revision used for attribution.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SignalBindingProvenance {
    binding_id: SignalBindingId,
    revision: RecordRevision,
}

impl SignalBindingProvenance {
    pub(super) fn new(binding_id: SignalBindingId, revision: RecordRevision) -> Self {
        Self {
            binding_id,
            revision,
        }
    }

    /// Returns the stable binding identity.
    pub const fn binding_id(&self) -> &SignalBindingId {
        &self.binding_id
    }

    /// Returns the immutable binding revision used for attribution.
    pub const fn revision(&self) -> RecordRevision {
        self.revision
    }
}

/// A decoded observation attributed to one exact semantic asset property.
///
/// The decoded observation remains embedded so source lineage, decoder
/// provenance, timestamps, quality, and value survive attribution. Exact kind
/// and unit compatibility has been verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundObservation {
    observation: DecodedObservation,
    target: SignalTarget,
    binding: SignalBindingProvenance,
    context: ResolutionContext,
}

impl BoundObservation {
    pub(super) fn new(
        observation: DecodedObservation,
        target: SignalTarget,
        binding: SignalBindingProvenance,
        context: ResolutionContext,
    ) -> Self {
        Self {
            observation,
            target,
            binding,
            context,
        }
    }

    /// Returns the decoded observation identity for downstream idempotency.
    pub const fn id(&self) -> &DecodedObservationId {
        self.observation.id()
    }

    /// Returns the decoded observation and its complete source provenance.
    pub const fn observation(&self) -> &DecodedObservation {
        &self.observation
    }

    /// Returns the semantic asset and property receiving this observation.
    pub const fn target(&self) -> &SignalTarget {
        &self.target
    }

    /// Returns the exact binding revision used for attribution.
    pub const fn binding(&self) -> &SignalBindingProvenance {
        &self.binding
    }

    /// Returns the bitemporal context in which attribution was resolved.
    pub const fn context(&self) -> ResolutionContext {
        self.context
    }
}

/// Identifies one competing binding revision in an ambiguous resolution.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BindingCandidate {
    binding_id: SignalBindingId,
    revision: RecordRevision,
}

impl BindingCandidate {
    pub(super) fn new(binding_id: SignalBindingId, revision: RecordRevision) -> Self {
        Self {
            binding_id,
            revision,
        }
    }

    /// Returns the stable candidate binding identity.
    pub const fn binding_id(&self) -> &SignalBindingId {
        &self.binding_id
    }

    /// Returns the candidate's immutable revision.
    pub const fn revision(&self) -> RecordRevision {
        self.revision
    }
}

/// Explains why a decoded observation could not be attributed confidently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnresolvedReason {
    /// No binding selected this source in the requested bitemporal snapshot.
    NoEffectiveBinding,
    /// More than one binding selected this source and no authority policy exists.
    AmbiguousBindings {
        /// Deterministically ordered competing binding revisions.
        candidates: Vec<BindingCandidate>,
    },
    /// The binding referenced a property unavailable from the definition view.
    MissingPropertyDefinition {
        /// Exact property definition required by the binding.
        property: PropertyDefinitionRef,
    },
    /// The decoded value kind differs from the target property's required kind.
    ValueKindMismatch {
        /// Kind emitted by the decoder.
        actual: PropertyValueKind,
        /// Kind required by the exact target property definition.
        expected: PropertyValueKind,
    },
    /// Source and canonical target units differ and no conversion was selected.
    UnitConversionRequired {
        /// Unit emitted by the decoder, or `None` when absent or unitless.
        source: Option<UnitId>,
        /// Canonical target unit, or `None` for a unitless property.
        target: Option<UnitId>,
    },
}

/// Preserves a decoded observation that requires configuration or policy work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedObservation {
    observation: DecodedObservation,
    reason: UnresolvedReason,
    context: ResolutionContext,
}

impl UnresolvedObservation {
    pub(super) fn new(
        observation: DecodedObservation,
        reason: UnresolvedReason,
        context: ResolutionContext,
    ) -> Self {
        Self {
            observation,
            reason,
            context,
        }
    }

    /// Returns the decoded observation retained for diagnosis or retry.
    pub const fn observation(&self) -> &DecodedObservation {
        &self.observation
    }

    /// Returns the configuration or compatibility reason blocking attribution.
    pub const fn reason(&self) -> &UnresolvedReason {
        &self.reason
    }

    /// Returns the bitemporal context of the failed resolution attempt.
    pub const fn context(&self) -> ResolutionContext {
        self.context
    }
}

/// Result of semantic attribution for one decoded observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionOutcome {
    /// Exactly one compatible binding produced a semantic observation.
    Bound(BoundObservation),
    /// Attribution requires missing configuration or an explicit later policy.
    Unresolved(UnresolvedObservation),
}
