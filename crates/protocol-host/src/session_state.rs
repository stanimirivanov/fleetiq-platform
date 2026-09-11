use std::{error::Error, fmt, num::NonZeroUsize};

use fleetiq_protocol_sdk::SessionStateAction;

/// Opaque protocol-package state owned and bounded by the host for one session.
///
/// The state starts empty and exists only for the lifetime of its owning
/// connection session. A package receives a borrowed view and proposes a whole
/// replacement through [`SessionStateAction`]. The host never exposes mutable
/// access to its retained state.
#[derive(Clone, Eq, PartialEq)]
pub struct SessionState {
    bytes: Box<[u8]>,
    limit: NonZeroUsize,
}

impl SessionState {
    /// Creates empty session state with the given hard byte limit.
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            bytes: Box::default(),
            limit,
        }
    }

    /// Lets a protocol package inspect and propose a change to session state.
    ///
    /// `Keep` preserves the current bytes. `Replace` transfers the proposed
    /// bytes to the host after their length is validated. Empty replacement
    /// bytes clear the state.
    ///
    /// # Errors
    ///
    /// Returns [`SessionStateUpdateError::Package`] when the package cannot
    /// produce an action. Returns [`SessionStateUpdateError::LimitExceeded`]
    /// when replacement bytes exceed the configured limit. Both failures leave
    /// the current state unchanged.
    pub fn update<E>(
        &mut self,
        decide: impl FnOnce(&[u8]) -> Result<SessionStateAction, E>,
    ) -> Result<(), SessionStateUpdateError<E>> {
        let action = decide(&self.bytes).map_err(SessionStateUpdateError::Package)?;

        let SessionStateAction::Replace(replacement) = action else {
            return Ok(());
        };

        if replacement.len() > self.limit.get() {
            return Err(SessionStateUpdateError::LimitExceeded {
                limit: self.limit.get(),
                attempted: replacement.len(),
            });
        }

        self.bytes = replacement;
        Ok(())
    }

    /// Returns the current opaque state bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the current state length in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the current state is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns the configured hard state-byte limit.
    pub fn limit(&self) -> NonZeroUsize {
        self.limit
    }
}

impl fmt::Debug for SessionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionState")
            .field("length", &self.len())
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

/// Failure while a protocol package proposes a session-state action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionStateUpdateError<E> {
    /// The package could not produce a state action.
    Package(E),
    /// The proposed replacement exceeds the host's hard state-byte limit.
    LimitExceeded {
        /// Maximum state bytes allowed for the session.
        limit: usize,
        /// Number of bytes in the rejected replacement.
        attempted: usize,
    },
}

impl<E: fmt::Display> fmt::Display for SessionStateUpdateError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => {
                write!(
                    formatter,
                    "protocol package session-state update failed: {error}"
                )
            }
            Self::LimitExceeded { limit, attempted } => write!(
                formatter,
                "protocol package proposed {attempted} state bytes, exceeding the {limit}-byte limit"
            ),
        }
    }
}

impl<E: Error + 'static> Error for SessionStateUpdateError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Package(error) => Some(error),
            Self::LimitExceeded { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
