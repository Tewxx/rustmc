//! Port of `BlockModelRenderer`'s smooth lighting: `fillQuadBounds`,
//! `AmbientOcclusionFace.updateVertexBrightness` and their lookup tables.

use mc_core::Facing;
use mc_data::block::LightProps;
use mc_world::World;

/// `fillQuadBounds` output: 6 bounds, then `1 - bound` for each (the "flip" half).
pub struct QuadBounds {
    pub bounds: [f32; 12],
    /// BitSet bit 0: the quad lies on the block face (or the block is a full cube),
    /// so lighting is sampled from the neighbour in front.
    pub on_face: bool,
    /// BitSet bit 1: the quad doesn't cover the whole face, so corners are interpolated.
    pub partial: bool,
}

pub fn fill_quad_bounds(pos: &[[f32; 3]; 4], face: Facing, full_cube: bool) -> QuadBounds {
    let (mut f, mut f1, mut f2) = (32.0f32, 32.0f32, 32.0f32);
    let (mut f3, mut f4, mut f5) = (-32.0f32, -32.0f32, -32.0f32);
    for p in pos {
        f = f.min(p[0]);
        f1 = f1.min(p[1]);
        f2 = f2.min(p[2]);
        f3 = f3.max(p[0]);
        f4 = f4.max(p[1]);
        f5 = f5.max(p[2]);
    }
    let mut bounds = [0.0; 12];
    bounds[Facing::West.index()] = f;
    bounds[Facing::East.index()] = f3;
    bounds[Facing::Down.index()] = f1;
    bounds[Facing::Up.index()] = f4;
    bounds[Facing::North.index()] = f2;
    bounds[Facing::South.index()] = f5;
    for i in 0..6 {
        bounds[i + 6] = 1.0 - bounds[i];
    }
    const LO: f32 = 1.0e-4;
    const HI: f32 = 0.9999;
    let (partial, on_face) = match face {
        Facing::Down => (f >= LO || f2 >= LO || f3 <= HI || f5 <= HI, (f1 < LO || full_cube) && f1 == f4),
        Facing::Up => (f >= LO || f2 >= LO || f3 <= HI || f5 <= HI, (f4 > HI || full_cube) && f1 == f4),
        Facing::North => (f >= LO || f1 >= LO || f3 <= HI || f4 <= HI, (f2 < LO || full_cube) && f2 == f5),
        Facing::South => (f >= LO || f1 >= LO || f3 <= HI || f4 <= HI, (f5 > HI || full_cube) && f2 == f5),
        Facing::West => (f1 >= LO || f2 >= LO || f4 <= HI || f5 <= HI, (f < LO || full_cube) && f == f3),
        Facing::East => (f1 >= LO || f2 >= LO || f4 <= HI || f5 <= HI, (f3 > HI || full_cube) && f == f3),
    };
    QuadBounds { bounds, on_face, partial }
}

/// `BlockModelRenderer.Orientation`: a facing index, +6 when flipped.
#[derive(Clone, Copy)]
struct O(usize);

const D: O = O(0);
const U: O = O(1);
const N: O = O(2);
const S: O = O(3);
const W: O = O(4);
const E: O = O(5);
const FD: O = O(6);
const FU: O = O(7);
const FN: O = O(8);
const FS: O = O(9);
const FW: O = O(10);
const FE: O = O(11);

struct NeighborInfo {
    corners: [Facing; 4],
    /// `field_178289_i`: whether partial quads interpolate (false for up/down in 1.8).
    interpolate: bool,
    vert: [[O; 8]; 4],
}

use Facing::{Down as DN, East as EA, North as NO, South as SO, Up as UP, West as WE};

const EMPTY: [O; 8] = [D; 8];

/// `EnumNeighborInfo`, indexed by facing.
const NEIGHBORS: [NeighborInfo; 6] = [
    NeighborInfo { corners: [WE, EA, NO, SO], interpolate: false, vert: [EMPTY; 4] },
    NeighborInfo { corners: [EA, WE, NO, SO], interpolate: false, vert: [EMPTY; 4] },
    NeighborInfo {
        corners: [UP, DN, EA, WE],
        interpolate: true,
        vert: [
            [U, FW, U, W, FU, W, FU, FW],
            [U, FE, U, E, FU, E, FU, FE],
            [D, FE, D, E, FD, E, FD, FE],
            [D, FW, D, W, FD, W, FD, FW],
        ],
    },
    NeighborInfo {
        corners: [WE, EA, DN, UP],
        interpolate: true,
        vert: [
            [U, FW, FU, FW, FU, W, U, W],
            [D, FW, FD, FW, FD, W, D, W],
            [D, FE, FD, FE, FD, E, D, E],
            [U, FE, FU, FE, FU, E, U, E],
        ],
    },
    NeighborInfo {
        corners: [UP, DN, NO, SO],
        interpolate: true,
        vert: [
            [U, S, U, FS, FU, FS, FU, S],
            [U, N, U, FN, FU, FN, FU, N],
            [D, N, D, FN, FD, FN, FD, N],
            [D, S, D, FS, FD, FS, FD, S],
        ],
    },
    NeighborInfo {
        corners: [DN, UP, NO, SO],
        interpolate: true,
        vert: [
            [FD, S, FD, FS, D, FS, D, S],
            [FD, N, FD, FN, D, FN, D, N],
            [FU, N, FU, FN, U, FN, U, N],
            [FU, S, FU, FS, U, FS, U, S],
        ],
    },
];

/// `VertexTranslations`, indexed by facing.
const VERTEX_TRANSLATIONS: [[usize; 4]; 6] = [[0, 1, 2, 3], [2, 3, 0, 1], [3, 0, 1, 2], [0, 1, 2, 3], [3, 0, 1, 2], [1, 2, 3, 0]];

