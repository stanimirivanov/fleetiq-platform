//! Opaque scope binding, not authorization or a signed server assertion.
use fleetiq_domain::{SignalTarget, TenantId};
use fleetiq_projection::DeliveryPosition;
use sha2::{Digest, Sha256};
use std::fmt::Write;

pub(super) fn scope(tenant: &TenantId, target: &SignalTarget) -> String {
    let bytes = serde_json::to_vec(&(
        tenant.as_str(),
        target.asset_id().as_str(),
        target.property().id().as_str(),
        target.property().version().get(),
    ))
    .expect("serializing identifier strings cannot fail");
    let mut result = String::with_capacity(64);
    for byte in &Sha256::digest(bytes) {
        write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
    }
    result
}
pub(super) fn encode(scope: &str, position: DeliveryPosition) -> String {
    format!("v1.{scope}.{}.{}", position.generation, position.revision)
}
pub(super) fn decode(scope: &str, value: &str) -> Option<DeliveryPosition> {
    if value.len() > 110 {
        return None;
    }
    let mut parts = value.split('.');
    if parts.next()? != "v1" || parts.next()? != scope {
        return None;
    }
    let generation = number(parts.next()?)?;
    let revision = number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(DeliveryPosition {
        generation,
        revision,
    })
}
fn number(value: &str) -> Option<u64> {
    if value.is_empty()
        || value.len() > 19
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value
        .parse::<i64>()
        .ok()
        .and_then(|n| u64::try_from(n).ok())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_is_canonical_bounded_and_scoped() {
        let scope = "a".repeat(64);
        let position = DeliveryPosition {
            generation: 1,
            revision: 20,
        };
        let value = encode(&scope, position);
        assert_eq!(decode(&scope, &value), Some(position));
        assert_eq!(decode(&"b".repeat(64), &value), None);
        for suffix in [
            "01.20",
            "1.-1",
            "1.9223372036854775808",
            "1.2.3",
            "1.",
            "1.2\n",
        ] {
            assert_eq!(decode(&scope, &format!("v1.{scope}.{suffix}")), None);
        }
        assert_eq!(decode(&scope, &"a".repeat(111)), None);
    }
}
