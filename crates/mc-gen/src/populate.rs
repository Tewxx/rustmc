//! `ChunkProviderGenerate.populate`: lakes, dungeons, `BiomeDecorator` and the per-biome
//! `decorate` overrides, then snow and ice.
use crate::biome::{self, GenBiome, Kind};
use crate::terrain::{grass_color_noise, temperature_at, Generator};
use crate::trees::{self, Tree};
use mc_core::math::{self, floor_double};
use mc_core::random::JavaRandom;
use mc_core::{BlockPos, Facing};
use mc_data::{ids, Block, Material, StateId};
use mc_logic::{blocks, Level, Rng};
use mc_world::World;

/// The world as population sees it: `World` with block hooks, light and the generator's random.
pub struct Pop<'a> {
    pub world: &'a mut World,
    pub rand: JavaRandom,
    world_rand: JavaRandom,
    pub seed: i64,
}

impl Level for Pop<'_> {
    fn state(&mut self, pos: BlockPos) -> StateId {
        self.world.state_at(pos.x, pos.y, pos.z)
    }
    /// `World.setBlockState` (block hooks, light; neighbour notification is not needed for flag 2).
    fn set_state(&mut self, pos: BlockPos, state: StateId, _flags: i32) -> bool {
        if !(0..256).contains(&pos.y) {
            return false;
        }
        let Some(p) = self.world.chunk_store(pos, state) else { return false };
        let old = p.old;
        if old.block().id != state.block().id {
            blocks::break_block(self, pos, old);
        }
        if self.world.chunk_finish_set(pos, state, p).is_none() {
            return false;
        }
        if old.block().id != state.block().id {
            blocks::on_block_added(self, pos, state);
        }
        let (nb, ob) = (state.block(), old.block());
        if nb.light_opacity != ob.light_opacity || nb.light_value != ob.light_value {
            self.world.check_light(pos);
        }
        true
    }
    fn light_for(&mut self, sky: bool, pos: BlockPos) -> i32 {
        self.world.light_for(sky, pos)
    }
    fn light(&mut self, pos: BlockPos) -> i32 {
        if !(0..256).contains(&pos.y) {
            return if pos.y < 0 { 0 } else { 15 };
        }
        let (s, b) = self.world.light(pos.x, pos.y, pos.z);
        s.max(b) as i32
    }
    fn is_area_loaded(&mut self, from: BlockPos, to: BlockPos) -> bool {
        self.world.is_area_loaded(from.x, from.y, from.z, to.x, to.y, to.z)
    }
    fn schedule_update(&mut self, _pos: BlockPos, _block: u16, _delay: i32) {}
    fn rng(&mut self, which: Rng) -> &mut JavaRandom {
        match which {
            Rng::Gen => &mut self.rand,
            Rng::World => &mut self.world_rand,
        }
    }
    fn as_level(&mut self) -> &mut dyn Level {
        self
    }
}

pub fn state(name: &str) -> StateId {
    Block::by_name(name).unwrap_or_else(|| panic!("no block {name}")).default_state
}

impl Pop<'_> {
    #[inline]
    pub fn ni(&mut self, n: i32) -> i32 {
        self.rand.next_int_bounded(n)
    }
    #[inline]
    pub fn nf(&mut self) -> f32 {
        self.rand.next_float()
    }
    pub fn id(&mut self, pos: BlockPos) -> u16 {
        self.state(pos).block().id
    }
    pub fn mat(&mut self, pos: BlockPos) -> Material {
        self.state(pos).block().material
    }
    pub fn air(&mut self, pos: BlockPos) -> bool {
        self.mat(pos) == Material::Air
    }
    /// `setBlockAndNotifyAdequately` / `setBlockState(..., 2)`
    pub fn put(&mut self, pos: BlockPos, s: StateId) {
        self.set_state(pos, s, 2);
    }
    /// `World.getHeight(pos)`
    pub fn height(&self, pos: BlockPos) -> BlockPos {
        let h = self.world.chunks.get(&(pos.x >> 4, pos.z >> 4)).map_or(0, |c| c.height((pos.x & 15) as usize, (pos.z & 15) as usize));
        BlockPos::new(pos.x, h, pos.z)
    }
    /// `World.getTopSolidOrLiquidBlock`
    pub fn top_solid_or_liquid(&self, pos: BlockPos) -> BlockPos {
        let Some(c) = self.world.chunks.get(&(pos.x >> 4, pos.z >> 4)) else { return BlockPos::new(pos.x, 0, pos.z) };
        let mut y = c.top_filled_segment() + 16;
        while y >= 0 {
            let m = c.state((pos.x & 15) as usize, y - 1, (pos.z & 15) as usize).block().material;
            if m.props().blocks_movement && m != Material::Leaves {
                break;
            }
            y -= 1;
        }
        BlockPos::new(pos.x, y, pos.z)
    }
    pub fn biome(&self, pos: BlockPos) -> &'static GenBiome {
        biome::biome_or_ocean(self.world.biome_id(pos) as i32)
    }
    /// `canBlockFreeze(pos, false)`
    fn can_freeze_water(&mut self, pos: BlockPos) -> bool {
        let b = self.biome(pos);
        if temperature_at(b, pos.x, pos.y, pos.z) > 0.15 {
            return false;
        }
        if (0..256).contains(&pos.y) && self.light_for(false, pos) < 10 {
            let s = self.state(pos);
            let id = s.block().id;
            if (id == ids::WATER || id == ids::FLOWING_WATER) && s.get_int("level") == 0 {
                return true;
            }
        }
        false
    }
    /// `canSnowAt(pos, true)`
    fn can_snow_at(&mut self, pos: BlockPos) -> bool {
        let b = self.biome(pos);
        if temperature_at(b, pos.x, pos.y, pos.z) > 0.15 {
            return false;
        }
        (0..256).contains(&pos.y)
            && self.light_for(false, pos) < 10
            && self.air(pos)
            && blocks::can_place_block_at(self, ids::SNOW_LAYER, pos)
    }
}

/// Persistent generator objects vanilla keeps per biome instance (`worldGeneratorBigTree`'s
/// `heightLimit`, the decorator's big mushroom type).
#[derive(Clone, Default)]
pub struct DecorState {
    pub big_tree_height: std::collections::HashMap<u8, i32>,
    pub big_mushroom: std::collections::HashMap<u8, u16>,
}

