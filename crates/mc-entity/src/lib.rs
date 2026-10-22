//! Entity simulation shared by the client (local player prediction) and the server.
pub mod item;
pub mod player;
pub mod world;

pub use player::{Keys, PlayerEntity};
pub use world::EntityWorld;
