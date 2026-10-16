//! Closed wire shapes reconstruct domain values; JSON never controls audit context.
use fleetiq_domain::{
    Asset, AssetId, AssetName, AssetTypeId, AssetTypeRef, CommunicationDevice, DefinitionVersion,
    DeviceEndpoint, DeviceEndpointId, DeviceEndpointKindId, DeviceEndpointName, ExternalIdentifier,
    ExternalIdentifierAuthority, ExternalIdentifierKind, ExternalIdentifierValue, TenantId,
};
use fleetiq_metadata::{MetadataRegistration, RegistrationReceipt};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssetBody {
    id: String,
    name: String,
    asset_type: TypeBody,
    external_identifiers: Vec<IdentifierBody>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeBody {
    id: String,
    version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentifierBody {
    kind: String,
    authority: Option<String>,
    value: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeviceBody {
    endpoints: Vec<EndpointBody>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointBody {
    id: String,
    kind: String,
    name: String,
}

impl AssetBody {
    pub(super) fn registration(self, tenant: TenantId) -> Option<MetadataRegistration> {
        if self.external_identifiers.len() > 100 {
            return None;
        }
        let mut asset = Asset::new(
            tenant,
            AssetId::new(self.id).ok()?,
            AssetTypeRef::new(
                AssetTypeId::new(self.asset_type.id).ok()?,
                DefinitionVersion::new(self.asset_type.version).ok()?,
            ),
            AssetName::new(self.name).ok()?,
        );
        for identifier in self.external_identifiers {
            asset
                .add_external_identifier(ExternalIdentifier::new(
                    ExternalIdentifierKind::new(identifier.kind).ok()?,
                    identifier
                        .authority
                        .map(ExternalIdentifierAuthority::new)
                        .transpose()
                        .ok()?,
                    ExternalIdentifierValue::new(identifier.value).ok()?,
                ))
                .ok()?;
        }
        Some(MetadataRegistration::Asset(asset))
    }
}
impl DeviceBody {
    pub(super) fn registration(
        self,
        tenant: TenantId,
        asset: String,
    ) -> Option<MetadataRegistration> {
        if self.endpoints.len() > 100 {
            return None;
        }
        let mut device = CommunicationDevice::new(tenant, AssetId::new(asset).ok()?);
        for endpoint in self.endpoints {
            device
                .add_endpoint(DeviceEndpoint::new(
                    DeviceEndpointId::new(endpoint.id).ok()?,
                    DeviceEndpointKindId::new(endpoint.kind).ok()?,
                    DeviceEndpointName::new(endpoint.name).ok()?,
                ))
                .ok()?;
        }
        Some(MetadataRegistration::Device(device))
    }
}
#[derive(Serialize)]
pub(super) struct RegistrationResponse<'a> {
    pub outcome: &'static str,
    pub registered_by: &'a str,
    pub recorded_at_ms: i64,
}
impl<'a> RegistrationResponse<'a> {
    pub(super) fn new(outcome: &'static str, receipt: &'a RegistrationReceipt) -> Self {
        Self {
            outcome,
            registered_by: receipt.actor_id().as_str(),
            recorded_at_ms: receipt.recorded_at().get(),
        }
    }
}
