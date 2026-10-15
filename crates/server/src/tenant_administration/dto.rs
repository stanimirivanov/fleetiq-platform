//! HTTP shapes; validated domain constructors own identifiers and exact subjects.
use fleetiq_domain::{
    ActorId, TenantId, TenantLifecycleChangeId, TenantProvisioningId, UnixTimeMillis,
};
use fleetiq_identity::{
    TenantLifecycleChangeEffect, TenantLifecycleChangeKind, TenantLifecycleChangeRequest,
    TenantProvisioningRequest,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProvisionBody {
    #[serde(rename = "provisioning_id")]
    provisioning: String,
    #[serde(rename = "tenant_id")]
    tenant: String,
    #[serde(rename = "initial_administrator_actor_id")]
    initial_administrator: String,
}
impl ProvisionBody {
    pub(super) fn request(self, now: UnixTimeMillis) -> Option<TenantProvisioningRequest> {
        Some(TenantProvisioningRequest::for_delivery(
            TenantProvisioningId::new(self.provisioning).ok()?,
            TenantId::new(self.tenant).ok()?,
            ActorId::new(self.initial_administrator).ok()?,
            now,
        ))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LifecycleBody {
    change_id: String,
    state: DesiredState,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum DesiredState {
    Active,
    Suspended,
}
impl LifecycleBody {
    pub(super) fn request(
        self,
        tenant: String,
        now: UnixTimeMillis,
    ) -> Option<TenantLifecycleChangeRequest> {
        Some(TenantLifecycleChangeRequest::for_delivery(
            TenantLifecycleChangeId::new(self.change_id).ok()?,
            TenantId::new(tenant).ok()?,
            match self.state {
                DesiredState::Active => TenantLifecycleChangeKind::Activate,
                DesiredState::Suspended => TenantLifecycleChangeKind::Suspend,
            },
            now,
        ))
    }
}
#[derive(Serialize)]
pub(super) struct ProvisionResponse {
    pub(super) outcome: &'static str,
}
#[derive(Serialize)]
pub(super) struct LifecycleResponse {
    pub(super) outcome: &'static str,
    pub(super) state_changed: bool,
}
pub(super) const fn state_changed(effect: TenantLifecycleChangeEffect) -> bool {
    matches!(effect, TenantLifecycleChangeEffect::StateChanged)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogParameters {
    pub(super) limit: Option<u16>,
    pub(super) after: Option<String>,
}
#[derive(Serialize)]
pub(super) struct TenantResponse<'a> {
    pub(super) tenant_id: &'a str,
    pub(super) state: &'static str,
}
#[derive(Serialize)]
pub(super) struct CatalogResponse<'a> {
    pub(super) tenants: Vec<TenantResponse<'a>>,
    pub(super) next_after: Option<&'a str>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuditParameters {
    pub(super) limit: Option<u16>,
    pub(super) before_recorded_at_ms: Option<i64>,
    pub(super) before_change_id: Option<String>,
}
#[derive(Serialize)]
pub(super) struct AuditCursor<'a> {
    pub(super) recorded_at_ms: i64,
    pub(super) change_id: &'a str,
}
#[derive(Serialize)]
pub(super) struct AuditEntry<'a> {
    pub(super) change_id: &'a str,
    pub(super) administrator_actor_id: &'a str,
    pub(super) tenant_id: &'a str,
    pub(super) kind: &'static str,
    pub(super) recorded_at_ms: i64,
    pub(super) state_changed: bool,
}
#[derive(Serialize)]
pub(super) struct AuditResponse<'a> {
    pub(super) changes: Vec<AuditEntry<'a>>,
    pub(super) next_before: Option<AuditCursor<'a>>,
}
