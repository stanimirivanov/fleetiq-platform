//! Canonical V1/PLAIN page fence before any native page decoder is used.
use crate::{ArchiveError, MAX_FIELD_BYTES};
fn varint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = u8::try_from(value & 127).expect("masked byte");
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            break;
        }
    }
}
fn header(rows: usize, bytes: usize) -> Result<Vec<u8>, ArchiveError> {
    let bytes = u32::try_from(bytes)
        .map_err(|_| ArchiveError)?
        .checked_mul(2)
        .ok_or(ArchiveError)?;
    let rows = u32::try_from(rows)
        .map_err(|_| ArchiveError)?
        .checked_mul(2)
        .ok_or(ArchiveError)?;
    let mut h = vec![0x15, 0, 0x15];
    varint(bytes, &mut h);
    h.push(0x15);
    varint(bytes, &mut h);
    h.extend([0x2c, 0x15]);
    varint(rows, &mut h);
    h.extend([0x15, 0, 0x15, 6, 0x15, 6, 0, 0]);
    Ok(h)
}
pub(crate) fn column(chunk: &[u8], rows: usize) -> Result<Vec<String>, ArchiveError> {
    if chunk.get(..3) != Some(&[0x15, 0, 0x15]) {
        return Err(ArchiveError);
    }
    let mut encoded = 0_u32;
    for (end, shift) in (3..).zip((0..35).step_by(7)) {
        let byte = *chunk.get(end).ok_or(ArchiveError)?;
        if shift == 28 && byte > 15 {
            return Err(ArchiveError);
        }
        encoded |= u32::from(byte & 127) << shift;
        if byte & 128 == 0 {
            break;
        }
        if shift == 28 {
            return Err(ArchiveError);
        }
    }
    if encoded & 1 != 0 {
        return Err(ArchiveError);
    }
    let expected = header(
        rows,
        usize::try_from(encoded / 2).map_err(|_| ArchiveError)?,
    )?;
    if !chunk.starts_with(&expected)
        || chunk.len() - expected.len() != usize::try_from(encoded / 2).map_err(|_| ArchiveError)?
    {
        return Err(ArchiveError);
    }
    let mut data = &chunk[expected.len()..];
    let mut result = Vec::with_capacity(rows);
    for _ in 0..rows {
        let n = u32::from_le_bytes(
            data.get(..4)
                .ok_or(ArchiveError)?
                .try_into()
                .map_err(|_| ArchiveError)?,
        );
        let n = usize::try_from(n).map_err(|_| ArchiveError)?;
        if n > MAX_FIELD_BYTES {
            return Err(ArchiveError);
        }
        data = &data[4..];
        let value = data.get(..n).ok_or(ArchiveError)?;
        result.push(
            std::str::from_utf8(value)
                .map_err(|_| ArchiveError)?
                .to_owned(),
        );
        data = &data[n..];
    }
    if !data.is_empty() {
        return Err(ArchiveError);
    }
    Ok(result)
}
