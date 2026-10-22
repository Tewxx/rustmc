//! World storage shared by the server and client: chunk columns plus vanilla's light engine
//! (`World.checkLightFor`, `Chunk.relightBlock`, `recheckGaps`, …).

pub mod chunk;
pub mod gen;
pub mod light;
pub mod region;

use chunk::Chunk;
use mc_core::{BlockPos, FxHashMap, FxHashSet};
use mc_data::{BlockAccess, StateId};

pub struct World {
    pub chunks: FxHashMap<(i32, i32), Chunk>,
    /// `!provider.getHasNoSky()`
    pub has_sky: bool,
    pub sea_level: i32,
    /// `skylightSubtracted`
    pub skylight_subtracted: i32,
    /// Render sections `(cx, sy, cz)` touched since the last [`World::take_dirty`]
    /// (`IWorldAccess.markBlockForUpdate` / `notifyLightSet`).
    pub dirty: FxHashSet<(i32, i32, i32)>,
    /// Whether to record [`World::dirty`] (only renderers need it).
    pub track_dirty: bool,
    /// `lightUpdateBlockList`
    light_queue: Vec<i32>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            chunks: FxHashMap::default(),
            has_sky: true,
            sea_level: 63,
            skylight_subtracted: 0,
            dirty: FxHashSet::default(),
            track_dirty: false,
            light_queue: vec![0; 32768],
        }
    }
}

/// `World.isValid`
/// What [`World::chunk_store`] hands to [`World::chunk_finish_set`].
#[derive(Clone, Copy, Debug)]
pub struct PendingSet {
    pub old: StateId,
    pub new_section: bool,
    pub old_height: i32,
}

#[inline]
pub fn is_valid(pos: BlockPos) -> bool {
    pos.x >= -30_000_000 && pos.z >= -30_000_000 && pos.x < 30_000_000 && pos.z < 30_000_000 && pos.y >= 0 && pos.y < 256
}

