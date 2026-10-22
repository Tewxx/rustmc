//! Game registries for 1.8.9: blocks and their states, materials, sounds.
//!
//! Static data (ids, states, meta mapping, per-block properties) is generated from golden dumps of
//! the real game by `tools/gen-data` into `generated/`. Behaviour that depends on the world
//! (`getActualState`, bounding boxes, ticks) is hand-ported on top of these types.

pub mod actual_state;
pub mod biome;
pub mod block;
pub mod color;
pub mod enchantment;
pub mod geometry;
pub mod item;
mod generated;
pub mod material;
pub mod model_location;
pub mod potion;

pub use actual_state::BlockAccess;
pub use block::{ids, Block, Property, PropertyKind, RenderLayer, SoundType, StateId};
pub use material::{Material, MaterialProps};
pub use item::Item;
pub use enchantment::Enchantment;
