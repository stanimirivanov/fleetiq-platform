use std::{error::Error, fmt};

use crate::{AssetId, DeviceEndpointId, DeviceEndpointKindId, TenantId};

const MAX_ENDPOINT_NAME_LENGTH: usize = 200;

/// An operator-facing label for one device endpoint.
///
/// The label helps commissioning and diagnostics but is not endpoint identity.
/// `FleetIQ` preserves it exactly after validation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceEndpointName(String);

impl DeviceEndpointName {
    /// Constructs a validated endpoint label.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceEndpointNameError`] when the label is empty, has
    /// surrounding whitespace, contains a control character, or exceeds 200
    /// Unicode scalar values.
    pub fn new(value: impl Into<String>) -> Result<Self, DeviceEndpointNameError> {
        let value = value.into();
        validate_name(&value)?;
        Ok(Self(value))
    }

    /// Returns the endpoint label as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the label and returns its text representation.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for DeviceEndpointName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for DeviceEndpointName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Describes why a device endpoint label could not be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceEndpointNameError {
    /// The supplied label is empty or contains only whitespace.
    Empty,
    /// The supplied label begins or ends with whitespace.
    SurroundingWhitespace,
    /// The supplied label exceeds the supported size.
    TooLong {
        /// The maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// The Unicode scalar-value count in the rejected label.
        actual: usize,
    },
    /// The supplied label contains a control character.
    ControlCharacter,
}

impl fmt::Display for DeviceEndpointNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("device endpoint name must not be empty"),
            Self::SurroundingWhitespace => {
                formatter.write_str("device endpoint name must not have surrounding whitespace")
            }
            Self::TooLong { maximum, actual } => write!(
                formatter,
                "device endpoint name contains {actual} characters; the maximum is {maximum}"
            ),
            Self::ControlCharacter => {
                formatter.write_str("device endpoint name must not contain control characters")
            }
        }
    }
}

impl Error for DeviceEndpointNameError {}

/// A stable physical or logical observation source exposed by a device.
///
/// An endpoint identifies where a signal originates under the communication
/// device. Vendor parameter selectors and installation-specific asset/property
/// attribution belong to device profiles and signal bindings respectively.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceEndpoint {
    id: DeviceEndpointId,
    kind: DeviceEndpointKindId,
    name: DeviceEndpointName,
}

impl DeviceEndpoint {
    /// Constructs a device endpoint.
    pub fn new(id: DeviceEndpointId, kind: DeviceEndpointKindId, name: DeviceEndpointName) -> Self {
        Self { id, kind, name }
    }

    /// Returns the stable identity scoped by the owning device.
    pub fn id(&self) -> &DeviceEndpointId {
        &self.id
    }

    /// Returns the configurable endpoint category.
    pub fn kind(&self) -> &DeviceEndpointKindId {
        &self.kind
    }

    /// Returns the operator-facing endpoint label.
    pub fn name(&self) -> &DeviceEndpointName {
        &self.name
    }
}

/// Communication capability attached to an existing asset.
///
/// A device reuses the asset's stable identity and semantic type rather than
/// creating a second physical identity. Its endpoints describe sources exposed
/// by that device without embedding protocol or installation binding details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommunicationDevice {
    tenant_id: TenantId,
    asset_id: AssetId,
    endpoints: Vec<DeviceEndpoint>,
}

impl CommunicationDevice {
    /// Creates a communication device without endpoints.
    pub fn new(tenant_id: TenantId, asset_id: AssetId) -> Self {
        Self {
            tenant_id,
            asset_id,
            endpoints: Vec::new(),
        }
    }

    /// Returns the tenant that owns the device asset.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the asset identity carrying communication capability.
    pub fn asset_id(&self) -> &AssetId {
        &self.asset_id
    }

    /// Returns the device's endpoints in declaration order.
    pub fn endpoints(&self) -> &[DeviceEndpoint] {
        &self.endpoints
    }

    /// Adds an endpoint whose identity is not already used by this device.
    ///
    /// # Errors
    ///
    /// Returns [`CommunicationDeviceError::DuplicateEndpoint`] when this device
    /// already contains the endpoint identity.
    pub fn add_endpoint(
        &mut self,
        endpoint: DeviceEndpoint,
    ) -> Result<(), CommunicationDeviceError> {
        if self
            .endpoints
            .iter()
            .any(|existing| existing.id() == endpoint.id())
        {
            return Err(CommunicationDeviceError::DuplicateEndpoint);
        }

        self.endpoints.push(endpoint);
        Ok(())
    }
}

/// Describes a rejected communication-device operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommunicationDeviceError {
    /// The endpoint identity is already declared by this device.
    DuplicateEndpoint,
}

impl fmt::Display for CommunicationDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("device endpoint identity is already declared by this device")
    }
}

impl Error for CommunicationDeviceError {}

fn validate_name(value: &str) -> Result<(), DeviceEndpointNameError> {
    if value.trim().is_empty() {
        return Err(DeviceEndpointNameError::Empty);
    }
    if value.trim() != value {
        return Err(DeviceEndpointNameError::SurroundingWhitespace);
    }
    if value.chars().any(char::is_control) {
        return Err(DeviceEndpointNameError::ControlCharacter);
    }

    let actual = value.chars().count();
    if actual > MAX_ENDPOINT_NAME_LENGTH {
        return Err(DeviceEndpointNameError::TooLong {
            maximum: MAX_ENDPOINT_NAME_LENGTH,
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str) -> DeviceEndpoint {
        DeviceEndpoint::new(
            DeviceEndpointId::new(id).expect("valid endpoint ID"),
            DeviceEndpointKindId::new("bus.can").expect("valid endpoint kind"),
            DeviceEndpointName::new("CAN interface").expect("valid endpoint name"),
        )
    }

    fn device() -> CommunicationDevice {
        CommunicationDevice::new(
            TenantId::new("tenant-a").expect("valid tenant ID"),
            AssetId::new("gateway-1").expect("valid asset ID"),
        )
    }

    #[test]
    fn device_reuses_asset_identity_and_declares_endpoints() {
        let mut device = device();
        device
            .add_endpoint(endpoint("can-primary"))
            .expect("new endpoint should be accepted");

        assert_eq!(device.tenant_id().as_str(), "tenant-a");
        assert_eq!(device.asset_id().as_str(), "gateway-1");
        assert_eq!(device.endpoints()[0].id().as_str(), "can-primary");
        assert_eq!(device.endpoints()[0].kind().as_str(), "bus.can");
    }

    #[test]
    fn rejects_duplicate_endpoint_identity_within_one_device() {
        let mut device = device();
        device
            .add_endpoint(endpoint("can-primary"))
            .expect("new endpoint should be accepted");

        assert_eq!(
            device.add_endpoint(endpoint("can-primary")),
            Err(CommunicationDeviceError::DuplicateEndpoint)
        );
    }

    #[test]
    fn endpoint_name_validation_preserves_operator_input() {
        let name = DeviceEndpointName::new("Primary CAN interface").expect("valid name");

        assert_eq!(name.as_str(), "Primary CAN interface");
        assert_eq!(
            DeviceEndpointName::new(" Primary CAN interface"),
            Err(DeviceEndpointNameError::SurroundingWhitespace)
        );
        assert_eq!(
            DeviceEndpointName::new("CAN\ninterface"),
            Err(DeviceEndpointNameError::ControlCharacter)
        );
    }
}
