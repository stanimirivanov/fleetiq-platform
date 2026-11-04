use fleetiq_analytics::{AlertDeliveryFailure, AlertNotificationLease, AlertPhase};
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
pub(super) fn encode(lease: &AlertNotificationLease) -> Result<Vec<u8>, AlertDeliveryFailure> {
    let invalid = AlertDeliveryFailure::InvalidPayload;
    if lease.revision == 0
        || lease.firing_version <= 0
        || lease.generation <= 0
        || !lease.definition.is_valid()
        || lease.ends_at < lease.starts_at
        || !match lease.phase {
            AlertPhase::Firing => lease.version == lease.firing_version,
            AlertPhase::Resolved => lease.version > lease.firing_version,
            _ => false,
        }
    {
        return Err(invalid);
    }
    let format = |ms: i64| {
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .map_err(|_| invalid)?
            .format(&Rfc3339)
            .map_err(|_| invalid)
    };
    let d = &lease.definition;
    let bytes=serde_json::to_vec(&json!([{
        "labels":{"alertname":"FleetIQNumericThreshold","fleetiq_tenant":lease.tenant.as_str(),"fleetiq_rule":lease.rule.as_str(),
            "fleetiq_revision":lease.revision.to_string(),"fleetiq_episode":lease.firing_version.to_string(),
            "fleetiq_scope":d.scope.as_str(),"fleetiq_target":d.target.as_str()},
        "annotations":{"fleetiq_transition":lease.version.to_string(),"fleetiq_phase":lease.phase.as_str(),
            "fleetiq_calculator":format!("{}@{}",d.calculator.id().as_str(),d.calculator.version().as_str()),
            "summary":"FleetIQ retained numeric threshold condition"},
        "startsAt":format(lease.starts_at.get())?,"endsAt":format(lease.ends_at.get())?
    }])).map_err(|_|invalid)?;
    if bytes.len() > 16384 {
        return Err(invalid);
    }
    Ok(bytes)
}
