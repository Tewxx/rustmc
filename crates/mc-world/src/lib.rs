//! Client-side world storage.

pub mod blocks;
pub mod chunk;
pub mod gen;
pub mod light;
pub mod region;

use chunk::Chunk;
use std::collections::HashMap;

#[derive(Default)]
pub struct World {
    pub chunks: HashMap<(i32, i32), Chunk>,
}

impl World {
    pub fn insert(&mut self, c: Chunk) {
        self.chunks.insert((c.x, c.z), c);
    }

    /// World coords. Unloaded chunks read as air.
    pub fn get(&self, x: i32, y: i32, z: i32) -> (u16, u8) {
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => c.get((x & 15) as usize, y, (z & 15) as usize),
            None => (0, 0),
        }
    }

    /// `(sky, block)` light. Outside 0..256 or in unloaded chunks this returns
    /// `EnumSkyBlock` defaults (15, 0), like `ChunkCache.getLightForExt`.
    pub fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !(0..256).contains(&y) {
            return (15, 0);
        }
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => {
                let i = chunk::column_index((x & 15) as usize, y, (z & 15) as usize);
                (c.sky_light[i], c.block_light[i])
            }
            None => (15, 0),
        }
    }

    /// `Block.getMixedBrightnessForBlock`: `sky << 20 | max(block, emission) << 4`,
    /// i.e. two 16-bit lightmap coordinates.
    pub fn mixed_brightness(&self, x: i32, y: i32, z: i32) -> i32 {
        let (id, meta) = self.get(x, y, z);
        let (sky, block) = self.light(x, y, z);
        let block = block.max(blocks::props(id, meta).emission);
        ((sky as i32) << 20) | ((block as i32) << 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coords_hit_right_chunk() {
        let mut w = World::default();
        let mut c = Chunk::new(-1, -1);
        c.set(15, 64, 15, 1, 0);
        w.insert(c);
        assert_eq!(w.get(-1, 64, -1), (1, 0));
        assert_eq!(w.get(-17, 64, -1), (0, 0));
    }

    #[test]
    fn generated_chunk_has_ground() {
        let c = gen::generate(0, 0);
        assert_eq!(c.get(0, 0, 0), (7, 0));
        assert_ne!(c.get(5, 40, 5).0, 0);
    }
}
