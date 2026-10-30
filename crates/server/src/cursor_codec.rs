//! Shared version-one bounded opaque positions; tokens never grant authority.
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Write;

// Hex keeps opaque JSON keys URL-safe without another runtime dependency. The
// prefix is a codec version, not a signature; every lookup rechecks scope/grant.
pub(crate) fn encode<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    let mut result = String::from("v1.");
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(result)
}
pub(crate) fn decode<T: DeserializeOwned>(value: &str) -> Option<T> {
    if value.len() > 8192 {
        return None;
    }
    let hex = value.strip_prefix("v1.")?;
    if !hex.len().is_multiple_of(2)
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    serde_json::from_slice(&bytes).ok()
}
