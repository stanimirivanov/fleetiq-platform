//! Exact PostgreSQL encodings for platform and tenant access actions.
//!
//! Keep the persisted names here so policy queries, grant writes, catalogues,
//! and audit reconstruction cannot silently diverge as action vocabularies grow.

use fleetiq_access::{PlatformAction, TenantAction};

use crate::error::{PostgresRepositoryError, corrupt};

/// Encodes an exact platform action as its persisted database name.
pub(crate) const fn platform_action_name(action: PlatformAction) -> &'static str {
    match action {
        PlatformAction::ManagePlatformAccess => "platform.access.manage",
        PlatformAction::ManageIdentity => "identity.manage",
        PlatformAction::ProvisionTenant => "tenant.provision",
        PlatformAction::ManageTenantLifecycle => "tenant.lifecycle.manage",
    }
}

/// Reconstructs a platform action, retaining the caller's exact column context.
pub(crate) fn parse_platform_action(
    value: &str,
    field: &'static str,
) -> Result<PlatformAction, PostgresRepositoryError> {
    match value {
        "platform.access.manage" => Ok(PlatformAction::ManagePlatformAccess),
        "identity.manage" => Ok(PlatformAction::ManageIdentity),
        "tenant.provision" => Ok(PlatformAction::ProvisionTenant),
        "tenant.lifecycle.manage" => Ok(PlatformAction::ManageTenantLifecycle),
        _ => Err(corrupt(field, value)),
    }
}

/// Encodes an exact tenant action as its persisted database name.
pub(crate) const fn tenant_action_name(action: TenantAction) -> &'static str {
    match action {
        TenantAction::ReadMetadata => "metadata.read",
        TenantAction::ReadTelemetry => "telemetry.read",
        TenantAction::ManageMetadata => "metadata.manage",
        TenantAction::ManageAccess => "access.manage",
        TenantAction::ManageProtocols => "protocol.manage",
        TenantAction::ManageAnalytics => "analytics.manage",
        TenantAction::ReadAlerts => "alerts.read",
        TenantAction::ManageAlerts => "alerts.manage",
    }
}

/// Reconstructs a tenant action, retaining the caller's exact column context.
pub(crate) fn parse_tenant_action(
    value: &str,
    field: &'static str,
) -> Result<TenantAction, PostgresRepositoryError> {
    match value {
        "metadata.read" => Ok(TenantAction::ReadMetadata),
        "telemetry.read" => Ok(TenantAction::ReadTelemetry),
        "metadata.manage" => Ok(TenantAction::ManageMetadata),
        "access.manage" => Ok(TenantAction::ManageAccess),
        "protocol.manage" => Ok(TenantAction::ManageProtocols),
        "analytics.manage" => Ok(TenantAction::ManageAnalytics),
        "alerts.read" => Ok(TenantAction::ReadAlerts),
        "alerts.manage" => Ok(TenantAction::ManageAlerts),
        _ => Err(corrupt(field, value)),
    }
}

#[cfg(test)]
mod tests {
    use fleetiq_access::{PlatformAction, TenantAction};

    use super::{
        PostgresRepositoryError, parse_platform_action, parse_tenant_action, platform_action_name,
        tenant_action_name,
    };

    const PLATFORM_CASES: [(PlatformAction, &str); 4] = [
        (
            PlatformAction::ManagePlatformAccess,
            "platform.access.manage",
        ),
        (PlatformAction::ManageIdentity, "identity.manage"),
        (PlatformAction::ProvisionTenant, "tenant.provision"),
        (
            PlatformAction::ManageTenantLifecycle,
            "tenant.lifecycle.manage",
        ),
    ];
    const TENANT_CASES: [(TenantAction, &str); 8] = [
        (TenantAction::ReadMetadata, "metadata.read"),
        (TenantAction::ReadTelemetry, "telemetry.read"),
        (TenantAction::ManageMetadata, "metadata.manage"),
        (TenantAction::ManageAccess, "access.manage"),
        (TenantAction::ManageProtocols, "protocol.manage"),
        (TenantAction::ManageAnalytics, "analytics.manage"),
        (TenantAction::ReadAlerts, "alerts.read"),
        (TenantAction::ManageAlerts, "alerts.manage"),
    ];

    #[test]
    fn platform_action_names_round_trip_exactly() {
        for (action, persisted_name) in PLATFORM_CASES {
            assert_eq!(platform_action_name(action), persisted_name);
            assert_eq!(
                parse_platform_action(persisted_name, "identity.platform_action_grants.action")
                    .expect("encoded platform action should decode"),
                action
            );
        }
    }

    #[test]
    fn tenant_action_names_round_trip_exactly() {
        for (action, persisted_name) in TENANT_CASES {
            assert_eq!(tenant_action_name(action), persisted_name);
            assert_eq!(
                parse_tenant_action(persisted_name, "identity.tenant_action_grants.action")
                    .expect("encoded tenant action should decode"),
                action
            );
        }
    }

    #[test]
    fn unknown_action_preserves_column_context() {
        assert!(matches!(
            parse_platform_action("future.action", "identity.platform_access_changes.action"),
            Err(PostgresRepositoryError::CorruptData {
                field: "identity.platform_access_changes.action",
                ..
            })
        ));
        assert!(matches!(
            parse_tenant_action("future.action", "identity.tenant_access_changes.action"),
            Err(PostgresRepositoryError::CorruptData {
                field: "identity.tenant_access_changes.action",
                ..
            })
        ));
    }
}
