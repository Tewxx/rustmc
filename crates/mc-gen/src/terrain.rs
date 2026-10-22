//! `ChunkProviderGenerate.provideChunk`: density noise, biome surface blocks, caves, ravines.
//! Population (decoration) is in [`crate::populate`].

use crate::biome::{self, GenBiome, Kind};
use crate::layer::Layers;
use crate::noise::{Octaves, Perlin};
use mc_core::{math, JavaRandom};
use mc_world::chunk::Chunk;

pub const STONE: u16 = 1 << 4;
pub const AIR: u16 = 0;
pub const WATER: u16 = 9 << 4;
pub const LAVA: u16 = 11 << 4;
pub const FLOWING_LAVA: u16 = 10 << 4;
pub const FLOWING_WATER: u16 = 8 << 4;
pub const BEDROCK: u16 = 7 << 4;
pub const GRAVEL: u16 = 13 << 4;
pub const ICE: u16 = 79 << 4;
pub const SAND: u16 = 12 << 4;
pub const RED_SAND: u16 = 12 << 4 | 1;
pub const SANDSTONE: u16 = 24 << 4;
pub const RED_SANDSTONE: u16 = 179 << 4;
pub const GRASS: u16 = 2 << 4;
pub const DIRT: u16 = 3 << 4;
pub const MYCELIUM: u16 = 110 << 4;
pub const HARDENED_CLAY: u16 = 172 << 4;
pub const STAINED_CLAY: u16 = 159 << 4;
pub const SNOW_LAYER: u16 = 78 << 4;
pub const WATERLILY: u16 = 111 << 4;

#[inline]
pub fn raw(id: u16, meta: u8) -> u16 {
    id << 4 | meta as u16
}

#[inline]
fn id_of(r: u16) -> u16 {
    r >> 4
}

/// `ChunkPrimer`: raw `id << 4 | meta`, indexed `x << 12 | z << 8 | y`.
pub struct Primer {
    pub data: Vec<u16>,
}

impl Primer {
    fn new() -> Self {
        Self { data: vec![0; 65536] }
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        let r = self.data[(x << 12 | z << 8 | y) as usize];
        // Unregistered values read as air, like `BLOCK_STATE_IDS.getByValue`.
        if mc_data::StateId::try_from_raw(r).is_some() {
            r
        } else {
            AIR
        }
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, z: i32, v: u16) {
        self.data[(x << 12 | z << 8 | y) as usize] = v;
    }
    #[inline]
    fn material_air(&self, x: i32, y: i32, z: i32) -> bool {
        mc_data::StateId::from_raw(self.get(x, y, z)).block().material == mc_data::Material::Air
    }
}

/// Mesa band/noise state, lazily built per seed like `BiomeGenMesa`'s fields.
#[derive(Clone)]
struct MesaState {
    bands: [u16; 64],
    band_noise: Perlin,
    pillar_noise: Perlin,
    pillar_roof: Perlin,
}

/// The overworld generator. One instance per worker thread; output is deterministic per chunk
/// (apart from vanilla's own order-dependent mutable biome fields, kept as `top`/`filler`).
#[derive(Clone)]
pub struct Generator {
    pub seed: i64,
    pub layers: Layers,
    rand: JavaRandom,
    min_limit: Octaves,
    max_limit: Octaves,
    main: Octaves,
    surface: Perlin,
    pub noise5: Octaves,
    depth: Octaves,
    pub mob_spawner: Octaves,
    parabolic: [f32; 25],
    density: Vec<f64>,
    stone_noise: Vec<f64>,
    main_buf: Vec<f64>,
    min_buf: Vec<f64>,
    max_buf: Vec<f64>,
    depth_buf: Vec<f64>,
    /// Mutable `topBlock`/`fillerBlock` per biome id (vanilla mutates them in `genTerrainBlocks`).
    pub top: Vec<u16>,
    pub filler: Vec<u16>,
    mesa: Option<std::sync::Arc<MesaState>>,
    pub sea_level: i32,
    biome_cache: mc_core::FxHashMap<(i32, i32), Box<[u8; 256]>>,
}

