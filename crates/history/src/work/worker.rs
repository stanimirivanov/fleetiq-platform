//! One bounded semantic turn; clocks, claims and atomic writes belong to ports.
use super::{SemanticTick, SemanticUnresolved, SemanticWorkRepository};
use crate::DecodedObservationView;
use fleetiq_domain::{
    PropertyDefinition, PropertyDefinitionRef, SignalBinding, SignalSource, TenantId,
};
use fleetiq_semantics::{
    AssetPropertyChange, EventTimeBasis, PropertyDefinitionView, ResolutionContext,
    ResolutionOutcome, SignalBindingResolver, SignalBindingSnapshotQuery, SignalBindingView,
    UnresolvedReason,
};
use std::{error::Error, fmt};

/// Coordinates durable claims with the existing read-only semantic policy.
pub struct SemanticWorker<R, O, B, D> {
    repository: R,
    observations: O,
    bindings: B,
    definitions: D,
}
impl<R, O, B, D> SemanticWorker<R, O, B, D> {
    /// Composes consumer ports; does not spawn tasks or infer time policies.
    pub const fn new(repository: R, observations: O, bindings: B, definitions: D) -> Self {
        Self {
            repository,
            observations,
            bindings,
            definitions,
        }
    }
}
impl<
    R: SemanticWorkRepository,
    O: DecodedObservationView,
    B: SignalBindingView,
    D: PropertyDefinitionView,
> SemanticWorker<R, O, B, D>
{
    /// Processes at most one claim and 32 backfill keys.
    ///
    /// # Errors
    /// Storage failures leave the claim recoverable. A pinned selection is
    /// never replaced, including after an ambiguous append or metadata correction.
    pub async fn run_once(&self) -> Result<SemanticTick, SemanticWorkerError> {
        self.repository.seed().await.map_err(storage)?;
        let Some(lease) = self.repository.claim().await.map_err(storage)? else {
            return Ok(SemanticTick::Idle);
        };
        let request = &lease.request;
        let observation = self
            .observations
            .find(request.tenant_id(), request.id())
            .await
            .map_err(storage)?
            .ok_or(SemanticWorkerError::Source)?;
        if observation.tenant_id() != request.tenant_id() || observation.id() != request.id() {
            return Err(SemanticWorkerError::Source);
        }
        let event_at = match request.basis() {
            EventTimeBasis::DeviceRecorded => observation
                .device_recorded_at()
                .ok_or(SemanticWorkerError::Source)?,
            EventTimeBasis::ReceivedAt => observation.received_at(),
        };
        let resolver = SignalBindingResolver::new(
            SelectedBindings {
                view: &self.bindings,
                selected: lease.selected.as_ref(),
            },
            Definitions(&self.definitions),
        );
        let outcome = resolver
            .resolve(
                observation,
                ResolutionContext::new(event_at, request.known_at()),
            )
            .await
            .map_err(storage)?;
        let bound = match outcome {
            ResolutionOutcome::Bound(bound) => bound,
            ResolutionOutcome::Unresolved(unresolved) => {
                // An immutable saved selection becoming incompatible is corruption,
                // not permission to erase or replace its original decision.
                if lease.selected.is_some() {
                    return Err(SemanticWorkerError::Source);
                }
                let reason = match unresolved.reason() {
                    UnresolvedReason::NoEffectiveBinding => SemanticUnresolved::NoBinding,
                    UnresolvedReason::AmbiguousBindings { .. } => SemanticUnresolved::Ambiguous,
                    UnresolvedReason::MissingPropertyDefinition { .. } => {
                        SemanticUnresolved::MissingDefinition
                    }
                    UnresolvedReason::ValueKindMismatch { .. } => SemanticUnresolved::ValueKind,
                    UnresolvedReason::UnitConversionRequired { .. } => SemanticUnresolved::Unit,
                };
                return self
                    .repository
                    .retain(&lease, reason)
                    .await
                    .map_err(storage)
                    .map(|saved| {
                        if saved {
                            SemanticTick::Unresolved
                        } else {
                            SemanticTick::Fenced
                        }
                    });
            }
        };
        let change = AssetPropertyChange::new(bound, request.basis()).map_err(storage)?;
        if !self
            .repository
            .pin(&lease, &change)
            .await
            .map_err(storage)?
        {
            return Ok(SemanticTick::Fenced);
        }
        self.repository
            .complete(&lease, &change)
            .await
            .map_err(storage)
            .map(|saved| {
                if saved {
                    SemanticTick::Completed
                } else {
                    SemanticTick::Fenced
                }
            })
    }
}
struct SelectedBindings<'a, B> {
    view: &'a B,
    selected: Option<&'a SignalBinding>,
}
impl<B: SignalBindingView> SignalBindingView for SelectedBindings<'_, B> {
    type Error = B::Error;
    async fn find_for_source(
        &self,
        tenant: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> Result<Vec<SignalBinding>, Self::Error> {
        if let Some(selected) = self.selected {
            return Ok(vec![selected.clone()]);
        }
        self.view.find_for_source(tenant, source, query).await
    }
}
struct Definitions<'a, D>(&'a D);
impl<D: PropertyDefinitionView> PropertyDefinitionView for Definitions<'_, D> {
    type Error = D::Error;
    async fn find_property(
        &self,
        tenant: &TenantId,
        reference: &PropertyDefinitionRef,
    ) -> Result<Option<PropertyDefinition>, Self::Error> {
        self.0.find_property(tenant, reference).await
    }
}
/// Redacted worker failure; causes remain available for internal classification.
pub enum SemanticWorkerError {
    /// A source or saved decision disagreed with immutable durable facts.
    Source,
    /// Storage or semantic policy could not complete; the lease remains recoverable.
    Operation(Box<dyn Error + Send + Sync>),
}
fn storage(error: impl Error + Send + Sync + 'static) -> SemanticWorkerError {
    SemanticWorkerError::Operation(Box::new(error))
}
impl fmt::Display for SemanticWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("semantic worker failed; durable work remains retained")
    }
}
impl fmt::Debug for SemanticWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for SemanticWorkerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source => None,
            Self::Operation(error) => Some(error.as_ref()),
        }
    }
}
