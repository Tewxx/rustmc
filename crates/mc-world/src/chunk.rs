//! 16×256×16 chunk column with 1.8 storage (`Chunk` + `ExtendedBlockStorage`).
//!
//! Blocks are stored as `id << 4 | meta` like `Block.BLOCK_STATE_IDS`. Light is one byte per
//! block (vanilla packs nibbles; values are identical). Methods that only touch this column live
//! here; the ones that reach into neighbouring chunks (`relightBlock`, `recheckGaps`, …) are on
//! [`crate::World`].

use mc_core::{nbt::Compound, BlockPos};
use mc_data::StateId;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub const SECTION_VOLUME: usize = 16 * 16 * 16;

#[derive(Clone)]
/// Mesh snapshots share immutable buffers; writes detach only the affected buffer.
pub struct SectionData<T: Clone>(Arc<[T; SECTION_VOLUME]>);

impl<T: Clone> Deref for SectionData<T> {
    type Target = [T; SECTION_VOLUME];
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl<T: Clone> SectionData<T> {
    /// The same buffer (no write detached either copy since one was cloned from the other).
    pub fn same(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}

impl<T: Clone> DerefMut for SectionData<T> {
    fn deref_mut(&mut self) -> &mut Self::Target { Arc::make_mut(&mut self.0) }
}

#[derive(Clone)]
pub struct Section {
    /// Indexed `y << 8 | z << 4 | x`, like `ExtendedBlockStorage`.
    pub blocks: SectionData<u16>,
    pub sky_light: SectionData<u8>,
    pub block_light: SectionData<u8>,
    /// `blockRefCount`: non-air blocks.
    pub block_count: u16,
    /// `tickRefCount`: blocks that tick randomly.
    pub tick_count: u16,
}

impl Section {
    pub fn empty() -> Self {
        Self {
            blocks: SectionData(Arc::new([0; SECTION_VOLUME])),
            sky_light: SectionData(Arc::new([0; SECTION_VOLUME])),
            block_light: SectionData(Arc::new([0; SECTION_VOLUME])),
            block_count: 0,
            tick_count: 0,
        }
    }

    #[inline]
    pub fn state(&self, i: usize) -> StateId {
        StateId::from_raw(self.blocks[i])
    }

    /// `ExtendedBlockStorage.set`, keeping the reference counts.
    #[inline]
    pub fn set(&mut self, i: usize, state: StateId) {
        let old = self.state(i).block();
        if old.id != 0 {
            self.block_count -= 1;
            if old.ticks_randomly {
                self.tick_count -= 1;
            }
        }
        let new = state.block();
        if new.id != 0 {
            self.block_count += 1;
            if new.ticks_randomly {
                self.tick_count += 1;
            }
        }
        self.blocks[i] = state.raw();
    }

    /// `removeInvalidBlocks`: recount after raw writes.
    pub fn recount(&mut self) {
        self.block_count = 0;
        self.tick_count = 0;
        for &raw in self.blocks.iter() {
            let b = StateId::from_raw(raw).block();
            if b.id != 0 {
                self.block_count += 1;
                if b.ticks_randomly {
                    self.tick_count += 1;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.block_count == 0
    }
}

#[inline]
pub fn section_index(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn shared_section_buffers_detach_independently() {
        let mut section = Section::empty();
        section.set(3, StateId::from_raw(16));
        section.sky_light[3] = 15;
        section.block_light[3] = 7;
        let mut snapshot = section.clone();
        assert!(Arc::ptr_eq(&section.blocks.0, &snapshot.blocks.0));
        assert!(Arc::ptr_eq(&section.sky_light.0, &snapshot.sky_light.0));
        assert!(Arc::ptr_eq(&section.block_light.0, &snapshot.block_light.0));
        section.set(3, StateId::from_raw(0));
        assert_eq!(snapshot.blocks[3], 16);
        assert_eq!(snapshot.block_count, 1);
        assert_eq!(section.block_count, 0);
        assert!(!Arc::ptr_eq(&section.blocks.0, &snapshot.blocks.0));
        assert!(Arc::ptr_eq(&section.sky_light.0, &snapshot.sky_light.0));
        snapshot.sky_light[..4].fill(9);
        assert_eq!(section.sky_light[3], 15);
        assert_eq!(snapshot.sky_light[3], 9);
        assert!(!Arc::ptr_eq(&section.sky_light.0, &snapshot.sky_light.0));
        assert!(Arc::ptr_eq(&section.block_light.0, &snapshot.block_light.0));
        for light in section.block_light.iter_mut() { *light = 1; }
        assert_eq!(snapshot.block_light[3], 7);
        assert_eq!(section.block_light[3], 1);
        assert!(!Arc::ptr_eq(&section.block_light.0, &snapshot.block_light.0));
    }
}

#[derive(Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub sections: [Option<Section>; 16],
    /// Biome ids, indexed `z << 4 | x`; 255 = not yet assigned (vanilla's -1).
    pub biomes: [u8; 256],
    /// Saved tile-entity compounds keyed by absolute block position.
    pub tile_entities: HashMap<BlockPos, Compound>,
    /// `heightMap`: lowest y at which the column sees the sky (first y above an opaque block).
    pub height_map: [i32; 256],
    pub precipitation_height_map: [i32; 256],
    pub update_skylight_columns: [bool; 256],
    pub is_gap_lighting_updated: bool,
    pub height_map_minimum: i32,
    pub terrain_populated: bool,
    pub light_populated: bool,
    /// `field_150815_m`: ticked at least once by the world.
    pub ticked: bool,
    pub modified: bool,
    pub inhabited_time: i64,
    /// `queuedLightChecks`
    pub queued_light_checks: i32,
}

impl Chunk {
    /// Copy only the section and its vertical neighbors while retaining column metadata.
    pub fn mesh_snapshot(&self, sy: i32) -> Self {
        let mut c = Self::new(self.x, self.z);
        for y in (sy - 1).max(0)..=(sy + 1).min(15) { c.sections[y as usize] = self.sections[y as usize].clone(); }
        c.biomes = self.biomes;
        let min_y = (sy - 1).max(0) * 16;
        let max_y = ((sy + 2).min(16) * 16) - 1;
        c.tile_entities = self.tile_entities.iter().filter(|(p, _)| (min_y..=max_y).contains(&p.y))
            .map(|(p, tag)| (*p, tag.clone())).collect();
        c.height_map = self.height_map;
        c.height_map_minimum = self.height_map_minimum;
        c.terrain_populated = self.terrain_populated;
        c.light_populated = self.light_populated;
        c.ticked = self.ticked;
        c
    }

    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            sections: Default::default(),
            biomes: [255; 256],
            tile_entities: HashMap::new(),
            height_map: [0; 256],
            precipitation_height_map: [-999; 256],
            update_skylight_columns: [false; 256],
            is_gap_lighting_updated: false,
            height_map_minimum: 0,
            terrain_populated: false,
            light_populated: false,
            ticked: false,
            modified: false,
            inhabited_time: 0,
            queued_light_checks: 4096,
        }
    }

    /// Local coords. Returns `(id, meta)`; air outside 0..256.
    pub fn get(&self, x: usize, y: i32, z: usize) -> (u16, u8) {
        if !(0..256).contains(&y) {
            return (0, 0);
        }
        match &self.sections[(y >> 4) as usize] {
            Some(s) => {
                let v = s.blocks[section_index(x, y as usize & 15, z)];
                (v >> 4, (v & 15) as u8)
            }
            None => (0, 0),
        }
    }

    /// Resolved state at local coords (`Chunk.getBlockState`).
    #[inline]
    pub fn state(&self, x: usize, y: i32, z: usize) -> StateId {
        if !(0..256).contains(&y) {
            return StateId::AIR;
        }
        match &self.sections[(y >> 4) as usize] {
            Some(s) => s.state(section_index(x, y as usize & 15, z)),
            None => StateId::AIR,
        }
    }

    /// Raw write without lighting (what `ChunkPrimer` and loaders do). Creates the section.
    pub fn set(&mut self, x: usize, y: i32, z: usize, id: u16, meta: u8) {
        if !(0..256).contains(&y) {
            return;
        }
        let raw = (id << 4) | (meta as u16 & 15);
        let s = self.sections[(y >> 4) as usize].get_or_insert_with(Section::empty);
        s.set(section_index(x, y as usize & 15, z), StateId::from_raw(raw));
        // Keep unregistered raw values as stored, like the char array does.
        s.blocks[section_index(x, y as usize & 15, z)] = raw;
    }

    /// Raw state write without lighting; air into a missing section is a no-op.
    pub fn set_state_raw(&mut self, x: usize, y: i32, z: usize, state: StateId) {
        if !(0..256).contains(&y) {
            return;
        }
        let slot = &mut self.sections[(y >> 4) as usize];
        if slot.is_none() {
            if state.is_air() {
                return;
            }
            *slot = Some(Section::empty());
        }
        slot.as_mut().unwrap().set(section_index(x, y as usize & 15, z), state);
    }

    /// `getBlockLightOpacity(x, y, z)` for local coords.
    #[inline]
    pub fn opacity(&self, x: usize, y: i32, z: usize) -> i32 {
        self.state(x, y, z).block().light_opacity as i32
    }

    /// `getTopFilledSegment`: base y of the highest allocated section.
    pub fn top_filled_segment(&self) -> i32 {
        (0..16).rev().find(|&i| self.sections[i].is_some()).map_or(0, |i| (i as i32) << 4)
    }

    #[inline]
    pub fn height(&self, x: usize, z: usize) -> i32 {
        self.height_map[z << 4 | x]
    }

    /// `canSeeSky`
    #[inline]
    pub fn can_see_sky(&self, x: usize, y: i32, z: usize) -> bool {
        y >= self.height_map[z << 4 | x]
    }

    /// `Chunk.getLightFor` with local coords; `y` must be in 0..256.
    #[inline]
    pub fn light_for(&self, sky: bool, x: usize, y: i32, z: usize) -> i32 {
        match &self.sections[(y >> 4) as usize] {
            None => {
                if sky && self.can_see_sky(x, y, z) {
                    15
                } else {
                    0
                }
            }
            Some(s) => {
                let i = section_index(x, y as usize & 15, z);
                if sky {
                    s.sky_light[i] as i32
                } else {
                    s.block_light[i] as i32
                }
            }
        }
    }

    /// `getLightSubtracted`
    pub fn light_subtracted(&self, x: usize, y: i32, z: usize, amount: i32) -> i32 {
        match &self.sections[(y >> 4) as usize] {
            None => {
                if amount < 15 {
                    15 - amount
                } else {
                    0
                }
            }
            Some(s) => {
                let i = section_index(x, y as usize & 15, z);
                (s.sky_light[i] as i32 - amount).max(s.block_light[i] as i32)
            }
        }
    }

    /// `generateHeightMap` (client chunk data path: no sky light writes).
    pub fn generate_height_map(&mut self) {
        let top = self.top_filled_segment();
        self.height_map_minimum = i32::MAX;
        for x in 0..16 {
            for z in 0..16 {
                self.precipitation_height_map[x + (z << 4)] = -999;
                for l in (1..=top + 16).rev() {
                    if self.opacity(x, l - 1, z) != 0 {
                        self.height_map[z << 4 | x] = l;
                        self.height_map_minimum = self.height_map_minimum.min(l);
                        break;
                    }
                }
            }
        }
        self.modified = true;
    }

    /// `generateSkylightMap` (for a world with sky).
    pub fn generate_skylight_map(&mut self) {
        let top = self.top_filled_segment();
        self.height_map_minimum = i32::MAX;
        for x in 0..16 {
            for z in 0..16 {
                self.precipitation_height_map[x + (z << 4)] = -999;
                for l in (1..=top + 16).rev() {
                    if self.opacity(x, l - 1, z) != 0 {
                        self.height_map[z << 4 | x] = l;
                        self.height_map_minimum = self.height_map_minimum.min(l);
                        break;
                    }
                }
                let mut k1 = 15;
                let mut i1 = top + 16 - 1;
                loop {
                    let mut j1 = self.opacity(x, i1, z);
                    if j1 == 0 && k1 != 15 {
                        j1 = 1;
                    }
                    k1 -= j1;
                    if k1 > 0 {
                        if let Some(s) = &mut self.sections[(i1 >> 4) as usize] {
                            s.sky_light[section_index(x, i1 as usize & 15, z)] = k1 as u8;
                        }
                    }
                    i1 -= 1;
                    if i1 <= 0 || k1 <= 0 {
                        break;
                    }
                }
            }
        }
        self.modified = true;
    }

    /// `getPrecipitationHeight` (local x/z), caching like vanilla.
    pub fn precipitation_height(&mut self, x: usize, z: usize) -> i32 {
        let k = x | z << 4;
        if self.precipitation_height_map[k] == -999 {
            let mut y = self.top_filled_segment() + 15;
            let mut found = -1;
            while y > 0 && found == -1 {
                let m = self.state(x, y, z).block().material.props();
                if !m.blocks_movement && !m.liquid {
                    y -= 1;
                } else {
                    found = y + 1;
                }
            }
            self.precipitation_height_map[k] = found;
        }
        self.precipitation_height_map[k]
    }

    /// `getAreLevelsEmpty`
    pub fn levels_empty(&self, start: i32, end: i32) -> bool {
        let (start, end) = (start.max(0), end.min(255));
        let mut y = start;
        while y <= end {
            if self.sections[(y >> 4) as usize].as_ref().is_some_and(|s| !s.is_empty()) {
                return false;
            }
            y += 16;
        }
        true
    }

    pub fn biome(&self, x: usize, z: usize) -> u8 {
        self.biomes[z << 4 | x]
    }

    /// Recomputes section counts after raw writes (`removeInvalidBlocks`).
    pub fn recount(&mut self) {
        for s in self.sections.iter_mut().flatten() {
            s.recount();
        }
    }
}
