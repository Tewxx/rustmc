//! Port of `net.minecraft.util.MathHelper`.

use std::sync::OnceLock;

const SIN_SCALE: f32 = 10430.378;

fn sin_table() -> &'static [f32; 65536] {
    static TABLE: OnceLock<Box<[f32; 65536]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Box::new([0.0f32; 65536]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
        }
        t
    })
}

struct AtanTables {
    asin: [f64; 257],
    cos: [f64; 257],
}

fn atan_tables() -> &'static AtanTables {
    static T: OnceLock<AtanTables> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = AtanTables { asin: [0.0; 257], cos: [0.0; 257] };
        for j in 0..257 {
            let d1 = (j as f64 / 256.0).asin();
            t.cos[j] = d1.cos();
            t.asin[j] = d1;
        }
        t
    })
}

const ATAN_FRAC_BIAS: u64 = 4805340802404319232;

/// Table sine. Movement and rotation rely on this, not real `sin`.
#[inline]
pub fn sin(v: f32) -> f32 {
    sin_table()[((v * SIN_SCALE) as i32 & 65535) as usize]
}

#[inline]
pub fn cos(v: f32) -> f32 {
    sin_table()[((v * SIN_SCALE + 16384.0) as i32 & 65535) as usize]
}

#[inline]
pub fn sqrt_float(v: f32) -> f32 {
    (v as f64).sqrt() as f32
}

#[inline]
pub fn sqrt_double(v: f64) -> f32 {
    v.sqrt() as f32
}

