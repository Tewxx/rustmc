//! Game registries for 1.8.9: blocks and their states, materials, sounds.
//!
//! Static data (ids, states, meta mapping, per-block properties) is generated from golden dumps of
//! the real game by `tools/gen-data` into `generated/`. Behaviour that depends on the world
//! (`getActualState`, bounding boxes, ticks) is hand-ported on top of these types.

pub mod block;
mod generated;
pub mod material;

pub use block::{Block, Property, PropertyKind, RenderLayer, SoundType, StateId};
pub use material::{Material, MaterialProps};