/// `ChunkProviderSettings` defaults (the "customized" fields of a default world).
pub mod settings {
    pub const COORDINATE_SCALE: f32 = 684.412;
    pub const HEIGHT_SCALE: f32 = 684.412;
    pub const UPPER_LIMIT_SCALE: f32 = 512.0;
    pub const LOWER_LIMIT_SCALE: f32 = 512.0;
    pub const DEPTH_NOISE_SCALE_X: f32 = 200.0;
    pub const DEPTH_NOISE_SCALE_Z: f32 = 200.0;
    pub const DEPTH_NOISE_SCALE_EXPONENT: f32 = 0.5;
    pub const MAIN_NOISE_SCALE_X: f32 = 80.0;
    pub const MAIN_NOISE_SCALE_Y: f32 = 160.0;
    pub const MAIN_NOISE_SCALE_Z: f32 = 80.0;
    pub const BASE_SIZE: f32 = 8.5;
    pub const STRETCH_Y: f32 = 12.0;
    pub const BIOME_DEPTH_WEIGHT: f32 = 1.0;
    pub const BIOME_DEPTH_OFFSET: f32 = 0.0;
    pub const BIOME_SCALE_WEIGHT: f32 = 1.0;
    pub const BIOME_SCALE_OFFSET: f32 = 0.0;
    pub const SEA_LEVEL: i32 = 63;
}

impl Generator {
    pub fn new(seed: i64) -> Self {
        let mut rand = JavaRandom::new(seed);
        let min_limit = Octaves::new(&mut rand, 16);
        let max_limit = Octaves::new(&mut rand, 16);
        let main = Octaves::new(&mut rand, 8);
        let surface = Perlin::new(&mut rand, 4);
        let noise5 = Octaves::new(&mut rand, 10);
        let depth = Octaves::new(&mut rand, 16);
        let mob_spawner = Octaves::new(&mut rand, 8);
        let mut parabolic = [0f32; 25];
        for i in -2i32..=2 {
            for j in -2i32..=2 {
                parabolic[(i + 2 + (j + 2) * 5) as usize] = 10.0 / math::sqrt_float((i * i + j * j) as f32 + 0.2);
            }
        }
        let mut top = vec![0u16; 256];
        let mut filler = vec![0u16; 256];
        for b in biome::table().iter().flatten() {
            top[b.id as usize] = raw(b.top.0, b.top.1);
            filler[b.id as usize] = raw(b.filler.0, b.filler.1);
        }
        Self {
            seed,
            layers: Layers::new(seed, false),
            rand,
            min_limit,
            max_limit,
            main,
            surface,
            noise5,
            depth,
            mob_spawner,
            parabolic,
            density: vec![0.0; 825],
            stone_noise: vec![0.0; 256],
            main_buf: Vec::new(),
            min_buf: Vec::new(),
            max_buf: Vec::new(),
            depth_buf: Vec::new(),
            top,
            filler,
            mesa: None,
            sea_level: settings::SEA_LEVEL,
            biome_cache: Default::default(),
        }
    }

    /// `getBiomesForGeneration`: 1:4 scale biome ids.
    pub fn biomes_for_generation(&mut self, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let n = self.layers.gen_biomes;
        self.layers.get_ints(n, x, z, w, h)
    }

    /// `loadBlockGeneratorData` / `getBiomeGenAt`: 1:1 biome ids.
    pub fn biomes_at(&mut self, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let n = self.layers.biome_index;
        self.layers.get_ints(n, x, z, w, h)
    }

    /// `WorldChunkManager.getBiomeGenerator(pos)` through a per-generator cache.
    pub fn biome_at(&mut self, x: i32, z: i32) -> u8 {
        let key = (x >> 4, z >> 4);
        if !self.biome_cache.contains_key(&key) {
            if self.biome_cache.len() > 1024 {
                self.biome_cache.clear();
            }
            let v = self.biomes_at(key.0 << 4, key.1 << 4, 16, 16);
            let mut arr = Box::new([0u8; 256]);
            for (i, b) in v.iter().enumerate() {
                arr[i] = biome::biome_or_ocean(*b).id;
            }
            self.biome_cache.insert(key, arr);
        }
        self.biome_cache[&key][((x & 15) | (z & 15) << 4) as usize]
    }

