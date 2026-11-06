//! Logical accounting values, never physical allocation or billing amounts.
/// One nonnegative exact count/byte pair reconciled from authoritative rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageMeasure {
    /// Number of stored records in the selected class.
    pub records: u128,
    /// Bytes according to the explicitly named measurement method.
    pub bytes: u128,
}
impl StorageMeasure {
    /// Reconstructs exact database totals without rounding or wrapping.
    /// # Errors
    /// Rejects invalid/overflowing totals and nonzero bytes for zero records.
    pub fn from_totals(records: &str, bytes: &str) -> Result<Self, InvalidStorageMeasure> {
        let parse = |v: &str| {
            if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InvalidStorageMeasure);
            }
            v.parse::<u128>().map_err(|_| InvalidStorageMeasure)
        };
        let records = parse(records)?;
        let bytes = parse(bytes)?;
        if records == 0 && bytes != 0 {
            return Err(InvalidStorageMeasure);
        }
        Ok(Self { records, bytes })
    }
}
/// Invalid or inconsistent accounting evidence.
#[derive(Clone, Copy, Debug)]
pub struct InvalidStorageMeasure;
impl std::fmt::Display for InvalidStorageMeasure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid storage measure")
    }
}
impl std::error::Error for InvalidStorageMeasure {}
/// One database snapshot of the supported logical storage classes.
pub struct StorageUsage {
    /// Database transaction observation time, in Unix milliseconds.
    pub observed_at_ms: i64,
    /// Accepted wire bytes, excluding row/index overhead.
    pub accepted: StorageMeasure,
    /// UTF-8 JSON serialization of complete decoded observation rows.
    pub decoded: StorageMeasure,
    /// UTF-8 JSON serialization of semantic interpretation rows (not copied values).
    pub interpretations: StorageMeasure,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn totals_preserve_precision_and_reject_inconsistency() {
        assert_eq!(
            StorageMeasure::from_totals("1", "18446744073709551616")
                .unwrap()
                .bytes,
            18_446_744_073_709_551_616
        );
        for (n, b) in [
            ("0", "1"),
            ("-1", "0"),
            ("1", "1.5"),
            ("1", "340282366920938463463374607431768211456"),
        ] {
            assert!(StorageMeasure::from_totals(n, b).is_err());
        }
        assert!(StorageMeasure::from_totals("0", "0").is_ok());
    }
}
