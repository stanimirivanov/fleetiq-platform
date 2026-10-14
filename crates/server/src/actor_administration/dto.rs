//! HTTP shapes; validated domain constructors own identifiers and exact subjects.
use fleetiq_domain::{
    ActorId, ActorLifecycleChangeId, ActorProvisioningId, ExternalIdentity,
    ExternalIdentitySubject, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_identity::{
    ActorLifecycleChangeEffect, ActorLifecycleChangeKind, ActorLifecycleChangeRequest,
    ActorProvisioningRequest,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProvisionBody {
    provisioning_id: String,
    actor_id: String,
    external_identity: IdentityBody,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityBody {
    authority_id: String,
    subject: String,
}
impl ProvisionBody {
    pub(super) fn request(self, now: UnixTimeMillis) -> Option<ActorProvisioningRequest> {
        Some(ActorProvisioningRequest::for_delivery(
            ActorProvisioningId::new(self.provisioning_id).ok()?,
            ActorId::new(self.actor_id).ok()?,
            ExternalIdentity::new(
                IdentityAuthorityId::new(self.external_identity.authority_id).ok()?,
                ExternalIdentitySubject::new(self.external_identity.subject).ok()?,
            ),
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
    Deactivated,
}
impl LifecycleBody {
    pub(super) fn request(
        self,
        actor: String,
        now: UnixTimeMillis,
    ) -> Option<ActorLifecycleChangeRequest> {
        Some(ActorLifecycleChangeRequest::for_delivery(
            ActorLifecycleChangeId::new(self.change_id).ok()?,
            ActorId::new(actor).ok()?,
            match self.state {
                DesiredState::Active => ActorLifecycleChangeKind::Activate,
                DesiredState::Deactivated => ActorLifecycleChangeKind::Deactivate,
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
pub(super) const fn state_changed(effect: ActorLifecycleChangeEffect) -> bool {
    matches!(effect, ActorLifecycleChangeEffect::StateChanged)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogParameters {
    pub(super) limit: Option<u16>,
    pub(super) after: Option<String>,
}
#[derive(Serialize)]
pub(super) struct ActorResponse<'a> {
    pub(super) actor_id: &'a str,
    pub(super) state: &'static str,
}
#[derive(Serialize)]
pub(super) struct CatalogResponse<'a> {
    pub(super) actors: Vec<ActorResponse<'a>>,
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
    pub(super) target_actor_id: &'a str,
    pub(super) kind: &'static str,
    pub(super) recorded_at_ms: i64,
    pub(super) state_changed: bool,
}
#[derive(Serialize)]
pub(super) struct AuditResponse<'a> {
    pub(super) changes: Vec<AuditEntry<'a>>,
    pub(super) next_before: Option<AuditCursor<'a>>,
}
