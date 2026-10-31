//! `BlockFluidRenderer`: water and lava geometry, ported from 1.8's
//! `RenderBlocks`/`BlockFluidRenderer`. Liquids have no blockstate model; the mesher calls this
//! for `render_type == 1` states instead of looking up a model.
use crate::mesh::{ChunkMesh, Vertex};
use crate::models::BlockModels;
use mc_assets::atlas::Sprite;
use mc_core::BlockPos;
use mc_data::{Material, RenderLayer, StateId};
use mc_world::World;

#[derive(Clone, Copy)]
enum Side {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Side {
    fn offset(self) -> (i32, i32, i32) {
        match self {
            Side::Down => (0, -1, 0),
            Side::Up => (0, 1, 0),
            Side::North => (0, 0, -1),
            Side::South => (0, 0, 1),
            Side::West => (-1, 0, 0),
            Side::East => (1, 0, 0),
        }
    }
}

/// `BlockLiquid.getLiquidHeightPercent`.
fn height_percent(meta: i32) -> f32 {
    let meta = if meta >= 8 { 0 } else { meta };
    (meta + 1) as f32 / 9.0
}

fn level(state: StateId) -> i32 {
    state.get_int("level")
}

fn material_at(world: &World, pos: BlockPos) -> Material {
    world.state_at(pos.x, pos.y, pos.z).block().material
}

/// `BlockLiquid.getEffectiveFlowDecay`.
fn effective_flow_decay(world: &World, pos: BlockPos, mat: Material) -> i32 {
    let state = world.state_at(pos.x, pos.y, pos.z);
    if state.block().material != mat {
        return -1;
    }
    let i = level(state);
    if i >= 8 { 0 } else { i }
}

/// `BlockFluidRenderer.getFluidHeight`, including the neighbour average used for corners.
fn fluid_height(world: &World, pos: BlockPos, mat: Material) -> f32 {
    let mut i = 0;
    let mut f = 0.0;
    for j in 0..4 {
        let p = BlockPos::new(pos.x - (j & 1), pos.y, pos.z - (j >> 1 & 1));
        if material_at(world, BlockPos::new(p.x, p.y + 1, p.z)) == mat {
            return 1.0;
        }
        let state = world.state_at(p.x, p.y, p.z);
        let m = state.block().material;
        if m != mat {
            if !m.props().solid {
                f += 1.0;
                i += 1;
            }
        } else {
            let k = level(state);
            if k >= 8 || k == 0 {
                f += height_percent(k) * 10.0;
                i += 10;
            }
            f += height_percent(k);
            i += 1;
        }
    }
    if i == 0 {
        return 1.0;
    }
    1.0 - f / i as f32
}

/// `BlockLiquid.getFlowVector`, x/z only (the y term only matters for falling-liquid direction,
/// which `flow_direction` discards because it uses `atan2(z, x)`).
fn flow_direction(world: &World, pos: BlockPos, mat: Material) -> f64 {
    let mut vx = 0.0f64;
    let mut vz = 0.0f64;
    let mut vy = 0.0f64;
    let i = effective_flow_decay(world, pos, mat);
    for side in [Side::North, Side::South, Side::West, Side::East] {
        let (dx, _, dz) = side.offset();
        let np = BlockPos::new(pos.x + dx, pos.y, pos.z + dz);
        let j = effective_flow_decay(world, np, mat);
        if j < 0 {
            if !material_at(world, np).props().blocks_movement {
                let below = BlockPos::new(np.x, np.y - 1, np.z);
                let j2 = effective_flow_decay(world, below, mat);
                if j2 >= 0 {
                    let k = (j2 - (i - 8)) as f64;
                    vx += dx as f64 * k;
                    vy += -1.0 * k;
                    vz += dz as f64 * k;
                }
            }
        } else {
            let l = (j - i) as f64;
            vx += dx as f64 * l;
            vz += dz as f64 * l;
        }
    }
    if level(world.state_at(pos.x, pos.y, pos.z)) >= 8 {
        for side in [Side::North, Side::South, Side::West, Side::East] {
            let (dx, _, dz) = side.offset();
            let np = BlockPos::new(pos.x + dx, pos.y, pos.z + dz);
            let above = BlockPos::new(np.x, np.y + 1, np.z);
            if is_block_solid(world, np, side) || is_block_solid(world, above, side) {
                let len = (vx * vx + vy * vy + vz * vz).sqrt();
                if len > 0.0 {
                    vx /= len;
                    vy /= len;
                    vz /= len;
                }
                vy -= 6.0;
                break;
            }
        }
    }
    let len = (vx * vx + vy * vy + vz * vz).sqrt();
    if len == 0.0 || (vx == 0.0 && vz == 0.0) {
        return -1000.0;
    }
    let vx = vx / len;
    let vz = vz / len;
    // `MathHelper.atan2` returns radians; the renderer feeds it to sin/cos directly.
    (vz.atan2(vx) - std::f64::consts::FRAC_PI_2) as f64
}

/// `BlockLiquid.isBlockSolid`.
fn is_block_solid(world: &World, pos: BlockPos, side: Side) -> bool {
    let state = world.state_at(pos.x, pos.y, pos.z);
    let m = state.block().material;
    if m == Material::Water || m == Material::Lava {
        return false;
    }
    if matches!(side, Side::Up) {
        return true;
    }
    if m == Material::Ice {
        return false;
    }
    m.props().solid
}

/// `BlockLiquid.shouldSideBeRendered`, plus a provably invisible cull when the neighbour is a
/// full opaque cube (vanilla still emits that face, but it is covered by the neighbour).
fn side_visible(world: &World, pos: BlockPos, mat: Material, side: Side, fancy: bool) -> bool {
    let state = world.state_at(pos.x, pos.y, pos.z);
    if state.block().material == mat {
        return false;
    }
    if !matches!(side, Side::Up) && state.block().is_opaque_cube(fancy) {
        return false;
    }
    true
}

/// `BlockLiquid.shouldRenderSides`: a waterfall needs a back face when any horizontal neighbour
/// is neither the liquid nor a full block.
fn should_render_sides(world: &World, pos: BlockPos, mat: Material) -> bool {
    for i in -1..=1 {
        for j in -1..=1 {
            let s = world.state_at(pos.x + i, pos.y, pos.z + j);
            if s.block().material != mat && !s.block().full_block {
                return true;
            }
        }
    }
    false
}

/// `BlockLiquid.getMixedBrightnessForBlock`: per channel, the max of `pos` and `pos.up()`.
fn light(world: &World, pos: BlockPos) -> [u8; 2] {
    let i = world.combined_light(pos.x, pos.y, pos.z, 0);
    let j = world.combined_light(pos.x, pos.y + 1, pos.z, 0);
    [(i & 0xFF).max(j & 0xFF) as u8, ((i >> 16) & 0xFF).max((j >> 16) & 0xFF) as u8]
}

#[allow(clippy::too_many_arguments)]
fn push(mesh: &mut ChunkMesh, base: [f32; 3], points: [[f32; 3]; 4], uvs: [[f32; 2]; 4], light: [u8; 2], color: [u8; 3], layer: RenderLayer) {
    let alpha = match layer {
        RenderLayer::Solid => 0,
        RenderLayer::Cutout => 255,
        _ => 128,
    };
    let translucent = if layer == RenderLayer::Translucent { 255 } else { 0 };
    let verts = std::array::from_fn(|i| Vertex {
        pos: [base[0] + points[i][0], base[1] + points[i][1], base[2] + points[i][2]],
        uv: uvs[i],
        color: [color[0], color[1], color[2], 255],
        light: [light[0], light[1], alpha, translucent],
    });
    mesh.push_quad(verts);
}

fn scale(tint: u32, shade: f32) -> [u8; 3] {
    let f = |v: u32| (shade * (v & 0xFF) as f32).clamp(0.0, 255.0) as u8;
    [f(tint >> 16), f(tint >> 8), f(tint)]
}

/// `BlockRendererDispatcher.renderBlock` case 1.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(world: &World, models: &BlockModels, state: StateId, pos: BlockPos, base: [f32; 3], mesh: &mut ChunkMesh, fancy: bool, layer: RenderLayer) {
    let mat = state.block().material;
    let water = mat == Material::Water;
    let still = models.atlas.sprite(if water { "blocks/water_still" } else { "blocks/lava_still" });
    let flow = models.atlas.sprite(if water { "blocks/water_flow" } else { "blocks/lava_flow" });
    // BlockLiquid.colorMultiplier: the biome's water colour at this exact position (no
    // biome blending, unlike block models); lava is untinted.
    let tint = if water {
        mc_data::biome::Biome::by_id(world.biome_id(pos)).map_or(0xFFFFFF, |b| b.water_color)
    } else {
        0xFFFFFF
    };

