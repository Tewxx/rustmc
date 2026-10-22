//! `GenLayer` and every overworld layer, as an arena. Layers are shared between chains in vanilla
//! (`GenLayer.initializeAllBiomeGenerators`), so a node index can appear as several parents.
//! Only `parent` links take part in `initWorldGenSeed`, which leaves the hills layer's river input
//! with a zero world seed exactly like vanilla.

use crate::biome::{self, ids, Class};

const MUL: i64 = 6364136223846793005;
const ADD: i64 = 1442695040888963407;

#[derive(Clone, Copy, Debug)]
enum Op {
    Island,
    Zoom { fuzzy: bool },
    AddIsland,
    RemoveTooMuchOcean,
    AddSnow,
    EdgeCoolWarm,
    EdgeHeatIce,
    EdgeSpecial,
    AddMushroomIsland,
    DeepOcean,
    RiverInit,
    Biome,
    BiomeEdge,
    /// `GenLayerHills`; `other` is the river-noise input.
    Hills { other: usize },
    River,
    Smooth,
    RareBiome,
    Shore,
    /// `GenLayerRiverMix(biomes, rivers)`
    RiverMix { biomes: usize, rivers: usize },
    VoronoiZoom,
}

#[derive(Clone)]
struct Node {
    op: Op,
    parent: Option<usize>,
    base_seed: i64,
    world_seed: i64,
    chunk_seed: i64,
}

#[derive(Clone)]
pub struct Layers {
    nodes: Vec<Node>,
    /// `genBiomes` (biomes at 1:4) and `biomeIndexLayer` (1:1).
    pub gen_biomes: usize,
    pub biome_index: usize,
}

fn base_seed(s: i64) -> i64 {
    let mut b = s;
    for _ in 0..3 {
        b = b.wrapping_mul(b.wrapping_mul(MUL).wrapping_add(ADD));
        b = b.wrapping_add(s);
    }
    b
}

impl Layers {
    fn add(&mut self, seed: i64, op: Op, parent: Option<usize>) -> usize {
        self.nodes.push(Node { op, parent, base_seed: base_seed(seed), world_seed: 0, chunk_seed: 0 });
        self.nodes.len() - 1
    }

    fn magnify(&mut self, seed: i64, mut l: usize, n: i32) -> usize {
        for i in 0..n {
            l = self.add(seed + i as i64, Op::Zoom { fuzzy: false }, Some(l));
        }
        l
    }

    /// `GenLayer.initializeAllBiomeGenerators` for DEFAULT (and LARGE_BIOMES when `large`).
    pub fn new(seed: i64, large_biomes: bool) -> Self {
        let mut s = Self { nodes: Vec::new(), gen_biomes: 0, biome_index: 0 };
        let mut l = s.add(1, Op::Island, None);
        l = s.add(2000, Op::Zoom { fuzzy: true }, Some(l));
        l = s.add(1, Op::AddIsland, Some(l));
        l = s.add(2001, Op::Zoom { fuzzy: false }, Some(l));
        l = s.add(2, Op::AddIsland, Some(l));
        l = s.add(50, Op::AddIsland, Some(l));
        l = s.add(70, Op::AddIsland, Some(l));
        l = s.add(2, Op::RemoveTooMuchOcean, Some(l));
        l = s.add(2, Op::AddSnow, Some(l));
        l = s.add(3, Op::AddIsland, Some(l));
        l = s.add(2, Op::EdgeCoolWarm, Some(l));
        l = s.add(2, Op::EdgeHeatIce, Some(l));
        l = s.add(3, Op::EdgeSpecial, Some(l));
        l = s.add(2002, Op::Zoom { fuzzy: false }, Some(l));
        l = s.add(2003, Op::Zoom { fuzzy: false }, Some(l));
        l = s.add(4, Op::AddIsland, Some(l));
        l = s.add(5, Op::AddMushroomIsland, Some(l));
        l = s.add(4, Op::DeepOcean, Some(l));
        let genlayer4 = s.magnify(1000, l, 0);
        let i = if large_biomes { 6 } else { 4 };
        let j = 4;
        let lvt8 = s.magnify(1000, genlayer4, 0);
        let river_init = s.add(100, Op::RiverInit, Some(lvt8));
        let lvt9 = s.add(200, Op::Biome, Some(genlayer4));
        let genlayer6 = s.magnify(1000, lvt9, 2);
        let biome_edge = s.add(1000, Op::BiomeEdge, Some(genlayer6));
        let lvt10 = s.magnify(1000, river_init, 2);
        let mut hills = s.add(1000, Op::Hills { other: lvt10 }, Some(biome_edge));
        let mut genlayer5 = s.magnify(1000, river_init, 2);
        genlayer5 = s.magnify(1000, genlayer5, j);
        let river = s.add(1, Op::River, Some(genlayer5));
        let smooth = s.add(1000, Op::Smooth, Some(river));
        hills = s.add(1001, Op::RareBiome, Some(hills));
        for k in 0..i {
            hills = s.add(1000 + k as i64, Op::Zoom { fuzzy: false }, Some(hills));
            if k == 0 {
                hills = s.add(3, Op::AddIsland, Some(hills));
            }
            if k == 1 || i == 1 {
                hills = s.add(1000, Op::Shore, Some(hills));
            }
        }
        let smooth1 = s.add(1000, Op::Smooth, Some(hills));
        let mix = s.add(100, Op::RiverMix { biomes: smooth1, rivers: smooth }, None);
        let voronoi = s.add(10, Op::VoronoiZoom, Some(mix));
        s.init_world_seed(mix, seed);
        s.init_world_seed(voronoi, seed);
        s.gen_biomes = mix;
        s.biome_index = voronoi;
        s
    }

