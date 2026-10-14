//! Listener-selected protection for mutations, shared with browser logout.
use crate::protected_request::ProtectedRequestError;
use axum::http::{HeaderMap, header};

/// Selected by composition, never inferred from request-controlled headers.
#[derive(Clone)]
pub(crate) enum MutationGuard {
    Workload,
    Browser { public_origin: String },
}
impl MutationGuard {
    pub(crate) fn check(&self, headers: &HeaderMap) -> Result<(), ProtectedRequestError> {
        match self {
            Self::Workload => Ok(()),
            Self::Browser { public_origin } => {
                if single_header(headers, header::ORIGIN) == Some(public_origin.as_str())
                    && single_header(headers, "x-fleetiq-csrf") == Some("1")
                {
                    Ok(())
                } else {
                    Err(ProtectedRequestError::Forbidden)
                }
            }
        }
    }
}
fn single_header(headers: &HeaderMap, name: impl axum::http::header::AsHeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    #[test]
    fn browser_guard_requires_one_exact_origin_and_marker() {
        let guard = MutationGuard::Browser {
            public_origin: "https://fleet.example".into(),
        };
        let mut headers = HeaderMap::new();
        assert!(guard.check(&headers).is_err());
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://fleet.example"),
        );
        headers.insert("x-fleetiq-csrf", HeaderValue::from_static("1"));
        assert!(guard.check(&headers).is_ok());
        for origin in [
            "null",
            "https://fleet.example.attacker.test",
            "http://fleet.example",
            "https://fleet.example/",
        ] {
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
            assert!(guard.check(&headers).is_err());
        }
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://fleet.example"),
        );
        headers.append(
            header::ORIGIN,
            HeaderValue::from_static("https://fleet.example"),
        );
        assert!(guard.check(&headers).is_err());
        headers.remove(header::ORIGIN);
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://fleet.example"),
        );
        headers.append("x-fleetiq-csrf", HeaderValue::from_static("1"));
        assert!(guard.check(&headers).is_err());
    }
}
