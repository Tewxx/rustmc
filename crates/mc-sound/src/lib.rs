//! Sound: Vorbis decoding, a software mixer on WinMM output, and `SoundManager`.
pub mod manager;
pub mod mixer;
pub mod vorbis;

pub use manager::{Category, SoundManager};
