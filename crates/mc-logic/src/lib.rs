//! Block behaviour shared by the server and world generation: the `Block` methods vanilla
//! dispatches through (`canPlaceBlockAt`, `onBlockAdded`, `breakBlock`, `updateTick`...), written
//! against the [`Level`] trait so they run on any world implementation.

pub mod blocks;
pub mod item;
pub mod liquid;

use mc_core::{BlockPos, Facing, JavaRandom};
use mc_data::{Material, StateId};

/// `World.setBlockState` flags.
pub mod flags {
    /// Notify neighbours (`onNeighborBlockChange`).
    pub const NOTIFY: i32 = 1;
    /// Send the change to clients (`markBlockForUpdate`).
    pub const UPDATE: i32 = 2;
    /// Client side: don't re-render.
    pub const NO_RERENDER: i32 = 4;
    pub const DEFAULT: i32 = NOTIFY | UPDATE;
}

/// The parts of `World` block logic touches.
pub trait Level {
    /// `getBlockState`; on a server this may load or generate the chunk.
    fn state(&mut self, pos: BlockPos) -> StateId;
    /// `setBlockState(pos, state, flags)`, including the `breakBlock`/`onBlockAdded` hooks.
    fn set_state(&mut self, pos: BlockPos, state: StateId, flags: i32) -> bool;
    /// `getLightFor(type, pos)`
    fn light_for(&mut self, sky: bool, pos: BlockPos) -> i32;
    /// `getLight(pos)`: `max(sky, block)` without the day/night subtraction.
    fn light(&mut self, pos: BlockPos) -> i32;
    /// `isAreaLoaded(from, to)` (chunks only, `allowEmpty = true`).
    fn is_area_loaded(&mut self, from: BlockPos, to: BlockPos) -> bool;
    /// `scheduleUpdate(pos, block, delay)`
    fn schedule_update(&mut self, pos: BlockPos, block: u16, delay: i32);
    /// A random source: `World.rand`, or the generator's random during population.
    fn rng(&mut self, which: Rng) -> &mut JavaRandom;
    /// `World.rand`
    fn rand(&mut self) -> &mut JavaRandom {
        self.rng(Rng::World)
    }
    fn is_remote(&self) -> bool {
        false
    }
    /// `provider.getHasNoSky()`
    fn has_no_sky(&self) -> bool {
        false
    }
    /// `provider.doesWaterVaporize()`
    fn water_vaporizes(&self) -> bool {
        false
    }
    /// `dropBlockAsItemWithChance`. Worldgen-only levels can ignore it.
    fn drop_block_as_item(&mut self, _pos: BlockPos, _state: StateId, _chance: f32, _fortune: i32) {}
    /// `playSoundEffect`
    fn play_sound(&mut self, _x: f64, _y: f64, _z: f64, _name: &str, _volume: f32, _pitch: f32) {}
    /// `getGameRules().getBoolean(name)`
    fn game_rule(&self, _name: &str) -> bool {
        true
    }

    // Provided helpers ------------------------------------------------------------------------

    fn set(&mut self, pos: BlockPos, state: StateId) -> bool {
        self.set_state(pos, state, flags::DEFAULT)
    }
    /// `setBlockToAir`
    fn set_air(&mut self, pos: BlockPos) -> bool {
        self.set_state(pos, StateId::AIR, flags::DEFAULT)
    }
    fn is_air(&mut self, pos: BlockPos) -> bool {
        self.state(pos).block().material == Material::Air
    }
    fn block_id(&mut self, pos: BlockPos) -> u16 {
        self.state(pos).block().id
    }
    fn material(&mut self, pos: BlockPos) -> Material {
        self.state(pos).block().material
    }

    /// `notifyNeighborsOfStateChange(pos, block)`: west, east, down, up, north, south.
    fn notify_neighbors(&mut self, pos: BlockPos, block: u16) {
        for f in NOTIFY_ORDER {
            self.notify_block_of_state_change(pos.offset_facing(f), block);
        }
    }

    /// `notifyBlockOfStateChange`
    fn notify_block_of_state_change(&mut self, pos: BlockPos, neighbor: u16) {
        if !self.is_remote() {
            let state = self.state(pos);
            blocks::on_neighbor_block_change(self.as_level(), pos, state, neighbor);
        }
    }

    /// `BlockSapling.generateTree` (tree generators live outside this crate).
    fn grow_tree(&mut self, _pos: BlockPos, _sapling: StateId) -> bool {
        false
    }
    fn as_level(&mut self) -> &mut dyn Level;
}

/// Which random an action draws from. Vanilla passes `Random` objects around; worldgen hands
/// block code the chunk provider's random (shared with chunk generation), ticks use `World.rand`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rng {
    World,
    Gen,
}

/// Order of `notifyNeighborsOfStateChange`.
pub const NOTIFY_ORDER: [Facing; 6] = [Facing::West, Facing::East, Facing::Down, Facing::Up, Facing::North, Facing::South];

/// `StateId` from storage id and meta (air when unregistered).
#[inline]
pub fn st(id: u16, meta: u8) -> StateId {
    StateId::from_raw(id << 4 | meta as u16)
}
