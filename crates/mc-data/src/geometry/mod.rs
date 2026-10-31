//! Ordered block geometry calls. Vanilla stores bounds on the singleton Block, rather than on
//! IBlockState. Keep that mutable state explicit: a serial world/client owns a `BlockGeometry`.
//! This preserves call-order quirks without putting mutable data in the shared registry.

mod bounds;
mod collision;

use crate::{Block, StateId};
use mc_core::{Aabb, BlockPos, Facing, Vec3};

/// Raw `Block.minX...maxZ`; unlike an AABB these values are not sorted. Unattached vines can
/// produce inverted bounds, which matter to ray tests and `shouldSideBeRendered`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds(pub [f64; 6]);

impl Bounds {
    pub const FULL: Self = Self([0., 0., 0., 1., 1., 1.]);
    pub fn new(x: f32, y: f32, z: f32, xx: f32, yy: f32, zz: f32) -> Self {
        Self([x, y, z, xx, yy, zz].map(f64::from))
    }
    pub fn world_box(self, pos: BlockPos) -> Aabb {
        let v = self.0;
        Aabb::new(
            pos.x as f64 + v[0],
            pos.y as f64 + v[1],
            pos.z as f64 + v[2],
            pos.x as f64 + v[3],
            pos.y as f64 + v[4],
            pos.z as f64 + v[5],
        )
    }
    fn raw_box(self) -> Aabb {
        let [min_x, min_y, min_z, max_x, max_y, max_z] = self.0;
        Aabb { min_x, min_y, min_z, max_x, max_y, max_z }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub position: Vec3,
    pub face: Facing,
}

/// The subset of TileEntityPiston read by geometry. `progress` is getProgress(0), i.e. lastProgress.
#[derive(Clone, Copy, Debug)]
pub struct MovingPiston {
    pub state: StateId,
    pub facing: Facing,
    pub progress: f32,
    pub extending: bool,
}

/// The only entity-specific block collision branch in 1.8.9 is lily pads ignoring boats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionEntity {
    Other,
    Boat,
}

pub struct BlockGeometry {
    bounds: Vec<Bounds>,
    stair_ray_pass: Vec<Option<u8>>,
}

impl Default for BlockGeometry {
    fn default() -> Self {
        Self { bounds: Block::all().iter().map(|b| Bounds(b.default_bounds)).collect(), stair_ray_pass: vec![None; Block::all().len()] }
    }
}

impl BlockGeometry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        for (bounds, block) in self.bounds.iter_mut().zip(Block::all()) { *bounds = Bounds(block.default_bounds); }
        self.stair_ray_pass.fill(None);
    }
    pub fn bounds(&self, state: StateId) -> Bounds {
        self.bounds[state.block().id as usize]
    }
    fn store(&mut self, state: StateId, bounds: Bounds) -> Bounds {
        self.bounds[state.block().id as usize] = bounds;
        bounds
    }
}
