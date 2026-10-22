//! `NoiseGeneratorImproved`, `NoiseGeneratorOctaves`, `NoiseGeneratorSimplex`, `NoiseGeneratorPerlin`.

use mc_core::{math, JavaRandom};

const GX: [f64; 16] = [1., -1., 1., -1., 1., -1., 1., -1., 0., 0., 0., 0., 1., 0., -1., 0.];
const GY: [f64; 16] = [1., 1., -1., -1., 0., 0., 0., 0., 1., -1., 1., -1., 1., -1., 1., -1.];
const GZ: [f64; 16] = [0., 0., 0., 0., 1., 1., -1., -1., 1., 1., -1., -1., 0., 1., 0., -1.];
const G2X: [f64; 16] = [1., -1., 1., -1., 1., -1., 1., -1., 0., 0., 0., 0., 1., 0., -1., 0.];
const G2Z: [f64; 16] = [0., 0., 0., 0., 1., 1., -1., -1., 1., 1., -1., -1., 0., 1., 0., -1.];

#[derive(Clone)]
pub struct Improved {
    p: [i32; 512],
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[inline]
fn grad2(h: i32, x: f64, z: f64) -> f64 {
    let i = (h & 15) as usize;
    G2X[i] * x + G2Z[i] * z
}

#[inline]
fn grad(h: i32, x: f64, y: f64, z: f64) -> f64 {
    let i = (h & 15) as usize;
    GX[i] * x + GY[i] * y + GZ[i] * z
}

#[inline]
fn floor_int(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 {
        i - 1
    } else {
        i
    }
}

#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn permutation(r: &mut JavaRandom) -> (f64, f64, f64, [i32; 512]) {
    let x = r.next_double() * 256.0;
    let y = r.next_double() * 256.0;
    let z = r.next_double() * 256.0;
    let mut p = [0i32; 512];
    for (i, v) in p.iter_mut().take(256).enumerate() {
        *v = i as i32;
    }
    for l in 0..256 {
        let j = r.next_int_bounded(256 - l as i32) as usize + l;
        p.swap(l, j);
        p[l + 256] = p[l];
    }
    (x, y, z, p)
}

impl Improved {
    pub fn new(r: &mut JavaRandom) -> Self {
        let (x, y, z, p) = permutation(r);
        Self { p, x, y, z }
    }

