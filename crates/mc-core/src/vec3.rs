//! Port of `net.minecraft.util.Vec3`.

use crate::math;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    /// Like vanilla, normalizes -0.0 to 0.0.
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        // `v == -0.0` is also true for +0.0, so this maps both zeros to +0.0.
        let fix = |v: f64| if v == 0.0 { 0.0 } else { v };
        Self { x: fix(x), y: fix(y), z: fix(z) }
    }

    pub fn subtract_reverse(self, v: Vec3) -> Vec3 {
        Vec3::new(v.x - self.x, v.y - self.y, v.z - self.z)
    }

    pub fn normalize(self) -> Vec3 {
        let d = math::sqrt_double(self.x * self.x + self.y * self.y + self.z * self.z) as f64;
        if d < 1.0e-4 {
            Vec3::new(0.0, 0.0, 0.0)
        } else {
            Vec3::new(self.x / d, self.y / d, self.z / d)
        }
    }

    pub fn dot(self, v: Vec3) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn cross(self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.y * v.z - self.z * v.y,
            self.z * v.x - self.x * v.z,
            self.x * v.y - self.y * v.x,
        )
    }

    pub fn subtract(self, v: Vec3) -> Vec3 {
        self.add(-v.x, -v.y, -v.z)
    }

    pub fn add_vector(self, v: Vec3) -> Vec3 {
        self.add(v.x, v.y, v.z)
    }

    pub fn add(self, x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(self.x + x, self.y + y, self.z + z)
    }

    pub fn distance_to(self, v: Vec3) -> f64 {
        let (dx, dy, dz) = (v.x - self.x, v.y - self.y, v.z - self.z);
        math::sqrt_double(dx * dx + dy * dy + dz * dz) as f64
    }

    pub fn square_distance_to(self, v: Vec3) -> f64 {
        let (dx, dy, dz) = (v.x - self.x, v.y - self.y, v.z - self.z);
        dx * dx + dy * dy + dz * dz
    }

    /// `lengthVector`: note the f32 round-trip through `sqrt_double`.
    pub fn length(self) -> f64 {
        math::sqrt_double(self.x * self.x + self.y * self.y + self.z * self.z) as f64
    }

    pub fn intermediate_with_x(self, v: Vec3, x: f64) -> Option<Vec3> {
        let (d0, d1, d2) = (v.x - self.x, v.y - self.y, v.z - self.z);
        if d0 * d0 < 1.0000000116860974e-7 {
            return None;
        }
        let t = (x - self.x) / d0;
        (0.0..=1.0).contains(&t).then(|| Vec3::new(self.x + d0 * t, self.y + d1 * t, self.z + d2 * t))
    }

    pub fn intermediate_with_y(self, v: Vec3, y: f64) -> Option<Vec3> {
        let (d0, d1, d2) = (v.x - self.x, v.y - self.y, v.z - self.z);
        if d1 * d1 < 1.0000000116860974e-7 {
            return None;
        }
        let t = (y - self.y) / d1;
        (0.0..=1.0).contains(&t).then(|| Vec3::new(self.x + d0 * t, self.y + d1 * t, self.z + d2 * t))
    }

    pub fn intermediate_with_z(self, v: Vec3, z: f64) -> Option<Vec3> {
        let (d0, d1, d2) = (v.x - self.x, v.y - self.y, v.z - self.z);
        if d2 * d2 < 1.0000000116860974e-7 {
            return None;
        }
        let t = (z - self.z) / d2;
        (0.0..=1.0).contains(&t).then(|| Vec3::new(self.x + d0 * t, self.y + d1 * t, self.z + d2 * t))
    }

    pub fn rotate_pitch(self, pitch: f32) -> Vec3 {
        let c = math::cos(pitch);
        let s = math::sin(pitch);
        Vec3::new(self.x, self.y * c as f64 + self.z * s as f64, self.z * c as f64 - self.y * s as f64)
    }

    pub fn rotate_yaw(self, yaw: f32) -> Vec3 {
        let c = math::cos(yaw);
        let s = math::sin(yaw);
        Vec3::new(self.x * c as f64 + self.z * s as f64, self.y, self.z * c as f64 - self.x * s as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_zero_is_normalized() {
        assert!(Vec3::new(-0.0, 0.0, -0.0).x.is_sign_positive());
    }

    #[test]
    fn intermediate() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(2.0, 2.0, 0.0);
        assert_eq!(a.intermediate_with_x(b, 1.0), Some(Vec3::new(1.0, 1.0, 0.0)));
        assert_eq!(a.intermediate_with_x(b, 3.0), None);
        assert_eq!(a.intermediate_with_z(b, 0.0), None);
    }
}
