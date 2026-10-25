//! Checked, allocation-free wire reads. All network integers are big-endian.
use crate::WireError;
pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        let end = self.offset.checked_add(count).ok_or(WireError::Limit)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(WireError::Malformed)?;
        self.offset = end;
        Ok(value)
    }
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        self.take(N)?.try_into().map_err(|_| WireError::Malformed)
    }
    pub(crate) fn byte(&mut self) -> Result<u8, WireError> {
        Ok(self.array::<1>()?[0])
    }
    pub(crate) fn short(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    pub(crate) fn count(&mut self, extended: bool) -> Result<usize, WireError> {
        Ok(if extended {
            usize::from(self.short()?)
        } else {
            usize::from(self.byte()?)
        })
    }
    pub(crate) fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
