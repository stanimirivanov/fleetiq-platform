//! Compile-time bindings for the versioned protocol-package component contract.

/// Semantic version of the protocol-package WIT package implemented here.
///
/// Package releases declare compatibility with this interface version. Before
/// `1.0.0`, incompatible changes advance the minor version; after `1.0.0`, they
/// advance the major version.
pub const PROTOCOL_PACKAGE_ABI_VERSION: &str = "0.1.0";

/// Fully qualified WIT world implemented by a protocol package.
pub const PROTOCOL_PACKAGE_WORLD: &str = "fleetiq:protocol-package/protocol-package@0.1.0";

/// Authoritative WIT source for the current protocol-package interface.
///
/// Tooling may publish this document with a package SDK. Runtime compatibility
/// still comes from the component's embedded world type during admission.
pub const PROTOCOL_PACKAGE_WIT: &str = include_str!("../wit/protocol-package.wit");

// Generating typed host bindings as part of the normal build makes the checked-in
// WIT document executable specification: invalid syntax or types fail compilation.
// The bindings remain private until component admission and invocation establish
// the runtime policy for exposing them.
mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "protocol-package",
    });
}

#[cfg(test)]
mod tests {
    use super::{
        PROTOCOL_PACKAGE_ABI_VERSION, PROTOCOL_PACKAGE_WIT, PROTOCOL_PACKAGE_WORLD,
        bindings::exports::fleetiq::protocol_package::runtime::{
            DecodedOutput, DecodedValue, FrameBoundary, FrameProcessingOutput, InvocationInput,
            ObservationQuality, ProposedReply, ReplyRelease, SessionStateAction,
        },
    };

    #[test]
    fn public_identity_matches_the_versioned_wit_package() {
        assert_eq!(PROTOCOL_PACKAGE_ABI_VERSION, "0.1.0");
        assert_eq!(
            PROTOCOL_PACKAGE_WORLD,
            "fleetiq:protocol-package/protocol-package@0.1.0"
        );
        assert!(PROTOCOL_PACKAGE_WIT.contains("package fleetiq:protocol-package@0.1.0;"));
        assert!(PROTOCOL_PACKAGE_WIT.contains("world protocol-package"));
    }

    #[test]
    fn generated_types_preserve_invocation_data_and_ordered_effects() {
        let input = InvocationInput {
            payload: b"frame".to_vec(),
            session_state: b"old".to_vec(),
        };
        let boundary = FrameBoundary::Frame(5);
        let output = FrameProcessingOutput {
            decoded_outputs: vec![DecodedOutput {
                endpoint_id: "can.primary".to_owned(),
                signal_id: "motor.speed".to_owned(),
                value: DecodedValue::Integer(1_720),
                source_unit: Some("revolutions-per-minute".to_owned()),
                device_recorded_at_ms: Some(1_725_000_000_000),
                quality: ObservationQuality::Good,
            }],
            session_state: SessionStateAction::Replace(b"new".to_vec()),
            replies: vec![ProposedReply {
                bytes: b"ack".to_vec(),
                release: ReplyRelease::AfterDurableAcceptance,
            }],
        };

        assert_eq!(input.payload, b"frame");
        assert!(matches!(boundary, FrameBoundary::Frame(5)));
        assert_eq!(output.decoded_outputs.len(), 1);
        assert!(matches!(
            output.session_state,
            SessionStateAction::Replace(ref state) if state == b"new"
        ));
        assert_eq!(output.replies.len(), 1);
    }
}
