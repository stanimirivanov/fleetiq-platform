//! WIT adapter: framing/decoding stay pure; the host validates all proposals.
// The owner-approved exception covers generated canonical ABI code only.
#[allow(unsafe_code, clippy::same_length_and_capacity)]
mod bindings;
mod observations;
use bindings::exports::fleetiq::protocol_package::{
    identification,
    runtime::{
        FrameBoundary, FrameProcessingOutput, Guest, InvocationInput, PackageError, ProposedReply,
        ReplyRelease, SessionStateAction,
    },
};
struct Package;
fn error(value: crate::WireError) -> PackageError {
    PackageError { code: value as u32 }
}
impl Guest for Package {
    fn locate_frame(input: InvocationInput) -> Result<FrameBoundary, PackageError> {
        crate::locate_frame(&input.payload, &input.session_state)
            .map_err(error)?
            .map_or(Ok(FrameBoundary::NeedMoreData), |size| {
                u32::try_from(size)
                    .map(FrameBoundary::Frame)
                    .map_err(|_| error(crate::WireError::Limit))
            })
    }
    fn process_frame(input: InvocationInput) -> Result<FrameProcessingOutput, PackageError> {
        match input.session_state.as_slice() {
            [] => {
                crate::identify(&input.payload).map_err(error)?;
                Ok(FrameProcessingOutput {
                    decoded_outputs: vec![],
                    session_state: SessionStateAction::Replace(vec![1]),
                    replies: vec![ProposedReply {
                        bytes: vec![1],
                        release: ReplyRelease::AfterProcessing,
                    }],
                })
            }
            [1] => {
                let packet = crate::decode(&input.payload).map_err(error)?;
                let count = u32::try_from(packet.records.len())
                    .map_err(|_| error(crate::WireError::Limit))?;
                Ok(FrameProcessingOutput {
                    decoded_outputs: observations::outputs(packet),
                    session_state: SessionStateAction::Keep,
                    replies: vec![ProposedReply {
                        bytes: count.to_be_bytes().to_vec(),
                        release: ReplyRelease::AfterDurableAcceptance,
                    }],
                })
            }
            _ => Err(error(crate::WireError::Unsupported)),
        }
    }
}
impl identification::Guest for Package {
    fn identify(input: InvocationInput) -> Result<identification::IdentityClaim, PackageError> {
        if !input.session_state.is_empty() {
            return Err(error(crate::WireError::Unsupported));
        }
        Ok(identification::IdentityClaim {
            kind: "imei".into(),
            value: crate::identify(&input.payload).map_err(error)?.into(),
        })
    }
}