    /// `provideChunk`
    pub fn provide_chunk(&mut self, cx: i32, cz: i32) -> Chunk {
        self.rand.set_seed((cx as i64).wrapping_mul(341873128712).wrapping_add((cz as i64).wrapping_mul(132897987541)));
        let mut primer = Primer::new();
        self.set_blocks_in_chunk(cx, cz, &mut primer);
        let biomes: Vec<u8> = self.biomes_at(cx * 16, cz * 16, 16, 16).iter().map(|&b| biome::biome_or_ocean(b).id).collect();
        self.replace_blocks_for_biome(cx, cz, &mut primer, &biomes);
        crate::caves::Caves::default().generate(self, cx, cz, &mut primer);
        crate::caves::Ravines::default().generate(self, cx, cz, &mut primer);
        crate::structure::generate_in_primer(self, cx, cz, &mut primer);
        let mut chunk = Chunk::new(cx, cz);
        for x in 0..16usize {
            for z in 0..16usize {
                for y in 0..256usize {
                    let v = primer.data[x << 12 | z << 8 | y];
                    if v == 0 {
                        continue;
                    }
                    let state = mc_data::StateId::from_raw(v);
                    if state.block().material != mc_data::Material::Air {
                        chunk.set_state_raw(x, y as i32, z, state);
                    }
                }
            }
        }
        chunk.biomes.copy_from_slice(&biomes);
        chunk.generate_skylight_map();
        chunk.modified = false;
        chunk
    }

    /// `setBlocksInChunk`
    fn set_blocks_in_chunk(&mut self, cx: i32, cz: i32, primer: &mut Primer) {
        let biomes = self.biomes_for_generation(cx * 4 - 2, cz * 4 - 2, 10, 10);
        self.density_field(cx * 4, 0, cz * 4, &biomes);
        let d = &self.density;
        for i in 0..4 {
            let j = i * 5;
            let k = (i + 1) * 5;
            for l in 0..4 {
                let i1 = (j + l) * 33;
                let j1 = (j + l + 1) * 33;
                let k1 = (k + l) * 33;
                let l1 = (k + l + 1) * 33;
                for i2 in 0..32 {
                    let d0 = 0.125;
                    let mut d1 = d[i1 + i2];
                    let mut d2 = d[j1 + i2];
                    let mut d3 = d[k1 + i2];
                    let mut d4 = d[l1 + i2];
                    let d5 = (d[i1 + i2 + 1] - d1) * d0;
                    let d6 = (d[j1 + i2 + 1] - d2) * d0;
                    let d7 = (d[k1 + i2 + 1] - d3) * d0;
                    let d8 = (d[l1 + i2 + 1] - d4) * d0;
                    for j2 in 0..8 {
                        let d9 = 0.25;
                        let mut d10 = d1;
                        let mut d11 = d2;
                        let d12 = (d3 - d1) * d9;
                        let d13 = (d4 - d2) * d9;
                        for k2 in 0..4 {
                            let d14 = 0.25;
                            let d16 = (d11 - d10) * d14;
                            let mut lvt = d10 - d16;
                            for l2 in 0..4 {
                                lvt += d16;
                                let (x, y, z) = ((i * 4 + k2) as i32, (i2 * 8 + j2) as i32, (l * 4 + l2) as i32);
                                if lvt > 0.0 {
                                    primer.set(x, y, z, STONE);
                                } else if y < self.sea_level {
                                    primer.set(x, y, z, WATER);
                                }
                            }
                            d10 += d12;
                            d11 += d13;
                        }
                        d1 += d5;
                        d2 += d6;
                        d3 += d7;
                        d4 += d8;
                    }
                }
            }
        }
    }

