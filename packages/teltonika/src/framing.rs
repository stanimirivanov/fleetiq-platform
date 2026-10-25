//! Side-effect-free framing, separate from checksum and record decoding.
use crate::{MAX_FRAME_BYTES, WireError};
/// Validates one exact 17-byte length-prefixed, 15-digit IMEI handshake.
///
/// The result is a routing claim, not authentication. No Luhn policy is imposed.
/// # Errors
/// Rejects incomplete, trailing, non-digit or non-15-byte handshakes.
pub fn identify(frame: &[u8]) -> Result<&str, WireError> {
    if frame.len() != 17 || frame[..2] != [0, 15] || !frame[2..].iter().all(u8::is_ascii_digit) {
        return Err(WireError::Malformed);
    }
    std::str::from_utf8(&frame[2..]).map_err(|_| WireError::Malformed)
}
/// Finds the first complete frame in possibly fragmented/coalesced input.
///
/// State is empty before the handshake and `[1]` afterwards. Incomplete input
/// returns `None`; bytes following the first frame belong to the next turn.
/// # Errors
/// Rejects invalid state, handshake prefixes, preambles and declared frame bounds.
pub fn locate_frame(payload: &[u8], state: &[u8]) -> Result<Option<usize>, WireError> {
    let length = match state {
        [] => {
            if payload.first().is_some_and(|b| *b != 0) || payload.get(1).is_some_and(|b| *b != 15)
            {
                return Err(WireError::Malformed);
            }
            17
        }
        [1] => {
            if payload.iter().take(4).any(|b| *b != 0) {
                return Err(WireError::Malformed);
            }
            let Some(header) = payload.get(4..8) else {
                return Ok(None);
            };
            let data = u32::from_be_bytes(header.try_into().map_err(|_| WireError::Malformed)?);
            let data = usize::try_from(data).map_err(|_| WireError::Limit)?;
            if data < 3 {
                return Err(WireError::Malformed);
            }
            let length = data.checked_add(12).ok_or(WireError::Limit)?;
            if length > MAX_FRAME_BYTES {
                return Err(WireError::Limit);
            }
            length
        }
        _ => return Err(WireError::Unsupported),
    };
    Ok((payload.len() >= length).then_some(length))
}