    fn init_world_seed(&mut self, n: usize, seed: i64) {
        if let Op::RiverMix { biomes, rivers } = self.nodes[n].op {
            self.init_world_seed(biomes, seed);
            self.init_world_seed(rivers, seed);
        }
        if let Some(p) = self.nodes[n].parent {
            self.init_world_seed(p, seed);
        }
        let b = self.nodes[n].base_seed;
        let mut w = seed;
        for _ in 0..3 {
            w = w.wrapping_mul(w.wrapping_mul(MUL).wrapping_add(ADD));
            w = w.wrapping_add(b);
        }
        self.nodes[n].world_seed = w;
    }

    #[inline]
    fn init_chunk_seed(&mut self, n: usize, x: i64, z: i64) {
        let node = &mut self.nodes[n];
        let mut c = node.world_seed;
        c = c.wrapping_mul(c.wrapping_mul(MUL).wrapping_add(ADD)).wrapping_add(x);
        c = c.wrapping_mul(c.wrapping_mul(MUL).wrapping_add(ADD)).wrapping_add(z);
        c = c.wrapping_mul(c.wrapping_mul(MUL).wrapping_add(ADD)).wrapping_add(x);
        c = c.wrapping_mul(c.wrapping_mul(MUL).wrapping_add(ADD)).wrapping_add(z);
        node.chunk_seed = c;
    }

    #[inline]
    fn next_int(&mut self, n: usize, bound: i32) -> i32 {
        let node = &mut self.nodes[n];
        let mut i = ((node.chunk_seed >> 24) % bound as i64) as i32;
        if i < 0 {
            i += bound;
        }
        node.chunk_seed = node.chunk_seed.wrapping_mul(node.chunk_seed.wrapping_mul(MUL).wrapping_add(ADD));
        node.chunk_seed = node.chunk_seed.wrapping_add(node.world_seed);
        i
    }

    fn select_random(&mut self, n: usize, v: &[i32]) -> i32 {
        v[self.next_int(n, v.len() as i32) as usize]
    }

    fn select_mode_or_random(&mut self, n: usize, a: i32, b: i32, c: i32, d: i32) -> i32 {
        if b == c && c == d {
            b
        } else if a == b && a == c {
            a
        } else if a == b && a == d {
            a
        } else if a == c && a == d {
            a
        } else if a == b && c != d {
            a
        } else if a == c && b != d {
            a
        } else if a == d && b != c {
            a
        } else if b == c && a != d {
            b
        } else if b == d && a != c {
            b
        } else if c == d && a != b {
            c
        } else {
            self.select_random(n, &[a, b, c, d])
        }
    }