impl World {
    /// Inserts a chunk as-is (light and heightmap are taken from the chunk).
    pub fn insert(&mut self, c: Chunk) {
        let (x, z) = (c.x, c.z);
        self.chunks.insert((x, z), c);
        if self.track_dirty {
            for sy in 0..16 {
                for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    self.dirty.insert((x + dx, sy, z + dz));
                }
            }
        }
    }

    pub fn remove(&mut self, x: i32, z: i32) -> Option<Chunk> {
        self.chunks.remove(&(x, z))
    }

    #[inline]
    pub fn chunk(&self, cx: i32, cz: i32) -> Option<&Chunk> {
        self.chunks.get(&(cx, cz))
    }

    #[inline]
    pub fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    /// `isAreaLoaded(x1, y1, z1, x2, y2, z2)`
    pub fn is_area_loaded(&self, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> bool {
        if y2 < 0 || y1 >= 256 {
            return false;
        }
        for cx in x1 >> 4..=x2 >> 4 {
            for cz in z1 >> 4..=z2 >> 4 {
                if !self.chunks.contains_key(&(cx, cz)) {
                    return false;
                }
            }
        }
        true
    }

    pub fn is_area_loaded_around(&self, pos: BlockPos, r: i32) -> bool {
        self.is_area_loaded(pos.x - r, pos.y - r, pos.z - r, pos.x + r, pos.y + r, pos.z + r)
    }

    pub fn is_block_loaded(&self, pos: BlockPos) -> bool {
        is_valid(pos) && self.is_chunk_loaded(pos.x >> 4, pos.z >> 4)
    }

    /// Raw storage `(id, meta)`. Unloaded chunks read as air.
    pub fn get(&self, x: i32, y: i32, z: i32) -> (u16, u8) {
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => c.get((x & 15) as usize, y, (z & 15) as usize),
            None => (0, 0),
        }
    }

    /// `getBlockState`, including vanilla's air fallback for unregistered raw ids.
    #[inline]
    pub fn state_at(&self, x: i32, y: i32, z: i32) -> StateId {
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => c.state((x & 15) as usize, y, (z & 15) as usize),
            None => StateId::AIR,
        }
    }

    #[inline]
    pub fn state_pos(&self, pos: BlockPos) -> StateId {
        self.state_at(pos.x, pos.y, pos.z)
    }

    /// `isAirBlock`: material is air.
    pub fn is_air(&self, pos: BlockPos) -> bool {
        self.state_pos(pos).block().material == mc_data::Material::Air
    }

    /// Biome id at `pos`; plains when unassigned.
    pub fn biome_id(&self, pos: BlockPos) -> u8 {
        match self.chunks.get(&(pos.x >> 4, pos.z >> 4)).map(|c| c.biomes[((pos.z & 15) << 4 | (pos.x & 15)) as usize]) {
            Some(255) | None => 1,
            Some(b) => b,
        }
    }

    pub fn tile_entity(&self, pos: BlockPos) -> Option<&mc_core::nbt::Compound> {
        self.chunks.get(&(pos.x >> 4, pos.z >> 4))?.tile_entities.get(&pos)
    }

    /// `getHeight(pos).getY()`: heightmap value, 0 for unloaded chunks.
    pub fn height(&self, x: i32, z: i32) -> i32 {
        if x >= -30_000_000 && z >= -30_000_000 && x < 30_000_000 && z < 30_000_000 {
            self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |c| c.height((x & 15) as usize, (z & 15) as usize))
        } else {
            self.sea_level + 1
        }
    }

    /// `getChunksLowestHorizon`
    pub fn chunks_lowest_horizon(&self, x: i32, z: i32) -> i32 {
        if x >= -30_000_000 && z >= -30_000_000 && x < 30_000_000 && z < 30_000_000 {
            self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |c| c.height_map_minimum)
        } else {
            self.sea_level + 1
        }
    }

    /// `canSeeSky`
    pub fn can_see_sky(&self, pos: BlockPos) -> bool {
        self.chunks
            .get(&(pos.x >> 4, pos.z >> 4))
            .is_some_and(|c| c.can_see_sky((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize))
    }

    /// `getPrecipitationHeight(pos).getY()`
    pub fn precipitation_height(&mut self, x: i32, z: i32) -> i32 {
        match self.chunks.get_mut(&(x >> 4, z >> 4)) {
            Some(c) => c.precipitation_height((x & 15) as usize, (z & 15) as usize),
            None => -999,
        }
    }

    /// `getTopSolidOrLiquidBlock`
    pub fn top_solid_or_liquid(&self, x: i32, z: i32) -> BlockPos {
        let Some(c) = self.chunks.get(&(x >> 4, z >> 4)) else { return BlockPos::new(x, 0, z) };
        let mut y = c.top_filled_segment() + 16;
        while y >= 0 {
            let m = c.state((x & 15) as usize, y - 1, (z & 15) as usize).block().material;
            if m.props().blocks_movement && m != mc_data::Material::Leaves {
                break;
            }
            y -= 1;
        }
        BlockPos::new(x, y, z)
    }

    // ---------------------------------------------------------------------------------------
    // Light accessors

    /// `World.getLightFor`
    pub fn light_for(&self, sky: bool, pos: BlockPos) -> i32 {
        let y = pos.y.max(0);
        let default = if sky { 15 } else { 0 };
        if !is_valid(BlockPos::new(pos.x, y, pos.z)) {
            return default;
        }
        match self.chunks.get(&(pos.x >> 4, pos.z >> 4)) {
            Some(c) => c.light_for(sky, (pos.x & 15) as usize, y, (pos.z & 15) as usize),
            None => default,
        }
    }

    /// `World.setLightFor`
    pub fn set_light_for(&mut self, sky: bool, pos: BlockPos, value: i32) {
        if !is_valid(pos) {
            return;
        }
        let Some(c) = self.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)) else { return };
        let (x, y, z) = ((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize);
        if c.sections[(y >> 4) as usize].is_none() {
            c.sections[(y >> 4) as usize] = Some(chunk::Section::empty());
            if self.has_sky {
                c.generate_skylight_map();
            }
        }
        c.modified = true;
        let s = c.sections[(y >> 4) as usize].as_mut().unwrap();
        let i = chunk::section_index(x, y as usize & 15, z);
        if sky {
            if self.has_sky {
                s.sky_light[i] = value as u8;
            }
        } else {
            s.block_light[i] = value as u8;
        }
        self.notify_light_set(pos);
    }

    /// `World.getLightFromNeighborsFor`
    pub fn light_from_neighbors_for(&self, sky: bool, pos: BlockPos) -> i32 {
        if sky && !self.has_sky {
            return 0;
        }
        let pos = BlockPos::new(pos.x, pos.y.max(0), pos.z);
        if !self.is_block_loaded(pos) {
            return if sky { 15 } else { 0 };
        }
        if self.state_pos(pos).block().use_neighbor_brightness {
            let mut l = self.light_for(sky, pos.up());
            for p in [pos.east(), pos.west(), pos.south(), pos.north()] {
                l = l.max(self.light_for(sky, p));
            }
            return l;
        }
        self.light_for(sky, pos)
    }

    /// `World.getLight(pos, checkNeighbors)` with `skylightSubtracted`.
    pub fn light_value(&self, pos: BlockPos, check_neighbors: bool) -> i32 {
        if !(pos.x >= -30_000_000 && pos.z >= -30_000_000 && pos.x < 30_000_000 && pos.z < 30_000_000) {
            return 15;
        }
        if check_neighbors && self.state_pos(pos).block().use_neighbor_brightness {
            let mut l = self.light_value(pos.up(), false);
            for p in [pos.east(), pos.west(), pos.south(), pos.north()] {
                l = l.max(self.light_value(p, false));
            }
            return l;
        }
        if pos.y < 0 {
            return 0;
        }
        let y = pos.y.min(255);
        match self.chunks.get(&(pos.x >> 4, pos.z >> 4)) {
            Some(c) => c.light_subtracted((pos.x & 15) as usize, y, (pos.z & 15) as usize, self.skylight_subtracted),
            None => 0,
        }
    }

    /// `ChunkCache.getLightFor`: raw chunk light, `(15, 0)` style defaults outside 0..256.
    #[inline]
    pub fn cache_light_for(&self, sky: bool, x: i32, y: i32, z: i32) -> i32 {
        if !(0..256).contains(&y) {
            return if sky { 15 } else { 0 };
        }
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => c.light_for(sky, (x & 15) as usize, y, (z & 15) as usize),
            // EmptyChunk.getLightFor
            None => if sky { 15 } else { 0 },
        }
    }

    /// `ChunkCache.getLightForExt`
    pub fn cache_light_for_ext(&self, sky: bool, x: i32, y: i32, z: i32) -> i32 {
        if sky && !self.has_sky {
            return 0;
        }
        if !(0..256).contains(&y) {
            return if sky { 15 } else { 0 };
        }
        if self.state_at(x, y, z).block().use_neighbor_brightness {
            let mut l = 0;
            for f in mc_core::Facing::ALL {
                let (dx, dy, dz) = f.offset();
                l = l.max(self.cache_light_for(sky, x + dx, y + dy, z + dz));
                if l >= 15 {
                    return l;
                }
            }
            return l;
        }
        self.cache_light_for(sky, x, y, z)
    }

    /// `(sky, block)` raw light, like `ChunkCache.getLightFor`.
    pub fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        (self.cache_light_for(true, x, y, z) as u8, self.cache_light_for(false, x, y, z) as u8)
    }

    /// `Block.getMixedBrightnessForBlock` through `ChunkCache.getCombinedLight`:
    /// `sky << 20 | max(block, emission) << 4`.
    pub fn mixed_brightness(&self, x: i32, y: i32, z: i32) -> i32 {
        let emission = self.state_at(x, y, z).block().light_value as i32;
        self.combined_light(x, y, z, emission)
    }

    /// `ChunkCache.getCombinedLight`
    pub fn combined_light(&self, x: i32, y: i32, z: i32, light_value: i32) -> i32 {
        let sky = self.cache_light_for_ext(true, x, y, z);
        let block = self.cache_light_for_ext(false, x, y, z).max(light_value);
        sky << 20 | block << 4
    }

    // ---------------------------------------------------------------------------------------
    // Render notifications

    /// `RenderGlobal.markBlocksForUpdate(x1, y1, z1, x2, y2, z2)`
    pub fn mark_blocks_for_update(&mut self, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) {
        if !self.track_dirty {
            return;
        }
        let (sx1, sy1, sz1) = (x1.div_euclid(16), y1.div_euclid(16), z1.div_euclid(16));
        let (sx2, sy2, sz2) = (x2.div_euclid(16), y2.div_euclid(16), z2.div_euclid(16));
        for sx in sx1..=sx2 {
            for sy in sy1.max(0)..=sy2.min(15) {
                for sz in sz1..=sz2 {
                    self.dirty.insert((sx, sy, sz));
                }
            }
        }
    }

    /// `markBlockForUpdate`
    pub fn mark_block_for_update(&mut self, pos: BlockPos) {
        self.mark_blocks_for_update(pos.x - 1, pos.y - 1, pos.z - 1, pos.x + 1, pos.y + 1, pos.z + 1);
    }

    /// `notifyLightSet`
    #[inline]
    pub fn notify_light_set(&mut self, pos: BlockPos) {
        if self.track_dirty {
            self.mark_block_for_update(pos);
        }
    }

    /// `markBlockRangeForRenderUpdate`
    pub fn mark_block_range_for_render_update(&mut self, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) {
        self.mark_blocks_for_update(x1 - 1, y1 - 1, z1 - 1, x2 + 1, y2 + 1, z2 + 1);
    }

    pub fn take_dirty(&mut self) -> FxHashSet<(i32, i32, i32)> {
        std::mem::take(&mut self.dirty)
    }

    // ---------------------------------------------------------------------------------------
    // Block changes

    /// `Chunk.setBlockState` + the lighting part of `World.setBlockState`. Returns the previous
    /// state, or `None` when nothing changed (same state, invalid position, unloaded chunk).
    /// Block behaviour hooks (`breakBlock`, `onBlockAdded`, neighbour updates) are the caller's.
    pub fn set_block_state(&mut self, pos: BlockPos, state: StateId) -> Option<StateId> {
        if !is_valid(pos) {
            return None;
        }
        let old = self.chunk_set_block_state(pos, state)?;
        let (nb, ob) = (state.block(), old.block());
        if nb.light_opacity != ob.light_opacity || nb.light_value != ob.light_value {
            self.check_light(pos);
        }
        self.mark_block_for_update(pos);
        Some(old)
    }

    /// Convenience wrapper over [`World::set_block_state`] taking storage id/meta.
    pub fn set_block(&mut self, pos: BlockPos, id: u16, meta: u8) -> bool {
        let Some(block) = mc_data::Block::by_id(id) else { return false };
        let Some(state) = block.state_from_meta(meta) else { return false };
        if !self.is_block_loaded(pos) {
            return false;
        }
        self.set_block_state(pos, state);
        true
    }

    /// `Chunk.setBlockState` up to the `breakBlock` call: stores the state. `None` when nothing
    /// changed. Finish with [`World::chunk_finish_set`] (after running `breakBlock`, if any).
    pub fn chunk_store(&mut self, pos: BlockPos, state: StateId) -> Option<PendingSet> {
        let (cx, cz) = (pos.x >> 4, pos.z >> 4);
        let c = self.chunks.get_mut(&(cx, cz))?;
        let (i, j, k) = ((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize);
        let l = k << 4 | i;
        if j >= c.precipitation_height_map[l] - 1 {
            c.precipitation_height_map[l] = -999;
        }
        let i1 = c.height_map[l];
        let old = c.state(i, j, k);
        if old == state {
            return None;
        }
        let mut flag = false;
        if c.sections[(j >> 4) as usize].is_none() {
            if state.block().id == 0 {
                return None;
            }
            c.sections[(j >> 4) as usize] = Some(chunk::Section::empty());
            flag = j >= i1;
        }
        c.sections[(j >> 4) as usize]
            .as_mut()
            .unwrap()
            .set(chunk::section_index(i, j as usize & 15, k), state);
        Some(PendingSet { old, new_section: flag, old_height: i1 })
    }

    /// The rest of `Chunk.setBlockState` after `breakBlock`: returns the old state, or `None`
    /// when `breakBlock` replaced the block. Tile entities and `onBlockAdded` are the caller's.
    pub fn chunk_finish_set(&mut self, pos: BlockPos, state: StateId, p: PendingSet) -> Option<StateId> {
        let (cx, cz) = (pos.x >> 4, pos.z >> 4);
        let has_sky = self.has_sky;
        let (i, j, k) = ((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize);
        let c = self.chunks.get_mut(&(cx, cz))?;
        if c.state(i, j, k).block().id != state.block().id {
            return None;
        }
        let (block, block1) = (state.block(), p.old.block());
        let i1 = p.old_height;
        if p.new_section {
            if has_sky {
                c.generate_skylight_map();
            } else {
                c.generate_height_map();
            }
        } else {
            let j1 = block.light_opacity as i32;
            let k1 = block1.light_opacity as i32;
            if j1 > 0 {
                if j >= i1 {
                    self.relight_block(cx, cz, i, j + 1, k);
                }
            } else if j == i1 - 1 {
                self.relight_block(cx, cz, i, j, k);
            }
            let c = self.chunks.get_mut(&(cx, cz)).unwrap();
            if j1 != k1 && (j1 < k1 || c.light_for(true, i, j, k) > 0 || c.light_for(false, i, j, k) > 0) {
                // propagateSkylightOcclusion
                c.update_skylight_columns[i + k * 16] = true;
                c.is_gap_lighting_updated = true;
            }
        }
        let c = self.chunks.get_mut(&(cx, cz)).unwrap();
        c.modified = true;
        Some(p.old)
    }

    /// Client-side `Chunk.setBlockState` (`isRemote`): no block hooks; tile entities of the
    /// old block are dropped.
    fn chunk_set_block_state(&mut self, pos: BlockPos, state: StateId) -> Option<StateId> {
        let p = self.chunk_store(pos, state)?;
        let old = p.old;
        if old.block().id != state.block().id && old.block().has_tile_entity {
            if let Some(c) = self.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)) {
                c.tile_entities.remove(&pos);
            }
        }
        self.chunk_finish_set(pos, state, p)
    }

    /// `Chunk.relightBlock`
    fn relight_block(&mut self, cx: i32, cz: i32, x: usize, y: i32, z: usize) {
        let has_sky = self.has_sky;
        let (wx, wz) = (cx * 16 + x as i32, cz * 16 + z as i32);
        let (i, mut j);
        {
            let c = self.chunks.get_mut(&(cx, cz)).unwrap();
            i = c.height_map[z << 4 | x] & 255;
            j = i.max(y);
            while j > 0 && c.opacity(x, j - 1, z) == 0 {
                j -= 1;
            }
            if j == i {
                return;
            }
        }
        self.mark_blocks_dirty_vertical(wx, wz, j, i);
        let c = self.chunks.get_mut(&(cx, cz)).unwrap();
        c.height_map[z << 4 | x] = j;
        let mut lit = Vec::new();
        if has_sky {
            let range = if j < i { j..i } else { i..j };
            let value = if j < i { 15 } else { 0 };
            for y1 in range {
                if let Some(s) = &mut c.sections[(y1 >> 4) as usize] {
                    s.sky_light[chunk::section_index(x, y1 as usize & 15, z)] = value;
                    lit.push(y1);
                }
            }
            let mut k1 = 15;
            let mut jj = j;
            while jj > 0 && k1 > 0 {
                jj -= 1;
                let mut i2 = c.opacity(x, jj, z);
                if i2 == 0 {
                    i2 = 1;
                }
                k1 = (k1 - i2).max(0);
                if let Some(s) = &mut c.sections[(jj >> 4) as usize] {
                    s.sky_light[chunk::section_index(x, jj as usize & 15, z)] = k1 as u8;
                }
            }
        }
        let l1 = c.height_map[z << 4 | x];
        let (j2, k2) = if l1 < i { (l1, i) } else { (i, l1) };
        if l1 < c.height_map_minimum {
            c.height_map_minimum = l1;
        }
        for y1 in lit {
            self.notify_light_set(BlockPos::new(wx, y1, wz));
        }
        if has_sky {
            for f in mc_core::Facing::PLANE_HORIZONTAL {
                let (dx, _, dz) = f.offset();
                self.update_skylight_neighbor_height(wx + dx, wz + dz, j2, k2);
            }
            self.update_skylight_neighbor_height(wx, wz, j2, k2);
        }
        self.chunks.get_mut(&(cx, cz)).unwrap().modified = true;
    }

    /// `markBlocksDirtyVertical`
    fn mark_blocks_dirty_vertical(&mut self, x: i32, z: i32, mut y1: i32, mut y2: i32) {
        if y1 > y2 {
            std::mem::swap(&mut y1, &mut y2);
        }
        if self.has_sky {
            for y in y1..=y2 {
                self.check_light_for(true, BlockPos::new(x, y, z));
            }
        }
        self.mark_block_range_for_render_update(x, y1, z, x, y2, z);
    }

    /// `Chunk.updateSkylightNeighborHeight`
    fn update_skylight_neighbor_height(&mut self, x: i32, z: i32, start: i32, end: i32) {
        if end > start && self.is_area_loaded_around(BlockPos::new(x, 0, z), 16) {
            for y in start..end {
                self.check_light_for(true, BlockPos::new(x, y, z));
            }
            if let Some(c) = self.chunks.get_mut(&(x >> 4, z >> 4)) {
                c.modified = true;
            }
        }
    }

    /// `Chunk.checkSkylightNeighborHeight`
    fn check_skylight_neighbor_height(&mut self, x: i32, z: i32, max: i32) {
        let i = self.height(x, z);
        if i > max {
            self.update_skylight_neighbor_height(x, z, max, i + 1);
        } else if i < max {
            self.update_skylight_neighbor_height(x, z, i, max + 1);
        }
    }

    /// `Chunk.recheckGaps(onlyOne)`
    pub fn recheck_gaps(&mut self, cx: i32, cz: i32, only_one: bool) {
        if !self.is_area_loaded_around(BlockPos::new(cx * 16 + 8, 0, cz * 16 + 8), 16) {
            return;
        }
        for i in 0..16usize {
            for j in 0..16usize {
                let Some(c) = self.chunks.get_mut(&(cx, cz)) else { return };
                if !c.update_skylight_columns[i + j * 16] {
                    continue;
                }
                c.update_skylight_columns[i + j * 16] = false;
                let k = c.height(i, j);
                let (l, i1) = (cx * 16 + i as i32, cz * 16 + j as i32);
                let mut j1 = i32::MAX;
                for f in mc_core::Facing::PLANE_HORIZONTAL {
                    let (dx, _, dz) = f.offset();
                    j1 = j1.min(self.chunks_lowest_horizon(l + dx, i1 + dz));
                }
                self.check_skylight_neighbor_height(l, i1, j1);
                for f in mc_core::Facing::PLANE_HORIZONTAL {
                    let (dx, _, dz) = f.offset();
                    self.check_skylight_neighbor_height(l + dx, i1 + dz, k);
                }
                if only_one {
                    return;
                }
            }
        }
        if let Some(c) = self.chunks.get_mut(&(cx, cz)) {
            c.is_gap_lighting_updated = false;
        }
    }

    /// `Chunk.func_150804_b`: per-tick chunk light upkeep. `is_remote` is the client.
    pub fn tick_chunk_light(&mut self, cx: i32, cz: i32, is_remote: bool, skip_gaps: bool) {
        let Some(c) = self.chunks.get_mut(&(cx, cz)) else { return };
        let gaps = c.is_gap_lighting_updated && self.has_sky && !skip_gaps;
        if gaps {
            self.recheck_gaps(cx, cz, is_remote);
        }
        let c = self.chunks.get_mut(&(cx, cz)).unwrap();
        c.ticked = true;
        if !c.light_populated && c.terrain_populated {
            self.populate_light(cx, cz);
        }
    }

    /// `Chunk.func_150809_p`: first full light pass of a populated chunk.
    pub fn populate_light(&mut self, cx: i32, cz: i32) {
        {
            let c = self.chunks.get_mut(&(cx, cz)).unwrap();
            c.terrain_populated = true;
            c.light_populated = true;
        }
        if !self.has_sky {
            return;
        }
        let (bx, bz) = (cx << 4, cz << 4);
        if !self.is_area_loaded(bx - 1, 0, bz - 1, bx + 16, self.sea_level, bz + 16) {
            self.chunks.get_mut(&(cx, cz)).unwrap().light_populated = false;
            return;
        }
        'outer: for i in 0..16 {
            for j in 0..16 {
                if !self.check_light_column(cx, cz, i, j) {
                    self.chunks.get_mut(&(cx, cz)).unwrap().light_populated = false;
                    break 'outer;
                }
            }
        }
        if self.chunks[&(cx, cz)].light_populated {
            for f in mc_core::Facing::PLANE_HORIZONTAL {
                let (dx, _, dz) = f.offset();
                let (ncx, ncz) = (cx + dx, cz + dz);
                // func_180700_a on the neighbour, for the edge facing back at this chunk.
                if !self.chunks.get(&(ncx, ncz)).is_some_and(|c| c.terrain_populated) {
                    continue;
                }
                for n in 0..16 {
                    let (x, z) = match f.opposite() {
                        mc_core::Facing::East => (15, n),
                        mc_core::Facing::West => (0, n),
                        mc_core::Facing::South => (n, 15),
                        _ => (n, 0),
                    };
                    self.check_light_column(ncx, ncz, x, z);
                }
            }
            // func_177441_y
            self.chunks.get_mut(&(cx, cz)).unwrap().update_skylight_columns = [true; 256];
            self.recheck_gaps(cx, cz, false);
        }
    }

    /// `Chunk.func_150811_f`
    fn check_light_column(&mut self, cx: i32, cz: i32, x: usize, z: usize) -> bool {
        let Some(c) = self.chunks.get(&(cx, cz)) else { return true };
        let top = c.top_filled_segment();
        let (wx, wz) = ((cx << 4) + x as i32, (cz << 4) + z as i32);
        let mut flag = false;
        let mut flag1 = false;
        let mut j = top + 16 - 1;
        while j > self.sea_level || j > 0 && !flag1 {
            let k = self.chunks[&(cx, cz)].opacity(x, j, z);
            if k == 255 && j < self.sea_level {
                flag1 = true;
            }
            if !flag && k > 0 {
                flag = true;
            } else if flag && k == 0 && !self.check_light(BlockPos::new(wx, j, wz)) {
                return false;
            }
            j -= 1;
        }
        let mut l = j;
        while l > 0 {
            if self.chunks[&(cx, cz)].state(x, l, z).block().light_value > 0 {
                self.check_light(BlockPos::new(wx, l, wz));
            }
            l -= 1;
        }
        true
    }

    /// `Chunk.enqueueRelightChecks`: one step of the round-robin relight.
    pub fn enqueue_relight_checks(&mut self, cx: i32, cz: i32) {
        let base = BlockPos::new(cx << 4, 0, cz << 4);
        for _ in 0..8 {
            let Some(c) = self.chunks.get_mut(&(cx, cz)) else { return };
            if c.queued_light_checks >= 4096 {
                return;
            }
            let q = c.queued_light_checks;
            let (j, k, l) = (q % 16, q / 16 % 16, q / 256);
            c.queued_light_checks += 1;
            for i1 in 0..16 {
                let p = base.offset(k, (j << 4) + i1, l);
                let edge = i1 == 0 || i1 == 15 || k == 0 || k == 15 || l == 0 || l == 15;
                let c = &self.chunks[&(cx, cz)];
                let check = match &c.sections[j as usize] {
                    None => edge,
                    Some(s) => s.state(chunk::section_index(k as usize, i1 as usize, l as usize)).block().material == mc_data::Material::Air,
                };
                if check {
                    for f in mc_core::Facing::ALL {
                        let n = p.offset_facing(f);
                        if self.state_pos(n).block().light_value > 0 {
                            self.check_light(n);
                        }
                    }
                    self.check_light(p);
                }
            }
        }
    }

    /// `World.checkLight`
    pub fn check_light(&mut self, pos: BlockPos) -> bool {
        let mut flag = false;
        if self.has_sky {
            flag |= self.check_light_for(true, pos);
        }
        flag | self.check_light_for(false, pos)
    }

    /// `World.getRawLight`
    fn raw_light(&self, pos: BlockPos, sky: bool) -> i32 {
        if sky && self.can_see_sky(pos) {
            return 15;
        }
        let block = self.state_pos(pos).block();
        let mut i = if sky { 0 } else { block.light_value as i32 };
        let mut j = block.light_opacity as i32;
        if j >= 15 && block.light_value > 0 {
            j = 1;
        }
        if j < 1 {
            j = 1;
        }
        if j >= 15 {
            return 0;
        }
        if i >= 14 {
            return i;
        }
        for f in mc_core::Facing::ALL {
            let k = self.light_for(sky, pos.offset_facing(f)) - j;
            if k > i {
                i = k;
            }
            if i >= 14 {
                return i;
            }
        }
        i
    }

    /// `World.checkLightFor`
    pub fn check_light_for(&mut self, sky: bool, pos: BlockPos) -> bool {
        if !self.is_area_loaded_around(pos, 17) {
            return false;
        }
        let mut queue = std::mem::take(&mut self.light_queue);
        let len = queue.len();
        let mut i = 0usize;
        let mut j = 0usize;
        let k = self.light_for(sky, pos);
        let l = self.raw_light(pos, sky);
        let (i1, j1, k1) = (pos.x, pos.y, pos.z);
        if l > k {
            queue[j] = 133152;
            j += 1;
        } else if l < k {
            queue[j] = 133152 | k << 18;
            j += 1;
            while i < j {
                let l1 = queue[i];
                i += 1;
                let i2 = (l1 & 63) - 32 + i1;
                let j2 = (l1 >> 6 & 63) - 32 + j1;
                let k2 = (l1 >> 12 & 63) - 32 + k1;
                let l2 = l1 >> 18 & 15;
                let bp = BlockPos::new(i2, j2, k2);
                let i3 = self.light_for(sky, bp);
                if i3 == l2 {
                    self.set_light_for(sky, bp, 0);
                    if l2 > 0 {
                        let d = (i2 - i1).abs() + (j2 - j1).abs() + (k2 - k1).abs();
                        if d < 17 {
                            for f in mc_core::Facing::ALL {
                                let (dx, dy, dz) = f.offset();
                                let n = BlockPos::new(i2 + dx, j2 + dy, k2 + dz);
                                let l4 = (self.state_pos(n).block().light_opacity as i32).max(1);
                                let i3 = self.light_for(sky, n);
                                if i3 == l2 - l4 && j < len {
                                    queue[j] = (n.x - i1 + 32) | (n.y - j1 + 32) << 6 | (n.z - k1 + 32) << 12 | (l2 - l4) << 18;
                                    j += 1;
                                }
                            }
                        }
                    }
                }
            }
            i = 0;
        }
        while i < j {
            let i5 = queue[i];
            i += 1;
            let j5 = (i5 & 63) - 32 + i1;
            let k5 = (i5 >> 6 & 63) - 32 + j1;
            let l5 = (i5 >> 12 & 63) - 32 + k1;
            let bp = BlockPos::new(j5, k5, l5);
            let i6 = self.light_for(sky, bp);
            let j6 = self.raw_light(bp, sky);
            if j6 != i6 {
                self.set_light_for(sky, bp, j6);
                if j6 > i6 {
                    let d = (j5 - i1).abs() + (k5 - j1).abs() + (l5 - k1).abs();
                    let room = j < len - 6;
                    if d < 17 && room {
                        let enc = |x: i32, y: i32, z: i32| x - i1 + 32 + ((y - j1 + 32) << 6) + ((z - k1 + 32) << 12);
                        if self.light_for(sky, bp.west()) < j6 {
                            queue[j] = enc(j5 - 1, k5, l5);
                            j += 1;
                        }
                        if self.light_for(sky, bp.east()) < j6 {
                            queue[j] = enc(j5 + 1, k5, l5);
                            j += 1;
                        }
                        if self.light_for(sky, bp.down()) < j6 {
                            queue[j] = enc(j5, k5 - 1, l5);
                            j += 1;
                        }
                        if self.light_for(sky, bp.up()) < j6 {
                            queue[j] = enc(j5, k5 + 1, l5);
                            j += 1;
                        }
                        if self.light_for(sky, bp.north()) < j6 {
                            queue[j] = enc(j5, k5, l5 - 1);
                            j += 1;
                        }
                        if self.light_for(sky, bp.south()) < j6 {
                            queue[j] = enc(j5, k5, l5 + 1);
                            j += 1;
                        }
                    }
                }
            }
        }
        self.light_queue = queue;
        true
    }
}

