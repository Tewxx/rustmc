//! Minecraft 1.8.9 network protocol (version 47): packet definitions for every state and
//! direction, `PacketBuffer` primitives, framing and compression, chunk payloads.

pub mod buf;
pub mod chunk;
pub mod crypto;
pub mod frame;
pub mod net;
pub mod packets;
pub mod status;

pub use buf::{Error, MetaValue, Metadata, Reader, Result, Slot, Uuid, Writer};

pub const PROTOCOL_VERSION: i32 = 47;
pub const VERSION_NAME: &str = "1.8.9";

/// `EnumConnectionState`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Handshaking,
    Play,
    Status,
    Login,
}

impl State {
    /// Handshake `next_state` → state (1 status, 2 login).
    pub fn from_handshake(id: i32) -> Option<State> {
        match id {
            1 => Some(State::Status),
            2 => Some(State::Login),
            _ => None,
        }
    }
}
