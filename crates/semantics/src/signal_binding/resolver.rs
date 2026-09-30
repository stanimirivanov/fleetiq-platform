use std::{error::Error, fmt};

use fleetiq_domain::{PropertyDefinition, SignalBinding};
use fleetiq_telemetry::DecodedObservation;

use super::{
    BindingCandidate, BoundObservation, PropertyDefinitionView, ResolutionContext,
    ResolutionOutcome, SignalBindingProvenance, SignalBindingSnapshotQuery, SignalBindingView,
    UnresolvedObservation, UnresolvedReason,
};

/// Resolves decoded sources through bitemporal binding and definition views.
#[derive(Clone, Debug)]
pub struct SignalBindingResolver<B, D> {
    bindings: B,
    definitions: D,
}

impl<B, D> SignalBindingResolver<B, D> {
    /// Constructs a resolver from consumer-owned semantic read views.
    pub const fn new(bindings: B, definitions: D) -> Self {
        Self {
            bindings,
            definitions,
        }
    }
}

impl<B, D> SignalBindingResolver<B, D>
where
    B: SignalBindingView,
    D: PropertyDefinitionView,
{
    /// Attributes one decoded observation without guessing missing policy.
    ///
    /// Exactly one effective binding must exist. Value kind and source unit must
    /// equal the exact property's declared kind and canonical unit. Expected
    /// configuration problems return [`ResolutionOutcome::Unresolved`] so the
    /// observation can be diagnosed and retried. Repository failures remain
    /// operational errors.
    ///
    /// # Errors
    ///
    /// Returns [`ResolutionError`] when either repository cannot serve the
    /// requested view.
    pub async fn resolve(
        &self,
        observation: DecodedObservation,
        context: ResolutionContext,
    ) -> Result<ResolutionOutcome, ResolutionError<B::Error, D::Error>> {
        let bindings = self
            .bindings
            .find_for_source(
                observation.tenant_id(),
                observation.source(),
                SignalBindingSnapshotQuery::new(context.effective_at(), context.known_at()),
            )
            .await
            .map_err(ResolutionError::BindingRepository)?;

        let binding = match select_binding(bindings) {
            Ok(binding) => binding,
            Err(reason) => return Ok(unresolved(observation, reason, context)),
        };

        let property_reference = binding.target().property();
        let property = PropertyDefinitionView::find_property(
            &self.definitions,
            observation.tenant_id(),
            property_reference,
        )
        .await
        .map_err(ResolutionError::DefinitionRepository)?;
        let Some(property) = property else {
            return Ok(unresolved(
                observation,
                UnresolvedReason::MissingPropertyDefinition {
                    property: property_reference.clone(),
                },
                context,
            ));
        };

        if let Err(reason) = validate_compatibility(&observation, &property) {
            return Ok(unresolved(observation, reason, context));
        }

        let provenance = SignalBindingProvenance::new(binding.id().clone(), binding.revision());
        let target = binding.target().clone();
        Ok(ResolutionOutcome::Bound(BoundObservation::new(
            observation,
            target,
            provenance,
            context,
        )))
    }
}

/// Selects the single authoritative binding without consulting infrastructure.
fn select_binding(mut bindings: Vec<SignalBinding>) -> Result<SignalBinding, UnresolvedReason> {
    match bindings.len() {
        0 => Err(UnresolvedReason::NoEffectiveBinding),
        1 => bindings.pop().ok_or(UnresolvedReason::NoEffectiveBinding),
        _ => {
            let mut candidates: Vec<_> = bindings
                .into_iter()
                .map(|binding| BindingCandidate::new(binding.id().clone(), binding.revision()))
                .collect();
            candidates.sort_unstable();
            Err(UnresolvedReason::AmbiguousBindings { candidates })
        }
    }
}

/// Applies compatibility rules that need no repository or operational context.
fn validate_compatibility(
    observation: &DecodedObservation,
    property: &PropertyDefinition,
) -> Result<(), UnresolvedReason> {
    let actual_kind = observation.value().kind();
    let expected_kind = property.value_kind();
    if actual_kind != expected_kind {
        return Err(UnresolvedReason::ValueKindMismatch {
            actual: actual_kind,
            expected: expected_kind,
        });
    }

    if observation.source_unit() != property.canonical_unit() {
        return Err(UnresolvedReason::UnitConversionRequired {
            source: observation.source_unit().cloned(),
            target: property.canonical_unit().cloned(),
        });
    }

    Ok(())
}

fn unresolved(
    observation: DecodedObservation,
    reason: UnresolvedReason,
    context: ResolutionContext,
) -> ResolutionOutcome {
    ResolutionOutcome::Unresolved(UnresolvedObservation::new(observation, reason, context))
}

/// Operational failure while loading semantic resolution state.
#[derive(Debug)]
pub enum ResolutionError<B, D> {
    /// The signal-binding repository could not serve the requested snapshot.
    BindingRepository(B),
    /// The semantic-definition repository could not load the target property.
    DefinitionRepository(D),
}

impl<B: fmt::Display, D: fmt::Display> fmt::Display for ResolutionError<B, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindingRepository(error) => {
                write!(formatter, "signal-binding lookup failed: {error}")
            }
            Self::DefinitionRepository(error) => {
                write!(formatter, "property-definition lookup failed: {error}")
            }
        }
    }
}

impl<B: Error + 'static, D: Error + 'static> Error for ResolutionError<B, D> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BindingRepository(error) => Some(error),
            Self::DefinitionRepository(error) => Some(error),
        }
    }
}