    /// `getInts(areaX, areaY, w, h)` on node `n`.
    pub fn get_ints(&mut self, n: usize, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let op = self.nodes[n].op;
        let parent = self.nodes[n].parent;
        let (wu, hu) = (w as usize, h as usize);
        let mut out = vec![0i32; wu * hu];
        match op {
            Op::Island => {
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (x + j) as i64, (z + i) as i64);
                        out[(j + i * w) as usize] = if self.next_int(n, 10) == 0 { 1 } else { 0 };
                    }
                }
                if x > -w && x <= 0 && z > -h && z <= 0 {
                    out[(-x + -z * w) as usize] = 1;
                }
            }
            Op::Zoom { fuzzy } => {
                let i = x >> 1;
                let j = z >> 1;
                let k = (w >> 1) + 2;
                let l = (h >> 1) + 2;
                let a = self.get_ints(parent.unwrap(), i, j, k, l);
                let i1 = (k - 1) << 1;
                let j1 = (l - 1) << 1;
                let mut a1 = vec![0i32; (i1 * j1) as usize];
                for k1 in 0..l - 1 {
                    let mut l1 = ((k1 << 1) * i1) as usize;
                    let mut i2 = 0;
                    let mut j2 = a[(i2 + k1 * k) as usize];
                    let mut k2 = a[(i2 + (k1 + 1) * k) as usize];
                    while i2 < k - 1 {
                        self.init_chunk_seed(n, ((i2 + i) << 1) as i64, ((k1 + j) << 1) as i64);
                        let l2 = a[(i2 + 1 + k1 * k) as usize];
                        let i3 = a[(i2 + 1 + (k1 + 1) * k) as usize];
                        a1[l1] = j2;
                        a1[l1 + i1 as usize] = self.select_random(n, &[j2, k2]);
                        l1 += 1;
                        a1[l1] = self.select_random(n, &[j2, l2]);
                        a1[l1 + i1 as usize] = if fuzzy {
                            self.select_random(n, &[j2, l2, k2, i3])
                        } else {
                            self.select_mode_or_random(n, j2, l2, k2, i3)
                        };
                        l1 += 1;
                        j2 = l2;
                        k2 = i3;
                        i2 += 1;
                    }
                }
                for j3 in 0..h {
                    let src = ((j3 + (z & 1)) * i1 + (x & 1)) as usize;
                    out[(j3 * w) as usize..(j3 * w + w) as usize].copy_from_slice(&a1[src..src + wu]);
                }
            }
            Op::AddIsland => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = a[(j1 + i1 * k) as usize];
                        let l1 = a[(j1 + 2 + i1 * k) as usize];
                        let i2 = a[(j1 + (i1 + 2) * k) as usize];
                        let j2 = a[(j1 + 2 + (i1 + 2) * k) as usize];
                        let k2 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        self.init_chunk_seed(n, (j1 + x) as i64, (i1 + z) as i64);
                        let o = &mut 0;
                        if k2 != 0 || k1 == 0 && l1 == 0 && i2 == 0 && j2 == 0 {
                            if k2 > 0 && (k1 == 0 || l1 == 0 || i2 == 0 || j2 == 0) {
                                if self.next_int(n, 5) == 0 {
                                    *o = if k2 == 4 { 4 } else { 0 };
                                } else {
                                    *o = k2;
                                }
                            } else {
                                *o = k2;
                            }
                        } else {
                            let mut l2 = 1;
                            let mut i3 = 1;
                            if k1 != 0 {
                                let r = self.next_int(n, l2);
                                l2 += 1;
                                if r == 0 {
                                    i3 = k1;
                                }
                            }
                            if l1 != 0 {
                                let r = self.next_int(n, l2);
                                l2 += 1;
                                if r == 0 {
                                    i3 = l1;
                                }
                            }
                            if i2 != 0 {
                                let r = self.next_int(n, l2);
                                l2 += 1;
                                if r == 0 {
                                    i3 = i2;
                                }
                            }
                            if j2 != 0 {
                                let r = self.next_int(n, l2);
                                if r == 0 {
                                    i3 = j2;
                                }
                            }
                            *o = if self.next_int(n, 3) == 0 {
                                i3
                            } else if i3 == 4 {
                                4
                            } else {
                                0
                            };
                        }
                        out[(j1 + i1 * w) as usize] = *o;
                    }
                }
            }
            Op::RemoveTooMuchOcean => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = a[(j1 + 1 + i1 * k) as usize];
                        let l1 = a[(j1 + 2 + (i1 + 1) * k) as usize];
                        let i2 = a[(j1 + (i1 + 1) * k) as usize];
                        let j2 = a[(j1 + 1 + (i1 + 2) * k) as usize];
                        let k2 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        out[(j1 + i1 * w) as usize] = k2;
                        self.init_chunk_seed(n, (j1 + x) as i64, (i1 + z) as i64);
                        if k2 == 0 && k1 == 0 && l1 == 0 && i2 == 0 && j2 == 0 && self.next_int(n, 2) == 0 {
                            out[(j1 + i1 * w) as usize] = 1;
                        }
                    }
                }
            }
            Op::AddSnow => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        self.init_chunk_seed(n, (j1 + x) as i64, (i1 + z) as i64);
                        out[(j1 + i1 * w) as usize] = if k1 == 0 {
                            0
                        } else {
                            let l1 = self.next_int(n, 6);
                            if l1 == 0 {
                                4
                            } else if l1 <= 1 {
                                3
                            } else {
                                1
                            }
                        };
                    }
                }
            }
            Op::EdgeCoolWarm | Op::EdgeHeatIce => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                let cool = matches!(op, Op::EdgeCoolWarm);
                for i1 in 0..h {
                    for j1 in 0..w {
                        if cool {
                            self.init_chunk_seed(n, (j1 + x) as i64, (i1 + z) as i64);
                        }
                        let mut k1 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        let l1 = a[(j1 + 1 + i1 * k) as usize];
                        let i2 = a[(j1 + 2 + (i1 + 1) * k) as usize];
                        let j2 = a[(j1 + (i1 + 1) * k) as usize];
                        let k2 = a[(j1 + 1 + (i1 + 2) * k) as usize];
                        let any = |v: i32| l1 == v || i2 == v || j2 == v || k2 == v;
                        if cool {
                            if k1 == 1 && (any(3) || any(4)) {
                                k1 = 2;
                            }
                        } else if k1 == 4 && (any(1) || any(2)) {
                            k1 = 3;
                        }
                        out[(j1 + i1 * w) as usize] = k1;
                    }
                }
            }
            Op::EdgeSpecial => {
                let a = self.get_ints(parent.unwrap(), x, z, w, h);
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        let mut k = a[(j + i * w) as usize];
                        if k != 0 && self.next_int(n, 13) == 0 {
                            k |= (1 + self.next_int(n, 15)) << 8 & 3840;
                        }
                        out[(j + i * w) as usize] = k;
                    }
                }
            }
            Op::AddMushroomIsland => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = a[(j1 + i1 * k) as usize];
                        let l1 = a[(j1 + 2 + i1 * k) as usize];
                        let i2 = a[(j1 + (i1 + 2) * k) as usize];
                        let j2 = a[(j1 + 2 + (i1 + 2) * k) as usize];
                        let k2 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        self.init_chunk_seed(n, (j1 + x) as i64, (i1 + z) as i64);
                        out[(j1 + i1 * w) as usize] =
                            if k2 == 0 && k1 == 0 && l1 == 0 && i2 == 0 && j2 == 0 && self.next_int(n, 100) == 0 { ids::MUSHROOM_ISLAND } else { k2 };
                    }
                }
            }
            Op::DeepOcean => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = a[(j1 + 1 + i1 * k) as usize];
                        let l1 = a[(j1 + 2 + (i1 + 1) * k) as usize];
                        let i2 = a[(j1 + (i1 + 1) * k) as usize];
                        let j2 = a[(j1 + 1 + (i1 + 2) * k) as usize];
                        let k2 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        let l2 = [k1, l1, i2, j2].iter().filter(|&&v| v == 0).count();
                        out[(j1 + i1 * w) as usize] = if k2 == 0 && l2 > 3 { ids::DEEP_OCEAN } else { k2 };
                    }
                }
            }
            Op::RiverInit => {
                let a = self.get_ints(parent.unwrap(), x, z, w, h);
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        out[(j + i * w) as usize] = if a[(j + i * w) as usize] > 0 { self.next_int(n, 299_999) + 2 } else { 0 };
                    }
                }
            }
            Op::Biome => {
                const WARM: [i32; 6] = [ids::DESERT, ids::DESERT, ids::DESERT, ids::SAVANNA, ids::SAVANNA, ids::PLAINS];
                const MEDIUM: [i32; 6] = [ids::FOREST, ids::ROOFED_FOREST, ids::EXTREME_HILLS, ids::PLAINS, ids::BIRCH_FOREST, ids::SWAMPLAND];
                const COLD: [i32; 4] = [ids::FOREST, ids::EXTREME_HILLS, ids::TAIGA, ids::PLAINS];
                const ICE: [i32; 4] = [ids::ICE_PLAINS, ids::ICE_PLAINS, ids::ICE_PLAINS, ids::COLD_TAIGA];
                let a = self.get_ints(parent.unwrap(), x, z, w, h);
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        let mut k = a[(j + i * w) as usize];
                        let l = (k & 3840) >> 8;
                        k &= -3841;
                        out[(j + i * w) as usize] = if is_oceanic(k) || k == ids::MUSHROOM_ISLAND {
                            k
                        } else if k == 1 {
                            if l > 0 {
                                if self.next_int(n, 3) == 0 {
                                    ids::MESA_PLATEAU
                                } else {
                                    ids::MESA_PLATEAU_F
                                }
                            } else {
                                WARM[self.next_int(n, 6) as usize]
                            }
                        } else if k == 2 {
                            if l > 0 {
                                ids::JUNGLE
                            } else {
                                MEDIUM[self.next_int(n, 6) as usize]
                            }
                        } else if k == 3 {
                            if l > 0 {
                                ids::MEGA_TAIGA
                            } else {
                                COLD[self.next_int(n, 4) as usize]
                            }
                        } else if k == 4 {
                            ICE[self.next_int(n, 4) as usize]
                        } else {
                            ids::MUSHROOM_ISLAND
                        };
                    }
                }
            }
            Op::BiomeEdge => {
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                let w2 = w + 2;
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        let k = a[(j + 1 + (i + 1) * w2) as usize];
                        let nb = [
                            a[(j + 1 + i * w2) as usize],
                            a[(j + 2 + (i + 1) * w2) as usize],
                            a[(j + (i + 1) * w2) as usize],
                            a[(j + 1 + (i + 2) * w2) as usize],
                        ];
                        let o = (j + i * w) as usize;
                        if let Some(v) = replace_edge_if_necessary(k, &nb, ids::EXTREME_HILLS, ids::EXTREME_HILLS_EDGE)
                            .or_else(|| replace_edge(k, &nb, ids::MESA_PLATEAU_F, ids::MESA))
                            .or_else(|| replace_edge(k, &nb, ids::MESA_PLATEAU, ids::MESA))
                            .or_else(|| replace_edge(k, &nb, ids::MEGA_TAIGA, ids::TAIGA))
                        {
                            out[o] = v;
                        } else if k == ids::DESERT {
                            out[o] = if nb.contains(&ids::ICE_PLAINS) { ids::EXTREME_HILLS_PLUS } else { k };
                        } else if k == ids::SWAMPLAND {
                            out[o] = if nb.iter().any(|&v| v == ids::DESERT || v == ids::COLD_TAIGA || v == ids::ICE_PLAINS) {
                                ids::PLAINS
                            } else if nb.contains(&ids::JUNGLE) {
                                ids::JUNGLE_EDGE
                            } else {
                                k
                            };
                        } else {
                            out[o] = k;
                        }
                    }
                }
            }
            Op::Hills { other } => {
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                let a1 = self.get_ints(other, x - 1, z - 1, w + 2, h + 2);
                let w2 = w + 2;
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        let k = a[(j + 1 + (i + 1) * w2) as usize];
                        let l = a1[(j + 1 + (i + 1) * w2) as usize];
                        let flag = (l - 2) % 29 == 0;
                        let o = (j + i * w) as usize;
                        if k != 0 && l >= 2 && (l - 2) % 29 == 1 && k < 128 {
                            out[o] = if biome::biome(k + 128).is_some() { k + 128 } else { k };
                        } else if self.next_int(n, 3) != 0 && !flag {
                            out[o] = k;
                        } else {
                            let mut i1 = k;
                            if k == ids::DESERT {
                                i1 = ids::DESERT_HILLS;
                            } else if k == ids::FOREST {
                                i1 = ids::FOREST_HILLS;
                            } else if k == ids::BIRCH_FOREST {
                                i1 = ids::BIRCH_FOREST_HILLS;
                            } else if k == ids::ROOFED_FOREST {
                                i1 = ids::PLAINS;
                            } else if k == ids::TAIGA {
                                i1 = ids::TAIGA_HILLS;
                            } else if k == ids::MEGA_TAIGA {
                                i1 = ids::MEGA_TAIGA_HILLS;
                            } else if k == ids::COLD_TAIGA {
                                i1 = ids::COLD_TAIGA_HILLS;
                            } else if k == ids::PLAINS {
                                i1 = if self.next_int(n, 3) == 0 { ids::FOREST_HILLS } else { ids::FOREST };
                            } else if k == ids::ICE_PLAINS {
                                i1 = ids::ICE_MOUNTAINS;
                            } else if k == ids::JUNGLE {
                                i1 = ids::JUNGLE_HILLS;
                            } else if k == ids::OCEAN {
                                i1 = ids::DEEP_OCEAN;
                            } else if k == ids::EXTREME_HILLS {
                                i1 = ids::EXTREME_HILLS_PLUS;
                            } else if k == ids::SAVANNA {
                                i1 = ids::SAVANNA_PLATEAU;
                            } else if equal_or_mesa_plateau(k, ids::MESA_PLATEAU_F) {
                                i1 = ids::MESA;
                            } else if k == ids::DEEP_OCEAN && self.next_int(n, 3) == 0 {
                                i1 = if self.next_int(n, 2) == 0 { ids::PLAINS } else { ids::FOREST };
                            }
                            if flag && i1 != k {
                                if biome::biome(i1 + 128).is_some() {
                                    i1 += 128;
                                } else {
                                    i1 = k;
                                }
                            }
                            if i1 == k {
                                out[o] = k;
                            } else {
                                let nb = [
                                    a[(j + 1 + i * w2) as usize],
                                    a[(j + 2 + (i + 1) * w2) as usize],
                                    a[(j + (i + 1) * w2) as usize],
                                    a[(j + 1 + (i + 2) * w2) as usize],
                                ];
                                let j2 = nb.iter().filter(|&&v| equal_or_mesa_plateau(v, k)).count();
                                out[o] = if j2 >= 3 { i1 } else { k };
                            }
                        }
                    }
                }
            }
            Op::River => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                let f = |v: i32| if v >= 2 { 2 + (v & 1) } else { v };
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = f(a[(j1 + (i1 + 1) * k) as usize]);
                        let l1 = f(a[(j1 + 2 + (i1 + 1) * k) as usize]);
                        let i2 = f(a[(j1 + 1 + i1 * k) as usize]);
                        let j2 = f(a[(j1 + 1 + (i1 + 2) * k) as usize]);
                        let k2 = f(a[(j1 + 1 + (i1 + 1) * k) as usize]);
                        out[(j1 + i1 * w) as usize] = if k2 == k1 && k2 == i2 && k2 == l1 && k2 == j2 { -1 } else { ids::RIVER };
                    }
                }
            }
            Op::Smooth => {
                let k = w + 2;
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i1 in 0..h {
                    for j1 in 0..w {
                        let k1 = a[(j1 + (i1 + 1) * k) as usize];
                        let l1 = a[(j1 + 2 + (i1 + 1) * k) as usize];
                        let i2 = a[(j1 + 1 + i1 * k) as usize];
                        let j2 = a[(j1 + 1 + (i1 + 2) * k) as usize];
                        let mut k2 = a[(j1 + 1 + (i1 + 1) * k) as usize];
                        if k1 == l1 && i2 == j2 {
                            self.init_chunk_seed(n, (j1 + x) as i64, (i1 + z) as i64);
                            k2 = if self.next_int(n, 2) == 0 { k1 } else { i2 };
                        } else {
                            if k1 == l1 {
                                k2 = k1;
                            }
                            if i2 == j2 {
                                k2 = i2;
                            }
                        }
                        out[(j1 + i1 * w) as usize] = k2;
                    }
                }
            }
            Op::RareBiome => {
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        let k = a[(j + 1 + (i + 1) * (w + 2)) as usize];
                        out[(j + i * w) as usize] = if self.next_int(n, 57) == 0 && k == ids::PLAINS { ids::PLAINS + 128 } else { k };
                    }
                }
            }
            Op::Shore => {
                let a = self.get_ints(parent.unwrap(), x - 1, z - 1, w + 2, h + 2);
                let w2 = w + 2;
                for i in 0..h {
                    for j in 0..w {
                        self.init_chunk_seed(n, (j + x) as i64, (i + z) as i64);
                        let k = a[(j + 1 + (i + 1) * w2) as usize];
                        let b = biome::biome(k);
                        let nb = [
                            a[(j + 1 + i * w2) as usize],
                            a[(j + 2 + (i + 1) * w2) as usize],
                            a[(j + (i + 1) * w2) as usize],
                            a[(j + 1 + (i + 2) * w2) as usize],
                        ];
                        let o = (j + i * w) as usize;
                        let no_ocean = nb.iter().all(|&v| !is_oceanic(v));
                        out[o] = if k == ids::MUSHROOM_ISLAND {
                            if nb.contains(&ids::OCEAN) {
                                ids::MUSHROOM_ISLAND_SHORE
                            } else {
                                k
                            }
                        } else if b.is_some_and(|b| b.class() == Class::Jungle) {
                            if nb.iter().all(|&v| jungle_compatible(v)) {
                                if no_ocean {
                                    k
                                } else {
                                    ids::BEACH
                                }
                            } else {
                                ids::JUNGLE_EDGE
                            }
                        } else if k != ids::EXTREME_HILLS && k != ids::EXTREME_HILLS_PLUS && k != ids::EXTREME_HILLS_EDGE {
                            if b.is_some_and(|b| b.is_snowy()) {
                                shore(k, no_ocean, ids::COLD_BEACH)
                            } else if k != ids::MESA && k != ids::MESA_PLATEAU_F {
                                if k != ids::OCEAN && k != ids::DEEP_OCEAN && k != ids::RIVER && k != ids::SWAMPLAND {
                                    if no_ocean {
                                        k
                                    } else {
                                        ids::BEACH
                                    }
                                } else {
                                    k
                                }
                            } else if no_ocean {
                                if nb.iter().all(|&v| biome::biome(v).is_some_and(|b| b.class() == Class::Mesa)) {
                                    k
                                } else {
                                    ids::DESERT
                                }
                            } else {
                                k
                            }
                        } else {
                            shore(k, no_ocean, ids::STONE_BEACH)
                        };
                    }
                }
            }
            Op::RiverMix { biomes, rivers } => {
                let a = self.get_ints(biomes, x, z, w, h);
                let a1 = self.get_ints(rivers, x, z, w, h);
                for i in 0..wu * hu {
                    out[i] = if a[i] != ids::OCEAN && a[i] != ids::DEEP_OCEAN {
                        if a1[i] == ids::RIVER {
                            if a[i] == ids::ICE_PLAINS {
                                ids::FROZEN_RIVER
                            } else if a[i] != ids::MUSHROOM_ISLAND && a[i] != ids::MUSHROOM_ISLAND_SHORE {
                                a1[i] & 255
                            } else {
                                ids::MUSHROOM_ISLAND_SHORE
                            }
                        } else {
                            a[i]
                        }
                    } else {
                        a[i]
                    };
                }
            }
            Op::VoronoiZoom => {
                let x = x - 2;
                let z = z - 2;
                let i = x >> 2;
                let j = z >> 2;
                let k = (w >> 2) + 2;
                let l = (h >> 2) + 2;
                let a = self.get_ints(parent.unwrap(), i, j, k, l);
                let i1 = (k - 1) << 2;
                let j1 = (l - 1) << 2;
                let mut a1 = vec![0i32; (i1 * j1) as usize];
                for k1 in 0..l - 1 {
                    let mut l1 = 0;
                    let mut i2 = a[(k1 * k) as usize];
                    let mut j2 = a[((k1 + 1) * k) as usize];
                    while l1 < k - 1 {
                        let r = |s: &mut Self| (s.next_int(n, 1024) as f64 / 1024.0 - 0.5) * 3.6;
                        self.init_chunk_seed(n, ((l1 + i) << 2) as i64, ((k1 + j) << 2) as i64);
                        let d1 = r(self);
                        let d2 = r(self);
                        self.init_chunk_seed(n, ((l1 + i + 1) << 2) as i64, ((k1 + j) << 2) as i64);
                        let d3 = r(self) + 4.0;
                        let d4 = r(self);
                        self.init_chunk_seed(n, ((l1 + i) << 2) as i64, ((k1 + j + 1) << 2) as i64);
                        let d5 = r(self);
                        let d6 = r(self) + 4.0;
                        self.init_chunk_seed(n, ((l1 + i + 1) << 2) as i64, ((k1 + j + 1) << 2) as i64);
                        let d7 = r(self) + 4.0;
                        let d8 = r(self) + 4.0;
                        let k2 = a[(l1 + 1 + k1 * k) as usize] & 255;
                        let l2 = a[(l1 + 1 + (k1 + 1) * k) as usize] & 255;
                        for i3 in 0..4 {
                            let mut j3 = (((k1 << 2) + i3) * i1 + (l1 << 2)) as usize;
                            for k3 in 0..4 {
                                let (fi, fk) = (i3 as f64, k3 as f64);
                                let d9 = (fi - d2) * (fi - d2) + (fk - d1) * (fk - d1);
                                let d10 = (fi - d4) * (fi - d4) + (fk - d3) * (fk - d3);
                                let d11 = (fi - d6) * (fi - d6) + (fk - d5) * (fk - d5);
                                let d12 = (fi - d8) * (fi - d8) + (fk - d7) * (fk - d7);
                                a1[j3] = if d9 < d10 && d9 < d11 && d9 < d12 {
                                    i2
                                } else if d10 < d9 && d10 < d11 && d10 < d12 {
                                    k2
                                } else if d11 < d9 && d11 < d10 && d11 < d12 {
                                    j2
                                } else {
                                    l2
                                };
                                j3 += 1;
                            }
                        }
                        i2 = k2;
                        j2 = l2;
                        l1 += 1;
                    }
                }
                for l3 in 0..h {
                    let src = ((l3 + (z & 3)) * i1 + (x & 3)) as usize;
                    out[(l3 * w) as usize..(l3 * w + w) as usize].copy_from_slice(&a1[src..src + wu]);
                }
            }
        }
        out
    }
}