type Pos = (i32, i32, i32);

fn off(p: Pos, f: Facing) -> Pos {
    let (x, y, z) = f.offset();
    (p.0 + x, p.1 + y, p.2 + z)
}

fn props(w: &World, p: Pos) -> LightProps {
    w.state_at(p.0, p.1, p.2).light_props()
}

fn ao_value(w: &World, p: Pos) -> f32 {
    props(w, p).ao_value()
}

fn brightness(w: &World, p: Pos) -> i32 {
    w.mixed_brightness(p.0, p.1, p.2)
}

fn ao_brightness(mut a: i32, mut b: i32, mut c: i32, d: i32) -> i32 {
    if a == 0 {
        a = d;
    }
    if b == 0 {
        b = d;
    }
    if c == 0 {
        c = d;
    }
    (a + b + c + d) >> 2 & 0xFF00FF
}

fn vertex_brightness(b: [i32; 4], w: [f32; 4]) -> i32 {
    let hi = (((b[0] >> 16 & 255) as f32 * w[0]
        + (b[1] >> 16 & 255) as f32 * w[1]
        + (b[2] >> 16 & 255) as f32 * w[2]
        + (b[3] >> 16 & 255) as f32 * w[3]) as i32)
        & 255;
    let lo = (((b[0] & 255) as f32 * w[0] + (b[1] & 255) as f32 * w[1] + (b[2] & 255) as f32 * w[2] + (b[3] & 255) as f32 * w[3])
        as i32)
        & 255;
    hi << 16 | lo
}

/// Per-vertex AO color multipliers and packed lightmap brightness, in stored vertex order.
pub struct AoFace {
    pub color: [f32; 4],
    pub brightness: [i32; 4],
}

pub fn vertex_brightness_for(w: &World, block: Pos, face: Facing, qb: &QuadBounds, is_opaque: impl Fn(Pos) -> bool) -> AoFace {
    let base = if qb.on_face { off(block, face) } else { block };
    let info = &NEIGHBORS[face.index()];
    let p1 = off(base, info.corners[0]);
    let p2 = off(base, info.corners[1]);
    let p3 = off(base, info.corners[2]);
    let p4 = off(base, info.corners[3]);
    let (i, j, k, l) = (brightness(w, p1), brightness(w, p2), brightness(w, p3), brightness(w, p4));
    let (f, f1, f2, f3) = (ao_value(w, p1), ao_value(w, p2), ao_value(w, p3), ao_value(w, p4));
    let flag = props(w, off(p1, face)).translucent;
    let flag1 = props(w, off(p2, face)).translucent;
    let flag2 = props(w, off(p3, face)).translucent;
    let flag3 = props(w, off(p4, face)).translucent;

    let corner = |a: Pos, dir: Facing| {
        let p = off(a, dir);
        (ao_value(w, p), brightness(w, p))
    };
    let (f4, i1) = if !flag2 && !flag { (f, i) } else { corner(p1, info.corners[2]) };
    let (f5, j1) = if !flag3 && !flag { (f, i) } else { corner(p1, info.corners[3]) };
    let (f6, k1) = if !flag2 && !flag1 { (f1, j) } else { corner(p2, info.corners[2]) };
    let (f7, l1) = if !flag3 && !flag1 { (f1, j) } else { corner(p2, info.corners[3]) };

    let mut i3 = brightness(w, block);
    if qb.on_face || !is_opaque(off(block, face)) {
        i3 = brightness(w, off(block, face));
    }
    let f8 = if qb.on_face { ao_value(w, base) } else { ao_value(w, block) };
    let t = VERTEX_TRANSLATIONS[face.index()];

    let c0 = (f3 + f + f5 + f8) * 0.25;
    let c1 = (f2 + f + f4 + f8) * 0.25;
    let c2 = (f2 + f1 + f6 + f8) * 0.25;
    let c3 = (f3 + f1 + f7 + f8) * 0.25;
    let b0 = ao_brightness(l, i, j1, i3);
    let b1 = ao_brightness(k, i, i1, i3);
    let b2 = ao_brightness(k, j, k1, i3);
    let b3 = ao_brightness(l, j, l1, i3);

    let mut out = AoFace { color: [0.0; 4], brightness: [0; 4] };
    if qb.partial && info.interpolate {
        let q = &qb.bounds;
        let weights = |v: &[O; 8]| [q[v[0].0] * q[v[1].0], q[v[2].0] * q[v[3].0], q[v[4].0] * q[v[5].0], q[v[6].0] * q[v[7].0]];
        for (n, &slot) in t.iter().enumerate() {
            let wt = weights(&info.vert[n]);
            out.color[slot] = c0 * wt[0] + c1 * wt[1] + c2 * wt[2] + c3 * wt[3];
            out.brightness[slot] = vertex_brightness([b0, b1, b2, b3], wt);
        }
    } else {
        let colors = [c0, c1, c2, c3];
        let brights = [b0, b1, b2, b3];
        for (n, &slot) in t.iter().enumerate() {
            out.color[slot] = colors[n];
            out.brightness[slot] = brights[n];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ao_brightness_falls_back_to_center() {
        let full = 240 << 16;
        assert_eq!(ao_brightness(0, 0, 0, full), full);
        assert_eq!(ao_brightness(full, full, full, full), full);
    }

    #[test]
    fn full_cube_top_face_is_on_face_and_not_partial() {
        let top = [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]];
        let qb = fill_quad_bounds(&top, Facing::Up, true);
        assert!(qb.on_face && !qb.partial);
    }
}
