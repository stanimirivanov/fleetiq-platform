//! Versioned bounded numeric rule grammar; no clock, lifecycle or delivery engine.
use crate::CalculatorRef;
use bigdecimal::BigDecimal;
use fleetiq_domain::{AssetId, PropertyDefinitionRef, UnitId};
/// Persisted grammar meaning; changes require a new reviewed grammar version.
pub const ALERT_RULE_GRAMMAR: &str = "numeric-threshold-v1";
/// Exact numeric comparison, without arbitrary expression evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertComparison {
    /// Strictly greater than threshold.
    Greater,
    /// Greater than or equal to threshold.
    GreaterOrEqual,
    /// Strictly less than threshold.
    Less,
    /// Less than or equal to threshold.
    LessOrEqual,
}
impl AlertComparison {
    /// Parses the closed version-one comparison vocabulary.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gt" => Some(Self::Greater),
            "gte" => Some(Self::GreaterOrEqual),
            "lt" => Some(Self::Less),
            "lte" => Some(Self::LessOrEqual),
            _ => None,
        }
    }
    /// Returns the stable persisted comparison name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Greater => "gt",
            Self::GreaterOrEqual => "gte",
            Self::Less => "lt",
            Self::LessOrEqual => "lte",
        }
    }
}
/// Resolution policy is deliberately independent of missing evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertResolution {
    /// Resolve only after explicit false evidence for the configured duration.
    OnFalse,
}
/// Predicate result; missing evidence never means a healthy false condition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertCondition {
    /// Numeric predicate is true.
    Matched,
    /// Numeric predicate is explicitly false.
    NotMatched,
    /// No usable completed output was supplied.
    Missing,
}
/// Immutable rule content; host validates exact release/output ownership and unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlertRuleDefinition {
    /// Exact admitted calculator release.
    pub calculator: CalculatorRef,
    /// Semantic evaluation input scope.
    pub scope: AssetId,
    /// Exact output target asset.
    pub target: AssetId,
    /// Exact declared output property/version.
    pub output: PropertyDefinitionRef,
    /// Ordered output slot; host currently admits zero for reviewed implementations.
    pub output_index: u32,
    /// Closed exact numeric operator.
    pub comparison: AlertComparison,
    /// Finite threshold in the selected property's canonical unit.
    pub threshold: BigDecimal,
    /// Canonical threshold unit; None is explicitly dimensionless.
    pub unit: Option<UnitId>,
    /// Event-time matched duration before firing, at most seven days.
    pub hold_ms: u32,
    /// Explicit false-evidence resolution policy.
    pub resolution: AlertResolution,
    /// Event-time false duration before resolution, at most seven days.
    pub resolve_after_ms: u32,
}
impl AlertRuleDefinition {
    /// Checks deterministic numeric/resource bounds without clocks or I/O.
    pub fn is_valid(&self) -> bool {
        let (coefficient, scale) = self.threshold.as_bigint_and_exponent();
        if !(-256..=256).contains(&scale) || coefficient.to_string().len() > 256 {
            return false;
        }
        let (coefficient, scale) = self.threshold.normalized().as_bigint_and_exponent();
        let digits = coefficient.to_string().trim_start_matches('-').len();
        self.output_index < 1024
            && self.hold_ms <= 604_800_000
            && self.resolve_after_ms <= 604_800_000
            && scale <= 64
            && i128::try_from(digits).is_ok_and(|n| n - i128::from(scale) <= 128)
    }
    /// Tests one already selected usable value; does not advance alert state.
    /// The lifecycle consumer must pass None for absent/failed/unusable evidence.
    pub fn condition(&self, value: Option<&BigDecimal>) -> AlertCondition {
        let Some(value) = value else {
            return AlertCondition::Missing;
        };
        let matched = match self.comparison {
            AlertComparison::Greater => value > &self.threshold,
            AlertComparison::GreaterOrEqual => value >= &self.threshold,
            AlertComparison::Less => value < &self.threshold,
            AlertComparison::LessOrEqual => value <= &self.threshold,
        };
        if matched {
            AlertCondition::Matched
        } else {
            AlertCondition::NotMatched
        }
    }
}
#[cfg(test)]
mod tests;
