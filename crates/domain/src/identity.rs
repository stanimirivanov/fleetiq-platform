use std::{error::Error, fmt};

use crate::IdentityAuthorityId;

const MAX_EXTERNAL_IDENTITY_SUBJECT_LENGTH: usize = 512;

/// Identifies a principal within one trusted external authentication authority.
///
/// Subjects are opaque and case-sensitive. `FleetIQ` preserves their exact
/// validated text and does not apply provider-specific normalization.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalIdentitySubject(String);

impl ExternalIdentitySubject {
    /// Constructs an opaque external subject after validating storage bounds.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalIdentitySubjectError`] when the subject is empty or
    /// whitespace-only, contains a control character, or exceeds 512 Unicode
    /// scalar values.
    pub fn new(value: impl Into<String>) -> Result<Self, ExternalIdentitySubjectError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ExternalIdentitySubjectError::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(ExternalIdentitySubjectError::ControlCharacter);
        }

        let actual = value.chars().count();
        if actual > MAX_EXTERNAL_IDENTITY_SUBJECT_LENGTH {
            return Err(ExternalIdentitySubjectError::TooLong {
                maximum: MAX_EXTERNAL_IDENTITY_SUBJECT_LENGTH,
                actual,
            });
        }

        Ok(Self(value))
    }

    /// Returns the exact external subject text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the subject and returns its exact text representation.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for ExternalIdentitySubject {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ExternalIdentitySubject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Describes why an external identity subject could not be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalIdentitySubjectError {
    /// The supplied value is empty or contains only whitespace.
    Empty,
    /// The supplied value contains a control character.
    ControlCharacter,
    /// The supplied value exceeds the supported size limit.
    TooLong {
        /// Maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// Unicode scalar-value count in the rejected value.
        actual: usize,
    },
}

impl fmt::Display for ExternalIdentitySubjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("external identity subject must not be empty"),
            Self::ControlCharacter => {
                formatter.write_str("external identity subject must not contain control characters")
            }
            Self::TooLong { maximum, actual } => write!(
                formatter,
                "external identity subject contains {actual} characters; the maximum is {maximum}"
            ),
        }
    }
}

impl Error for ExternalIdentitySubjectError {}

/// Provider-neutral key for an externally authenticated principal.
///
/// This value identifies a principal but does not prove authentication. A
/// trusted authentication adapter must establish the authority and subject
/// before asking an actor-identity resolver to map them to an internal actor.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExternalIdentity {
    authority_id: IdentityAuthorityId,
    subject: ExternalIdentitySubject,
}

impl ExternalIdentity {
    /// Constructs an external identity from validated authority and subject values.
    pub const fn new(authority_id: IdentityAuthorityId, subject: ExternalIdentitySubject) -> Self {
        Self {
            authority_id,
            subject,
        }
    }

    /// Returns the trusted authentication authority configuration identity.
    pub const fn authority_id(&self) -> &IdentityAuthorityId {
        &self.authority_id
    }

    /// Returns the exact authority-local principal subject.
    pub const fn subject(&self) -> &ExternalIdentitySubject {
        &self.subject
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_opaque_case_sensitive_subject_text() {
        let subject = ExternalIdentitySubject::new(" User/Ä-01 ")
            .expect("surrounding whitespace is meaningful opaque subject text");

        assert_eq!(subject.as_str(), " User/Ä-01 ");
        assert_ne!(
            subject,
            ExternalIdentitySubject::new(" user/ä-01 ").expect("valid distinct subject")
        );
    }

    #[test]
    fn rejects_empty_control_and_oversized_subjects() {
        assert_eq!(
            ExternalIdentitySubject::new("   "),
            Err(ExternalIdentitySubjectError::Empty)
        );
        assert_eq!(
            ExternalIdentitySubject::new("subject\nother"),
            Err(ExternalIdentitySubjectError::ControlCharacter)
        );
        assert_eq!(
            ExternalIdentitySubject::new("é".repeat(MAX_EXTERNAL_IDENTITY_SUBJECT_LENGTH + 1)),
            Err(ExternalIdentitySubjectError::TooLong {
                maximum: MAX_EXTERNAL_IDENTITY_SUBJECT_LENGTH,
                actual: MAX_EXTERNAL_IDENTITY_SUBJECT_LENGTH + 1,
            })
        );
    }
}
