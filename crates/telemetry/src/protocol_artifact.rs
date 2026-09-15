use std::fmt;

/// SHA-256 content identity of an exact protocol component artifact.
///
/// This value describes content; it does not prove compatibility, admission,
/// authenticity, or activation permission. The runtime computes it from artifact
/// bytes. Persistence adapters may reconstruct it from stored digest bytes.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolArtifactDigest([u8; 32]);

impl ProtocolArtifactDigest {
    /// Reconstructs a SHA-256 digest from its fixed-width binary representation.
    ///
    /// Supplying digest bytes does not create runtime admission evidence.
    pub const fn from_sha256(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the fixed-width binary SHA-256 digest.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ProtocolArtifactDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ProtocolArtifactDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ProtocolArtifactDigest({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolArtifactDigest;

    #[test]
    fn reconstruction_preserves_binary_digest_and_canonical_hexadecimal_text() {
        let bytes = [0xab; 32];
        let digest = ProtocolArtifactDigest::from_sha256(bytes);

        assert_eq!(digest.as_bytes(), &bytes);
        assert_eq!(digest.to_string(), "ab".repeat(32));
        assert_eq!(
            format!("{digest:?}"),
            format!("ProtocolArtifactDigest({digest})")
        );
    }
}
