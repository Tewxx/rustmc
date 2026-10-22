//! The biome fields needed by block tinting. Terrain generation is a separate subsystem.

use crate::color::Colorizer;
use mc_core::{random::JavaRandom, BlockPos};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub enum ColorMode {
    Default,
    Swamp,
    Mesa,
    RoofedForest,
}

#[derive(Debug)]
pub struct Biome {
    pub id: u8,
    pub name: &'static str,
    pub temperature: f32,
    pub rainfall: f32,
    pub water_color: u32,
    /// `BiomeGenMutated` delegates color methods to its base biome.
    pub color_base: u8,
    pub color_mode: ColorMode,
}

impl Biome {
    pub fn all() -> &'static [Biome] {
        &crate::generated::biomes::BIOMES
    }

    pub fn by_id(id: u8) -> Option<&'static Biome> {
        Self::all().binary_search_by_key(&id, |b| b.id).ok().map(|i| &Self::all()[i])
    }

    pub fn temperature_at(&self, pos: BlockPos) -> f32 {
        if pos.y <= 64 {
            return self.temperature;
        }
        static NOISE: OnceLock<Simplex> = OnceLock::new();
        let f = (NOISE.get_or_init(|| Simplex::new(1234)).sample(pos.x as f64 / 8.0, pos.z as f64 / 8.0) * 4.0) as f32;
        self.temperature - (f + pos.y as f32 - 64.0) * 0.05 / 30.0
    }

    pub fn grass_color(&self, pos: BlockPos, colors: &impl Colorizer) -> u32 {
        let b = Self::by_id(self.color_base).unwrap();
        match b.color_mode {
            ColorMode::Swamp => {
                static NOISE: OnceLock<Simplex> = OnceLock::new();
                if NOISE.get_or_init(|| Simplex::new(2345)).sample(pos.x as f64 * 0.0225, pos.z as f64 * 0.0225) < -0.1 {
                    5_011_004
                } else {
                    6_975_545
                }
            }
            ColorMode::Mesa => 9_470_285,
            _ => {
                let c = colors.grass(b.temperature_at(pos).clamp(0.0, 1.0) as f64, b.rainfall.clamp(0.0, 1.0) as f64);
                if matches!(b.color_mode, ColorMode::RoofedForest) {
                    ((c & 16_711_422) + 2_634_762) >> 1
                } else {
                    c
                }
            }
        }
    }

    pub fn foliage_color(&self, pos: BlockPos, colors: &impl Colorizer) -> u32 {
        let b = Self::by_id(self.color_base).unwrap();
        match b.color_mode {
            ColorMode::Swamp => 6_975_545,
            ColorMode::Mesa => 10_387_789,
            _ => colors.foliage(b.temperature_at(pos).clamp(0.0, 1.0) as f64, b.rainfall.clamp(0.0, 1.0) as f64),
        }
    }
}

/// The scalar 2D path of `NoiseGeneratorSimplex`, as used by one-octave biome color noise.
/// Its floor operation intentionally returns -1 for zero and n-1 for negative integers.
struct Simplex {
    perm: [usize; 512],
}

impl Simplex {
    fn new(seed: i64) -> Self {
        let mut random = JavaRandom::new(seed);
        for _ in 0..3 {
            random.next_double();
        }
        let mut perm = [0; 512];
        for (i, v) in perm[..256].iter_mut().enumerate() {
            *v = i;
        }
        for i in 0..256 {
            let j = random.next_int_bounded((256 - i) as i32) as usize + i;
            perm.swap(i, j);
            perm[i + 256] = perm[i];
        }
        Self { perm }
    }

    fn sample(&self, x: f64, z: f64) -> f64 {
        let root = 3.0f64.sqrt();
        let skew = (x + z) * (0.5 * (root - 1.0));
        let floor = |v: f64| if v > 0.0 { v as i32 } else { (v as i32).wrapping_sub(1) };
        let (i, j) = (floor(x + skew), floor(z + skew));
        let g = (3.0 - root) / 6.0;
        let unskew = i.wrapping_add(j) as f64 * g;
        let (x0, z0) = (x - (i as f64 - unskew), z - (j as f64 - unskew));
        let (di, dj) = if x0 > z0 { (1, 0) } else { (0, 1) };
        let (x1, z1) = (x0 - di as f64 + g, z0 - dj as f64 + g);
        let (x2, z2) = (x0 - 1.0 + 2.0 * g, z0 - 1.0 + 2.0 * g);
        let (ii, jj) = ((i & 255) as usize, (j & 255) as usize);
        let a = self.perm[ii + self.perm[jj]] % 12;
        let b = self.perm[ii + di + self.perm[jj + dj]] % 12;
        let c = self.perm[ii + 1 + self.perm[jj + 1]] % 12;
        const GRAD: [[f64; 2]; 12] = [
            [1., 1.],
            [-1., 1.],
            [1., -1.],
            [-1., -1.],
            [1., 0.],
            [-1., 0.],
            [1., 0.],
            [-1., 0.],
            [0., 1.],
            [0., -1.],
            [0., 1.],
            [0., -1.],
        ];
        let corner = |index: usize, dx: f64, dz: f64| {
            let t = 0.5 - dx * dx - dz * dz;
            if t < 0.0 {
                0.0
            } else {
                let t = t * t;
                t * t * (GRAD[index][0] * dx + GRAD[index][1] * dz)
            }
        };
        70.0 * (corner(a, x0, z0) + corner(b, x1, z1) + corner(c, x2, z2))
    }
}