    /// `func_147423_a`
    fn density_field(&mut self, x: i32, y: i32, z: i32, biomes: &[i32]) {
        use settings::*;
        self.depth.generate_2d(&mut self.depth_buf, x, z, 5, 5, DEPTH_NOISE_SCALE_X as f64, DEPTH_NOISE_SCALE_Z as f64);
        let f = COORDINATE_SCALE;
        let f1 = HEIGHT_SCALE;
        self.main.generate(&mut self.main_buf, x, y, z, 5, 33, 5, (f / MAIN_NOISE_SCALE_X) as f64, (f1 / MAIN_NOISE_SCALE_Y) as f64, (f / MAIN_NOISE_SCALE_Z) as f64);
        self.min_limit.generate(&mut self.min_buf, x, y, z, 5, 33, 5, f as f64, f1 as f64, f as f64);
        self.max_limit.generate(&mut self.max_buf, x, y, z, 5, 33, 5, f as f64, f1 as f64, f as f64);
        let mut i = 0usize;
        let mut j = 0usize;
        for k in 0..5i32 {
            for l in 0..5i32 {
                let mut f2 = 0.0f32;
                let mut f3 = 0.0f32;
                let mut f4 = 0.0f32;
                let center = biome::biome_or_ocean(biomes[(k + 2 + (l + 2) * 10) as usize]);
                for j1 in -2..=2 {
                    for k1 in -2..=2 {
                        let b1 = biome::biome_or_ocean(biomes[(k + j1 + 2 + (l + k1 + 2) * 10) as usize]);
                        let f5 = BIOME_DEPTH_OFFSET + b1.min_height * BIOME_DEPTH_WEIGHT;
                        let f6 = BIOME_SCALE_OFFSET + b1.max_height * BIOME_SCALE_WEIGHT;
                        let mut f7 = self.parabolic[(j1 + 2 + (k1 + 2) * 5) as usize] / (f5 + 2.0);
                        if b1.min_height > center.min_height {
                            f7 /= 2.0;
                        }
                        f2 += f6 * f7;
                        f3 += f5 * f7;
                        f4 += f7;
                    }
                }
                f2 /= f4;
                f3 /= f4;
                f2 = f2 * 0.9 + 0.1;
                f3 = (f3 * 4.0 - 1.0) / 8.0;
                let mut d7 = self.depth_buf[j] / 8000.0;
                if d7 < 0.0 {
                    d7 = -d7 * 0.3;
                }
                d7 = d7 * 3.0 - 2.0;
                if d7 < 0.0 {
                    d7 /= 2.0;
                    if d7 < -1.0 {
                        d7 = -1.0;
                    }
                    d7 /= 1.4;
                    d7 /= 2.0;
                } else {
                    if d7 > 1.0 {
                        d7 = 1.0;
                    }
                    d7 /= 8.0;
                }
                j += 1;
                let mut d8 = f3 as f64;
                let d9 = f2 as f64;
                d8 += d7 * 0.2;
                d8 = d8 * BASE_SIZE as f64 / 8.0;
                let d0 = BASE_SIZE as f64 + d8 * 4.0;
                for l1 in 0..33 {
                    let mut d1 = (l1 as f64 - d0) * STRETCH_Y as f64 * 128.0 / 256.0 / d9;
                    if d1 < 0.0 {
                        d1 *= 4.0;
                    }
                    let d2 = self.min_buf[i] / LOWER_LIMIT_SCALE as f64;
                    let d3 = self.max_buf[i] / UPPER_LIMIT_SCALE as f64;
                    let d4 = (self.main_buf[i] / 10.0 + 1.0) / 2.0;
                    let mut d5 = math::denormalize_clamp(d2, d3, d4) - d1;
                    if l1 > 29 {
                        let d6 = ((l1 - 29) as f32 / 3.0) as f64;
                        d5 = d5 * (1.0 - d6) + -10.0 * d6;
                    }
                    self.density[i] = d5;
                    i += 1;
                }
            }
        }
    }

    /// `replaceBlocksForBiome`
    fn replace_blocks_for_biome(&mut self, cx: i32, cz: i32, primer: &mut Primer, biomes: &[u8]) {
        let d0 = 0.03125;
        let mut stone = std::mem::take(&mut self.stone_noise);
        self.surface.generate(&mut stone, (cx * 16) as f64, (cz * 16) as f64, 16, 16, d0 * 2.0, d0 * 2.0, 1.0);
        for i in 0..16 {
            for j in 0..16 {
                let b = biome::biome(biomes[(j + i * 16) as usize] as i32).unwrap();
                self.gen_terrain_blocks(b, primer, cx * 16 + i, cz * 16 + j, stone[(j + i * 16) as usize]);
            }
        }
        self.stone_noise = stone;
    }