fn shore(k: i32, no_ocean: bool, beach: i32) -> i32 {
    if is_oceanic(k) || no_ocean {
        k
    } else {
        beach
    }
}

fn jungle_compatible(v: i32) -> bool {
    if biome::biome(v).is_some_and(|b| b.class() == Class::Jungle) {
        return true;
    }
    v == ids::JUNGLE_EDGE || v == ids::JUNGLE || v == ids::JUNGLE_HILLS || v == ids::FOREST || v == ids::TAIGA || is_oceanic(v)
}

/// `isBiomeOceanic`
pub fn is_oceanic(id: i32) -> bool {
    id == ids::OCEAN || id == ids::DEEP_OCEAN || id == ids::FROZEN_OCEAN
}

/// `biomesEqualOrMesaPlateau`
pub fn equal_or_mesa_plateau(a: i32, b: i32) -> bool {
    if a == b {
        return true;
    }
    if a != ids::MESA_PLATEAU_F && a != ids::MESA_PLATEAU {
        match (biome::biome(a), biome::biome(b)) {
            (Some(x), Some(y)) => x.is_equal_to(y),
            _ => false,
        }
    } else {
        b == ids::MESA_PLATEAU_F || b == ids::MESA_PLATEAU
    }
}

fn can_be_neighbors(a: i32, b: i32) -> bool {
    if equal_or_mesa_plateau(a, b) {
        return true;
    }
    match (biome::biome(a), biome::biome(b)) {
        (Some(x), Some(y)) => {
            use biome::TempCategory::Medium;
            let (c, c1) = (x.temp_category(), y.temp_category());
            c == c1 || c == Medium || c1 == Medium
        }
        _ => false,
    }
}

fn replace_edge_if_necessary(k: i32, nb: &[i32; 4], target: i32, edge: i32) -> Option<i32> {
    if !equal_or_mesa_plateau(k, target) {
        return None;
    }
    Some(if nb.iter().all(|&v| can_be_neighbors(v, target)) { k } else { edge })
}

fn replace_edge(k: i32, nb: &[i32; 4], target: i32, edge: i32) -> Option<i32> {
    if k != target {
        return None;
    }
    Some(if nb.iter().all(|&v| equal_or_mesa_plateau(v, target)) { k } else { edge })
}
