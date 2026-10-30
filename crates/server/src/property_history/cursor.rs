//! Versioned bounded opaque HTTP keys. Encoding grants no authorization.
use fleetiq_domain::{SignalBindingId, SignalTarget, TenantId};
use fleetiq_history::PropertyHistoryCursor;
use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedObservationId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Interpretation {
    message_id: String,
    package_id: String,
    package_version: String,
    output_index: u32,
    binding_id: String,
    revision: String,
    basis: String,
}
impl Interpretation {
    pub(super) fn from_id(id: &AssetPropertyChangeId) -> Self {
        let source = id.observation_id();
        Self {
            message_id: source.message_id().as_str().to_owned(),
            package_id: source.decoded_by().id().as_str().to_owned(),
            package_version: source.decoded_by().version().as_str().to_owned(),
            output_index: source.output_index(),
            binding_id: id.binding_id().as_str().to_owned(),
            revision: id.binding_revision().get().to_string(),
            basis: basis_name(id.event_time_basis()).to_owned(),
        }
    }
    pub(super) fn id(self) -> Option<AssetPropertyChangeId> {
        let basis = match self.basis.as_str() {
            "device_recorded" => EventTimeBasis::DeviceRecorded,
            "received_at" => EventTimeBasis::ReceivedAt,
            _ => return None,
        };
        Some(AssetPropertyChangeId::from_parts(
            DecodedObservationId::new(
                AcceptedMessageId::new(self.message_id).ok()?,
                ProtocolPackageRef::new(
                    ProtocolPackageId::new(self.package_id).ok()?,
                    ProtocolPackageVersion::new(self.package_version).ok()?,
                ),
                self.output_index,
            ),
            SignalBindingId::new(self.binding_id).ok()?,
            crate::protected_request::parse_record_revision(&self.revision)?,
            basis,
        ))
    }
}
pub(super) const fn basis_name(basis: EventTimeBasis) -> &'static str {
    match basis {
        EventTimeBasis::DeviceRecorded => "device_recorded",
        EventTimeBasis::ReceivedAt => "received_at",
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageKey {
    scope: String,
    event: i64,
    interpretation: Interpretation,
}
pub(super) fn page_token(
    tenant: &TenantId,
    query: &fleetiq_history::PropertyHistoryQuery,
    key: &PropertyHistoryCursor,
) -> Result<String, serde_json::Error> {
    encode(&PageKey {
        scope: scope(
            tenant,
            query.target(),
            query.start().get(),
            query.end().get(),
        )?,
        event: key.event_at().get(),
        interpretation: Interpretation::from_id(key.id()),
    })
}
pub(super) fn page_key(
    token: &str,
    tenant: &TenantId,
    target: &SignalTarget,
    start: i64,
    end: i64,
) -> Option<PropertyHistoryCursor> {
    let key: PageKey = decode(token)?;
    if key.scope != scope(tenant, target, start, end).ok()? {
        return None;
    }
    Some(PropertyHistoryCursor::new(
        fleetiq_domain::UnixTimeMillis::new(key.event),
        key.interpretation.id()?,
    ))
}
fn scope(
    tenant: &TenantId,
    target: &SignalTarget,
    start: i64,
    end: i64,
) -> Result<String, serde_json::Error> {
    // Length-delimited JSON avoids concatenation ambiguity. This public digest
    // binds query scope without repeating large Unicode identifiers in URLs.
    let bytes = serde_json::to_vec(&(
        tenant.as_str(),
        target.asset_id().as_str(),
        target.property().id().as_str(),
        target.property().version().get(),
        start,
        end,
    ))?;
    // sha2's digest array does not implement LowerHex; encode each byte to
    // preserve the version-one lowercase, zero-padded scope representation.
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in &digest {
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(hex)
}
pub(super) use crate::cursor_codec::{decode, encode};

#[cfg(test)]
mod tests {
    use super::*;
    use fleetiq_domain::{AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef};

    #[test]
    fn scope_digest_preserves_the_version_one_cursor_representation() {
        let tenant = TenantId::new("tenant-a").unwrap();
        let target = SignalTarget::new(
            AssetId::new("asset-a").unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        );
        // Independent SHA-256 fixture for the exact serialized scope tuple;
        // changing this representation requires a deliberate codec decision.
        assert_eq!(
            scope(&tenant, &target, 0, 3000).unwrap(),
            "12552961965bac6cb8db952f45e8022cae0f5705baa9517800e64fefd47773da"
        );
    }
}