    /// `BiomeGenBase.genTerrainBlocks` dispatch (note vanilla's swapped x/z, kept on purpose).
    fn gen_terrain_blocks(&mut self, b: &GenBiome, primer: &mut Primer, x: i32, z: i32, noise: f64) {
        let id = b.id as usize;
        match b.kind {
            Kind::Mutated => {
                let base = b.base_biome();
                return self.gen_terrain_blocks(base, primer, x, z, noise);
            }
            Kind::Hills(mode) => {
                self.top[id] = GRASS;
                self.filler[id] = DIRT;
                if (noise < -1.0 || noise > 2.0) && mode == 2 {
                    self.top[id] = GRAVEL;
                    self.filler[id] = GRAVEL;
                } else if noise > 1.0 && mode != 1 {
                    self.top[id] = STONE;
                    self.filler[id] = STONE;
                }
            }
            Kind::Taiga(t) if t == 1 || t == 2 => {
                self.top[id] = GRASS;
                self.filler[id] = DIRT;
                if noise > 1.75 {
                    self.top[id] = raw(3, 1);
                } else if noise > -0.95 {
                    self.top[id] = raw(3, 2);
                }
            }
            Kind::SavannaMutated => {
                self.top[id] = GRASS;
                self.filler[id] = DIRT;
                if noise > 1.75 {
                    self.top[id] = STONE;
                    self.filler[id] = STONE;
                } else if noise > -0.5 {
                    self.top[id] = raw(3, 1);
                }
            }
            Kind::Swamp => {
                let d0 = grass_color_noise().value(x as f64 * 0.25, z as f64 * 0.25);
                if d0 > 0.0 {
                    let i = x & 15;
                    let j = z & 15;
                    for k in (0..=255).rev() {
                        if !primer.material_air(j, k, i) {
                            if k == 62 && id_of(primer.get(j, k, i)) != 9 {
                                primer.set(j, k, i, WATER);
                                if d0 < 0.12 {
                                    primer.set(j, k + 1, i, WATERLILY);
                                }
                            }
                            break;
                        }
                    }
                }
            }
            Kind::Mesa(bryce, forest) => {
                return self.mesa_terrain(b, bryce, forest, primer, x, z, noise);
            }
            _ => {}
        }
        self.generate_biome_terrain(b, primer, x, z, noise);
    }

    /// `generateBiomeTerrain`
    fn generate_biome_terrain(&mut self, b: &GenBiome, primer: &mut Primer, x: i32, z: i32, noise: f64) {
        let i = self.sea_level;
        let id = b.id as usize;
        let mut top: Option<u16> = Some(self.top[id]);
        let mut fill = self.filler[id];
        let mut j = -1;
        let k = (noise / 3.0 + 3.0 + self.rand.next_double() * 0.25) as i32;
        let l = x & 15;
        let i1 = z & 15;
        for j1 in (0..=255).rev() {
            if j1 <= self.rand.next_int_bounded(5) {
                primer.set(i1, j1, l, BEDROCK);
                continue;
            }
            let s = primer.get(i1, j1, l);
            if mc_data::StateId::from_raw(s).block().material == mc_data::Material::Air {
                j = -1;
            } else if id_of(s) == 1 {
                if j == -1 {
                    if k <= 0 {
                        top = None;
                        fill = STONE;
                    } else if j1 >= i - 4 && j1 <= i + 1 {
                        top = Some(self.top[id]);
                        fill = self.filler[id];
                    }
                    if j1 < i && top.is_none_or(|t| mc_data::StateId::from_raw(t).block().material == mc_data::Material::Air) {
                        top = Some(if temperature_at(b, x, j1, z) < 0.15 { ICE } else { WATER });
                    }
                    j = k;
                    if j1 >= i - 1 {
                        primer.set(i1, j1, l, top.unwrap_or(AIR));
                    } else if j1 < i - 7 - k {
                        top = None;
                        fill = STONE;
                        primer.set(i1, j1, l, GRAVEL);
                    } else {
                        primer.set(i1, j1, l, fill);
                    }
                } else if j > 0 {
                    j -= 1;
                    primer.set(i1, j1, l, fill);
                    if j == 0 && id_of(fill) == 12 {
                        j = self.rand.next_int_bounded(4) + (j1 - 63).max(0);
                        fill = if fill & 15 == 1 { RED_SANDSTONE } else { SANDSTONE };
                    }
                }
            }
        }
    }

