//! Wire vocabulary kept independent of WIT, host policy and business assets.
/// Stable, payload-free package rejection categories (also WIT error codes).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WireError {
    /// Invalid wire shape, count, state or field value.
    Malformed = 1,
    /// Frame, record or IO limits exceeded.
    Limit = 2,
    /// Unsupported codec or handshake sequence.
    Unsupported = 3,
    /// CRC field does not match CRC-16/IBM of the data field.
    Checksum = 4,
}
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Teltonika wire rejection: {self:?}")
    }
}
impl std::error::Error for WireError {}

/// Raw GPS fields; coordinates retain their signed 10^-7 degree scale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gps {
    /// Signed longitude in ten-millionths of a degree.
    pub longitude: i32,
    /// Signed latitude in ten-millionths of a degree.
    pub latitude: i32,
    /// Uninterpreted two-byte altitude field.
    pub altitude_raw: u16,
    /// Heading field in degrees, from 0 through 360.
    pub angle: u16,
    /// Satellites used; zero makes location-related observations uncertain.
    pub satellites: u8,
    /// Speed field in kilometres per hour.
    pub speed: u16,
}
/// A vendor AVL identifier with its exact big-endian value bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IoValue {
    /// Numeric vendor AVL identifier, including unknown identifiers.
    pub id: u16,
    /// Exact bytes, preserving width, unsigned range and vendor interpretation.
    pub bytes: Vec<u8>,
}
/// One wire record, preserving device time and IO ordering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Device-reported Unix time in milliseconds; never host receipt time.
    pub timestamp_ms: i64,
    /// Teltonika priority: 0 low, 1 high, 2 panic.
    pub priority: u8,
    /// Raw position/navigation fields.
    pub gps: Gps,
    /// Event-generating IO identifier (zero means periodic).
    pub event_io_id: u16,
    /// IO values in their transmitted group/order, with unique IDs per record.
    pub io: Vec<IoValue>,
}
/// A completely validated packet; partial decoding is never returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Packet {
    /// Codec 8 (0x08) or Codec 8 Extended (0x8e).
    pub codec: u8,
    /// Nonempty, bounded records in wire order.
    pub records: Vec<Record>,
}