#[inline]
pub fn floor_float(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

#[inline]
pub fn truncate_double_to_int(v: f64) -> i32 {
    ((v + 1024.0) as i32).wrapping_sub(1024)
}

#[inline]
pub fn floor_double(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 { i - 1 } else { i }
}

#[inline]
pub fn floor_double_long(v: f64) -> i64 {
    let i = v as i64;
    if v < i as f64 { i - 1 } else { i }
}

#[inline]
pub fn abs(v: f32) -> f32 {
    if v >= 0.0 { v } else { -v }
}

#[inline]
pub fn abs_int(v: i32) -> i32 {
    if v >= 0 { v } else { v.wrapping_neg() }
}

#[inline]
pub fn ceiling_float_int(v: f32) -> i32 {
    let i = v as i32;
    if v > i as f32 { i + 1 } else { i }
}

#[inline]
pub fn ceiling_double_int(v: f64) -> i32 {
    let i = v as i32;
    if v > i as f64 { i + 1 } else { i }
}

#[inline]
pub fn clamp_int(n: i32, min: i32, max: i32) -> i32 {
    if n < min { min } else if n > max { max } else { n }
}

#[inline]
pub fn clamp_float(n: f32, min: f32, max: f32) -> f32 {
    if n < min { min } else if n > max { max } else { n }
}

#[inline]
pub fn clamp_double(n: f64, min: f64, max: f64) -> f64 {
    if n < min { min } else if n > max { max } else { n }
}

pub fn denormalize_clamp(lower: f64, upper: f64, slide: f64) -> f64 {
    if slide < 0.0 {
        lower
    } else if slide > 1.0 {
        upper
    } else {
        lower + (upper - lower) * slide
    }
}

pub fn abs_max(a: f64, b: f64) -> f64 {
    let a = if a < 0.0 { -a } else { a };
    let b = if b < 0.0 { -b } else { b };
    if a > b { a } else { b }
}

pub fn bucket_int(i: i32, size: i32) -> i32 {
    if i < 0 { -((-i - 1) / size) - 1 } else { i / size }
}

pub fn epsilon_equals(a: f32, b: f32) -> bool {
    abs(b - a) < 1.0e-5
}

pub fn normalize_angle(a: i32, b: i32) -> i32 {
    (a % b + b) % b
}

pub fn wrap_angle_to_180_float(v: f32) -> f32 {
    let mut v = v % 360.0;
    if v >= 180.0 {
        v -= 360.0;
    }
    if v < -180.0 {
        v += 360.0;
    }
    v
}

pub fn wrap_angle_to_180_double(v: f64) -> f64 {
    let mut v = v % 360.0;
    if v >= 180.0 {
        v -= 360.0;
    }
    if v < -180.0 {
        v += 360.0;
    }
    v
}

pub fn round_up_to_power_of_two(v: i32) -> i32 {
    let mut i = v.wrapping_sub(1);
    i |= i >> 1;
    i |= i >> 2;
    i |= i >> 4;
    i |= i >> 8;
    i |= i >> 16;
    i.wrapping_add(1)
}

fn is_power_of_two(v: i32) -> bool {
    v != 0 && (v & v.wrapping_sub(1)) == 0
}

const DE_BRUIJN: [i32; 32] = [
    0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16, 7, 26, 12,
    18, 6, 11, 5, 10, 9,
];

fn log_base_two_de_bruijn(v: i32) -> i32 {
    let v = if is_power_of_two(v) { v } else { round_up_to_power_of_two(v) };
    DE_BRUIJN[(((v as i64).wrapping_mul(125613361) >> 27) as i32 & 31) as usize]
}

pub fn calculate_log_base_two(v: i32) -> i32 {
    log_base_two_de_bruijn(v) - if is_power_of_two(v) { 0 } else { 1 }
}

pub fn round_up(n: i32, mut m: i32) -> i32 {
    if m == 0 {
        return 0;
    }
    if n == 0 {
        return m;
    }
    if n < 0 {
        m *= -1;
    }
    let i = n % m;
    if i == 0 { n } else { n + m - i }
}

/// `func_180181_b`: pack RGB components.
pub fn rgb(r: i32, g: i32, b: i32) -> i32 {
    let v = (r << 8).wrapping_add(g);
    (v << 8).wrapping_add(b)
}

/// `func_180183_b`
pub fn rgb_float(r: f32, g: f32, b: f32) -> i32 {
    rgb(floor_float(r * 255.0), floor_float(g * 255.0), floor_float(b * 255.0))
}

/// `func_180188_d`: multiply two packed colors, keeping the first one's alpha.
pub fn multiply_color(a: i32, b: i32) -> i32 {
    let i = (a & 0xFF0000) >> 16;
    let j = (b & 0xFF0000) >> 16;
    let k = (a & 0xFF00) >> 8;
    let l = (b & 0xFF00) >> 8;
    let i1 = a & 0xFF;
    let j1 = b & 0xFF;
    let k1 = (i as f32 * j as f32 / 255.0) as i32;
    let l1 = (k as f32 * l as f32 / 255.0) as i32;
    let i2 = (i1 as f32 * j1 as f32 / 255.0) as i32;
    (a & -16777216) | (k1 << 16) | (l1 << 8) | i2
}

/// `func_181162_h`: fractional part.
pub fn frac(v: f64) -> f64 {
    v - v.floor()
}

/// `getCoordinateRandom`: per-position hash used for model variants and block offsets.
pub fn coordinate_random(x: i32, y: i32, z: i32) -> i64 {
    let i = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ y as i64;
    i.wrapping_mul(i).wrapping_mul(42317861).wrapping_add(i.wrapping_mul(11))
}

/// `func_181160_c`: inverse lerp.
pub fn inverse_lerp(v: f64, min: f64, max: f64) -> f64 {
    (v - min) / (max - min)
}

/// `func_181161_i`: fast inverse square root.
pub fn fast_inv_sqrt(v: f64) -> f64 {
    let half = 0.5 * v;
    let i = 6910469410427058090i64.wrapping_sub((v.to_bits() as i64) >> 1);
    let x = f64::from_bits(i as u64);
    x * (1.5 - half * x * x)
}

/// Table-based `atan2(y, x)`, used for entity yaw.
pub fn atan2(mut y: f64, mut x: f64) -> f64 {
    let d0 = x * x + y * y;
    if d0.is_nan() {
        return f64::NAN;
    }
    let neg_y = y < 0.0;
    if neg_y {
        y = -y;
    }
    let neg_x = x < 0.0;
    if neg_x {
        x = -x;
    }
    let swap = y > x;
    if swap {
        std::mem::swap(&mut x, &mut y);
    }
    let inv = fast_inv_sqrt(d0);
    x *= inv;
    y *= inv;
    let bias = f64::from_bits(ATAN_FRAC_BIAS);
    let d2 = bias + y;
    let i = d2.to_bits() as i32 as usize;
    let t = atan_tables();
    let d3 = t.asin[i];
    let d4 = t.cos[i];
    let d5 = d2 - bias;
    let d6 = y * d4 - x * d5;
    let d7 = (6.0 + d6 * d6) * d6 * 0.16666666666666666;
    let mut d8 = d3 + d7;
    if swap {
        d8 = std::f64::consts::FRAC_PI_2 - d8;
    }
    if neg_x {
        d8 = std::f64::consts::PI - d8;
    }
    if neg_y {
        d8 = -d8;
    }
    d8
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> i32 {
    let i = (h * 6.0) as i32 % 6;
    let f = h * 6.0 - i as f32;
    let f1 = v * (1.0 - s);
    let f2 = v * (1.0 - f * s);
    let f3 = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i {
        0 => (v, f3, f1),
        1 => (f2, v, f1),
        2 => (f1, v, f3),
        3 => (f1, f2, v),
        4 => (f3, f1, v),
        5 => (v, f1, f2),
        _ => panic!("Something went wrong when converting from HSV to RGB. Input was {h}, {s}, {v}"),
    };
    let j = clamp_int((r * 255.0) as i32, 0, 255);
    let k = clamp_int((g * 255.0) as i32, 0, 255);
    let l = clamp_int((b * 255.0) as i32, 0, 255);
    (j << 16) | (k << 8) | l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_table_indexing() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        let x = 1.2345f32;
        assert!((sin(x) - x.sin()).abs() < 1e-3);
    }

    #[test]
    fn floors_and_ceils() {
        assert_eq!(floor_double(-0.5), -1);
        assert_eq!(floor_double(2.0), 2);
        assert_eq!(floor_float(-1.0), -1);
        assert_eq!(ceiling_double_int(1.0001), 2);
        assert_eq!(ceiling_double_int(-1.5), -1);
        assert_eq!(floor_double(f64::NAN), 0);
    }

    #[test]
    fn log_base_two() {
        assert_eq!(calculate_log_base_two(1), 0);
        assert_eq!(calculate_log_base_two(37), 5);
        assert_eq!(calculate_log_base_two(round_up_to_power_of_two(30_000_000)), 25);
    }

    #[test]
    fn wrap_angles() {
        assert_eq!(wrap_angle_to_180_float(190.0), -170.0);
        assert_eq!(wrap_angle_to_180_float(-190.0), 170.0);
        assert_eq!(wrap_angle_to_180_double(540.0), -180.0);
    }

    #[test]
    fn atan2_close_to_real() {
        for &(y, x) in &[(1.0, 1.0), (-3.0, 2.0), (0.5, -7.0), (-1.0, -1.0), (0.0, 1.0)] {
            assert!((atan2(y, x) - f64::atan2(y, x)).abs() < 1e-6, "{y},{x}");
        }
    }

    #[test]
    fn hsv() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), 0xFF0000);
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), 0xFFFFFF);
    }
}
