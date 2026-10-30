//! Exact, bounded latest-point median/MAD policy; no clock or learned state.
use crate::CalculatorExecutionFailure;
use bigdecimal::BigDecimal;
use fleetiq_domain::PropertyValue;

/// Reproducible explanation in the input's canonical unit; threshold is 6 * MAD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MadEvidence {
    /// Median of preceding samples, excluding the newest point.
    pub median: BigDecimal,
    /// Median absolute deviation of the same baseline from its median.
    pub mad: BigDecimal,
    /// Newest sample in canonical event/evidence order.
    pub observed: BigDecimal,
    /// Absolute difference between newest sample and baseline median.
    pub deviation: BigDecimal,
    /// Six times raw MAD; not a normal-distribution calibrated z-score.
    pub threshold: BigDecimal,
    /// Strictly above threshold; zero MAD flags any nonzero departure.
    pub anomalous: bool,
}
/// Evaluates window-mad-v1 using at most 1024 ordered semantic values.
/// Returns no evidence for fewer than max(minimum, 4) values or any unusable value.
/// Integers convert exactly; decimals retain their precision. Even medians use
/// exact midpoints, without float conversion or library-default division.
/// # Errors
/// Rejects invalid sample bounds/types or coefficients >1024 characters/scales
/// outside +/-256. Missing evidence is never a normal result.
pub fn detect_mad(
    samples: &[Option<PropertyValue>],
    minimum: u16,
) -> Result<Option<MadEvidence>, CalculatorExecutionFailure> {
    if !(1..=1024).contains(&minimum) || samples.len() > 1024 {
        return Err(CalculatorExecutionFailure::ResourceLimit);
    }
    if samples.len() < usize::from(minimum.max(4)) || samples.iter().any(Option::is_none) {
        return Ok(None);
    }
    let mut values = Vec::with_capacity(samples.len());
    for sample in samples.iter().flatten() {
        let value = match sample {
            PropertyValue::Integer(value) => BigDecimal::from(*value),
            PropertyValue::Decimal(value) => value.clone(),
            _ => return Err(CalculatorExecutionFailure::IncompatibleInput),
        };
        let (coefficient, scale) = value.as_bigint_and_exponent();
        if !(-256..=256).contains(&scale) || coefficient.to_string().len() > 1024 {
            return Err(CalculatorExecutionFailure::ResourceLimit);
        }
        values.push(value);
    }
    let observed = values
        .pop()
        .ok_or(CalculatorExecutionFailure::IncompatibleInput)?;
    let median = midpoint(&mut values);
    let mut deviations: Vec<_> = values.iter().map(|v| (v - &median).abs()).collect();
    let mad = midpoint(&mut deviations);
    let deviation = (&observed - &median).abs();
    let threshold = &mad * BigDecimal::from(6);
    let anomalous = deviation > threshold;
    Ok(Some(MadEvidence {
        median,
        mad,
        observed,
        deviation,
        threshold,
        anomalous,
    }))
}
fn midpoint(values: &mut [BigDecimal]) -> BigDecimal {
    values.sort();
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (&values[middle - 1] + &values[middle]) * BigDecimal::new(5.into(), 1)
    } else {
        values[middle].clone()
    }
}
#[cfg(test)]
mod tests;