    /// `populateNoiseArray`
    #[allow(clippy::too_many_arguments)]
    pub fn populate(&self, out: &mut [f64], xo: f64, yo: f64, zo: f64, xs: usize, ys: usize, zs: usize, xsc: f64, ysc: f64, zsc: f64, scale: f64) {
        let p = &self.p;
        if ys == 1 {
            let mut l5 = 0;
            let d16 = 1.0 / scale;
            for j2 in 0..xs {
                let mut d17 = xo + j2 as f64 * xsc + self.x;
                let i6 = floor_int(d17);
                let k2 = (i6 & 255) as usize;
                d17 -= i6 as f64;
                let d18 = fade(d17);
                for j6 in 0..zs {
                    let mut d19 = zo + j6 as f64 * zsc + self.z;
                    let k6 = floor_int(d19);
                    let l6 = k6 & 255;
                    d19 -= k6 as f64;
                    let d20 = fade(d19);
                    let i5 = p[k2];
                    let j5 = (p[i5 as usize] + l6) as usize;
                    let j = p[k2 + 1];
                    let k5 = (p[j as usize] + l6) as usize;
                    let d14 = lerp(d18, grad2(p[j5], d17, d19), grad(p[k5], d17 - 1.0, 0.0, d19));
                    let d15 = lerp(d18, grad(p[j5 + 1], d17, 0.0, d19 - 1.0), grad(p[k5 + 1], d17 - 1.0, 0.0, d19 - 1.0));
                    let d21 = lerp(d20, d14, d15);
                    out[l5] += d21 * d16;
                    l5 += 1;
                }
            }
        } else {
            let mut i = 0;
            let d0 = 1.0 / scale;
            let mut k = -1;
            let (mut d1, mut d2, mut d3, mut d4) = (0.0, 0.0, 0.0, 0.0);
            for l2 in 0..xs {
                let mut d5 = xo + l2 as f64 * xsc + self.x;
                let i3 = floor_int(d5);
                let j3 = (i3 & 255) as usize;
                d5 -= i3 as f64;
                let d6 = fade(d5);
                for k3 in 0..zs {
                    let mut d7 = zo + k3 as f64 * zsc + self.z;
                    let l3 = floor_int(d7);
                    let i4 = l3 & 255;
                    d7 -= l3 as f64;
                    let d8 = fade(d7);
                    for j4 in 0..ys {
                        let mut d9 = yo + j4 as f64 * ysc + self.y;
                        let k4 = floor_int(d9);
                        let l4 = k4 & 255;
                        d9 -= k4 as f64;
                        let d10 = fade(d9);
                        if j4 == 0 || l4 != k {
                            k = l4;
                            let l = (p[j3] + l4) as usize;
                            let i1 = (p[l] + i4) as usize;
                            let j1 = (p[l + 1] + i4) as usize;
                            let k1 = (p[j3 + 1] + l4) as usize;
                            let l1 = (p[k1] + i4) as usize;
                            let i2 = (p[k1 + 1] + i4) as usize;
                            d1 = lerp(d6, grad(p[i1], d5, d9, d7), grad(p[l1], d5 - 1.0, d9, d7));
                            d2 = lerp(d6, grad(p[j1], d5, d9 - 1.0, d7), grad(p[i2], d5 - 1.0, d9 - 1.0, d7));
                            d3 = lerp(d6, grad(p[i1 + 1], d5, d9, d7 - 1.0), grad(p[l1 + 1], d5 - 1.0, d9, d7 - 1.0));
                            d4 = lerp(d6, grad(p[j1 + 1], d5, d9 - 1.0, d7 - 1.0), grad(p[i2 + 1], d5 - 1.0, d9 - 1.0, d7 - 1.0));
                        }
                        let d11 = lerp(d10, d1, d2);
                        let d12 = lerp(d10, d3, d4);
                        let d13 = lerp(d8, d11, d12);
                        out[i] += d13 * d0;
                        i += 1;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct Octaves {
    gens: Vec<Improved>,
}

impl Octaves {
    pub fn new(r: &mut JavaRandom, n: usize) -> Self {
        Self { gens: (0..n).map(|_| Improved::new(r)).collect() }
    }

    /// `generateNoiseOctaves` (3D)
    #[allow(clippy::too_many_arguments)]
    pub fn generate(&self, out: &mut Vec<f64>, xo: i32, yo: i32, zo: i32, xs: usize, ys: usize, zs: usize, xsc: f64, ysc: f64, zsc: f64) {
        out.clear();
        out.resize(xs * ys * zs, 0.0);
        let mut d3 = 1.0f64;
        for g in &self.gens {
            let mut d0 = xo as f64 * d3 * xsc;
            let d1 = yo as f64 * d3 * ysc;
            let mut d2 = zo as f64 * d3 * zsc;
            let mut k = math::floor_double_long(d0);
            let mut l = math::floor_double_long(d2);
            d0 -= k as f64;
            d2 -= l as f64;
            k %= 16_777_216;
            l %= 16_777_216;
            d0 += k as f64;
            d2 += l as f64;
            g.populate(out, d0, d1, d2, xs, ys, zs, xsc * d3, ysc * d3, zsc * d3, d3);
            d3 /= 2.0;
        }
    }

    /// The 2D overload: `generateNoiseOctaves(arr, x, 10, z, xs, 1, zs, xsc, 1, zsc)`.
    pub fn generate_2d(&self, out: &mut Vec<f64>, xo: i32, zo: i32, xs: usize, zs: usize, xsc: f64, zsc: f64) {
        self.generate(out, xo, 10, zo, xs, 1, zs, xsc, 1.0, zsc);
    }
}

const GRAD3: [[i32; 3]; 12] = [[1, 1, 0], [-1, 1, 0], [1, -1, 0], [-1, -1, 0], [1, 0, 1], [-1, 0, 1], [1, 0, -1], [-1, 0, -1], [0, 1, 1], [0, -1, 1], [0, 1, -1], [0, -1, -1]];

#[derive(Clone)]
pub struct Simplex {
    p: [i32; 512],
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[inline]
fn sfloor(v: f64) -> i32 {
    if v > 0.0 {
        v as i32
    } else {
        (v as i32).wrapping_sub(1)
    }
}

impl Simplex {
    pub fn new(r: &mut JavaRandom) -> Self {
        let (x, y, z, p) = permutation(r);
        Self { p, x, y, z }
    }

    #[inline]
    fn corners(&self, x: f64, z: f64) -> f64 {
        let sqrt3 = 3.0f64.sqrt();
        let f = 0.5 * (sqrt3 - 1.0);
        let g = (3.0 - sqrt3) / 6.0;
        let d4 = (x + z) * f;
        let i = sfloor(x + d4);
        let j = sfloor(z + d4);
        let d6 = i.wrapping_add(j) as f64 * g;
        let d9 = x - (i as f64 - d6);
        let d10 = z - (j as f64 - d6);
        let (k, l) = if d9 > d10 { (1, 0) } else { (0, 1) };
        let d11 = d9 - k as f64 + g;
        let d12 = d10 - l as f64 + g;
        let d13 = d9 - 1.0 + 2.0 * g;
        let d14 = d10 - 1.0 + 2.0 * g;
        let i1 = (i & 255) as usize;
        let j1 = (j & 255) as usize;
        let p = &self.p;
        let k1 = (p[i1 + p[j1] as usize] % 12) as usize;
        let l1 = (p[i1 + k + p[j1 + l] as usize] % 12) as usize;
        let i2 = (p[i1 + 1 + p[j1 + 1] as usize] % 12) as usize;
        let c = |gi: usize, dx: f64, dz: f64| {
            let t = 0.5 - dx * dx - dz * dz;
            if t < 0.0 {
                0.0
            } else {
                let t = t * t;
                t * t * (GRAD3[gi][0] as f64 * dx + GRAD3[gi][1] as f64 * dz)
            }
        };
        70.0 * (c(k1, d9, d10) + c(l1, d11, d12) + c(i2, d13, d14))
    }

    /// `func_151605_a`
    pub fn value(&self, x: f64, z: f64) -> f64 {
        self.corners(x, z)
    }

    /// `func_151606_a`
    #[allow(clippy::too_many_arguments)]
    pub fn add(&self, out: &mut [f64], xo: f64, zo: f64, xs: usize, zs: usize, xsc: f64, zsc: f64, amp: f64) {
        let mut i = 0;
        for j in 0..zs {
            let d0 = (zo + j as f64) * zsc + self.y;
            for k in 0..xs {
                let d1 = (xo + k as f64) * xsc + self.x;
                out[i] += self.corners(d1, d0) * amp;
                i += 1;
            }
        }
    }
}

#[derive(Clone)]
pub struct Perlin {
    gens: Vec<Simplex>,
}

impl Perlin {
    pub fn new(r: &mut JavaRandom, n: usize) -> Self {
        Self { gens: (0..n).map(|_| Simplex::new(r)).collect() }
    }

    /// `func_151601_a`
    pub fn value(&self, x: f64, z: f64) -> f64 {
        let mut d0 = 0.0;
        let mut d1 = 1.0;
        for g in &self.gens {
            d0 += g.value(x * d1, z * d1) / d1;
            d1 /= 2.0;
        }
        d0
    }

    /// `func_151599_a`
    #[allow(clippy::too_many_arguments)]
    pub fn generate(&self, out: &mut Vec<f64>, xo: f64, zo: f64, xs: usize, zs: usize, xsc: f64, zsc: f64, p12: f64) {
        self.generate_lac(out, xo, zo, xs, zs, xsc, zsc, p12, 0.5)
    }

    /// `func_151600_a`
    #[allow(clippy::too_many_arguments)]
    pub fn generate_lac(&self, out: &mut Vec<f64>, xo: f64, zo: f64, xs: usize, zs: usize, xsc: f64, zsc: f64, p12: f64, p14: f64) {
        if out.len() >= xs * zs {
            out.iter_mut().for_each(|v| *v = 0.0);
        } else {
            out.clear();
            out.resize(xs * zs, 0.0);
        }
        let mut d1 = 1.0;
        let mut d0 = 1.0;
        for g in &self.gens {
            g.add(out, xo, zo, xs, zs, xsc * d0 * d1, zsc * d0 * d1, 0.55 / d1);
            d0 *= p12;
            d1 *= p14;
        }
    }
}
