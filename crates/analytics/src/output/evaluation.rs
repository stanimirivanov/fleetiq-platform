use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};

use crate::{CalculatorEvaluationId, CalculatorId, CalculatorVersion};

/// Pins calculator provenance to one immutable implementation release.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CalculatorRef {
    id: CalculatorId,
    version: CalculatorVersion,
}

impl CalculatorRef {
    /// Constructs an exact calculator reference.
    pub const fn new(id: CalculatorId, version: CalculatorVersion) -> Self {
        Self { id, version }
    }

    /// Returns the stable calculator identity.
    pub const fn id(&self) -> &CalculatorId {
        &self.id
    }

    /// Returns the immutable calculator implementation version.
    pub const fn version(&self) -> &CalculatorVersion {
        &self.version
    }
}

/// Host-owned context for one logical calculator evaluation.
///
/// The evaluation ID links outputs to the durable configuration and input
/// selection recorded by the orchestrator. Event time describes the logical
/// instant or window boundary being evaluated; evaluated time records when the
/// host performed the work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatorEvaluation {
    id: CalculatorEvaluationId,
    tenant_id: TenantId,
    scope_asset_id: AssetId,
    calculator: CalculatorRef,
    event_at: UnixTimeMillis,
    evaluated_at: UnixTimeMillis,
}

impl CalculatorEvaluation {
    /// Constructs host-owned context for a logical calculator evaluation.
    pub const fn new(
        id: CalculatorEvaluationId,
        tenant_id: TenantId,
        scope_asset_id: AssetId,
        calculator: CalculatorRef,
        event_at: UnixTimeMillis,
        evaluated_at: UnixTimeMillis,
    ) -> Self {
        Self {
            id,
            tenant_id,
            scope_asset_id,
            calculator,
            event_at,
            evaluated_at,
        }
    }

    /// Returns the logical evaluation identity retained across retries.
    pub const fn id(&self) -> &CalculatorEvaluationId {
        &self.id
    }

    /// Returns the tenant whose data is being evaluated.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the asset that scopes calculator state and input selection.
    pub const fn scope_asset_id(&self) -> &AssetId {
        &self.scope_asset_id
    }

    /// Returns the exact calculator release used for the evaluation.
    pub const fn calculator(&self) -> &CalculatorRef {
        &self.calculator
    }

    /// Returns the logical event-time instant or window boundary evaluated.
    pub const fn event_at(&self) -> UnixTimeMillis {
        self.event_at
    }

    /// Returns when the host performed the evaluation.
    pub const fn evaluated_at(&self) -> UnixTimeMillis {
        self.evaluated_at
    }
}
