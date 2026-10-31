//! CPU chunk meshing, following `BlockModelRenderer`: smooth lighting + AO for
//! models that allow it, flat neighbour lighting otherwise.

use crate::ao;
use crate::models::BlockModels;
use bytemuck::{Pod, Zeroable};
use mc_assets::model::pick_weighted;
use mc_core::BlockPos;
use mc_data::block::OffsetType;
use mc_data::geometry::BlockGeometry;
use mc_data::RenderLayer;
use mc_world::World;
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    /// Relative to the chunk's origin block (x, 0, z).
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// Shade × AO × tint, in vanilla's (non-linear) color space.
    pub color: [u8; 4],
    /// Lightmap coordinates (0–240), alpha-test enabled (0/255), translucent layer (0/255).
    pub light: [u8; 4],
}

/// 20-byte GPU vertex: position relative to the section origin as `(v + 4) * 2048` (exact for all
/// multiples of 1/2048 in -4..28), full-precision UV, light (block, sky), colour and a layer flag in `color[3]`
/// (0 solid, 255 cutout, 128 cutout-mipped, 64 translucent).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct PackedVertex {
    pub pos: [u16; 3],
    pub light: [u8; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

impl PackedVertex {
    /// Only use the compact format when it exactly represents every position.
    pub fn can_pack(v: &Vertex, origin_y: f32) -> bool {
        [v.pos[0], v.pos[1] - origin_y, v.pos[2]].iter().all(|&x| {
            let q = (x + 4.0) * 2048.0;
            q.is_finite() && q >= 0.0 && q <= 65535.0 && q.fract() == 0.0 && q / 2048.0 - 4.0 == x
        })
    }
    pub fn pack(v: &Vertex, origin_y: f32) -> Self {
        let q = |x: f32| ((x + 4.0) * 2048.0).round().clamp(0.0, 65535.0) as u16;
        let flag = match (v.light[2], v.light[3]) {
            (_, t) if t != 0 => 64,
            (a, _) => a,
        };
        Self {
            pos: [q(v.pos[0]), q(v.pos[1] - origin_y), q(v.pos[2])],
            light: [v.light[0], v.light[1]],
            uv: v.uv,
            color: [v.color[0], v.color[1], v.color[2], flag],
        }
    }
}

#[derive(Default)]
pub struct ChunkMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Face connectivity for a single section; `None` is conservatively unknown.
    pub visibility: Option<u64>,
}

pub struct PreparedSection {
    pub packed: Vec<PackedVertex>,
    pub use_packed: bool,
    pub indices: Vec<u32>,
    pub solid_count: u32,
    pub transparent: Vec<([u32; 6], Vec3)>,
    pub min: Vec3,
    pub max: Vec3,
    pub cull_safe: bool,
}

impl PreparedSection {
    pub fn new(mesh: &ChunkMesh, sy: i32) -> Self {
        let section_y = (sy * 16) as f32;
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut use_packed = true;
        for v in &mesh.vertices {
            min = min.min(Vec3::from(v.pos));
            max = max.max(Vec3::from(v.pos));
            use_packed &= PackedVertex::can_pack(v, section_y);
        }
        let packed = if use_packed { mesh.vertices.iter().map(|v| PackedVertex::pack(v, section_y)).collect() } else { Vec::new() };
        let mut indices = Vec::with_capacity(mesh.indices.len());
        let mut transparent: Vec<([u32; 6], Vec3)> = Vec::new();
        for q in mesh.indices.chunks_exact(6) {
            if mesh.vertices[q[0] as usize].light[3] != 0 {
                let center = [q[0], q[1], q[2], q[5]].into_iter()
                    .map(|i| Vec3::from(mesh.vertices[i as usize].pos) - Vec3::new(0.0, section_y, 0.0))
                    .sum::<Vec3>() * 0.25;
                transparent.push((q.try_into().unwrap(), center));
            } else { indices.extend_from_slice(q); }
        }
        let solid_count = indices.len() as u32;
        for (q, _) in &transparent { indices.extend_from_slice(q); }
        Self { packed, use_packed, indices, solid_count, transparent, min, max,
            cull_safe: min.x >= 0.0 && min.z >= 0.0 && min.y >= section_y && max.x <= 16.0 && max.z <= 16.0 && max.y <= section_y + 16.0 }
    }