    let top = side_visible(world, BlockPos::new(pos.x, pos.y + 1, pos.z), mat, Side::Up, fancy);
    let bottom = side_visible(world, BlockPos::new(pos.x, pos.y - 1, pos.z), mat, Side::Down, fancy);
    let sides = [
        side_visible(world, BlockPos::new(pos.x, pos.y, pos.z - 1), mat, Side::North, fancy),
        side_visible(world, BlockPos::new(pos.x, pos.y, pos.z + 1), mat, Side::South, fancy),
        side_visible(world, BlockPos::new(pos.x - 1, pos.y, pos.z), mat, Side::West, fancy),
        side_visible(world, BlockPos::new(pos.x + 1, pos.y, pos.z), mat, Side::East, fancy),
    ];
    if !top && !bottom && !sides.iter().any(|&b| b) {
        return;
    }

    let mut h = [0.0f32; 4];
    h[0] = fluid_height(world, pos, mat);
    h[1] = fluid_height(world, BlockPos::new(pos.x, pos.y, pos.z + 1), mat);
    h[2] = fluid_height(world, BlockPos::new(pos.x + 1, pos.y, pos.z + 1), mat);
    h[3] = fluid_height(world, BlockPos::new(pos.x + 1, pos.y, pos.z), mat);
    let eps = 0.001f32;

