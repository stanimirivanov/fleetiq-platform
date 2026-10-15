//! HTTP shapes and exhaustive platform action mapping; application types stay transport-free.
use fleetiq_access::PlatformAction;
use fleetiq_domain::{ActorId, PlatformAccessChangeId, UnixTimeMillis};
use fleetiq_identity::{
    PlatformAccessAuditCursor, PlatformAccessChangeEffect, PlatformAccessChangeKind,
    PlatformAccessChangeRequest, PlatformGrantCatalogCursor,
};
use serde::{Deserialize, Serialize};

/// Closed HTTP vocabulary. Exhaustive conversions force review when policy adds an action.
#[derive(Clone, Copy, Deserialize, Serialize)]
pub(super) enum Action {
    #[serde(rename = "platform.access.manage")]
    ManagePlatformAccess,
    #[serde(rename = "identity.manage")]
    ManageIdentity,
    #[serde(rename = "tenant.provision")]
    ProvisionTenant,
    #[serde(rename = "tenant.lifecycle.manage")]
    ManageTenantLifecycle,
}
impl From<Action> for PlatformAction {
    fn from(action: Action) -> Self {
        match action {
            Action::ManagePlatformAccess => Self::ManagePlatformAccess,
            Action::ManageIdentity => Self::ManageIdentity,
            Action::ProvisionTenant => Self::ProvisionTenant,
            Action::ManageTenantLifecycle => Self::ManageTenantLifecycle,
        }
    }
}
impl From<PlatformAction> for Action {
    fn from(action: PlatformAction) -> Self {
        match action {
            PlatformAction::ManagePlatformAccess => Self::ManagePlatformAccess,
            PlatformAction::ManageIdentity => Self::ManageIdentity,
            PlatformAction::ProvisionTenant => Self::ProvisionTenant,
            PlatformAction::ManageTenantLifecycle => Self::ManageTenantLifecycle,
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
    ) -> Option<PlatformAccessChangeRequest> {
        Some(PlatformAccessChangeRequest::for_delivery(
            PlatformAccessChangeId::new(self.change_id).ok()?,
            ActorId::new(actor).ok()?,
            self.action.into(),
            match self.kind {
                ChangeKind::Grant => PlatformAccessChangeKind::Grant,
                ChangeKind::Revoke => PlatformAccessChangeKind::Revoke,
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
pub(super) const fn state_changed(effect: PlatformAccessChangeEffect) -> bool {
    match effect {
        PlatformAccessChangeEffect::StateChanged => true,
        PlatformAccessChangeEffect::NoStateChange => false,
    }
}
pub(super) const fn kind_name(kind: PlatformAccessChangeKind) -> &'static str {
    match kind {
        PlatformAccessChangeKind::Grant => "grant",
        PlatformAccessChangeKind::Revoke => "revoke",
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogParameters {
    pub(super) limit: Option<u16>,
    after_action: Option<Action>,
}
impl CatalogParameters {
    pub(super) fn cursor(self) -> Option<PlatformGrantCatalogCursor> {
        self.after_action
            .map(|action| PlatformGrantCatalogCursor::new(action.into()))
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
    pub(super) fn cursor(self) -> Result<Option<PlatformAccessAuditCursor>, ()> {
        match (self.before_recorded_at_ms, self.before_change_id) {
            (None, None) => Ok(None),
            (Some(time), Some(id)) => Ok(Some(PlatformAccessAuditCursor::new(
                UnixTimeMillis::new(time),
                PlatformAccessChangeId::new(id).map_err(|_| ())?,
            ))),
            _ => Err(()),
        }
    }
}
#[derive(Serialize)]
pub(super) struct AuditEntry<'a> {
    pub(super) change_id: &'a str,
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
