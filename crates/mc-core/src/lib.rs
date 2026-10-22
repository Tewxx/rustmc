//! Core math and value types, bit-exact with Minecraft 1.8.9.
//!
//! Java semantics that matter here and differ from Rust defaults:
//! - `Math.min`/`Math.max` propagate NaN; Rust's `f64::min`/`max` ignore it. Use [`jmin`]/[`jmax`].
//! - Float-to-int casts saturate and map NaN to 0 in both languages, so `as` is fine.

pub mod aabb;
pub mod block_pos;
pub mod facing;
pub mod math;
pub mod nbt;
pub mod random;
pub mod strict_math;
pub mod vec3;

pub use aabb::Aabb;
pub use block_pos::BlockPos;
pub use facing::{Axis, Facing};
pub use random::JavaRandom;
pub use vec3::Vec3;

/// Java `Math.min(double, double)`: NaN-propagating, and -0.0 < 0.0.
#[inline]
pub fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a <= b { a } else { b }
}

/// Java `Math.max(double, double)`.
#[inline]
pub fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_positive() { a } else { b };
    }
    if a >= b { a } else { b }
}