    if top {
        let (f7, f8, f9, f10) = (h[0] - eps, h[1] - eps, h[2] - eps, h[3] - eps);
        let dir = flow_direction(world, pos, mat);
        let uvs = if dir <= -999.0 {
            [[still.u(0.0), still.v(0.0)], [still.u(0.0), still.v(16.0)], [still.u(16.0), still.v(16.0)], [still.u(16.0), still.v(0.0)]]
        } else {
            let a = dir as f32;
            let f21 = a.sin() * 0.25;
            let f22 = a.cos() * 0.25;
            let interp = |t: &Sprite| {
                [
                    [t.u(8.0 + (-f22 - f21) * 16.0), t.v(8.0 + (-f22 + f21) * 16.0)],
                    [t.u(8.0 + (-f22 + f21) * 16.0), t.v(8.0 + (f22 + f21) * 16.0)],
                    [t.u(8.0 + (f22 + f21) * 16.0), t.v(8.0 + (f22 - f21) * 16.0)],
                    [t.u(8.0 + (f22 - f21) * 16.0), t.v(8.0 + (-f22 - f21) * 16.0)],
                ]
            };
            interp(&flow)
        };
        let c = scale(tint, 1.0);
        push(mesh, base, [[0.0, f7, 0.0], [0.0, f8, 1.0], [1.0, f9, 1.0], [1.0, f10, 0.0]], uvs, light(world, pos), c, layer);
        if should_render_sides(world, BlockPos::new(pos.x, pos.y + 1, pos.z), mat) {
            push(mesh, base, [[0.0, f7, 0.0], [1.0, f10, 0.0], [1.0, f9, 1.0], [0.0, f8, 1.0]], [uvs[0], uvs[3], uvs[2], uvs[1]], light(world, pos), c, layer);
        }
    }