    pub fn upload_bytes(&self, mesh: &ChunkMesh) -> usize {
        (if self.use_packed { self.packed.len() * std::mem::size_of::<PackedVertex>() }
        else { mesh.vertices.len() * std::mem::size_of::<Vertex>() }) + self.indices.len() * 4
    }
}

impl ChunkMesh {
    pub(crate) fn push_quad(&mut self, v: [Vertex; 4]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&v);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// `FaceBakery.getFaceShadeColor`: the diffuse face brightness is rounded to a byte when the quad
/// is baked, and `WorldRenderer.putColorMultiplier` then scales that byte — so the AO/tint
/// multiply truncates twice, not once.
fn baked_shade(shade: f32) -> u8 {
    (shade * 255.0).clamp(0.0, 255.0) as u8
}

/// `WorldRenderer.putColorMultiplier` for one colour channel: the baked shading byte times the
/// vertex colour multiplier (`vertexColorMultiplier` = AO) times the tint factor, truncated like
/// the Java `(int)` cast.
fn shade_channel(baked: u8, multiplier: f32, tint: f32) -> u8 {
    (baked as f32 * (multiplier * tint)).clamp(0.0, 255.0) as u8
}

/// The `(dx, dy, dz)` that `BlockModelRenderer` adds to every vertex of a block model through
/// `WorldRenderer.putPosition`, from `Block.getOffsetType`.
///
/// The two render paths use different hashes: `renderModelAmbientOcclusionQuads` calls
/// `MathHelper.getPositionRandom` (`getCoordinateRandom(x, y, z)`), while
/// `renderModelStandardQuads` builds `k = (long)(x * 3129871) ^ (long)z * 116129781L` with no
/// `^ (long)y` term, so without smooth lighting every block in a column gets the same jitter.
fn block_offset(kind: OffsetType, x: i32, y: i32, z: i32, smooth: bool) -> [f64; 3] {
    if kind == OffsetType::None {
        return [0.0; 3];
    }
    let random = if smooth {
        mc_core::math::coordinate_random(x, y, z)
    } else {
        // Same hash with `y = 0`, exactly what dropping vanilla's `^ (long)y` term does.
        mc_core::math::coordinate_random(x, 0, z)
    };
    // `(float)(random >> n & 15L) / 15.0F`: the division rounds as a float, then widens to double.
    let frac = |shift: u32| ((random >> shift & 15) as f32 / 15.0) as f64;
    [
        (frac(16) - 0.5) * 0.5,
        if kind == OffsetType::Xyz { (frac(20) - 1.0) * 0.2 } else { 0.0 },
        (frac(24) - 0.5) * 0.5,
    ]
}

/// The block model's origin in mesh space: `WorldRenderer.putPosition` adds
/// `(float)(blockPos + offset)` to the baked local vertex coordinate, and chunk vertices are
/// relative to the chunk's block origin on x/z (y stays absolute).
#[inline]
fn block_origin(off: [f64; 3], lx: i32, y: i32, lz: i32) -> [f32; 3] {
    [(lx as f64 + off[0]) as f32, (y as f64 + off[1]) as f32, (lz as f64 + off[2]) as f32]
}

/// Meshes one chunk using fresh geometry state. Runtime clients should use `build_with_geometry`
/// to retain vanilla's mutable Block bounds across ordered rendering/selection calls.
/// `smooth_lighting` is `Minecraft.isAmbientOcclusionEnabled` (the option isn't Off).
pub fn build(world: &World, models: &BlockModels, cx: i32, cz: i32, smooth_lighting: bool, fancy_graphics: bool) -> ChunkMesh {
    build_with_geometry(world, models, cx, cz, smooth_lighting, fancy_graphics, &mut BlockGeometry::new())
}

pub fn build_with_geometry(world: &World, models: &BlockModels, cx: i32, cz: i32, smooth_lighting: bool, fancy_graphics: bool, geometry: &mut BlockGeometry) -> ChunkMesh {
    build_sections(world, models, cx, cz, 0..16, smooth_lighting, fancy_graphics, geometry, ChunkMesh::default())
}

/// Meshes one 16³ render section (`RenderChunk`), the unit vanilla rebuilds after an edit.
pub fn build_section(world: &World, models: &BlockModels, cx: i32, sy: i32, cz: i32, smooth_lighting: bool, fancy_graphics: bool, geometry: &mut BlockGeometry) -> ChunkMesh {
    build_section_reusing(world, models, cx, sy, cz, smooth_lighting, fancy_graphics, geometry, ChunkMesh::default())
}

#[allow(clippy::too_many_arguments)]
pub fn build_section_reusing(world: &World, models: &BlockModels, cx: i32, sy: i32, cz: i32, smooth_lighting: bool, fancy_graphics: bool, geometry: &mut BlockGeometry, mesh: ChunkMesh) -> ChunkMesh {
    build_sections(world, models, cx, cz, sy as usize..sy as usize + 1, smooth_lighting, fancy_graphics, geometry, mesh)
}

#[allow(clippy::too_many_arguments)]
fn build_sections(world: &World, models: &BlockModels, cx: i32, cz: i32, range: std::ops::Range<usize>, smooth_lighting: bool, fancy_graphics: bool, geometry: &mut BlockGeometry, mut mesh: ChunkMesh) -> ChunkMesh {
    mesh.vertices.clear(); mesh.indices.clear(); mesh.visibility = None;
    let single_section = range.end == range.start + 1;
    let mut vis = crate::visibility::VisGraph::default();
    let Some(chunk) = world.chunks.get(&(cx, cz)) else { return mesh };
    let is_opaque = |p: (i32, i32, i32)| {
        world.state_at(p.0, p.1, p.2).block().is_opaque_cube(fancy_graphics)
    };
    for (sy, section) in chunk.sections.iter().enumerate() {
        if !range.contains(&sy) { continue; }
        let Some(section) = section else { continue };
        for ly in 0..16 {
            let y = sy as i32 * 16 + ly;
            for lz in 0..16 {
                for lx in 0..16 {
                    let (x, z) = (cx * 16 + lx as i32, cz * 16 + lz as i32);
                    let pos = BlockPos::new(x, y, z);
                    let stored = section.state(((ly as usize) << 8) | (lz << 4) | lx);
                    let block = stored.block();
                    if single_section && block.is_opaque_cube(fancy_graphics) { vis.set_opaque(lx, ly as usize, lz); }
                    let layer = block.render_layer(fancy_graphics);
                    if block.render_type == 1 {
                        // `BlockRendererDispatcher` case 1: `BlockFluidRenderer`.
                        crate::fluid::build(world, models, stored, pos, [lx as f32, y as f32, lz as f32], &mut mesh, fancy_graphics, layer);
                        continue;
                    }
                    if block.render_type != 3 {
                        continue;
                    }
                    let actual = stored.actual_state(world, pos);
                    let Some(entry) = models.get_state(actual) else { continue };
                    if entry.models.is_empty() { continue; }
                    let variant = pick_weighted(&entry.weights, x, y, z);
                    let model = &entry.models[variant].1;
                    geometry.update_bounds(stored, world, pos);
                    let mut visible = [None; 6];
                    let props = stored.light_props();
                    let smooth = smooth_lighting && props.emission == 0 && model.ambient_occlusion;
                    // `BlockModelRenderer.renderModel` computes one offset per block model and
                    // `putPosition` adds it to all four vertices of each quad.
                    let off = block_offset(block.offset_type(), x, y, z, smooth);
                    let base = block_origin(off, lx as i32, y, lz as i32);
                    let tint = if entry.tinted[variant] {
                        models.tint_color(world, stored, pos)
                    } else { 0xFFFFFF };
                    for (q, bounds) in model.quads.iter().zip(&entry.bounds[variant]) {
                        if let Some(cull) = q.cullface {
                            let show = *visible[cull.index()].get_or_insert_with(|| geometry.should_side_be_rendered(stored, world, pos, cull, fancy_graphics));
                            if !show {
                                continue;
                            }
                        }
                        let qb = &bounds[props.full_cube as usize];
                        let (mult, bright) = if smooth {
                            let a = ao::vertex_brightness_for(world, (x, y, z), q.face, qb, is_opaque);
                            (a.color, a.brightness)
                        } else {
                            // renderModelStandardQuads
                            let b = match q.cullface {
                                Some(f) => {
                                    let (dx, dy, dz) = f.offset();
                                    world.mixed_brightness(x + dx, y + dy, z + dz)
                                }
                                None if qb.on_face => {
                                    let (dx, dy, dz) = q.face.offset();
                                    world.mixed_brightness(x + dx, y + dy, z + dz)
                                }
                                None => world.mixed_brightness(x, y, z),
                            };
                            ([1.0; 4], [b; 4])
                        };
                        let c = if q.tint_index >= 0 { tint } else { 0xFFFFFF };
                        // `ModelBakery`/`FaceBakery` store the face brightness as a byte; tint is
                        // `(float)(j >> 16 & 255) / 255.0F` times `vertexColorMultiplier`.
                        let baked = baked_shade(q.shade);
                        let tint_rgb = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF].map(|v| v as f32 / 255.0);
                        let verts = std::array::from_fn(|i| {
                            let m = mult[i];
                            Vertex {
                                pos: [base[0] + q.pos[i][0], base[1] + q.pos[i][1], base[2] + q.pos[i][2]],
                                uv: q.uv[i],
                                color: [
                                    shade_channel(baked, m, tint_rgb[0]),
                                    shade_channel(baked, m, tint_rgb[1]),
                                    shade_channel(baked, m, tint_rgb[2]),
                                    255,
                                ],
                                light: [
                                    (bright[i] & 0xFF) as u8,
                                    ((bright[i] >> 16) & 0xFF) as u8,
                                    match layer {
                                        RenderLayer::Solid => 0,
                                        RenderLayer::Cutout => 255,
                                        _ => 128,
                                    },
                                    if layer == RenderLayer::Translucent { 255 } else { 0 },
                                ],
                            }
                        });
                        mesh.push_quad(verts);
                    }
                }
            }
        }
    }
    if single_section { mesh.visibility = Some(vis.compute()); }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FaceBakery.getFaceShadeColor` stores `MathHelper.clamp_int((int)(f * 255.0F), 0, 255)` in
    /// the baked vertex colour, so the DOWN face is 127/255 rather than exactly 0.5.
    #[test]
    fn face_shade_is_quantised_to_a_byte_like_face_bakery() {
        assert_eq!(baked_shade(1.0), 255);
        assert_eq!(baked_shade(0.8), 204);
        assert_eq!(baked_shade(0.6), 153);
        assert_eq!(baked_shade(0.5), 127);
        assert_eq!(baked_shade(-1.0), 0);
    }

    /// `WorldRenderer.putColorMultiplier` scales the *baked byte*, so AO and tint truncate twice.
    /// Scaling `0.5` in one step gives 51 here; vanilla stores 127 first and ends on 50.
    #[test]
    fn ao_and_tint_scale_the_baked_shade_byte() {
        assert_eq!(shade_channel(127, 0.4, 1.0), 50);
        assert_eq!((0.5f32 * 0.4 * 255.0) as u8, 51);
        assert_eq!(shade_channel(255, 1.0, 1.0), 255);
        assert_eq!(shade_channel(204, 1.0, 1.0), 204);
        // `putColorMultiplier(vcm * f)`: the tint multiplies `vertexColorMultiplier` first.
        assert_eq!(shade_channel(204, 0.8, 0.5), (204.0f32 * (0.8f32 * 0.5)) as u8);
        assert_eq!(shade_channel(255, 1.0, 0.5), 127);
    }

    /// `BlockModelRenderer.renderModelAmbientOcclusionQuads`: the jitter comes from
    /// `MathHelper.getPositionRandom`, which hashes x, y and z. Values are the Java formulas
    /// evaluated by hand with `int`/`long` wrapping and `float` division.
    #[test]
    fn ambient_occlusion_offset_matches_vanilla() {
        assert_eq!(block_offset(OffsetType::Xyz, 0, 0, 0, true), [-0.25, -0.2, -0.25]);
        assert_eq!(
            block_offset(OffsetType::Xyz, 1, 64, -2, true),
            [-0.11666665971279144, -0.026666665077209474, -0.25]
        );
        assert_eq!(
            block_offset(OffsetType::Xyz, -5, 12, 7, true),
            [0.21666666865348816, -0.15999999940395357, -0.25]
        );
        assert_eq!(
            block_offset(OffsetType::Xz, 10, 255, -3, true),
            [0.0833333432674408, 0.0, 0.21666666865348816]
        );
        assert_eq!(
            block_offset(OffsetType::Xyz, -1, -1, -1, true),
            [-0.18333332985639572, 0.0, 0.21666666865348816]
        );
    }

    /// `renderModelStandardQuads` hashes only x and z (`k` has no `^ (long)y` term), so a whole
    /// column shares one jitter — a vanilla quirk — and it differs from the AO path.
    #[test]
    fn standard_offset_ignores_y() {
        let expected = [0.25, -0.1466666638851166, -0.04999999701976776];
        assert_eq!(block_offset(OffsetType::Xyz, 1, 0, -2, false), expected);
        assert_eq!(block_offset(OffsetType::Xyz, 1, 64, -2, false), expected);
        assert_eq!(block_offset(OffsetType::Xyz, 1, 127, -2, false), expected);
        assert_ne!(block_offset(OffsetType::Xyz, 1, 64, -2, true), expected);
        assert_eq!(
            block_offset(OffsetType::Xyz, -5, 12, 7, false),
            [-0.04999999701976776, -0.18666666597127915, 0.11666667461395264]
        );
        // XZ leaves y alone on both paths.
        assert_eq!(block_offset(OffsetType::Xz, 1, 64, -2, false), [0.25, 0.0, -0.04999999701976776]);
        assert_eq!(block_offset(OffsetType::Xz, 1, 64, -2, true), [-0.11666665971279144, 0.0, -0.25]);
    }

    /// `x * 3129871` is Java `int` arithmetic and wraps; both paths hash the wrapped value at the
    /// ±30,000,000 world border.
    #[test]
    fn offset_hash_wraps_like_java_ints() {
        assert_eq!(
            block_offset(OffsetType::Xyz, 30_000_000, 64, -30_000_000, true),
            [0.0833333432674408, -0.06666666269302368, -0.0833333283662796]
        );
        assert_eq!(
            block_offset(OffsetType::Xyz, 30_000_000, 64, -30_000_000, false),
            [0.0833333432674408, -0.026666665077209474, -0.14999999850988388]
        );
    }

    #[test]
    fn no_offset_type_is_zero() {
        assert_eq!(block_offset(OffsetType::None, 1, 64, -2, true), [0.0; 3]);
        assert_eq!(block_offset(OffsetType::None, 1, 64, -2, false), [0.0; 3]);
    }

    /// Blocks without an offset must keep the exact same vertices as before the port. With a zero
    /// offset `block_origin` reduces to the old `lx`/`y`/`lz` bases, so the vertex sum is unchanged.
    #[test]
    fn zero_offset_leaves_positions_unchanged() {
        for (lx, y, lz) in [(0, 0, 0), (15, 255, 15), (7, 64, 3)] {
            let base = block_origin(block_offset(OffsetType::None, lx, y, lz, true), lx, y, lz);
            for local in [0.0f32, 0.5, 0.3, 1.0] {
                assert_eq!(base[0] + local, local + lx as f32);
                assert_eq!(base[1] + local, local + y as f32);
                assert_eq!(base[2] + local, local + lz as f32);
            }
        }
    }
}
