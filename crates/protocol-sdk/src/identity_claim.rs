//! Untrusted identification evidence; never an authenticated device context.
use std::{error::Error, fmt};

/// A bounded external identifier proposed by a protocol package.
///
/// The host must resolve it against registered devices and channel admission
/// policy. Neither possession of this value nor a successful handshake grants
/// authentication, tenant membership, or permission to execute a package.
#[derive(Clone, Eq, PartialEq)]
pub struct DeviceIdentityClaim {
    kind: String,
    value: String,
}
impl DeviceIdentityClaim {
    /// Validates a nonempty ASCII identifier kind (at most 64 bytes) and opaque
    /// nonempty value (at most 128 UTF-8 bytes, without control characters).
    /// Values are preserved exactly, including case; no normalization occurs.
    ///
    /// # Errors
    /// Returns [`InvalidIdentityClaim`] for invalid syntax or exceeded bounds.
    pub fn new(kind: String, value: String) -> Result<Self, InvalidIdentityClaim> {
        if kind.is_empty()
            || kind.len() > 64
            || !kind
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            || value.is_empty()
            || value.len() > 128
            || value.chars().any(char::is_control)
        {
            return Err(InvalidIdentityClaim);
        }
        Ok(Self { kind, value })
    }
    /// Returns the exact external identifier kind, such as `imei` or `serial`.
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// Returns untrusted, case-sensitive identifier evidence; keep it out of logs.
    pub fn value(&self) -> &str {
        &self.value
    }
}
impl fmt::Debug for DeviceIdentityClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceIdentityClaim")
            .field("kind", &self.kind)
            .field("value_length", &self.value.len())
            .finish_non_exhaustive()
    }
}
/// An identity proposal violated the bounded semantic contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidIdentityClaim;
impl fmt::Display for InvalidIdentityClaim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid bounded device identity claim")
    }
}
impl Error for InvalidIdentityClaim {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_evidence_is_redacted_and_bounded() {
        let claim = DeviceIdentityClaim::new("serial".into(), "Secret-ABC".into()).unwrap();
        assert_eq!(claim.value(), "Secret-ABC");
        assert!(!format!("{claim:?}").contains("Secret-ABC"));
        for (kind, value) in [
            (String::new(), "x".into()),
            ("bad kind".into(), "x".into()),
            ("k".repeat(65), "x".into()),
            ("serial".into(), String::new()),
            ("serial".into(), "x".repeat(129)),
            ("serial".into(), "x\n".into()),
        ] {
            assert_eq!(
                DeviceIdentityClaim::new(kind, value),
                Err(InvalidIdentityClaim)
            );
        }
        assert!(DeviceIdentityClaim::new("k".repeat(64), "v".repeat(128)).is_ok());
    }
}
