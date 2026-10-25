//! Bounded Teltonika TCP handshake and Codec 8/8 Extended wire decoder.
//!
//! This package proposes source facts only. Identity is an untrusted IMEI claim;
//! the trusted host owns registration, authorization, durability and transport.
mod avl;
#[cfg(target_arch = "wasm32")]
mod component;
mod cursor;
mod framing;
mod model;

pub use avl::decode;
pub use framing::{identify, locate_frame};
pub use model::{Gps, IoValue, Packet, Record, WireError};

/// Maximum complete TCP AVL frame size accepted by this release, in bytes.
pub const MAX_FRAME_BYTES: usize = 1280;
/// Maximum AVL records accepted atomically in one frame.
pub const MAX_RECORDS: usize = 16;
/// Maximum IO values per record, including variable-width values.
pub const MAX_IO_VALUES: usize = 64;
/// Maximum bytes in one variable-width IO value.
pub const MAX_VARIABLE_BYTES: usize = 256;
