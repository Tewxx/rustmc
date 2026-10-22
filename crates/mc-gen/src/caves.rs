//! `MapGenCaves` and `MapGenRavine` (with `MapGenBase.generate`).

use crate::terrain::{self, Generator, Primer, AIR, LAVA};
use mc_core::{math, JavaRandom};

const PI: f32 = std::f32::consts::PI;
const RANGE: i32 = 8;

#[inline]
fn id(r: u16) -> u16 {
    r >> 4
}

/// `MapGenBase.generate`: visits every chunk within `range` with a per-chunk seed.
fn for_each_source(seed: i64, x: i32, z: i32, mut f: impl FnMut(&mut JavaRandom, i32, i32)) {
    let mut rand = JavaRandom::new(seed);
    let j = rand.next_long();
    let k = rand.next_long();
    for l in x - RANGE..=x + RANGE {
        for i1 in z - RANGE..=z + RANGE {
            rand.set_seed((l as i64).wrapping_mul(j) ^ (i1 as i64).wrapping_mul(k) ^ seed);
            f(&mut rand, l, i1);
        }
    }
}

/// The water scan shared by both carvers: true if any water touches the carve box shell.
#[allow(clippy::too_many_arguments)]
fn touches_water(p: &Primer, k2: i32, k: i32, l2: i32, l: i32, i3: i32, i1: i32) -> bool {
    for j1 in k2..k {
        for k1 in i3..i1 {
            let mut l1 = l + 1;
            while l1 >= l2 - 1 {
                if (0..256).contains(&l1) {
                    let b = id(p.get(j1, l1, k1));
                    if b == 8 || b == 9 {
                        return true;
                    }
                    if l1 != l2 - 1 && j1 != k2 && j1 != k - 1 && k1 != i3 && k1 != i1 - 1 {
                        l1 = l2;
                    }
                }
                l1 -= 1;
            }
        }
    }
    false
}

#[derive(Default)]
pub struct Caves;

impl Caves {
    pub fn generate(&self, g: &mut Generator, x: i32, z: i32, p: &mut Primer) {
        for_each_source(g.seed, x, z, |rand, cx, cz| Self::recursive(g, rand, cx, cz, x, z, p));
    }

    #[allow(clippy::too_many_arguments)]
    fn recursive(g: &mut Generator, rand: &mut JavaRandom, cx: i32, cz: i32, ox: i32, oz: i32, p: &mut Primer) {
        let a = rand.next_int_bounded(15) + 1;
        let b = rand.next_int_bounded(a) + 1;
        let mut i = rand.next_int_bounded(b);
        if rand.next_int_bounded(7) != 0 {
            i = 0;
        }
        for _ in 0..i {
            let d0 = (cx * 16 + rand.next_int_bounded(16)) as f64;
            let r = rand.next_int_bounded(120) + 8;
            let d1 = rand.next_int_bounded(r) as f64;
            let d2 = (cz * 16 + rand.next_int_bounded(16)) as f64;
            let mut k = 1;
            if rand.next_int_bounded(4) == 0 {
                let seed = rand.next_long();
                // func_180703_a: a large room.
                let width = 1.0 + rand.next_float() * 6.0;
                Self::tunnel(g, seed, ox, oz, p, d0, d1, d2, width, 0.0, 0.0, -1, -1, 0.5);
                k += rand.next_int_bounded(4);
            }
            for _ in 0..k {
                let f = rand.next_float() * PI * 2.0;
                let f1 = (rand.next_float() - 0.5) * 2.0 / 8.0;
                let mut f2 = rand.next_float() * 2.0 + rand.next_float();
                if rand.next_int_bounded(10) == 0 {
                    f2 *= rand.next_float() * rand.next_float() * 3.0 + 1.0;
                }
                let seed = rand.next_long();
                Self::tunnel(g, seed, ox, oz, p, d0, d1, d2, f2, f, f1, 0, 0, 1.0);
            }
        }
    }

