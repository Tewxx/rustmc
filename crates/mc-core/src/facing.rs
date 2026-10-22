//! Port of `net.minecraft.util.EnumFacing`.

use crate::BlockPos;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Facing {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Facing {
    /// Ordered by `index` (the `VALUES` array).
    pub const ALL: [Facing; 6] = [Facing::Down, Facing::Up, Facing::North, Facing::South, Facing::West, Facing::East];

    /// Ordered by `horizontalIndex` (S, W, N, E), as used by yaw and `getHorizontal`.
    pub const HORIZONTALS: [Facing; 4] = [Facing::South, Facing::West, Facing::North, Facing::East];

    /// `EnumFacing.Plane.HORIZONTAL` iteration order (N, E, S, W); differs from [`Facing::HORIZONTALS`].
    pub const PLANE_HORIZONTAL: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Facing {
        Self::ALL[i % 6]
    }

    /// `getHorizontalIndex`; -1 for up/down like vanilla.
    pub fn horizontal_index(self) -> i32 {
        match self {
            Facing::South => 0,
            Facing::West => 1,
            Facing::North => 2,
            Facing::East => 3,
            Facing::Down | Facing::Up => -1,
        }
    }

    pub fn from_horizontal_index(i: i32) -> Facing {
        Self::HORIZONTALS[(i & 3) as usize]
    }

    /// `fromAngle`: yaw in degrees to horizontal facing.
    pub fn from_angle(angle: f64) -> Facing {
        Self::from_horizontal_index(crate::math::floor_double(angle / 90.0 + 0.5) & 3)
    }

    pub fn opposite(self) -> Facing {
        match self {
            Facing::Down => Facing::Up,
            Facing::Up => Facing::Down,
            Facing::North => Facing::South,
            Facing::South => Facing::North,
            Facing::West => Facing::East,
            Facing::East => Facing::West,
        }
    }

    pub fn axis(self) -> Axis {
        match self {
            Facing::Down | Facing::Up => Axis::Y,
            Facing::North | Facing::South => Axis::Z,
            Facing::West | Facing::East => Axis::X,
        }
    }

    pub fn offset(self) -> (i32, i32, i32) {
        match self {
            Facing::Down => (0, -1, 0),
            Facing::Up => (0, 1, 0),
            Facing::North => (0, 0, -1),
            Facing::South => (0, 0, 1),
            Facing::West => (-1, 0, 0),
            Facing::East => (1, 0, 0),
        }
    }

    /// `rotateY`: clockwise looking down. Panics on vertical facings, like vanilla throws.
    pub fn rotate_y(self) -> Facing {
        match self {
            Facing::North => Facing::East,
            Facing::East => Facing::South,
            Facing::South => Facing::West,
            Facing::West => Facing::North,
            f => panic!("Unable to get Y-rotated facing of {f:?}"),
        }
    }

    /// `rotateYCCW`: counter-clockwise looking down. Panics on vertical facings, like vanilla throws.
    pub fn rotate_y_ccw(self) -> Facing {
        match self {
            Facing::North => Facing::West,
            Facing::East => Facing::North,
            Facing::South => Facing::East,
            Facing::West => Facing::South,
            f => panic!("Unable to get CCW facing of {f:?}"),
        }
    }

    /// `rotateX`: North→Down→South→Up→North.
    pub fn rotate_x(self) -> Facing {
        match self {
            Facing::North => Facing::Down,
            Facing::Down => Facing::South,
            Facing::South => Facing::Up,
            Facing::Up => Facing::North,
            f => panic!("Unable to get X-rotated facing of {f:?}"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Facing::Down => "down",
            Facing::Up => "up",
            Facing::North => "north",
            Facing::South => "south",
            Facing::West => "west",
            Facing::East => "east",
        }
    }

    pub fn by_name(s: &str) -> Option<Facing> {
        Self::ALL.into_iter().find(|f| f.name() == s)
    }
}

impl BlockPos {
    pub fn offset_facing(self, f: Facing) -> BlockPos {
        let (x, y, z) = f.offset();
        self.offset(x, y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposites_and_rotation() {
        for f in Facing::ALL {
            assert_eq!(f.opposite().opposite(), f);
        }
        assert_eq!(Facing::North.rotate_y().rotate_y(), Facing::South);
        for f in Facing::PLANE_HORIZONTAL {
            assert_eq!(f.rotate_y().rotate_y_ccw(), f);
        }
        assert_eq!(Facing::from_angle(0.0), Facing::South);
        assert_eq!(Facing::from_angle(90.0), Facing::West);
        assert_eq!(Facing::from_angle(-90.0), Facing::East);
    }
}