/// Populates chunk `(cx, cz)`; vanilla requires the chunks at +1 x, +1 z and +1 both loaded.
pub fn populate(g: &mut Generator, world: &mut World, cx: i32, cz: i32) {
    let seed = g.seed;
    let mut p = Pop { world, rand: JavaRandom::new(seed), world_rand: JavaRandom::new(seed), seed };
    let i = cx * 16;
    let j = cz * 16;
    let base = BlockPos::new(i, 0, j);
    let biome = p.biome(base.offset(16, 0, 16));
    let k = p.rand.next_long() / 2 * 2 + 1;
    let l = p.rand.next_long() / 2 * 2 + 1;
    p.rand.set_seed((cx as i64).wrapping_mul(k).wrapping_add((cz as i64).wrapping_mul(l)) ^ seed);

    // Structures are not generated yet (villages would set `flag`).
    if biome.id != 2 && biome.id != 17 && p.ni(4) == 0 {
        let x = p.ni(16) + 8;
        let y = p.ni(256);
        let z = p.ni(16) + 8;
        lakes(&mut p, state("minecraft:water"), base.offset(x, y, z));
    }
    if p.ni(80 / 10) == 0 {
        let x = p.ni(16) + 8;
        let inner = p.ni(248) + 8;
        let y = p.ni(inner);
        let z = p.ni(16) + 8;
        if y < 63 || p.ni(80 / 8) == 0 {
            lakes(&mut p, state("minecraft:lava"), base.offset(x, y, z));
        }
    }
    for _ in 0..8 {
        let x = p.ni(16) + 8;
        let y = p.ni(256);
        let z = p.ni(16) + 8;
        dungeon(&mut p, base.offset(x, y, z));
    }
    biome_decorate(g, &mut p, biome, base);
    // SpawnerAnimals.performWorldGenSpawning only draws from `rand` after this point.
    let b8 = base.offset(8, 0, 8);
    for x in 0..16 {
        for z in 0..16 {
            let top = precipitation_height(p.world, b8.offset(x, 0, z));
            let below = top.down();
            if p.can_freeze_water(below) {
                p.put(below, state("minecraft:ice"));
            }
            if p.can_snow_at(top) {
                p.put(top, state("minecraft:snow_layer"));
            }
        }
    }
}

fn precipitation_height(w: &mut World, pos: BlockPos) -> BlockPos {
    let y = w.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)).map_or(0, |c| c.precipitation_height((pos.x & 15) as usize, (pos.z & 15) as usize));
    BlockPos::new(pos.x, y, pos.z)
}

/// `BiomeGenBase.decorate` and overrides.
fn biome_decorate(g: &mut Generator, p: &mut Pop, b: &'static GenBiome, pos: BlockPos) {
    match b.kind {
        Kind::Desert => {
            decorator(g, p, b, b, pos);
            if p.ni(1000) == 0 {
                let i = p.ni(16) + 8;
                let j = p.ni(16) + 8;
                let at = p.height(pos.offset(i, 0, j)).up();
                desert_well(p, at);
            }
        }
        Kind::Hills(_) => {
            decorator(g, p, b, b, pos);
            let n = 3 + p.ni(6);
            for _ in 0..n {
                let k = p.ni(16);
                let l = p.ni(28) + 4;
                let i1 = p.ni(16);
                let bp = pos.offset(k, l, i1);
                if p.id(bp) == ids::STONE {
                    p.put(bp, state("minecraft:emerald_ore"));
                }
            }
            for _ in 0..7 {
                let x = p.ni(16);
                let y = p.ni(64);
                let z = p.ni(16);
                minable(p, state("minecraft:monster_egg"), 9, pos.offset(x, y, z), |s| s.block().id == ids::STONE && s.meta() == 0);
            }
        }
        Kind::Forest(t) => {
            if t == 3 {
                for i in 0..4 {
                    for j in 0..4 {
                        let k = i * 4 + 1 + 8 + p.ni(3);
                        let l = j * 4 + 1 + 8 + p.ni(3);
                        let bp = p.height(pos.offset(k, 0, l));
                        if p.ni(20) == 0 {
                            let mut kind = 0;
                            trees::big_mushroom(p, &mut kind, bp);
                        } else {
                            let tree = gen_big_tree_chance(p, b);
                            trees::generate(g, p, b, tree, bp);
                        }
                    }
                }
            }
            let mut n = p.ni(5) - 3;
            if t == 1 {
                n += 2;
            }
            for _ in 0..n {
                let variant = match p.ni(3) {
                    0 => "syringa",
                    1 => "double_rose",
                    _ => "paeonia",
                };
                for _ in 0..5 {
                    let j2 = p.ni(16) + 8;
                    let k2 = p.ni(16) + 8;
                    let h = p.height(pos.offset(j2, 0, k2)).y + 32;
                    let i1 = p.ni(h);
                    if double_plant(p, variant, BlockPos::new(pos.x + j2, i1, pos.z + k2)) {
                        break;
                    }
                }
            }
            decorator(g, p, b, b, pos);
        }
        Kind::Plains(sunflowers) => {
            let d0 = grass_color_noise().value((pos.x + 8) as f64 / 200.0, (pos.z + 8) as f64 / 200.0);
            let (flowers, grass) = if d0 < -0.8 {
                (15, 5)
            } else {
                for _ in 0..7 {
                    let j = p.ni(16) + 8;
                    let k = p.ni(16) + 8;
                    let h = p.height(pos.offset(j, 0, k)).y + 32;
                    let l = p.ni(h);
                    double_plant(p, "double_grass", pos.offset(j, l, k));
                }
                (4, 10)
            };
            if sunflowers {
                for _ in 0..10 {
                    let j = p.ni(16) + 8;
                    let k = p.ni(16) + 8;
                    let h = p.height(pos.offset(j, 0, k)).y + 32;
                    let l = p.ni(h);
                    double_plant(p, "sunflower", pos.offset(j, l, k));
                }
            }
            let mut d = b.clone();
            d.decorator.flowers = flowers;
            d.decorator.grass = grass;
            decorator_counts(g, p, &d, b, b, pos);
        }
        Kind::Savanna => {
            for _ in 0..7 {
                let j = p.ni(16) + 8;
                let k = p.ni(16) + 8;
                let h = p.height(pos.offset(j, 0, k)).y + 32;
                let l = p.ni(h);
                double_plant(p, "double_grass", pos.offset(j, l, k));
            }
            decorator(g, p, b, b, pos);
        }
        Kind::Taiga(t) => {
            if t == 1 || t == 2 {
                let n = p.ni(3);
                for _ in 0..n {
                    let k = p.ni(16) + 8;
                    let l = p.ni(16) + 8;
                    let bp = p.height(pos.offset(k, 0, l));
                    block_blob(p, state("minecraft:mossy_cobblestone"), 0, bp);
                }
            }
            for _ in 0..7 {
                let j = p.ni(16) + 8;
                let k = p.ni(16) + 8;
                let h = p.height(pos.offset(j, 0, k)).y + 32;
                let l = p.ni(h);
                double_plant(p, "double_fern", pos.offset(j, l, k));
            }
            decorator(g, p, b, b, pos);
        }
        Kind::Jungle(_) => {
            decorator(g, p, b, b, pos);
            let i = p.ni(16) + 8;
            let j = p.ni(16) + 8;
            let h = p.height(pos.offset(i, 0, j)).y * 2;
            let k = p.ni(h);
            melons(p, pos.offset(i, k, j));
            for _ in 0..50 {
                let k = p.ni(16) + 8;
                let i1 = p.ni(16) + 8;
                vines(p, pos.offset(k, 128, i1));
            }
        }
        Kind::Snow(true) => {
            for _ in 0..3 {
                let j = p.ni(16) + 8;
                let k = p.ni(16) + 8;
                let at = p.height(pos.offset(j, 0, k));
                ice_spike(p, at);
            }
            for _ in 0..2 {
                let i1 = p.ni(16) + 8;
                let j1 = p.ni(16) + 8;
                let at = p.height(pos.offset(i1, 0, j1));
                ice_path(p, 4, at);
            }
            decorator(g, p, b, b, pos);
        }
        Kind::Mutated => {
            let base = biome::biome_or_ocean(b.base as i32);
            if matches!(base.kind, Kind::Forest(3)) {
                // Roofed Forest M: `baseBiome.decorate`, entirely as the base biome.
                biome_decorate(g, p, base, pos);
            } else {
                decorator(g, p, base, b, pos);
            }
        }
        _ => decorator(g, p, b, b, pos),
    }
}

