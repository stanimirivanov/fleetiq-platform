//! Explicit identifier-only routing is an operator decision, never peer input.
use super::TcpIngestionError;
use fleetiq_domain::{ExternalIdentifierAuthority, TenantId};
use fleetiq_ingestion::{
    ProtocolAdmissionWorld, ProtocolChannelId, ProtocolRouteSelection, RegisteredIdentityChannel,
};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use std::{fmt, net::SocketAddr};
/// One trusted tenant/channel and immutable identity-capable decoder.
/// No browser identity, device credential or automatic tenant discovery is used.
#[derive(Clone)]
pub struct TcpIngestionConfig {
    pub(super) database_url: String,
    pub(super) bind: SocketAddr,
    pub(super) channel: RegisteredIdentityChannel,
}
impl fmt::Debug for TcpIngestionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TcpIngestionConfig").finish_non_exhaustive()
    }
}
impl TcpIngestionConfig {
    /// Reads explicit listener scope. Identifier routing is not authentication.
    /// # Errors
    /// Rejects missing/malformed configuration without echoing values. This
    /// function neither binds a port nor changes the database schema.
    pub fn from_env() -> Result<Self, TcpIngestionError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }
    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, TcpIngestionError> {
        let invalid = || TcpIngestionError::category("TCP ingestion configuration invalid");
        let required = |key| {
            get(key)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(invalid)
        };
        if required("FLEETIQ_TCP_IDENTITY_POLICY")? != "registered-identifiers" {
            return Err(invalid());
        }
        let database_url = required("DATABASE_URL")?;
        let bind = get("FLEETIQ_TCP_BIND_ADDRESS")
            .unwrap_or_else(|| "127.0.0.1:55027".into())
            .parse::<SocketAddr>()
            .map_err(|_| invalid())?;
        let channel = RegisteredIdentityChannel {
            tenant_id: TenantId::new(required("FLEETIQ_TCP_TENANT_ID")?).map_err(|_| invalid())?,
            channel_id: ProtocolChannelId::new(required("FLEETIQ_TCP_CHANNEL_ID")?)
                .map_err(|_| invalid())?,
            authority: get("FLEETIQ_TCP_IDENTIFIER_AUTHORITY")
                .map(ExternalIdentifierAuthority::new)
                .transpose()
                .map_err(|_| invalid())?,
            selection: ProtocolRouteSelection {
                package: ProtocolPackageRef::new(
                    ProtocolPackageId::new(required("FLEETIQ_TCP_PACKAGE_ID")?)
                        .map_err(|_| invalid())?,
                    ProtocolPackageVersion::new(required("FLEETIQ_TCP_PACKAGE_VERSION")?)
                        .map_err(|_| invalid())?,
                ),
                world: ProtocolAdmissionWorld::Identity,
            },
        };
        Ok(Self {
            database_url,
            bind,
            channel,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn values(key: &str) -> Option<String> {
        Some(
            match key {
                "DATABASE_URL" => "postgres://private:test@localhost/test",
                "FLEETIQ_TCP_IDENTITY_POLICY" => "registered-identifiers",
                "FLEETIQ_TCP_TENANT_ID" => "tenant-a",
                "FLEETIQ_TCP_CHANNEL_ID" => "teltonika-tcp",
                "FLEETIQ_TCP_PACKAGE_ID" => "vendor.teltonika",
                "FLEETIQ_TCP_PACKAGE_VERSION" => "1",
                _ => return None,
            }
            .into(),
        )
    }
    #[test]
    fn requires_explicit_scope_and_identifier_only_policy_without_leaking_configuration() {
        let config = TcpIngestionConfig::from_lookup(values).unwrap();
        assert_eq!(config.bind, "127.0.0.1:55027".parse().unwrap());
        assert!(!format!("{config:?}").contains("private"));
        for key in [
            "DATABASE_URL",
            "FLEETIQ_TCP_IDENTITY_POLICY",
            "FLEETIQ_TCP_TENANT_ID",
            "FLEETIQ_TCP_CHANNEL_ID",
            "FLEETIQ_TCP_PACKAGE_ID",
            "FLEETIQ_TCP_PACKAGE_VERSION",
        ] {
            assert!(
                TcpIngestionConfig::from_lookup(|k| if k == key { None } else { values(k) })
                    .is_err()
            );
        }
        for key in [
            "FLEETIQ_TCP_IDENTITY_POLICY",
            "FLEETIQ_TCP_BIND_ADDRESS",
            "FLEETIQ_TCP_IDENTIFIER_AUTHORITY",
        ] {
            let error = TcpIngestionConfig::from_lookup(|k| {
                if k == key {
                    Some(
                        if key == "FLEETIQ_TCP_IDENTIFIER_AUTHORITY" {
                            ""
                        } else {
                            "private-invalid-value/"
                        }
                        .into(),
                    )
                } else {
                    values(k)
                }
            })
            .unwrap_err();
            assert!(!format!("{error:?} {error}").contains("private-invalid"));
        }
    }
}