    fn mesa_state(&mut self) -> std::sync::Arc<MesaState> {
        if self.mesa.is_none() {
            let mut bands = [HARDENED_CLAY; 64];
            let mut random = JavaRandom::new(self.seed);
            let pillar_roof = Perlin::new(&mut random, 1);
            let orange = raw(159, 1);
            let mut l1 = 0;
            while l1 < 64 {
                l1 += random.next_int_bounded(5) + 1;
                if l1 < 64 {
                    bands[l1 as usize] = orange;
                }
                l1 += 1;
            }
            let stripe = |random: &mut JavaRandom, bands: &mut [u16; 64], extra: i32, color: u8| {
                let n = random.next_int_bounded(4) + 2;
                for _ in 0..n {
                    let j = random.next_int_bounded(3) + extra;
                    let k = random.next_int_bounded(64);
                    let mut l = 0;
                    while k + l < 64 && l < j {
                        bands[(k + l) as usize] = raw(159, color);
                        l += 1;
                    }
                }
            };
            stripe(&mut random, &mut bands, 1, 4);
            stripe(&mut random, &mut bands, 2, 12);
            stripe(&mut random, &mut bands, 1, 14);
            let k3 = random.next_int_bounded(3) + 3;
            let mut j4 = 0;
            for _ in 0..k3 {
                j4 += random.next_int_bounded(16) + 4;
                let k1 = 0;
                if j4 + k1 < 64 {
                    bands[(j4 + k1) as usize] = raw(159, 0);
                    if j4 + k1 > 1 && random.next_boolean() {
                        bands[(j4 + k1 - 1) as usize] = raw(159, 8);
                    }
                    if j4 + k1 < 63 && random.next_boolean() {
                        bands[(j4 + k1 + 1) as usize] = raw(159, 8);
                    }
                }
            }
            // Vanilla seeds these from `field_150622_aD` before assigning it, so the first world
            // of a session gets noise from seed 0.
            let mut r2 = JavaRandom::new(0);
            let band_noise = Perlin::new(&mut r2, 4);
            let pillar_noise = Perlin::new(&mut r2, 1);
            self.mesa = Some(std::sync::Arc::new(MesaState { bands, band_noise, pillar_noise, pillar_roof }));
        }
        self.mesa.clone().unwrap()
    }

