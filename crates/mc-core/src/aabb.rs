//! Port of `net.minecraft.util.AxisAlignedBB`. The `calculate_*_offset` functions are
//! the core of entity collision and must stay bit-exact.

use crate::{jmax, jmin, BlockPos, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

/// Which face a ray hit; matches `EnumFacing` naming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitFace {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Aabb {
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Self {
        Self {
            min_x: jmin(x1, x2),
            min_y: jmin(y1, y2),
            min_z: jmin(z1, z2),
            max_x: jmax(x1, x2),
            max_y: jmax(y1, y2),
            max_z: jmax(z1, z2),
        }
    }

    /// The `(BlockPos, BlockPos)` constructor: no min/max sorting.
    pub fn from_positions(a: BlockPos, b: BlockPos) -> Self {
        Self {
            min_x: a.x as f64,
            min_y: a.y as f64,
            min_z: a.z as f64,
            max_x: b.x as f64,
            max_y: b.y as f64,
            max_z: b.z as f64,
        }
    }

    pub fn add_coord(&self, x: f64, y: f64, z: f64) -> Aabb {
        let (mut d0, mut d1, mut d2) = (self.min_x, self.min_y, self.min_z);
        let (mut d3, mut d4, mut d5) = (self.max_x, self.max_y, self.max_z);
        if x < 0.0 {
            d0 += x;
        } else if x > 0.0 {
            d3 += x;
        }
        if y < 0.0 {
            d1 += y;
        } else if y > 0.0 {
            d4 += y;
        }
        if z < 0.0 {
            d2 += z;
        } else if z > 0.0 {
            d5 += z;
        }
        Aabb::new(d0, d1, d2, d3, d4, d5)
    }

    pub fn expand(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x - x, self.min_y - y, self.min_z - z, self.max_x + x, self.max_y + y, self.max_z + z)
    }

    pub fn contract(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x + x, self.min_y + y, self.min_z + z, self.max_x - x, self.max_y - y, self.max_z - z)
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb::new(
            jmin(self.min_x, o.min_x),
            jmin(self.min_y, o.min_y),
            jmin(self.min_z, o.min_z),
            jmax(self.max_x, o.max_x),
            jmax(self.max_y, o.max_y),
            jmax(self.max_z, o.max_z),
        )
    }

    pub fn offset(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x + x, self.min_y + y, self.min_z + z, self.max_x + x, self.max_y + y, self.max_z + z)
    }

    pub fn calculate_x_offset(&self, o: &Aabb, mut off: f64) -> f64 {
        if o.max_y > self.min_y && o.min_y < self.max_y && o.max_z > self.min_z && o.min_z < self.max_z {
            if off > 0.0 && o.max_x <= self.min_x {
                let d = self.min_x - o.max_x;
                if d < off {
                    off = d;
                }
            } else if off < 0.0 && o.min_x >= self.max_x {
                let d = self.max_x - o.min_x;
                if d > off {
                    off = d;
                }
            }
        }
        off
    }

    pub fn calculate_y_offset(&self, o: &Aabb, mut off: f64) -> f64 {
        if o.max_x > self.min_x && o.min_x < self.max_x && o.max_z > self.min_z && o.min_z < self.max_z {
            if off > 0.0 && o.max_y <= self.min_y {
                let d = self.min_y - o.max_y;
                if d < off {
                    off = d;
                }
            } else if off < 0.0 && o.min_y >= self.max_y {
                let d = self.max_y - o.min_y;
                if d > off {
                    off = d;
                }
            }
        }
        off
    }

    pub fn calculate_z_offset(&self, o: &Aabb, mut off: f64) -> f64 {
        if o.max_x > self.min_x && o.min_x < self.max_x && o.max_y > self.min_y && o.min_y < self.max_y {
            if off > 0.0 && o.max_z <= self.min_z {
                let d = self.min_z - o.max_z;
                if d < off {
                    off = d;
                }
            } else if off < 0.0 && o.min_z >= self.max_z {
                let d = self.max_z - o.min_z;
                if d > off {
                    off = d;
                }
            }
        }
        off
    }

    pub fn intersects_with(&self, o: &Aabb) -> bool {
        o.max_x > self.min_x
            && o.min_x < self.max_x
            && o.max_y > self.min_y
            && o.min_y < self.max_y
            && o.max_z > self.min_z
            && o.min_z < self.max_z
    }

    pub fn is_vec_inside(&self, v: Vec3) -> bool {
        v.x > self.min_x && v.x < self.max_x && v.y > self.min_y && v.y < self.max_y && v.z > self.min_z && v.z < self.max_z
    }

    pub fn average_edge_length(&self) -> f64 {
        ((self.max_x - self.min_x) + (self.max_y - self.min_y) + (self.max_z - self.min_z)) / 3.0
    }

    fn in_yz(&self, v: Vec3) -> bool {
        v.y >= self.min_y && v.y <= self.max_y && v.z >= self.min_z && v.z <= self.max_z
    }

    fn in_xz(&self, v: Vec3) -> bool {
        v.x >= self.min_x && v.x <= self.max_x && v.z >= self.min_z && v.z <= self.max_z
    }

    fn in_xy(&self, v: Vec3) -> bool {
        v.x >= self.min_x && v.x <= self.max_x && v.y >= self.min_y && v.y <= self.max_y
    }

    /// Ray/box intercept. Candidates are checked in vanilla order (W, E, D, U, N, S) with
    /// strict `<`, so ties go to the earlier face, matching Java.
    pub fn calculate_intercept(&self, a: Vec3, b: Vec3) -> Option<(Vec3, HitFace)> {
        let candidates = [
            (a.intermediate_with_x(b, self.min_x).filter(|v| self.in_yz(*v)), HitFace::West),
            (a.intermediate_with_x(b, self.max_x).filter(|v| self.in_yz(*v)), HitFace::East),
            (a.intermediate_with_y(b, self.min_y).filter(|v| self.in_xz(*v)), HitFace::Down),
            (a.intermediate_with_y(b, self.max_y).filter(|v| self.in_xz(*v)), HitFace::Up),
            (a.intermediate_with_z(b, self.min_z).filter(|v| self.in_xy(*v)), HitFace::North),
            (a.intermediate_with_z(b, self.max_z).filter(|v| self.in_xy(*v)), HitFace::South),
        ];
        let mut best: Option<(Vec3, HitFace)> = None;
        for (v, face) in candidates {
            if let Some(v) = v {
                if best.map_or(true, |(bv, _)| a.square_distance_to(v) < a.square_distance_to(bv)) {
                    best = Some((v, face));
                }
            }
        }
        best
    }

    pub fn has_nan(&self) -> bool {
        [self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z].iter().any(|v| v.is_nan())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_offsets() {
        let block = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let player = Aabb::new(-0.6, 0.0, 0.2, -0.0001, 1.8, 0.8);
        assert!((block.calculate_x_offset(&player, 0.5) - 0.0001).abs() < 1e-12);
        assert_eq!(block.calculate_x_offset(&player, -0.5), -0.5);
        let above = Aabb::new(0.2, 1.0, 0.2, 0.8, 2.8, 0.8);
        assert_eq!(block.calculate_y_offset(&above, -0.08), 0.0);
    }

    #[test]
    fn intercept_face() {
        let b = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let hit = b.calculate_intercept(Vec3::new(0.5, 3.0, 0.5), Vec3::new(0.5, -3.0, 0.5));
        assert_eq!(hit, Some((Vec3::new(0.5, 1.0, 0.5), HitFace::Up)));
    }
}