    /// `func_180702_a`
    #[allow(clippy::too_many_arguments)]
    fn tunnel(
        g: &mut Generator, seed: i64, cx: i32, cz: i32, p: &mut Primer, mut x: f64, mut y: f64, mut z: f64,
        width: f32, mut yaw: f32, mut pitch: f32, mut step: i32, mut steps: i32, vscale: f64,
    ) {
        let d0 = (cx * 16 + 8) as f64;
        let d1 = (cz * 16 + 8) as f64;
        let mut f = 0.0f32;
        let mut f1 = 0.0f32;
        let mut random = JavaRandom::new(seed);
        if steps <= 0 {
            let i = RANGE * 16 - 16;
            steps = i - random.next_int_bounded(i / 4);
        }
        let mut room = false;
        if step == -1 {
            step = steps / 2;
            room = true;
        }
        let j = random.next_int_bounded(steps / 2) + steps / 4;
        let steep = random.next_int_bounded(6) == 0;
        while step < steps {
            let d2 = 1.5 + (math::sin(step as f32 * PI / steps as f32) * width * 1.0) as f64;
            let d3 = d2 * vscale;
            let f2 = math::cos(pitch);
            let f3 = math::sin(pitch);
            x += (math::cos(yaw) * f2) as f64;
            y += f3 as f64;
            z += (math::sin(yaw) * f2) as f64;
            pitch *= if steep { 0.92 } else { 0.7 };
            pitch += f1 * 0.1;
            yaw += f * 0.1;
            f1 *= 0.9;
            f *= 0.75;
            f1 += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
            f += (random.next_float() - random.next_float()) * random.next_float() * 4.0;

            if !room && step == j && width > 1.0 && steps > 0 {
                let s1 = random.next_long();
                let w1 = random.next_float() * 0.5 + 0.5;
                Self::tunnel(g, s1, cx, cz, p, x, y, z, w1, yaw - PI / 2.0, pitch / 3.0, step, steps, 1.0);
                let s2 = random.next_long();
                let w2 = random.next_float() * 0.5 + 0.5;
                Self::tunnel(g, s2, cx, cz, p, x, y, z, w2, yaw + PI / 2.0, pitch / 3.0, step, steps, 1.0);
                return;
            }

            if room || random.next_int_bounded(4) != 0 {
                let d4 = x - d0;
                let d5 = z - d1;
                let d6 = (steps - step) as f64;
                let d7 = (width + 2.0 + 16.0) as f64;
                if d4 * d4 + d5 * d5 - d6 * d6 > d7 * d7 {
                    return;
                }
                if x >= d0 - 16.0 - d2 * 2.0 && z >= d1 - 16.0 - d2 * 2.0 && x <= d0 + 16.0 + d2 * 2.0 && z <= d1 + 16.0 + d2 * 2.0 {
                    let k2 = (math::floor_double(x - d2) - cx * 16 - 1).max(0);
                    let k = (math::floor_double(x + d2) - cx * 16 + 1).min(16);
                    let l2 = (math::floor_double(y - d3) - 1).max(1);
                    let l = (math::floor_double(y + d3) + 1).min(248);
                    let i3 = (math::floor_double(z - d2) - cz * 16 - 1).max(0);
                    let i1 = (math::floor_double(z + d2) - cz * 16 + 1).min(16);
                    if !touches_water(p, k2, k, l2, l, i3, i1) {
                        for j3 in k2..k {
                            let d10 = ((j3 + cx * 16) as f64 + 0.5 - x) / d2;
                            for i2 in i3..i1 {
                                let d8 = ((i2 + cz * 16) as f64 + 0.5 - z) / d2;
                                let mut grass = false;
                                if d10 * d10 + d8 * d8 < 1.0 {
                                    let mut j2 = l;
                                    while j2 > l2 {
                                        let d9 = ((j2 - 1) as f64 + 0.5 - y) / d3;
                                        if d9 > -0.7 && d10 * d10 + d9 * d9 + d8 * d8 < 1.0 {
                                            let s1 = p.get(j3, j2, i2);
                                            let s2 = p.get(j3, j2 + 1, i2);
                                            if id(s1) == 2 || id(s1) == 110 {
                                                grass = true;
                                            }
                                            if Self::replaceable(s1, s2) {
                                                if j2 - 1 < 10 {
                                                    p.set(j3, j2, i2, LAVA);
                                                } else {
                                                    p.set(j3, j2, i2, AIR);
                                                    if id(s2) == 12 {
                                                        p.set(j3, j2 + 1, i2, if s2 & 15 == 1 { terrain::RED_SANDSTONE } else { terrain::SANDSTONE });
                                                    }
                                                    if grass && id(p.get(j3, j2 - 1, i2)) == 3 {
                                                        let b = g.biome_at(j3 + cx * 16, i2 + cz * 16);
                                                        // `topBlock.getBlock().getDefaultState()`
                                                        p.set(j3, j2 - 1, i2, g.top[b as usize] & !15);
                                                    }
                                                }
                                            }
                                        }
                                        j2 -= 1;
                                    }
                                }
                            }
                        }
                        if room {
                            break;
                        }
                    }
                }
            }
            step += 1;
        }
    }

    /// `func_175793_a`
    fn replaceable(s: u16, above: u16) -> bool {
        match id(s) {
            1 | 3 | 2 | 172 | 159 | 24 | 179 | 110 | 78 => true,
            12 | 13 => !matches!(id(above), 8 | 9),
            _ => false,
        }
    }
}

#[derive(Default)]
pub struct Ravines;

impl Ravines {
    pub fn generate(&self, g: &mut Generator, x: i32, z: i32, p: &mut Primer) {
        let mut widths = [0f32; 1024];
        for_each_source(g.seed, x, z, |rand, cx, cz| {
            if rand.next_int_bounded(50) == 0 {
                let d0 = (cx * 16 + rand.next_int_bounded(16)) as f64;
                let r = rand.next_int_bounded(40) + 8;
                let d1 = (rand.next_int_bounded(r) + 20) as f64;
                let d2 = (cz * 16 + rand.next_int_bounded(16)) as f64;
                let f = rand.next_float() * PI * 2.0;
                let f1 = (rand.next_float() - 0.5) * 2.0 / 8.0;
                let f2 = (rand.next_float() * 2.0 + rand.next_float()) * 2.0;
                let seed = rand.next_long();
                Self::ravine(g, &mut widths, seed, x, z, p, d0, d1, d2, f2, f, f1, 0, 0, 3.0);
            }
        });
    }

