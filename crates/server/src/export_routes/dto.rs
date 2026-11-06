//! Closed transport requests and exact bounded export metadata.
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget, TenantId,
};
use fleetiq_history::{
    exports::{ExportInfo, ExportRequest, ExportStatus},
    restore::RestoreTicket,
};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Create {
    asset_id: String,
    property_id: String,
    property_version: u32,
    start_at_ms: String,
    end_at_ms: String,
    manifest_id: Option<String>,
    restore_generation: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Empty {}
pub(super) fn integer(s: &str) -> Option<i64> {
    let n = s.parse::<i64>().ok()?;
    (n.to_string() == s).then_some(n)
}
impl Create {
    pub(super) fn request(self, tenant: TenantId) -> Option<ExportRequest> {
        let restore = match (self.manifest_id, self.restore_generation) {
            (None, None) => None,
            (Some(m), Some(g)) => Some(RestoreTicket::new(integer(&m)?, integer(&g)?).ok()?),
            _ => return None,
        };
        let target = SignalTarget::new(
            AssetId::new(self.asset_id).ok()?,
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new(self.property_id).ok()?,
                DefinitionVersion::new(self.property_version).ok()?,
            ),
        );
        ExportRequest::new(
            tenant,
            target,
            integer(&self.start_at_ms)?,
            integer(&self.end_at_ms)?,
            restore,
        )
        .ok()
    }
}
pub(super) fn job(info: &ExportInfo) -> Value {
    json!({
     "export_id":info.ticket.id().to_string(),"state":match info.status{ExportStatus::Missing=>"missing",ExportStatus::Pending=>"pending",ExportStatus::Ready=>"ready",ExportStatus::Cancelled=>"cancelled",ExportStatus::Expired=>"expired"},
     "selection_generation":"1","format":"property-history-v1","format_version":1,
     "expires_at_ms":info.expires_at_ms.to_string(),"row_count":info.rows,"file_bytes":info.file_bytes.to_string(),
     "coverage":{"scope":if info.restore.is_some(){"hot_and_selected_manifest_snapshot"}else{"hot_snapshot"},"manifest_id":info.restore.map(|t|t.manifest().to_string()),"restore_generation":info.restore.map(|t|t.generation().to_string())}
    })
}
