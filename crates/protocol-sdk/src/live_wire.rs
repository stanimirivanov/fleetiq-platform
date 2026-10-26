//! Closed private live-child envelope. Unlike replay, this carries gated proposals.
//! Neither a proposed identity nor a serialized reply grants transport authority.
use crate::replay_wire::ReplayOutput;
use serde::{Deserialize, Serialize};
/// Bounded unread input and exact host-selected content, sent over private stdin.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRequest {
    /// Version-one live limits and interpretation.
    pub policy: u32,
    /// Exact execution identity shared with recovery.
    pub execution: [u8; 32],
    /// Host-selected immutable package name.
    pub package_id: String,
    /// Host-selected immutable package version.
    pub package_version: String,
    /// Exact component bytes; the parent verifies their catalogue digest.
    pub artifact: Vec<u8>,
    /// At most 64 KiB of unread socket data, possibly fragmented or coalesced.
    pub unread: Vec<u8>,
    /// Complete pre-turn state, at most 16 KiB.
    pub state: Vec<u8>,
    /// True only before the connection has a host-resolved registered identity.
    pub identify: bool,
}
/// A bounded proposal; the parent must restage and enforce its durability flag.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveReply {
    /// Nonempty bytes, bounded individually and in aggregate by both processes.
    pub bytes: Vec<u8>,
    /// True requires confirmed raw acceptance before transport delivery.
    pub durable: bool,
}
/// Results never contain host tenant/device authority or a socket handle.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveResponse {
    /// Must match the requesting host's execution identity.
    pub execution: [u8; 32],
    /// None means framing needs more bytes and has no effects.
    pub turn: Option<LiveTurn>,
}
/// One complete prefix and validated, still untrusted processing proposals.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveTurn {
    /// Exact nonzero prefix length, bounded by the parent's unread buffer.
    pub consumed: usize,
    /// First-turn claim only: identifier kind and exact value.
    pub identity: Option<(String, String)>,
    /// Complete successor state.
    pub state: Vec<u8>,
    /// Ordered source outputs; trusted provenance is assembled by the parent.
    pub outputs: Vec<ReplayOutput>,
    /// Ordered proposals, never evidence of successful socket delivery.
    pub replies: Vec<LiveReply>,
}