impl BlockAccess for World {
    fn state(&self, pos: BlockPos) -> StateId {
        self.state_at(pos.x, pos.y, pos.z)
    }

    fn flower_pot(&self, pos: BlockPos) -> Option<(i32, i32)> {
        flower_pot_of(self.tile_entity(pos)?)
    }

    fn comparator_output(&self, pos: BlockPos) -> Option<i32> {
        comparator_of(self.tile_entity(pos)?)
    }

    fn moving_piston(&self, pos: BlockPos) -> Option<mc_data::geometry::MovingPiston> {
        moving_piston_of(self.tile_entity(pos)?)
    }
}

pub fn flower_pot_of(tile: &mc_core::nbt::Compound) -> Option<(i32, i32)> {
    use mc_core::nbt::Tag;
    if tile.get("id").and_then(Tag::as_str) != Some("FlowerPot") {
        return None;
    }
    let item = tile
        .get("Item")
        .and_then(|t| t.as_i64().map(|i| i as i32).or_else(|| t.as_str().and_then(mc_data::Block::by_name).map(|b| b.id as i32)))
        .unwrap_or(0);
    Some((item, tile.get("Data").and_then(Tag::as_i64).unwrap_or(0) as i32))
}

pub fn comparator_of(tile: &mc_core::nbt::Compound) -> Option<i32> {
    use mc_core::nbt::Tag;
    if tile.get("id").and_then(Tag::as_str) != Some("Comparator") {
        return None;
    }
    Some(tile.get("OutputSignal").and_then(Tag::as_i64).unwrap_or(0) as i32)
}

pub fn moving_piston_of(tile: &mc_core::nbt::Compound) -> Option<mc_data::geometry::MovingPiston> {
    use mc_core::nbt::Tag;
    if tile.get("id").and_then(Tag::as_str) != Some("Piston") {
        return None;
    }
    let id = tile.get("blockId").and_then(Tag::as_i64).unwrap_or(0) as u16;
    let meta = tile.get("blockData").and_then(Tag::as_i64).unwrap_or(0) as u8;
    let block = mc_data::Block::by_id(id).unwrap_or_else(|| mc_data::Block::by_id(0).unwrap());
    Some(mc_data::geometry::MovingPiston {
        state: block.state_from_meta(meta)?,
        // EnumFacing.getFront uses abs(index % 6), including for negative saved values.
        facing: mc_core::Facing::from_index(((tile.get("facing").and_then(Tag::as_i64).unwrap_or(0) as i32) % 6).unsigned_abs() as usize),
        progress: tile.get("progress").and_then(Tag::as_f32).unwrap_or(0.),
        extending: tile.get("extending").and_then(Tag::as_i64).unwrap_or(0) != 0,
    })
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
}
