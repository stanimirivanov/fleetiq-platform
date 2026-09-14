//! Durable acceptance orchestration for one processed protocol frame.

mod context;
mod pending;
mod result;

pub use context::{FrameAcceptanceContext, PreparedFrameAcceptance};
pub use pending::PendingFrameAcceptance;
pub use result::{DurablyAcceptedFrame, FrameAcceptanceError};

#[cfg(test)]
mod tests;
