//! HTTP shapes and exhaustive tenant action mapping; application types stay transport-free.
use fleetiq_access::TenantAction;
use fleetiq_domain::{ActorId, TenantAccessChangeId, UnixTimeMillis};
use fleetiq_identity::{
    TenantAccessAuditCursor, TenantAccessChangeEffect, TenantAccessChangeKind,
    TenantAccessChangeRequest, TenantGrantCatalogCursor,
};
use serde::{Deserialize, Serialize};

/// Closed HTTP vocabulary. Exhaustive conversions force review when policy adds an action.
#[derive(Clone, Copy, Deserialize, Serialize)]
pub(super) enum Action {
    #[serde(rename = "metadata.read")]
    ReadMetadata,
    #[serde(rename = "telemetry.read")]
    ReadTelemetry,
    #[serde(rename = "metadata.manage")]
    ManageMetadata,
    #[serde(rename = "access.manage")]
    ManageAccess,
    #[serde(rename = "protocol.manage")]
    ManageProtocols,
    #[serde(rename = "analytics.manage")]
    ManageAnalytics,
    #[serde(rename = "alerts.read")]
    ReadAlerts,
    #[serde(rename = "alerts.manage")]
    ManageAlerts,
}
impl From<Action> for TenantAction {
    fn from(action: Action) -> Self {
        match action {
            Action::ReadMetadata => Self::ReadMetadata,
            Action::ReadTelemetry => Self::ReadTelemetry,
            Action::ManageMetadata => Self::ManageMetadata,
            Action::ManageAccess => Self::ManageAccess,
            Action::ManageProtocols => Self::ManageProtocols,
            Action::ManageAnalytics => Self::ManageAnalytics,
            Action::ReadAlerts => Self::ReadAlerts,
            Action::ManageAlerts => Self::ManageAlerts,
        }
    }
}
impl From<TenantAction> for Action {
    fn from(action: TenantAction) -> Self {
        match action {
            TenantAction::ReadMetadata => Self::ReadMetadata,
            TenantAction::ReadTelemetry => Self::ReadTelemetry,
            TenantAction::ManageMetadata => Self::ManageMetadata,
            TenantAction::ManageAccess => Self::ManageAccess,
            TenantAction::ManageProtocols => Self::ManageProtocols,
            TenantAction::ManageAnalytics => Self::ManageAnalytics,
            TenantAction::ReadAlerts => Self::ReadAlerts,
            TenantAction::ManageAlerts => Self::ManageAlerts,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangeBody {
    change_id: String,
    kind: ChangeKind,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChangeKind {
    Grant,
    Revoke,
}
impl ChangeBody {
    pub(super) fn request(
        self,
        actor: String,
        now: UnixTimeMillis,
    ) -> Option<TenantAccessChangeRequest> {
        Some(TenantAccessChangeRequest::for_delivery(
            TenantAccessChangeId::new(self.change_id).ok()?,
            ActorId::new(actor).ok()?,
            self.action.into(),
            match self.kind {
                ChangeKind::Grant => TenantAccessChangeKind::Grant,
                ChangeKind::Revoke => TenantAccessChangeKind::Revoke,
            },
            now,
        ))
    }
}
#[derive(Serialize)]
pub(super) struct ChangeResponse {
    pub(super) outcome: &'static str,
    pub(super) state_changed: bool,
}
pub(super) const fn state_changed(effect: TenantAccessChangeEffect) -> bool {
    match effect {
        TenantAccessChangeEffect::StateChanged => true,
        TenantAccessChangeEffect::NoStateChange => false,
    }
}
pub(super) const fn kind_name(kind: TenantAccessChangeKind) -> &'static str {
    match kind {
        TenantAccessChangeKind::Grant => "grant",
        TenantAccessChangeKind::Revoke => "revoke",
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogParameters {
    pub(super) limit: Option<u16>,
    after_action: Option<Action>,
}
impl CatalogParameters {
    pub(super) fn cursor(self) -> Option<TenantGrantCatalogCursor> {
        self.after_action
            .map(|action| TenantGrantCatalogCursor::new(action.into()))
    }
}
#[derive(Serialize)]
pub(super) struct GrantResponse {
    pub(super) action: Action,
}
#[derive(Serialize)]
pub(super) struct CatalogResponse {
    pub(super) grants: Vec<GrantResponse>,
    pub(super) next_after: Option<Action>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuditParameters {
    pub(super) limit: Option<u16>,
    before_recorded_at_ms: Option<i64>,
    before_change_id: Option<String>,
}
impl AuditParameters {
    pub(super) fn cursor(self) -> Result<Option<TenantAccessAuditCursor>, ()> {
        match (self.before_recorded_at_ms, self.before_change_id) {
            (None, None) => Ok(None),
            (Some(time), Some(id)) => Ok(Some(TenantAccessAuditCursor::new(
                UnixTimeMillis::new(time),
                TenantAccessChangeId::new(id).map_err(|_| ())?,
            ))),
            _ => Err(()),
        }
    }
}
#[derive(Serialize)]
pub(super) struct AuditEntry<'a> {
    pub(super) change_id: &'a str,
    pub(super) tenant_id: &'a str,
    pub(super) administrator_actor_id: &'a str,
    pub(super) target_actor_id: &'a str,
    pub(super) action: Action,
    pub(super) kind: &'static str,
    pub(super) recorded_at_ms: i64,
    pub(super) state_changed: bool,
}
#[derive(Serialize)]
pub(super) struct AuditCursor<'a> {
    pub(super) recorded_at_ms: i64,
    pub(super) change_id: &'a str,
}
#[derive(Serialize)]
pub(super) struct AuditResponse<'a> {
    pub(super) changes: Vec<AuditEntry<'a>>,
    pub(super) next_before: Option<AuditCursor<'a>>,
}
