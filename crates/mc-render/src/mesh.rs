//! CPU chunk meshing, following `BlockModelRenderer`: smooth lighting + AO for
//! models that allow it, flat neighbour lighting otherwise.

use crate::ao;
use crate::models::BlockModels;
use bytemuck::{Pod, Zeroable};
use mc_assets::model::pick_weighted;
use mc_world::blocks::Layer;
use mc_world::World;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    /// Relative to the chunk's origin block (x, 0, z).
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// Shade × AO × tint, in vanilla's (non-linear) color space.
    pub color: [u8; 4],
    /// Lightmap coordinates: block light, sky light (0–240), unused, unused.
    pub light: [u8; 4],
}

#[derive(Default)]
pub struct ChunkMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl ChunkMesh {
    fn push_quad(&mut self, v: [Vertex; 4]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&v);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Meshes chunk `(cx, cz)`. Solid and cutout share one mesh; the shader alpha-tests.
/// `smooth_lighting` is `Minecraft.isAmbientOcclusionEnabled` (the option isn't Off).
pub fn build(world: &World, models: &BlockModels, cx: i32, cz: i32, smooth_lighting: bool) -> ChunkMesh {
    let mut mesh = ChunkMesh::default();
    let Some(chunk) = world.chunks.get(&(cx, cz)) else { return mesh };
    let is_opaque = |p: (i32, i32, i32)| {
        let (id, meta) = world.get(p.0, p.1, p.2);
        models.is_opaque(id, meta)
    };
    for (sy, section) in chunk.sections.iter().enumerate() {
        if section.is_none() {
            continue;
        }
        for ly in 0..16 {
            let y = sy as i32 * 16 + ly;
            for lz in 0..16 {
                for lx in 0..16 {
                    let (id, meta) = chunk.get(lx, y, lz);
                    if id == 0 {
                        continue;
                    }
                    let Some(entry) = models.get(id, meta) else { continue };
                    if entry.info.layer == Layer::Translucent || entry.models.is_empty() {
                        continue;
                    }
                    let (x, z) = (cx * 16 + lx as i32, cz * 16 + lz as i32);
                    let model = &entry.models[pick_weighted(&entry.weights, x, y, z)].1;
                    let props = entry.info.props;
                    let smooth = smooth_lighting && props.emission == 0 && model.ambient_occlusion;
                    let tint = models.tint_color(entry.info.tint);
                    for q in &model.quads {
                        if let Some(cull) = q.cullface {
                            let (dx, dy, dz) = cull.offset();
                            if is_opaque((x + dx, y + dy, z + dz)) {
                                continue;
                            }
                        }
                        let qb = ao::fill_quad_bounds(&q.pos, q.face, props.full_cube);
                        let (mult, bright) = if smooth {
                            let a = ao::vertex_brightness_for(world, (x, y, z), q.face, &qb, is_opaque);
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
                        let tint_rgb = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF].map(|v| v as f32 / 255.0);
                        let verts = std::array::from_fn(|i| {
                            let m = q.shade * mult[i];
                            let ch = |t: f32| (m * t * 255.0).clamp(0.0, 255.0) as u8;
                            Vertex {
                                pos: [q.pos[i][0] + lx as f32, q.pos[i][1] + y as f32, q.pos[i][2] + lz as f32],
                                uv: q.uv[i],
                                color: [ch(tint_rgb[0]), ch(tint_rgb[1]), ch(tint_rgb[2]), 255],
                                light: [(bright[i] & 0xFF) as u8, ((bright[i] >> 16) & 0xFF) as u8, 0, 0],
                            }
                        });
                        mesh.push_quad(verts);
                    }
                }
            }
        }
    }
    mesh
}
