use std::fmt;

/// A protocol package's proposed change to host-controlled session state.
///
/// Session state is opaque to the host. The package may preserve the current
/// bytes or propose complete replacement bytes. Replacing the state with an
/// empty byte sequence explicitly clears it.
///
/// The host decides whether the replacement satisfies its resource policy. This
/// semantic Rust type does not define the serialization or memory-transfer ABI
/// used by a future isolated runtime.
#[derive(Clone, Eq, PartialEq)]
pub enum SessionStateAction {
    /// Preserve the current session state exactly.
    Keep,
    /// Replace the current session state with these complete opaque bytes.
    Replace(Box<[u8]>),
}

impl fmt::Debug for SessionStateAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep => formatter.write_str("Keep"),
            Self::Replace(bytes) => formatter
                .debug_struct("Replace")
                .field("length", &bytes.len())
                .finish(),
        }
    }
}