    if bottom {
        let c = scale(tint, 0.5);
        let uvs = [[still.min_u, still.max_v], [still.min_u, still.min_v], [still.max_u, still.min_v], [still.max_u, still.max_v]];
        push(mesh, base, [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]], uvs, light(world, BlockPos::new(pos.x, pos.y - 1, pos.z)), c, layer);
    }

    for (i, &show) in sides.iter().enumerate() {
        if !show {
            continue;
        }
        let (dx, dz) = match i {
            0 => (0, -1),
            1 => (0, 1),
            2 => (-1, 0),
            _ => (1, 0),
        };
        let (f39, f40, d3, d4, d5, d6) = match i {
            0 => (h[0], h[3], 0.0, eps, 1.0, eps),
            1 => (h[2], h[1], 1.0, 1.0 - eps, 0.0, 1.0 - eps),
            2 => (h[1], h[0], eps, 1.0, eps, 0.0),
            _ => (h[3], h[2], 1.0 - eps, 0.0, 1.0 - eps, 1.0),
        };
        let shade = if i < 2 { 0.8 } else { 0.6 };
        let c = scale(tint, shade);
        let u0 = flow.u(0.0);
        let u8 = flow.u(8.0);
        let v39 = flow.v((1.0 - f39) * 16.0 * 0.5);
        let v40 = flow.v((1.0 - f40) * 16.0 * 0.5);
        let v8 = flow.v(8.0);
        let l = light(world, BlockPos::new(pos.x + dx, pos.y, pos.z + dz));
        let quad = [[d3, f39, d4], [d5, f40, d6], [d5, 0.0, d6], [d3, 0.0, d4]];
        let uvs = [[u0, v39], [u8, v40], [u8, v8], [u0, v8]];
        push(mesh, base, quad, uvs, l, c, layer);
        push(mesh, base, [quad[3], quad[2], quad[1], quad[0]], [uvs[3], uvs[2], uvs[1], uvs[0]], l, c, layer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_world::chunk::Chunk;

    fn pool() -> World {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                c.set(x, 1, z, 1, 0);
                c.set(x, 2, z, 9, 0);
            }
        }
        w.insert(c);
        w
    }

    #[test]
    fn liquid_heights_match_vanilla() {
        assert!((height_percent(0) - 1.0 / 9.0).abs() < 1e-6);
        assert!((height_percent(7) - 8.0 / 9.0).abs() < 1e-6);
        assert!((height_percent(8) - 1.0 / 9.0).abs() < 1e-6);
    }

    #[test]
    fn flat_pool_centre_and_edge_heights() {
        let w = pool();
        let centre = fluid_height(&w, BlockPos::new(8, 2, 8), Material::Water);
        assert!((centre - 8.0 / 9.0).abs() < 1e-6, "{centre}");
        let edge = fluid_height(&w, BlockPos::new(0, 2, 8), Material::Water);
        assert!(edge < centre && edge > 0.75, "{edge}");
        assert_eq!(flow_direction(&w, BlockPos::new(8, 2, 8), Material::Water), -1000.0);
    }

    #[test]
    fn same_material_sides_are_culled() {
        let w = pool();
        assert!(!side_visible(&w, BlockPos::new(9, 2, 8), Material::Water, Side::East, true));
        assert!(!side_visible(&w, BlockPos::new(8, 2, 7), Material::Water, Side::North, true));
        // Above the pool is air: the top face renders.
        assert!(side_visible(&w, BlockPos::new(8, 3, 8), Material::Water, Side::Up, true));
        // The stone floor below hides the bottom face.
        assert!(!side_visible(&w, BlockPos::new(8, 1, 8), Material::Water, Side::Down, true));
    }
}