    /// `func_180707_a`
    #[allow(clippy::too_many_arguments)]
    fn ravine(
        g: &mut Generator, widths: &mut [f32; 1024], seed: i64, cx: i32, cz: i32, p: &mut Primer,
        mut x: f64, mut y: f64, mut z: f64, width: f32, mut yaw: f32, mut pitch: f32, mut step: i32, mut steps: i32,
        vscale: f64,
    ) {
        let mut random = JavaRandom::new(seed);
        let d0 = (cx * 16 + 8) as f64;
        let d1 = (cz * 16 + 8) as f64;
        let mut f = 0.0f32;
        let mut f1 = 0.0f32;
        if steps <= 0 {
            let i = RANGE * 16 - 16;
            steps = i - random.next_int_bounded(i / 4);
        }
        let mut room = false;
        if step == -1 {
            step = steps / 2;
            room = true;
        }
        let mut f2 = 1.0f32;
        for j in 0..256 {
            if j == 0 || random.next_int_bounded(3) == 0 {
                f2 = 1.0 + random.next_float() * random.next_float() * 1.0;
            }
            widths[j] = f2 * f2;
        }
        while step < steps {
            let mut d9 = 1.5 + (math::sin(step as f32 * PI / steps as f32) * width * 1.0) as f64;
            let mut d2 = d9 * vscale;
            d9 *= random.next_float() as f64 * 0.25 + 0.75;
            d2 *= random.next_float() as f64 * 0.25 + 0.75;
            let f3 = math::cos(pitch);
            let f4 = math::sin(pitch);
            x += (math::cos(yaw) * f3) as f64;
            y += f4 as f64;
            z += (math::sin(yaw) * f3) as f64;
            pitch *= 0.7;
            pitch += f1 * 0.05;
            yaw += f * 0.05;
            f1 *= 0.8;
            f *= 0.5;
            f1 += (random.next_float() - random.next_float()) * random.next_float() * 2.0;
            f += (random.next_float() - random.next_float()) * random.next_float() * 4.0;

            if room || random.next_int_bounded(4) != 0 {
                let d3 = x - d0;
                let d4 = z - d1;
                let d5 = (steps - step) as f64;
                let d6 = (width + 2.0 + 16.0) as f64;
                if d3 * d3 + d4 * d4 - d5 * d5 > d6 * d6 {
                    return;
                }
                if x >= d0 - 16.0 - d9 * 2.0 && z >= d1 - 16.0 - d9 * 2.0 && x <= d0 + 16.0 + d9 * 2.0 && z <= d1 + 16.0 + d9 * 2.0 {
                    let k2 = (math::floor_double(x - d9) - cx * 16 - 1).max(0);
                    let k = (math::floor_double(x + d9) - cx * 16 + 1).min(16);
                    let l2 = (math::floor_double(y - d2) - 1).max(1);
                    let l = (math::floor_double(y + d2) + 1).min(248);
                    let i3 = (math::floor_double(z - d9) - cz * 16 - 1).max(0);
                    let i1 = (math::floor_double(z + d9) - cz * 16 + 1).min(16);
                    if !touches_water(p, k2, k, l2, l, i3, i1) {
                        for j3 in k2..k {
                            let d10 = ((j3 + cx * 16) as f64 + 0.5 - x) / d9;
                            for i2 in i3..i1 {
                                let d7 = ((i2 + cz * 16) as f64 + 0.5 - z) / d9;
                                let mut grass = false;
                                if d10 * d10 + d7 * d7 < 1.0 {
                                    let mut j2 = l;
                                    while j2 > l2 {
                                        let d8 = ((j2 - 1) as f64 + 0.5 - y) / d2;
                                        if (d10 * d10 + d7 * d7) * widths[(j2 - 1) as usize] as f64 + d8 * d8 / 6.0 < 1.0 {
                                            let s1 = p.get(j3, j2, i2);
                                            if id(s1) == 2 {
                                                grass = true;
                                            }
                                            if matches!(id(s1), 1 | 2 | 3) {
                                                if j2 - 1 < 10 {
                                                    p.set(j3, j2, i2, terrain::FLOWING_LAVA);
                                                } else {
                                                    p.set(j3, j2, i2, AIR);
                                                    if grass && id(p.get(j3, j2 - 1, i2)) == 3 {
                                                        let b = g.biome_at(j3 + cx * 16, i2 + cz * 16);
                                                        p.set(j3, j2 - 1, i2, g.top[b as usize]);
                                                    }
                                                }
                                            }
                                        }
                                        j2 -= 1;
                                    }
                                }
                            }
                        }
                        if room {
                            break;
                        }
                    }
                }
            }
            step += 1;
        }
    }
}
