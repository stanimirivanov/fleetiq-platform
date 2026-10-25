//! Whole-packet validation and bounded record parsing; no partial successes.
use crate::{
    Gps, IoValue, MAX_IO_VALUES, MAX_RECORDS, MAX_VARIABLE_BYTES, Packet, Record, WireError,
    cursor::Cursor, locate_frame,
};

/// Decodes one exact AVL TCP packet after the handshake.
///
/// Supports Codec 8 and Codec 8 Extended, fixed 1/2/4/8-byte IO and bounded
/// Extended variable IO. Unknown IDs remain exact raw bytes. All records, both
/// counts and the zero-extended CRC must validate before any value is returned.
/// # Errors
/// Rejects trailing/truncated input, CRC mismatch, unsupported codecs, duplicate
/// IO IDs, impossible GPS/count fields and exceeded declared limits.
pub fn decode(frame: &[u8]) -> Result<Packet, WireError> {
    if locate_frame(frame, &[1])? != Some(frame.len()) {
        return Err(WireError::Malformed);
    }
    let end = frame.len().checked_sub(4).ok_or(WireError::Malformed)?;
    let data = frame.get(8..end).ok_or(WireError::Malformed)?;
    let crc = u32::from_be_bytes(frame[end..].try_into().map_err(|_| WireError::Malformed)?);
    if crc != u32::from(crc16(data)) {
        return Err(WireError::Checksum);
    }
    let mut cursor = Cursor::new(data);
    let codec = cursor.byte()?;
    let extended = match codec {
        8 => false,
        0x8e => true,
        _ => return Err(WireError::Unsupported),
    };
    let count = usize::from(cursor.byte()?);
    if count == 0 {
        return Err(WireError::Malformed);
    }
    if count > MAX_RECORDS {
        return Err(WireError::Limit);
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(record(&mut cursor, extended)?);
    }
    if usize::from(cursor.byte()?) != count || !cursor.finished() {
        return Err(WireError::Malformed);
    }
    Ok(Packet { codec, records })
}
fn record(cursor: &mut Cursor<'_>, extended: bool) -> Result<Record, WireError> {
    let timestamp_ms =
        i64::try_from(u64::from_be_bytes(cursor.array()?)).map_err(|_| WireError::Malformed)?;
    let priority = cursor.byte()?;
    let gps = Gps {
        longitude: i32::from_be_bytes(cursor.array()?),
        latitude: i32::from_be_bytes(cursor.array()?),
        altitude_raw: cursor.short()?,
        angle: cursor.short()?,
        satellites: cursor.byte()?,
        speed: cursor.short()?,
    };
    if priority > 2
        || gps.longitude.unsigned_abs() > 1_800_000_000
        || gps.latitude.unsigned_abs() > 900_000_000
        || gps.angle > 360
    {
        return Err(WireError::Malformed);
    }
    let event_io_id = u16::try_from(cursor.count(extended)?).map_err(|_| WireError::Malformed)?;
    let total = cursor.count(extended)?;
    if total > MAX_IO_VALUES {
        return Err(WireError::Limit);
    }
    let mut io = Vec::with_capacity(total);
    for width in [1, 2, 4, 8] {
        let count = cursor.count(extended)?;
        check_count(io.len(), count, total)?;
        for _ in 0..count {
            let id = u16::try_from(cursor.count(extended)?).map_err(|_| WireError::Malformed)?;
            append(&mut io, id, cursor.take(width)?)?;
        }
    }
    if extended {
        let count = cursor.count(true)?;
        check_count(io.len(), count, total)?;
        for _ in 0..count {
            let id = cursor.short()?;
            let size = usize::from(cursor.short()?);
            if size == 0 || size > MAX_VARIABLE_BYTES {
                return Err(WireError::Limit);
            }
            append(&mut io, id, cursor.take(size)?)?;
        }
    }
    if io.len() != total {
        return Err(WireError::Malformed);
    }
    Ok(Record {
        timestamp_ms,
        priority,
        gps,
        event_io_id,
        io,
    })
}
fn check_count(current: usize, count: usize, total: usize) -> Result<(), WireError> {
    if current.checked_add(count).is_none_or(|sum| sum > total) {
        return Err(WireError::Malformed);
    }
    Ok(())
}
fn append(io: &mut Vec<IoValue>, id: u16, bytes: &[u8]) -> Result<(), WireError> {
    if io.iter().any(|value| value.id == id) {
        return Err(WireError::Malformed);
    }
    io.push(IoValue {
        id,
        bytes: bytes.to_vec(),
    });
    Ok(())
}
// CRC-16/ARC (IBM): reflected polynomial 0xA001, initial value/xorout zero.
// The wire field is four bytes; comparing as u32 also rejects nonzero high bits.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for &byte in bytes {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xa001
            };
        }
    }
    crc
}