/// `decorator_owner.theBiomeDecorator.decorate(world, rand, biome, pos)`
fn decorator(g: &mut Generator, p: &mut Pop, owner: &'static GenBiome, biome: &'static GenBiome, pos: BlockPos) {
    decorator_counts(g, p, owner, owner, biome, pos)
}

/// `BiomeDecorator.genDecorations` with counts from `counts`, persistent state keyed by `owner`.
fn decorator_counts(g: &mut Generator, p: &mut Pop, counts: &GenBiome, owner: &'static GenBiome, biome: &'static GenBiome, pos: BlockPos) {
    let d = counts.decorator;
    generate_ores(p, pos);
    for _ in 0..d.sand2 {
        let j = p.ni(16) + 8;
        let k = p.ni(16) + 8;
        let at = p.top_solid_or_liquid(pos.offset(j, 0, k));
        disk(p, state("minecraft:sand"), 7, 2, &[ids::DIRT, ids::GRASS], at);
    }
    for _ in 0..d.clay {
        let j = p.ni(16) + 8;
        let k = p.ni(16) + 8;
        let at = p.top_solid_or_liquid(pos.offset(j, 0, k));
        disk(p, state("minecraft:clay"), 4, 1, &[ids::DIRT, ids::CLAY], at);
    }
    for _ in 0..d.sand {
        let j = p.ni(16) + 8;
        let k = p.ni(16) + 8;
        let at = p.top_solid_or_liquid(pos.offset(j, 0, k));
        disk(p, state("minecraft:gravel"), 6, 2, &[ids::DIRT, ids::GRASS], at);
    }
    let mut trees = d.trees;
    if p.ni(10) == 0 {
        trees += 1;
    }
    for _ in 0..trees {
        let k6 = p.ni(16) + 8;
        let l = p.ni(16) + 8;
        let tree = gen_big_tree_chance(p, biome);
        let at = p.height(pos.offset(k6, 0, l));
        trees::generate(g, p, biome, tree, at);
    }
    for _ in 0..d.big_mushrooms {
        let x = p.ni(16) + 8;
        let z = p.ni(16) + 8;
        let at = p.height(pos.offset(x, 0, z));
        let mut kind = g.decor.big_mushroom.get(&owner.id).copied().unwrap_or(0);
        trees::big_mushroom(p, &mut kind, at);
        g.decor.big_mushroom.insert(owner.id, kind);
    }
    for _ in 0..d.flowers {
        let i7 = p.ni(16) + 8;
        let l10 = p.ni(16) + 8;
        let j14 = p.height(pos.offset(i7, 0, l10)).y + 32;
        if j14 > 0 {
            let k17 = p.ni(j14);
            let at = pos.offset(i7, k17, l10);
            let flower = pick_flower(p, biome, at);
            flowers(p, flower, at);
        }
    }
    for _ in 0..d.grass {
        let j7 = p.ni(16) + 8;
        let i11 = p.ni(16) + 8;
        let k14 = p.height(pos.offset(j7, 0, i11)).y * 2;
        if k14 > 0 {
            let l17 = p.ni(k14);
            let kind = grass_kind(p, biome);
            tall_grass(p, kind, pos.offset(j7, l17, i11));
        }
    }
    for _ in 0..d.dead_bush {
        let k7 = p.ni(16) + 8;
        let j11 = p.ni(16) + 8;
        let l14 = p.height(pos.offset(k7, 0, j11)).y * 2;
        if l14 > 0 {
            let i18 = p.ni(l14);
            dead_bush(p, pos.offset(k7, i18, j11));
        }
    }
    for _ in 0..d.waterlily {
        let l7 = p.ni(16) + 8;
        let k11 = p.ni(16) + 8;
        let i15 = p.height(pos.offset(l7, 0, k11)).y * 2;
        if i15 > 0 {
            let j18 = p.ni(i15);
            let mut bp = pos.offset(l7, j18, k11);
            while bp.y > 0 {
                let below = bp.down();
                if !p.air(below) {
                    break;
                }
                bp = below;
            }
            waterlily(p, bp);
        }
    }
    for _ in 0..d.mushrooms {
        if p.ni(4) == 0 {
            let i8 = p.ni(16) + 8;
            let l11 = p.ni(16) + 8;
            let at = p.height(pos.offset(i8, 0, l11));
            bush_patch(p, ids::BROWN_MUSHROOM, at);
        }
        if p.ni(8) == 0 {
            let j8 = p.ni(16) + 8;
            let i12 = p.ni(16) + 8;
            let j15 = p.height(pos.offset(j8, 0, i12)).y * 2;
            if j15 > 0 {
                let k18 = p.ni(j15);
                bush_patch(p, ids::RED_MUSHROOM, pos.offset(j8, k18, i12));
            }
        }
    }
    if p.ni(4) == 0 {
        let i4 = p.ni(16) + 8;
        let k8 = p.ni(16) + 8;
        let j12 = p.height(pos.offset(i4, 0, k8)).y * 2;
        if j12 > 0 {
            let k15 = p.ni(j12);
            bush_patch(p, ids::BROWN_MUSHROOM, pos.offset(i4, k15, k8));
        }
    }
    if p.ni(8) == 0 {
        let j4 = p.ni(16) + 8;
        let l8 = p.ni(16) + 8;
        let k12 = p.height(pos.offset(j4, 0, l8)).y * 2;
        if k12 > 0 {
            let l15 = p.ni(k12);
            bush_patch(p, ids::RED_MUSHROOM, pos.offset(j4, l15, l8));
        }
    }
    for _ in 0..d.reeds {
        let i9 = p.ni(16) + 8;
        let l12 = p.ni(16) + 8;
        let i16 = p.height(pos.offset(i9, 0, l12)).y * 2;
        if i16 > 0 {
            let l18 = p.ni(i16);
            reeds(p, pos.offset(i9, l18, l12));
        }
    }
    for _ in 0..10 {
        let j9 = p.ni(16) + 8;
        let i13 = p.ni(16) + 8;
        let j16 = p.height(pos.offset(j9, 0, i13)).y * 2;
        if j16 > 0 {
            let i19 = p.ni(j16);
            reeds(p, pos.offset(j9, i19, i13));
        }
    }
    if p.ni(32) == 0 {
        let i5 = p.ni(16) + 8;
        let k9 = p.ni(16) + 8;
        let j13 = p.height(pos.offset(i5, 0, k9)).y * 2;
        if j13 > 0 {
            let k16 = p.ni(j13);
            pumpkins(p, pos.offset(i5, k16, k9));
        }
    }
    for _ in 0..d.cacti {
        let l9 = p.ni(16) + 8;
        let k13 = p.ni(16) + 8;
        let l16 = p.height(pos.offset(l9, 0, k13)).y * 2;
        if l16 > 0 {
            let j19 = p.ni(l16);
            cacti(p, pos.offset(l9, j19, k13));
        }
    }
    if d.generate_lakes {
        for _ in 0..50 {
            let i10 = p.ni(16) + 8;
            let l13 = p.ni(16) + 8;
            let i17 = p.ni(248) + 8;
            if i17 > 0 {
                let k19 = p.ni(i17);
                liquid_spring(p, ids::FLOWING_WATER, pos.offset(i10, k19, l13));
            }
        }
        for _ in 0..20 {
            let j10 = p.ni(16) + 8;
            let i14 = p.ni(16) + 8;
            let a = p.ni(240) + 8;
            let b = p.ni(a) + 8;
            let j17 = p.ni(b);
            liquid_spring(p, ids::FLOWING_LAVA, pos.offset(j10, j17, i14));
        }
    }
}

