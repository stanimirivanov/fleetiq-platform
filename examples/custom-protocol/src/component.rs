//! Generated canonical ABI glue stays at the guest boundary.
// Only generated canonical ABI exports may use unsafe; the architecture sensor
// validates this out-of-line module and rejects every other unsafe allowance.
#[allow(unsafe_code, clippy::same_length_and_capacity)]
mod bindings;
use bindings::exports::fleetiq::protocol_package::{
    identification,
    runtime::{
        DecodedOutput, DecodedValue, FrameBoundary, FrameProcessingOutput, Guest, InvocationInput,
        ObservationQuality, PackageError, ProposedReply, ReplyRelease, SessionStateAction,
    },
};
struct Package;
fn error() -> PackageError {
    PackageError { code: 1 }
}
fn serial(frame: &[u8]) -> Result<String, PackageError> {
    if !(3..=34).contains(&frame.len())
        || usize::from(frame[0]) + 1 != frame.len()
        || frame[1] != 1
        || !frame[2..].iter().all(u8::is_ascii_alphanumeric)
    {
        return Err(error());
    }
    String::from_utf8(frame[2..].to_vec()).map_err(|_| error())
}
impl Guest for Package {
    fn locate_frame(input: InvocationInput) -> Result<FrameBoundary, PackageError> {
        let Some(&length) = input.payload.first() else {
            return Ok(FrameBoundary::NeedMoreData);
        };
        if !(2..=33).contains(&length) {
            return Err(error());
        }
        let length = u32::from(length) + 1;
        Ok(
            if input.payload.len() < usize::try_from(length).map_err(|_| error())? {
                FrameBoundary::NeedMoreData
            } else {
                FrameBoundary::Frame(length)
            },
        )
    }
    fn process_frame(input: InvocationInput) -> Result<FrameProcessingOutput, PackageError> {
        let frame = &input.payload;
        if frame.len() < 3 || usize::from(frame[0]) + 1 != frame.len() {
            return Err(error());
        }
        match frame[1] {
            1 => {
                let serial = serial(frame)?;
                Ok(FrameProcessingOutput {
                    decoded_outputs: vec![],
                    session_state: SessionStateAction::Replace(serial.into_bytes()),
                    replies: vec![ProposedReply {
                        bytes: vec![1],
                        release: ReplyRelease::AfterProcessing,
                    }],
                })
            }
            2 if frame.len() == 4 && !input.session_state.is_empty() => {
                let value = i16::from_be_bytes([frame[2], frame[3]]);
                Ok(FrameProcessingOutput {
                    decoded_outputs: vec![DecodedOutput {
                        endpoint_id: "main".into(),
                        signal_id: "temperature".into(),
                        value: DecodedValue::Integer(i64::from(value)),
                        source_unit: None,
                        device_recorded_at_ms: None,
                        quality: ObservationQuality::Good,
                    }],
                    session_state: SessionStateAction::Keep,
                    replies: vec![ProposedReply {
                        bytes: vec![1],
                        release: ReplyRelease::AfterDurableAcceptance,
                    }],
                })
            }
            _ => Err(error()),
        }
    }
}
impl identification::Guest for Package {
    fn identify(input: InvocationInput) -> Result<identification::IdentityClaim, PackageError> {
        Ok(identification::IdentityClaim {
            kind: "serial".into(),
            value: serial(&input.payload)?,
        })
    }
}