    /// `BiomeGenMesa.genTerrainBlocks`
    #[allow(clippy::too_many_arguments)]
    fn mesa_terrain(&mut self, b: &GenBiome, bryce: bool, forest: bool, primer: &mut Primer, x: i32, z: i32, noise: f64) {
        let mesa = self.mesa_state();
        let mut d4 = 0.0;
        if bryce {
            let i = (x & -16) + (z & 15);
            let j = (z & -16) + (x & 15);
            let d0 = noise.abs().min(mesa.band_noise.value(i as f64 * 0.25, j as f64 * 0.25));
            if d0 > 0.0 {
                let d1 = 0.001953125;
                let d2 = mesa.pillar_noise.value(i as f64 * d1, j as f64 * d1).abs();
                d4 = d0 * d0 * 2.5;
                let d3 = (d2 * 50.0).ceil() + 14.0;
                if d4 > d3 {
                    d4 = d3;
                }
                d4 += 64.0;
            }
        }
        let band = |y: i32| {
            // `Math.round`: floor(v + 0.5)
            let i = (mesa.pillar_roof.value(x as f64 * 1.0 / 512.0, x as f64 * 1.0 / 512.0) * 2.0 + 0.5).floor() as i32;
            mesa.bands[((y + i + 64) % 64) as usize]
        };
        let j1 = x & 15;
        let k1 = z & 15;
        let l1 = self.sea_level;
        let mut top: Option<u16> = Some(STAINED_CLAY);
        let mut fill = self.filler[b.id as usize];
        let k = (noise / 3.0 + 3.0 + self.rand.next_double() * 0.25) as i32;
        let flag = (noise / 3.0 * std::f64::consts::PI).cos() > 0.0;
        let mut l = -1;
        let mut flag1 = false;
        let orange = raw(159, 1);
        for i1 in (0..=255).rev() {
            if primer.material_air(k1, i1, j1) && i1 < d4 as i32 {
                primer.set(k1, i1, j1, STONE);
            }
            if i1 <= self.rand.next_int_bounded(5) {
                primer.set(k1, i1, j1, BEDROCK);
                continue;
            }
            let s = primer.get(k1, i1, j1);
            if mc_data::StateId::from_raw(s).block().material == mc_data::Material::Air {
                l = -1;
            } else if id_of(s) == 1 {
                if l == -1 {
                    flag1 = false;
                    if k <= 0 {
                        top = None;
                        fill = STONE;
                    } else if i1 >= l1 - 4 && i1 <= l1 + 1 {
                        top = Some(STAINED_CLAY);
                        fill = self.filler[b.id as usize];
                    }
                    if i1 < l1 && top.is_none_or(|t| mc_data::StateId::from_raw(t).block().material == mc_data::Material::Air) {
                        top = Some(WATER);
                    }
                    l = k + (i1 - l1).max(0);
                    if i1 < l1 - 1 {
                        primer.set(k1, i1, j1, fill);
                        if id_of(fill) == 159 {
                            primer.set(k1, i1, j1, orange);
                        }
                    } else if forest && i1 > 86 + k * 2 {
                        primer.set(k1, i1, j1, if flag { raw(3, 1) } else { GRASS });
                    } else if i1 <= l1 + 3 + k {
                        primer.set(k1, i1, j1, self.top[b.id as usize]);
                        flag1 = true;
                    } else {
                        let v = if (64..=127).contains(&i1) {
                            if flag {
                                HARDENED_CLAY
                            } else {
                                band(i1)
                            }
                        } else {
                            orange
                        };
                        primer.set(k1, i1, j1, v);
                    }
                } else if l > 0 {
                    l -= 1;
                    if flag1 {
                        primer.set(k1, i1, j1, orange);
                    } else {
                        primer.set(k1, i1, j1, band(i1));
                    }
                }
            }
        }
        let _ = top;
    }
}

/// `BiomeGenBase.GRASS_COLOR_NOISE`
pub fn grass_color_noise() -> &'static Perlin {
    static N: std::sync::OnceLock<Perlin> = std::sync::OnceLock::new();
    N.get_or_init(|| Perlin::new(&mut JavaRandom::new(2345), 1))
}

/// `BiomeGenBase.temperatureNoise`
pub fn temperature_noise() -> &'static Perlin {
    static N: std::sync::OnceLock<Perlin> = std::sync::OnceLock::new();
    N.get_or_init(|| Perlin::new(&mut JavaRandom::new(1234), 1))
}

/// `getFloatTemperature(pos)`
pub fn temperature_at(b: &GenBiome, x: i32, y: i32, z: i32) -> f32 {
    if y > 64 {
        let f = (temperature_noise().value(x as f64 / 8.0, z as f64 / 8.0) * 4.0) as f32;
        b.temperature - (f + y as f32 - 64.0) * 0.05 / 30.0
    } else {
        b.temperature
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_has_bedrock_stone_and_surface() {
        let mut g = Generator::new(12345);
        let t = std::time::Instant::now();
        let c = g.provide_chunk(0, 0);
        eprintln!("chunk in {:?}", t.elapsed());
        assert_eq!(c.get(0, 0, 0).0, 7);
        let h = c.height(8, 8);
        assert!(h > 40 && h < 200, "height {h}");
        let mut ids = std::collections::BTreeMap::new();
        for x in 0..16 {
            for z in 0..16 {
                let y = c.height(x, z) - 1;
                *ids.entry(c.get(x as usize, y, z as usize).0).or_insert(0) += 1;
            }
        }
        eprintln!("surface ids {ids:?} biome {}", c.biomes[0]);
        let t = std::time::Instant::now();
        for i in 0..16 {
            g.provide_chunk(i, 3);
        }
        eprintln!("16 chunks in {:?}", t.elapsed());
    }
}