/// `BiomeDecorator.generateOres` with default `ChunkProviderSettings`.
fn generate_ores(p: &mut Pop, pos: BlockPos) {
    let stone = |s: StateId| s.block().id == ids::STONE && s.meta() == 0 || s.block().id == ids::STONE && s.value("variant") == Some("stone");
    let st = |v: &str| state("minecraft:stone").set("variant", v);
    let ores: [(i32, StateId, i32, i32, i32); 10] = [
        (10, state("minecraft:dirt"), 33, 0, 256),
        (8, state("minecraft:gravel"), 33, 0, 256),
        (10, st("diorite"), 33, 0, 80),
        (10, st("granite"), 33, 0, 80),
        (10, st("andesite"), 33, 0, 80),
        (20, state("minecraft:coal_ore"), 17, 0, 128),
        (20, state("minecraft:iron_ore"), 9, 0, 64),
        (2, state("minecraft:gold_ore"), 9, 0, 32),
        (8, state("minecraft:redstone_ore"), 8, 0, 16),
        (1, state("minecraft:diamond_ore"), 8, 0, 16),
    ];
    for (count, ore, size, min, max) in ores {
        for _ in 0..count {
            let x = p.ni(16);
            let y = p.ni(max - min) + min;
            let z = p.ni(16);
            minable(p, ore, size, pos.offset(x, y, z), stone);
        }
    }
    let lapis = state("minecraft:lapis_ore");
    let x = p.ni(16);
    let a = p.ni(16);
    let b = p.ni(16);
    let z = p.ni(16);
    minable(p, lapis, 7, pos.offset(x, a + b + 16 - 16, z), stone);
}

