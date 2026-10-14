//! Transport-only shapes. Do not derive Debug: bodies and cursors contain opaque subjects.
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentityChangeId, ExternalIdentitySubject,
    IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_identity::{
    ExternalIdentityAuditCursor, ExternalIdentityCatalogCursor, ExternalIdentityChangeEffect,
    ExternalIdentityChangeKind, ExternalIdentityChangeRequest,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangeBody {
    change_id: String,
    kind: ChangeKind,
    external_identity: IdentityBody,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChangeKind {
    Attach,
    Detach,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityBody {
    authority_id: String,
    subject: String,
}
impl ChangeBody {
    pub(super) fn request(
        self,
        actor: String,
        now: UnixTimeMillis,
    ) -> Option<ExternalIdentityChangeRequest> {
        Some(ExternalIdentityChangeRequest::for_delivery(
            ExternalIdentityChangeId::new(self.change_id).ok()?,
            ActorId::new(actor).ok()?,
            ExternalIdentity::new(
                IdentityAuthorityId::new(self.external_identity.authority_id).ok()?,
                ExternalIdentitySubject::new(self.external_identity.subject).ok()?,
            ),
            match self.kind {
                ChangeKind::Attach => ExternalIdentityChangeKind::Attach,
                ChangeKind::Detach => ExternalIdentityChangeKind::Detach,
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
pub(super) const fn state_changed(effect: ExternalIdentityChangeEffect) -> bool {
    match effect {
        ExternalIdentityChangeEffect::StateChanged => true,
        ExternalIdentityChangeEffect::NoStateChange => false,
    }
}
pub(super) const fn kind_name(kind: ExternalIdentityChangeKind) -> &'static str {
    match kind {
        ExternalIdentityChangeKind::Attach => "attach",
        ExternalIdentityChangeKind::Detach => "detach",
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogParameters {
    pub(super) limit: Option<u16>,
    after_authority_id: Option<String>,
    after_subject: Option<String>,
}
impl CatalogParameters {
    // Both fields are required together. Never normalize a provider's subject.
    pub(super) fn cursor(self) -> Result<Option<ExternalIdentityCatalogCursor>, ()> {
        match (self.after_authority_id, self.after_subject) {
            (None, None) => Ok(None),
            (Some(authority), Some(subject)) => Ok(Some(ExternalIdentityCatalogCursor::new(
                IdentityAuthorityId::new(authority).map_err(|_| ())?,
                ExternalIdentitySubject::new(subject).map_err(|_| ())?,
            ))),
            _ => Err(()),
        }
    }
}
#[derive(Serialize)]
pub(super) struct IdentityResponse<'a> {
    pub(super) authority_id: &'a str,
    pub(super) subject: &'a str,
}
#[derive(Serialize)]
pub(super) struct CatalogResponse<'a> {
    pub(super) identities: Vec<IdentityResponse<'a>>,
    pub(super) next_after: Option<IdentityResponse<'a>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuditParameters {
    pub(super) limit: Option<u16>,
    before_recorded_at_ms: Option<i64>,
    before_change_id: Option<String>,
}
impl AuditParameters {
    pub(super) fn cursor(self) -> Result<Option<ExternalIdentityAuditCursor>, ()> {
        match (self.before_recorded_at_ms, self.before_change_id) {
            (None, None) => Ok(None),
            (Some(time), Some(id)) => Ok(Some(ExternalIdentityAuditCursor::new(
                UnixTimeMillis::new(time),
                ExternalIdentityChangeId::new(id).map_err(|_| ())?,
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
    pub(super) external_identity: IdentityResponse<'a>,
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
