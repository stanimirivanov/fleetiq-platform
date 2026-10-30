//! Explicit wire projection; lifecycle versions and epoch times stay exact strings.
use fleetiq_analytics::AlertRuleDefinition;
use fleetiq_domain::UnixTimeMillis;
use fleetiq_postgres::{AlertRead, AlertTransitionRead};
use serde_json::{Value, json};
pub(super) fn transition(r: &AlertTransitionRead, d: &AlertRuleDefinition) -> Value {
    json!({"version":r.version.to_string(),"phase":r.phase.as_str(),"event_at_ms":r.event_at.get().to_string(),
        "recorded_at_ms":r.recorded_at.get().to_string(),"activation_generation":r.activation_generation,
        "evidence":{"evaluation_id":r.evaluation.as_str(),"scope_asset_id":d.scope.as_str(),
            "output_index":r.output_available.then_some(d.output_index)}})
}
pub(super) fn summary(r: &AlertRead) -> Result<Value, ()> {
    let last = r.state.last_event().ok_or(())?;
    Ok(
        json!({"rule_id":r.rule.as_str(),"rule_revision":r.revision,"scope_asset_id":r.definition.scope.as_str(),
        "target_asset_id":r.definition.target.as_str(),"version":r.version.to_string(),"phase":r.state.phase().as_str(),"selected":r.selected,
        "candidate_since_ms":r.state.candidate_since().map(UnixTimeMillis::get).map(|v|v.to_string()),
        "last_event_at_ms":last.get().to_string(),"latest_transition":transition(&r.latest,&r.definition)}),
    )
}
