//! Port of `BlockModelRenderer`'s smooth lighting: `fillQuadBounds`,
//! `AmbientOcclusionFace.updateVertexBrightness` and their lookup tables.

use mc_core::Facing;
use mc_data::block::LightProps;
use mc_world::World;

/// `fillQuadBounds` output: 6 bounds, then `1 - bound` for each (the "flip" half).
#[derive(Clone, Copy)]
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

/// `Block.getAmbientOcclusionLightValue`: `0.2` for a normal cube, `1.0` otherwise. `BlockBarrier`
/// overrides the derived value back to `1.0`, so use the dumped per-block value.
fn ao_value(w: &World, p: Pos) -> f32 {
    w.state_at(p.0, p.1, p.2).block().ao_light_value
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
    use mc_data::ids;
    use mc_world::chunk::Chunk;

    /// A lit 3×3 stone floor at y=64 (sky 15, block 0) plus a stone wall at `(9, 65, 8)`, so the
    /// top face of `(8, 64, 8)` has exactly one `BlockStone` corner.
    fn wall_scene() -> World {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        for x in 7..=9 {
            for z in 7..=9 {
                c.set(x as usize, 64, z as usize, ids::STONE, 0);
            }
        }
        c.set(9, 65, 8, ids::STONE, 0);
        for v in c.sections[4].as_mut().unwrap().sky_light.iter_mut() {
            *v = 15;
        }
        w.insert(c);
        w
    }

    fn opaque(w: &World, p: Pos) -> bool {
        w.state_at(p.0, p.1, p.2).block().is_opaque_cube(true)
    }

    /// The six full-cube quads, in `EnumFaceDirection` corner order.
    const CUBE_FACES: [([[f32; 3]; 4], Facing); 6] = [
        ([[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]], Facing::Down),
        ([[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]], Facing::Up),
        ([[1.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]], Facing::North),
        ([[0.0, 1.0, 1.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0]], Facing::South),
        ([[0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0]], Facing::West),
        ([[1.0, 1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]], Facing::East),
    ];

    #[test]
    fn ao_brightness_falls_back_to_center() {
        let full = 240 << 16;
        assert_eq!(ao_brightness(0, 0, 0, full), full);
        assert_eq!(ao_brightness(full, full, full, full), full);
        // Only the two light bytes survive `& 16711935`; the shifted sum is the average.
        assert_eq!(ao_brightness(0xF00090, 0xF00000, 0xF00000, 0xF00000), 0xF00024);
        assert_eq!(ao_brightness(0xF00090, 0xF00050, 0, 0xF00000), 0xF00038);
    }

    /// `fillQuadBounds`: a quad covering the whole cube face sets bit 0 (on face) and clears bit
    /// 1 (partial); a quad inset from every edge clears bit 0 and sets bit 1.
    #[test]
    fn fill_quad_bounds_flags_match_vanilla_for_every_face() {
        for (pos, face) in CUBE_FACES {
            let qb = fill_quad_bounds(&pos, face, true);
            assert!(qb.on_face && !qb.partial, "full {face:?}");
            assert!(fill_quad_bounds(&pos, face, false).on_face, "full cube flag {face:?}");

            let inset: [[f32; 3]; 4] = std::array::from_fn(|i| pos[i].map(|v| 0.25 + 0.5 * v));
            let qb = fill_quad_bounds(&inset, face, false);
            assert!(!qb.on_face && qb.partial, "inset {face:?}");
        }
    }

    /// The bounds table is `WEST, EAST, DOWN, UP, NORTH, SOUTH` followed by `1 - bound`.
    #[test]
    fn fill_quad_bounds_writes_vanilla_bound_order() {
        let pos = [[0.25, 0.5, 0.75], [0.0, 1.0, 0.5], [0.75, 0.0, 1.0], [0.5, 0.25, 0.0]];
        let qb = fill_quad_bounds(&pos, Facing::Up, false);
        assert_eq!(qb.bounds[0..6], [0.0, 1.0, 0.0, 1.0, 0.0, 0.75]);
        assert_eq!(qb.bounds[6..12], [1.0, 0.0, 1.0, 0.0, 1.0, 0.25]);
    }

    /// `Block.getAmbientOcclusionLightValue`: `BlockBarrier` is a full normal cube but overrides
    /// the derived `0.2` back to `1.0`, so it must not darken its neighbours' corners.
    #[test]
    fn barrier_ambient_occlusion_value_is_one() {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        c.set(8, 64, 8, ids::BARRIER, 0);
        c.set(8, 65, 8, ids::STONE, 0);
        w.insert(c);
        assert_eq!(ao_value(&w, (8, 64, 8)), 1.0, "BlockBarrier.getAmbientOcclusionLightValue");
        assert_eq!(ao_value(&w, (8, 65, 8)), 0.2, "Block.getAmbientOcclusionLightValue");
        assert_eq!(ao_value(&w, (8, 66, 8)), 1.0, "air");
    }

    /// `updateVertexBrightness` on a full-cube top face: one `BlockStone` (0.2) corner gives
    /// `(1 + 0.2 + 1 + 1) / 4 = 0.8` for two of the four vertices, stored through
    /// `VertexTranslations.UP = (2, 3, 0, 1)`. Verified against the Java oracle.
    #[test]
    fn ao_colour_multipliers_match_vanilla_for_a_wall_corner() {
        let w = wall_scene();
        let qb = fill_quad_bounds(&CUBE_FACES[1].0, Facing::Up, true);
        let a = vertex_brightness_for(&w, (8, 64, 8), Facing::Up, &qb, |p| opaque(&w, p));
        assert_eq!(a.color, [1.0, 1.0, 0.8, 0.8]);
        assert_eq!(a.brightness, [240 << 16; 4]);
    }

    /// A partial side quad takes the interpolated branch (`field_178289_i` is only false for
    /// up/down): the weights are products of flipped bounds from `EnumNeighborInfo`. Verified
    /// against the Java oracle.
    #[test]
    fn partial_side_quads_interpolate_the_corner_multipliers() {
        let w = wall_scene();
        let quad = [[0.0, 1.0, 0.25], [0.0, 0.0, 0.25], [0.5, 0.0, 0.75], [0.5, 1.0, 0.75]];
        let qb = fill_quad_bounds(&quad, Facing::North, false);
        assert!(qb.partial && !qb.on_face);
        let a = vertex_brightness_for(&w, (9, 65, 8), Facing::North, &qb, |p| opaque(&w, p));
        assert_eq!(a.color.map(f32::to_bits), [0x3f4c_cccd, 0x3f00_0000, 0x3ecc_ccce, 0x3f4c_cccd]);
        assert_eq!(a.brightness, [240 << 16; 4]);
    }
}