/// `WorldGenMinable`
pub fn minable(p: &mut Pop, ore: StateId, n: i32, pos: BlockPos, pred: impl Fn(StateId) -> bool) {
    let f = p.nf() * std::f32::consts::PI;
    let nb = n as f32;
    let d0 = ((pos.x + 8) as f32 + math::sin(f) * nb / 8.0) as f64;
    let d1 = ((pos.x + 8) as f32 - math::sin(f) * nb / 8.0) as f64;
    let d2 = ((pos.z + 8) as f32 + math::cos(f) * nb / 8.0) as f64;
    let d3 = ((pos.z + 8) as f32 - math::cos(f) * nb / 8.0) as f64;
    let d4 = (pos.y + p.ni(3) - 2) as f64;
    let d5 = (pos.y + p.ni(3) - 2) as f64;
    for i in 0..n {
        let f1 = i as f32 / nb;
        let d6 = d0 + (d1 - d0) * f1 as f64;
        let d7 = d4 + (d5 - d4) * f1 as f64;
        let d8 = d2 + (d3 - d2) * f1 as f64;
        let d9 = p.rand.next_double() * n as f64 / 16.0;
        let d10 = (math::sin(std::f32::consts::PI * f1) + 1.0) as f64 * d9 + 1.0;
        let d11 = (math::sin(std::f32::consts::PI * f1) + 1.0) as f64 * d9 + 1.0;
        let j = floor_double(d6 - d10 / 2.0);
        let k = floor_double(d7 - d11 / 2.0);
        let l = floor_double(d8 - d10 / 2.0);
        let i1 = floor_double(d6 + d10 / 2.0);
        let j1 = floor_double(d7 + d11 / 2.0);
        let k1 = floor_double(d8 + d10 / 2.0);
        for l1 in j..=i1 {
            let d12 = (l1 as f64 + 0.5 - d6) / (d10 / 2.0);
            if d12 * d12 < 1.0 {
                for i2 in k..=j1 {
                    let d13 = (i2 as f64 + 0.5 - d7) / (d11 / 2.0);
                    if d12 * d12 + d13 * d13 < 1.0 {
                        for j2 in l..=k1 {
                            let d14 = (j2 as f64 + 0.5 - d8) / (d10 / 2.0);
                            if d12 * d12 + d13 * d13 + d14 * d14 < 1.0 {
                                let bp = BlockPos::new(l1, i2, j2);
                                let s = p.state(bp);
                                if pred(s) {
                                    p.put(bp, ore);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// `WorldGenSand` / `WorldGenClay`: a disk under water.
fn disk(p: &mut Pop, block: StateId, radius: i32, half_height: i32, replace: &[u16], pos: BlockPos) {
    if p.mat(pos) != Material::Water {
        return;
    }
    let i = p.ni(radius - 2) + 2;
    for k in pos.x - i..=pos.x + i {
        for l in pos.z - i..=pos.z + i {
            let (i1, j1) = (k - pos.x, l - pos.z);
            if i1 * i1 + j1 * j1 <= i * i {
                for k1 in pos.y - half_height..=pos.y + half_height {
                    let bp = BlockPos::new(k, k1, l);
                    if replace.contains(&p.id(bp)) {
                        p.put(bp, block);
                    }
                }
            }
        }
    }
}

fn gen_big_tree_chance(p: &mut Pop, b: &'static GenBiome) -> Tree {
    match b.kind {
        Kind::Hills(_) => {
            if p.ni(3) > 0 {
                Tree::Spruce
            } else {
                default_tree(p)
            }
        }
        Kind::Forest(t) => {
            if t == 3 && p.ni(3) > 0 {
                Tree::Canopy
            } else if t != 2 && p.ni(5) != 0 {
                Tree::Oak
            } else {
                Tree::Birch
            }
        }
        Kind::Savanna | Kind::SavannaMutated => {
            if p.ni(5) > 0 {
                Tree::Savanna
            } else {
                Tree::Oak
            }
        }
        Kind::Taiga(t) => {
            if (t == 1 || t == 2) && p.ni(3) == 0 {
                if t != 2 && p.ni(13) != 0 {
                    Tree::MegaPine(false)
                } else {
                    Tree::MegaPine(true)
                }
            } else if p.ni(3) == 0 {
                Tree::Pine
            } else {
                Tree::Spruce
            }
        }
        Kind::Jungle(edge) => {
            if p.ni(10) == 0 {
                Tree::BigOak
            } else if p.ni(2) == 0 {
                Tree::JungleShrub
            } else if !edge && p.ni(3) == 0 {
                Tree::MegaJungle
            } else {
                Tree::Jungle(4 + p.ni(7))
            }
        }
        Kind::Swamp => Tree::Swamp,
        Kind::Snow(_) => Tree::Spruce,
        Kind::Mesa(..) => Tree::Oak,
        Kind::Mutated => {
            let base = biome::biome_or_ocean(b.base as i32);
            if matches!(base.kind, Kind::Forest(2)) {
                // Birch Forest M / Birch Forest Hills M: tall birches half the time.
                if p.rand.next_boolean() {
                    Tree::TallBirch
                } else {
                    Tree::Birch
                }
            } else {
                gen_big_tree_chance(p, base)
            }
        }
        _ => default_tree(p),
    }
}

fn default_tree(p: &mut Pop) -> Tree {
    if p.ni(10) == 0 {
        Tree::BigOak
    } else {
        Tree::Oak
    }
}

/// `pickRandomFlower`: (block name, type).
fn pick_flower(p: &mut Pop, b: &GenBiome, pos: BlockPos) -> (&'static str, &'static str) {
    const RED: &str = "minecraft:red_flower";
    match b.kind {
        Kind::Plains(_) => {
            let d0 = grass_color_noise().value(pos.x as f64 / 200.0, pos.z as f64 / 200.0);
            if d0 < -0.8 {
                (RED, ["orange_tulip", "red_tulip", "pink_tulip", "white_tulip"][p.ni(4) as usize])
            } else if p.ni(3) > 0 {
                (RED, ["poppy", "houstonia", "oxeye_daisy"][p.ni(3) as usize])
            } else {
                ("minecraft:yellow_flower", "dandelion")
            }
        }
        Kind::Forest(1) => {
            const ALL: [(&str, &str); 10] = [
                ("minecraft:yellow_flower", "dandelion"),
                (RED, "poppy"),
                (RED, "blue_orchid"),
                (RED, "allium"),
                (RED, "houstonia"),
                (RED, "red_tulip"),
                (RED, "orange_tulip"),
                (RED, "white_tulip"),
                (RED, "pink_tulip"),
                (RED, "oxeye_daisy"),
            ];
            let d0 = ((1.0 + grass_color_noise().value(pos.x as f64 / 48.0, pos.z as f64 / 48.0)) / 2.0).clamp(0.0, 0.9999);
            let f = ALL[(d0 * ALL.len() as f64) as usize];
            if f.1 == "blue_orchid" {
                (RED, "poppy")
            } else {
                f
            }
        }
        Kind::Swamp => (RED, "blue_orchid"),
        _ => {
            if p.ni(3) > 0 {
                ("minecraft:yellow_flower", "dandelion")
            } else {
                (RED, "poppy")
            }
        }
    }
}

/// `getRandomWorldGenForGrass`: tall grass type.
fn grass_kind(p: &mut Pop, b: &GenBiome) -> &'static str {
    match b.kind {
        Kind::Taiga(_) => {
            if p.ni(5) > 0 {
                "fern"
            } else {
                "tall_grass"
            }
        }
        Kind::Jungle(_) => {
            if p.ni(4) == 0 {
                "fern"
            } else {
                "tall_grass"
            }
        }
        _ => "tall_grass",
    }
}

fn scatter(p: &mut Pop, pos: BlockPos) -> BlockPos {
    let x = p.ni(8) - p.ni(8);
    let y = p.ni(4) - p.ni(4);
    let z = p.ni(8) - p.ni(8);
    pos.offset(x, y, z)
}

/// `WorldGenFlowers`
fn flowers(p: &mut Pop, flower: (&str, &str), pos: BlockPos) {
    let s = state(flower.0).set("type", flower.1);
    for _ in 0..64 {
        let bp = scatter(p, pos);
        if p.air(bp) && bp.y < 255 && blocks::can_block_stay(p, bp, s) {
            p.put(bp, s);
        }
    }
}

fn sink_through_air_and_leaves(p: &mut Pop, mut pos: BlockPos) -> BlockPos {
    loop {
        let m = p.mat(pos);
        if !((m == Material::Air || m == Material::Leaves) && pos.y > 0) {
            return pos;
        }
        pos = pos.down();
    }
}

/// `WorldGenTallGrass`
fn tall_grass(p: &mut Pop, kind: &str, pos: BlockPos) {
    let s = state("minecraft:tallgrass").set("type", kind);
    let pos = sink_through_air_and_leaves(p, pos);
    for _ in 0..128 {
        let bp = scatter(p, pos);
        if p.air(bp) && blocks::can_block_stay(p, bp, s) {
            p.put(bp, s);
        }
    }
}

/// `WorldGenDeadBush`
fn dead_bush(p: &mut Pop, pos: BlockPos) {
    let s = state("minecraft:deadbush");
    let pos = sink_through_air_and_leaves(p, pos);
    for _ in 0..4 {
        let bp = scatter(p, pos);
        if p.air(bp) && blocks::can_block_stay(p, bp, s) {
            p.put(bp, s);
        }
    }
}

/// `WorldGenWaterlily`
fn waterlily(p: &mut Pop, pos: BlockPos) {
    for _ in 0..10 {
        let j = pos.x + p.ni(8) - p.ni(8);
        let k = pos.y + p.ni(4) - p.ni(4);
        let l = pos.z + p.ni(8) - p.ni(8);
        let bp = BlockPos::new(j, k, l);
        if p.air(bp) && blocks::can_place_block_at(p, ids::WATERLILY, bp) && blocks::can_block_stay(p, bp, state("minecraft:waterlily")) {
            p.put(bp, state("minecraft:waterlily"));
        }
    }
}

/// `WorldGenBush` (mushrooms)
fn bush_patch(p: &mut Pop, block: u16, pos: BlockPos) {
    let s = mc_logic::st(block, 0);
    for _ in 0..64 {
        let bp = scatter(p, pos);
        if p.air(bp) && bp.y < 255 && blocks::can_block_stay(p, bp, s) {
            p.put(bp, s);
        }
    }
}

/// `WorldGenReed`
fn reeds(p: &mut Pop, pos: BlockPos) {
    for _ in 0..20 {
        let x = p.ni(4) - p.ni(4);
        let z = p.ni(4) - p.ni(4);
        let bp = pos.offset(x, 0, z);
        if p.air(bp) {
            let d = bp.down();
            let water = [d.offset(-1, 0, 0), d.offset(1, 0, 0), d.offset(0, 0, -1), d.offset(0, 0, 1)]
                .into_iter()
                .any(|q| p.mat(q) == Material::Water);
            if water {
                let inner = p.ni(3) + 1;
                let j = 2 + p.ni(inner);
                for k in 0..j {
                    if blocks::reeds_can_place_at(p, bp) {
                        p.put(bp.offset(0, k, 0), state("minecraft:reeds"));
                    }
                }
            }
        }
    }
}

/// `WorldGenPumpkin`
fn pumpkins(p: &mut Pop, pos: BlockPos) {
    for _ in 0..64 {
        let bp = scatter(p, pos);
        if p.air(bp) && p.id(bp.down()) == ids::GRASS && blocks::can_place_block_at(p, ids::PUMPKIN, bp) {
            let f = Facing::PLANE_HORIZONTAL[p.ni(4) as usize];
            p.put(bp, state("minecraft:pumpkin").set("facing", f.name()));
        }
    }
}

/// `WorldGenCactus`
fn cacti(p: &mut Pop, pos: BlockPos) {
    for _ in 0..10 {
        let bp = scatter(p, pos);
        if p.air(bp) {
            let inner = p.ni(3) + 1;
            let j = 1 + p.ni(inner);
            for k in 0..j {
                if blocks::can_block_stay(p, bp, state("minecraft:cactus")) {
                    p.put(bp.offset(0, k, 0), state("minecraft:cactus"));
                }
            }
        }
    }
}

/// `WorldGenMelon`
fn melons(p: &mut Pop, pos: BlockPos) {
    for _ in 0..64 {
        let bp = scatter(p, pos);
        if blocks::can_place_block_at(p, ids::MELON_BLOCK, bp) && p.id(bp.down()) == ids::GRASS {
            p.put(bp, state("minecraft:melon_block"));
        }
    }
}

/// `WorldGenVines`
fn vines(p: &mut Pop, mut pos: BlockPos) {
    while pos.y < 128 {
        if p.air(pos) {
            for f in Facing::PLANE_HORIZONTAL {
                if blocks::vine_can_place_on_side(p, pos, f) {
                    let s = state("minecraft:vine")
                        .set_bool("north", f == Facing::North)
                        .set_bool("east", f == Facing::East)
                        .set_bool("south", f == Facing::South)
                        .set_bool("west", f == Facing::West);
                    p.put(pos, s);
                    break;
                }
            }
        } else {
            let x = p.ni(4) - p.ni(4);
            let z = p.ni(4) - p.ni(4);
            pos = pos.offset(x, 0, z);
        }
        pos = pos.up();
    }
}

/// `WorldGenDoublePlant` with `setPlantType`.
fn double_plant(p: &mut Pop, variant: &str, pos: BlockPos) -> bool {
    let mut placed = false;
    for _ in 0..64 {
        let bp = scatter(p, pos);
        if p.air(bp) && bp.y < 254 && blocks::can_place_block_at(p, ids::DOUBLE_PLANT, bp) {
            // BlockDoublePlant.placeAt
            let lower = state("minecraft:double_plant").set("half", "lower").set("variant", variant);
            p.put(bp, lower);
            p.put(bp.up(), state("minecraft:double_plant").set("half", "upper"));
            placed = true;
        }
    }
    placed
}

/// `WorldGenLiquids`
fn liquid_spring(p: &mut Pop, block: u16, pos: BlockPos) {
    if p.id(pos.up()) != ids::STONE || p.id(pos.down()) != ids::STONE {
        return;
    }
    if !p.air(pos) && p.id(pos) != ids::STONE {
        return;
    }
    let sides = [pos.offset(-1, 0, 0), pos.offset(1, 0, 0), pos.offset(0, 0, -1), pos.offset(0, 0, 1)];
    let stone = sides.iter().filter(|&&q| p.id(q) == ids::STONE).count();
    let air = sides.iter().filter(|&&q| p.air(q)).count();
    if stone == 3 && air == 1 {
        let s = mc_logic::st(block, 0);
        p.put(pos, s);
        // forceBlockUpdateTick
        blocks::update_tick(p, pos, s, Rng::Gen);
    }
}

/// `WorldGenBlockBlob`
fn block_blob(p: &mut Pop, block: StateId, radius: i32, mut pos: BlockPos) {
    loop {
        if pos.y > 3 {
            if p.air(pos.down()) {
                pos = pos.down();
                continue;
            }
            let b = p.id(pos.down());
            if b != ids::GRASS && b != ids::DIRT && b != ids::STONE {
                pos = pos.down();
                continue;
            }
        }
        if pos.y <= 3 {
            return;
        }
        let i1 = radius;
        let mut i = 0;
        while i1 >= 0 && i < 3 {
            let j = i1 + p.ni(2);
            let k = i1 + p.ni(2);
            let l = i1 + p.ni(2);
            let f = (j + k + l) as f32 * 0.333 + 0.5;
            // BlockPos.getAllInBox: x fastest, then y, then z.
            for z in pos.z - l..=pos.z + l {
                for y in pos.y - k..=pos.y + k {
                    for x in pos.x - j..=pos.x + j {
                        let (dx, dy, dz) = ((x - pos.x) as f64, (y - pos.y) as f64, (z - pos.z) as f64);
                        if dx * dx + dy * dy + dz * dz <= (f * f) as f64 {
                            p.put(BlockPos::new(x, y, z), block);
                        }
                    }
                }
            }
            let ox = -(i1 + 1) + p.ni(2 + i1 * 2);
            let oy = 0 - p.ni(2);
            let oz = -(i1 + 1) + p.ni(2 + i1 * 2);
            pos = pos.offset(ox, oy, oz);
            i += 1;
        }
        return;
    }
}

/// `WorldGenIceSpike`
fn ice_spike(p: &mut Pop, mut pos: BlockPos) {
    while p.air(pos) && pos.y > 2 {
        pos = pos.down();
    }
    if p.id(pos) != ids::SNOW {
        return;
    }
    let up = p.ni(4);
    pos = pos.offset(0, up, 0);
    let i = p.ni(4) + 7;
    let j = i / 4 + p.ni(2);
    if j > 1 && p.ni(60) == 0 {
        let u = 10 + p.ni(30);
        pos = pos.offset(0, u, 0);
    }
    let packed = state("minecraft:packed_ice");
    let replaceable = |s: StateId| {
        let b = s.block();
        b.material == Material::Air || b.id == ids::DIRT || b.id == ids::SNOW || b.id == ids::ICE
    };
    for k in 0..i {
        let f = (1.0 - k as f32 / i as f32) * j as f32;
        let l = math::ceiling_float_int(f);
        for i1 in -l..=l {
            let f1 = i1.abs() as f32 - 0.25;
            for j1 in -l..=l {
                let f2 = j1.abs() as f32 - 0.25;
                if (i1 == 0 && j1 == 0 || f1 * f1 + f2 * f2 <= f * f) && (i1 != -l && i1 != l && j1 != -l && j1 != l || p.nf() <= 0.75) {
                    let a = pos.offset(i1, k, j1);
                    if replaceable(p.state(a)) {
                        p.put(a, packed);
                    }
                    if k != 0 && l > 1 {
                        let b = pos.offset(i1, -k, j1);
                        if replaceable(p.state(b)) {
                            p.put(b, packed);
                        }
                    }
                }
            }
        }
    }
    let k1 = (j - 1).clamp(0, 1);
    for l1 in -k1..=k1 {
        for i2 in -k1..=k1 {
            let mut bp = pos.offset(l1, -1, i2);
            let mut j2 = 50;
            if l1.abs() == 1 && i2.abs() == 1 {
                j2 = p.ni(5);
            }
            while bp.y > 50 {
                let s = p.state(bp);
                if !(replaceable(s) || s.block().id == ids::PACKED_ICE) {
                    break;
                }
                p.put(bp, packed);
                bp = bp.down();
                j2 -= 1;
                if j2 <= 0 {
                    let d = p.ni(5) + 1;
                    bp = bp.offset(0, -d, 0);
                    j2 = p.ni(5);
                }
            }
        }
    }
}

/// `WorldGenIcePath`
fn ice_path(p: &mut Pop, width: i32, mut pos: BlockPos) {
    while p.air(pos) && pos.y > 2 {
        pos = pos.down();
    }
    if p.id(pos) != ids::SNOW {
        return;
    }
    let i = p.ni(width - 2) + 2;
    for k in pos.x - i..=pos.x + i {
        for l in pos.z - i..=pos.z + i {
            let (i1, j1) = (k - pos.x, l - pos.z);
            if i1 * i1 + j1 * j1 <= i * i {
                for k1 in pos.y - 1..=pos.y + 1 {
                    let bp = BlockPos::new(k, k1, l);
                    if matches!(p.id(bp), ids::DIRT | ids::SNOW | ids::ICE) {
                        p.put(bp, state("minecraft:packed_ice"));
                    }
                }
            }
        }
    }
}

/// `WorldGenDesertWells`
fn desert_well(p: &mut Pop, mut pos: BlockPos) {
    while p.air(pos) && pos.y > 2 {
        pos = pos.down();
    }
    let s = p.state(pos);
    if !(s.block().id == ids::SAND && s.meta() == 0) {
        return;
    }
    for i in -2..=2 {
        for j in -2..=2 {
            if p.air(pos.offset(i, -1, j)) && p.air(pos.offset(i, -2, j)) {
                return;
            }
        }
    }
    let sandstone = state("minecraft:sandstone");
    let slab = state("minecraft:stone_slab").set("variant", "sandstone").set("half", "bottom");
    let water = state("minecraft:flowing_water");
    for l in -1..=0 {
        for l1 in -2..=2 {
            for k in -2..=2 {
                p.put(pos.offset(l1, l, k), sandstone);
            }
        }
    }
    p.put(pos, water);
    for f in Facing::PLANE_HORIZONTAL {
        p.put(pos.offset_facing(f), water);
    }
    for i1 in -2..=2 {
        for i2 in -2..=2 {
            if i1 == -2 || i1 == 2 || i2 == -2 || i2 == 2 {
                p.put(pos.offset(i1, 1, i2), sandstone);
            }
        }
    }
    for (x, z) in [(2, 0), (-2, 0), (0, 2), (0, -2)] {
        p.put(pos.offset(x, 1, z), slab);
    }
    for j1 in -1..=1 {
        for j2 in -1..=1 {
            p.put(pos.offset(j1, 4, j2), if j1 == 0 && j2 == 0 { sandstone } else { slab });
        }
    }
    for k1 in 1..=3 {
        for (x, z) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
            p.put(pos.offset(x, k1, z), sandstone);
        }
    }
}

/// `WorldGenLakes`
fn lakes(p: &mut Pop, block: StateId, mut pos: BlockPos) {
    pos = pos.offset(-8, 0, -8);
    while pos.y > 5 && p.air(pos) {
        pos = pos.down();
    }
    if pos.y <= 4 {
        return;
    }
    pos = pos.offset(0, -4, 0);
    let mut a = [false; 2048];
    let i = p.ni(4) + 4;
    for _ in 0..i {
        let d0 = p.rand.next_double() * 6.0 + 3.0;
        let d1 = p.rand.next_double() * 4.0 + 2.0;
        let d2 = p.rand.next_double() * 6.0 + 3.0;
        let d3 = p.rand.next_double() * (16.0 - d0 - 2.0) + 1.0 + d0 / 2.0;
        let d4 = p.rand.next_double() * (8.0 - d1 - 4.0) + 2.0 + d1 / 2.0;
        let d5 = p.rand.next_double() * (16.0 - d2 - 2.0) + 1.0 + d2 / 2.0;
        for l in 1..15 {
            for i1 in 1..15 {
                for j1 in 1..7 {
                    let d6 = (l as f64 - d3) / (d0 / 2.0);
                    let d7 = (j1 as f64 - d4) / (d1 / 2.0);
                    let d8 = (i1 as f64 - d5) / (d2 / 2.0);
                    if d6 * d6 + d7 * d7 + d8 * d8 < 1.0 {
                        a[((l * 16 + i1) * 8 + j1) as usize] = true;
                    }
                }
            }
        }
    }
    let at = |x: i32, z: i32, y: i32| a[((x * 16 + z) * 8 + y) as usize];
    let edge = |x: i32, z: i32, y: i32| {
        !at(x, z, y)
            && (x < 15 && at(x + 1, z, y)
                || x > 0 && at(x - 1, z, y)
                || z < 15 && at(x, z + 1, y)
                || z > 0 && at(x, z - 1, y)
                || y < 7 && at(x, z, y + 1)
                || y > 0 && at(x, z, y - 1))
    };
    let liquid_id = block.block().id;
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..8 {
                if edge(x, z, y) {
                    let s = p.state(pos.offset(x, y, z));
                    let m = s.block().material;
                    if y >= 4 && m.props().liquid {
                        return;
                    }
                    if y < 4 && !m.props().solid && s.block().id != liquid_id {
                        return;
                    }
                }
            }
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..8 {
                if at(x, z, y) {
                    p.put(pos.offset(x, y, z), if y >= 4 { StateId::AIR } else { block });
                }
            }
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            for y in 4..8 {
                if at(x, z, y) {
                    let bp = pos.offset(x, y - 1, z);
                    if p.id(bp) == ids::DIRT && p.light_for(true, pos.offset(x, y, z)) > 0 {
                        let top = p.biome(bp).top.0;
                        p.put(bp, if top == ids::MYCELIUM { state("minecraft:mycelium") } else { state("minecraft:grass") });
                    }
                }
            }
        }
    }
    if block.block().material == Material::Lava {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    if edge(x, z, y) && (y < 4 || p.ni(2) != 0) && p.mat(pos.offset(x, y, z)).props().solid {
                        p.put(pos.offset(x, y, z), state("minecraft:stone"));
                    }
                }
            }
        }
    }
    if block.block().material == Material::Water {
        for x in 0..16 {
            for z in 0..16 {
                if p.can_freeze_water(pos.offset(x, 4, z)) {
                    p.put(pos.offset(x, 4, z), state("minecraft:ice"));
                }
            }
        }
    }
}

/// `WorldGenDungeons` (chest loot and spawner type are drawn for RNG parity; tile entities are
/// stored as NBT).
fn dungeon(p: &mut Pop, pos: BlockPos) -> bool {
    let j = p.ni(2) + 2;
    let k = -j - 1;
    let l = j + 1;
    let k1 = p.ni(2) + 2;
    let l1 = -k1 - 1;
    let i2 = k1 + 1;
    let mut j2 = 0;
    for x in k..=l {
        for y in -1..=4 {
            for z in l1..=i2 {
                let bp = pos.offset(x, y, z);
                let solid = p.mat(bp).props().solid;
                if y == -1 && !solid || y == 4 && !solid {
                    return false;
                }
                if (x == k || x == l || z == l1 || z == i2) && y == 0 && p.air(bp) && p.air(bp.up()) {
                    j2 += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&j2) {
        return false;
    }
    for x in k..=l {
        for y in (-1..=3).rev() {
            for z in l1..=i2 {
                let bp = pos.offset(x, y, z);
                if x != k && y != -1 && z != l1 && x != l && y != 4 && z != i2 {
                    if p.id(bp) != ids::CHEST {
                        p.put(bp, StateId::AIR);
                    }
                } else if bp.y >= 0 && !p.mat(bp.down()).props().solid {
                    p.put(bp, StateId::AIR);
                } else if p.mat(bp).props().solid && p.id(bp) != ids::CHEST {
                    if y == -1 && p.ni(4) != 0 {
                        p.put(bp, state("minecraft:mossy_cobblestone"));
                    } else {
                        p.put(bp, state("minecraft:cobblestone"));
                    }
                }
            }
        }
    }
    for _ in 0..2 {
        for _ in 0..3 {
            let x = pos.x + p.ni(j * 2 + 1) - j;
            let y = pos.y;
            let z = pos.z + p.ni(k1 * 2 + 1) - k1;
            let bp = BlockPos::new(x, y, z);
            if p.air(bp) {
                let n = Facing::PLANE_HORIZONTAL.iter().filter(|&&f| p.mat(bp.offset_facing(f)).props().solid).count();
                if n == 1 {
                    let s = blocks::chest_correct_facing(p, bp, state("minecraft:chest"));
                    p.put(bp, s);
                    dungeon_chest(p, bp);
                    break;
                }
            }
        }
    }
    p.put(pos, state("minecraft:mob_spawner"));
    let mob = ["Skeleton", "Zombie", "Zombie", "Spider"][p.ni(4) as usize];
    if let Some(c) = p.world.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)) {
        let mut t = mc_core::nbt::Compound::new();
        t.insert("id".into(), mc_core::nbt::Tag::String("MobSpawner".into()));
        t.insert("EntityId".into(), mc_core::nbt::Tag::String(mob.into()));
        c.tile_entities.insert(pos, t);
    }
    true
}

/// `WeightedRandomChestContent.generateChestContents` for the dungeon loot table.
fn dungeon_chest(p: &mut Pop, pos: BlockPos) {
    use mc_logic::item::{generate_chest_contents, random_enchanted_book, slots_to_nbt, ChestContent};
    let c = |name: &str, min: i32, max: i32, weight: i32| ChestContent::new(name, 0, min, max, weight);
    let mut list = vec![
        c("minecraft:saddle", 1, 1, 10),
        c("minecraft:iron_ingot", 1, 4, 10),
        c("minecraft:bread", 1, 1, 10),
        c("minecraft:wheat", 1, 4, 10),
        c("minecraft:gunpowder", 1, 4, 10),
        c("minecraft:string", 1, 4, 10),
        c("minecraft:bucket", 1, 1, 10),
        c("minecraft:golden_apple", 1, 1, 1),
        c("minecraft:redstone", 1, 4, 10),
        c("minecraft:record_13", 1, 1, 4),
        c("minecraft:record_cat", 1, 1, 4),
        c("minecraft:name_tag", 1, 1, 10),
        c("minecraft:golden_horse_armor", 1, 1, 2),
        c("minecraft:iron_horse_armor", 1, 1, 5),
        c("minecraft:diamond_horse_armor", 1, 1, 1),
    ];
    // Items.enchanted_book.getRandom(rand): func_92112_a(rand, 1, 1, 1)
    list.push(random_enchanted_book(&mut p.rand, 1, 1, 1));
    let mut slots = vec![None; 27];
    generate_chest_contents(&mut p.rand, &list, &mut slots, 8);
    if let Some(ch) = p.world.chunks.get_mut(&(pos.x >> 4, pos.z >> 4)) {
        let mut t = mc_core::nbt::Compound::new();
        t.insert("id".into(), mc_core::nbt::Tag::String("Chest".into()));
        t.insert("Items".into(), slots_to_nbt(&slots));
        ch.tile_entities.insert(pos, t);
    }
}
