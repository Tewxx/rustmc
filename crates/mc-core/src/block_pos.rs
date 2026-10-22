//! Port of `net.minecraft.util.BlockPos` / `Vec3i`.

use crate::{math, Vec3};

// 1 + log2(roundUpToPowerOfTwo(30_000_000)) = 26 bits for X and Z, 12 for Y.
const NUM_X_BITS: u32 = 26;
const NUM_Z_BITS: u32 = NUM_X_BITS;
const NUM_Y_BITS: u32 = 64 - NUM_X_BITS - NUM_Z_BITS;
const Y_SHIFT: u32 = NUM_Z_BITS;
const X_SHIFT: u32 = Y_SHIFT + NUM_Y_BITS;
const X_MASK: i64 = (1 << NUM_X_BITS) - 1;
const Y_MASK: i64 = (1 << NUM_Y_BITS) - 1;
const Z_MASK: i64 = (1 << NUM_Z_BITS) - 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const ORIGIN: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn from_f64(x: f64, y: f64, z: f64) -> Self {
        Self::new(math::floor_double(x), math::floor_double(y), math::floor_double(z))
    }

    pub fn from_vec3(v: Vec3) -> Self {
        Self::from_f64(v.x, v.y, v.z)
    }

    pub fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x.wrapping_add(dx), self.y.wrapping_add(dy), self.z.wrapping_add(dz))
    }

    pub fn up(self) -> Self {
        self.offset(0, 1, 0)
    }

    pub fn down(self) -> Self {
        self.offset(0, -1, 0)
    }

    pub fn north(self) -> Self {
        self.offset(0, 0, -1)
    }

    pub fn south(self) -> Self {
        self.offset(0, 0, 1)
    }

    pub fn west(self) -> Self {
        self.offset(-1, 0, 0)
    }

    pub fn east(self) -> Self {
        self.offset(1, 0, 0)
    }

    pub fn distance_sq(self, x: f64, y: f64, z: f64) -> f64 {
        let (dx, dy, dz) = (self.x as f64 - x, self.y as f64 - y, self.z as f64 - z);
        dx * dx + dy * dy + dz * dz
    }

    /// `toLong`: the packed position format used by the 1.8 protocol.
    pub fn to_long(self) -> i64 {
        ((self.x as i64 & X_MASK) << X_SHIFT) | ((self.y as i64 & Y_MASK) << Y_SHIFT) | (self.z as i64 & Z_MASK)
    }

    pub fn from_long(v: i64) -> Self {
        let x = (v << (64 - X_SHIFT - NUM_X_BITS)) >> (64 - NUM_X_BITS);
        let y = (v << (64 - Y_SHIFT - NUM_Y_BITS)) >> (64 - NUM_Y_BITS);
        let z = (v << (64 - NUM_Z_BITS)) >> (64 - NUM_Z_BITS);
        Self::new(x as i32, y as i32, z as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_layout_matches_vanilla_constants() {
        assert_eq!(
            NUM_X_BITS as i32,
            1 + math::calculate_log_base_two(math::round_up_to_power_of_two(30_000_000))
        );
    }

    #[test]
    fn pack_roundtrip() {
        for p in [
            BlockPos::new(0, 0, 0),
            BlockPos::new(-1, 64, -1),
            BlockPos::new(29_999_999, 255, -29_999_999),
            BlockPos::new(-30_000_000, 0, 123_456),
        ] {
            assert_eq!(BlockPos::from_long(p.to_long()), p);
        }
        assert_eq!(BlockPos::new(1, 2, 3).to_long(), (1i64 << 38) | (2 << 26) | 3);
    }

    #[test]
    fn floors_negative_coords() {
        assert_eq!(BlockPos::from_f64(-0.5, 64.9, 1.0), BlockPos::new(-1, 64, 1));
    }
}
