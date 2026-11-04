use fleetiq_analytics::{AlertRuleId, CalculatorEvaluationId};
use fleetiq_telemetry::AcceptedMessageId;
use serde::Deserialize;
use serde_json::Value;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Command {
    pub operation: Operation,
    pub expected_generation: String,
    pub target: Value,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Operation {
    Retry,
    Pause,
    Resume,
}
impl Operation {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::Pause => "pause",
            Self::Resume => "resume",
        }
    }
}
impl Command {
    pub fn validate(&self, kind: &str) -> Option<i64> {
        if !super::kind(kind) {
            return None;
        }
        let g = self.expected_generation.parse::<i64>().ok()?;
        if g < 0 || g == i64::MAX || g.to_string() != self.expected_generation {
            return None;
        }
        if self.operation != Operation::Retry {
            return (self.target == serde_json::json!({})).then_some(g);
        }
        let x = self.target.as_object()?;
        match kind {
            "replay" | "calculator" => {
                let id = x.get("id")?.as_str()?;
                if x.len() != 1
                    || (kind == "replay" && AcceptedMessageId::new(id).is_err())
                    || (kind == "calculator" && CalculatorEvaluationId::new(id).is_err())
                {
                    return None;
                }
            }
            "notification" => {
                if x.len() != 4 {
                    return None;
                }
                let digest = x.get("destination")?.as_str()?;
                if digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return None;
                }
                AlertRuleId::new(x.get("rule_id")?.as_str()?).ok()?;
                let revision = x.get("revision")?.as_str()?;
                let r = revision.parse::<u32>().ok()?;
                let firing = x.get("firing_version")?.as_str()?;
                let f = firing.parse::<i64>().ok()?;
                if r == 0 || f <= 0 || r.to_string() != revision || f.to_string() != firing {
                    return None;
                }
            }
            _ => return None,
        }
        Some(g)
    }
}
