//! Block model JSON (`ModelBlock` / `BlockPart`) and baking (`FaceBakery`).

use crate::atlas::{Atlas, MISSING};
use crate::blockstate::Variant;
use crate::AssetSource;
use glam::{Mat3, Vec3};
use mc_core::{Axis, Facing};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct FaceDef {
    pub uv: [f32; 4],
    pub texture: String,
    pub cullface: Option<Facing>,
    pub rotation: i32,
    pub tint_index: i32,
}

#[derive(Clone, Debug)]
pub struct PartRotation {
    pub origin: Vec3,
    pub axis: Axis,
    pub angle: f32,
    pub rescale: bool,
}

#[derive(Clone, Debug)]
pub struct Element {
    pub from: Vec3,
    pub to: Vec3,
    pub rotation: Option<PartRotation>,
    pub shade: bool,
    pub faces: Vec<(Facing, FaceDef)>,
}

/// A model with its parent chain flattened.
#[derive(Clone, Debug, Default)]
pub struct UnbakedModel {
    pub textures: HashMap<String, String>,
    pub elements: Vec<Element>,
    pub ambient_occlusion: bool,
}

impl UnbakedModel {
    /// `resolveTextureName`: follows `#ref` chains; unresolved names become missingno.
    pub fn resolve_texture(&self, name: &str) -> String {
        let mut cur = name;
        for _ in 0..16 {
            match cur.strip_prefix('#') {
                Some(key) => match self.textures.get(key) {
                    Some(next) => cur = next,
                    None => return MISSING.to_string(),
                },
                None => return cur.to_string(),
            }
        }
        MISSING.to_string()
    }

    pub fn texture_names(&self) -> impl Iterator<Item = String> + '_ {
        self.elements.iter().flat_map(|e| e.faces.iter().map(|(_, f)| self.resolve_texture(&f.texture)))
    }
}

/// Loads and caches flattened models by name (`block/stone`).
pub struct ModelLoader<'a> {
    assets: &'a AssetSource,
    cache: HashMap<String, Option<UnbakedModel>>,
}

struct RawModel {
    parent: Option<String>,
    textures: HashMap<String, String>,
    elements: Option<Vec<Element>>,
    ambient_occlusion: bool,
}

impl<'a> ModelLoader<'a> {
    pub fn new(assets: &'a AssetSource) -> Self {
        Self { assets, cache: HashMap::new() }
    }

    pub fn get(&mut self, name: &str) -> Option<&UnbakedModel> {
        if !self.cache.contains_key(name) {
            let m = self.flatten(name, 0);
            self.cache.insert(name.to_string(), m);
        }
        self.cache.get(name).unwrap().as_ref()
    }

    fn flatten(&self, name: &str, depth: usize) -> Option<UnbakedModel> {
        if depth > 32 {
            return None;
        }
        let raw = parse_raw(&self.assets.read_json(&format!("models/{name}.json"))?)?;
        let mut out = match &raw.parent {
            Some(p) if !p.starts_with("builtin/") => self.flatten(p, depth + 1)?,
            _ => UnbakedModel { ambient_occlusion: true, ..Default::default() },
        };
        out.textures.extend(raw.textures);
        if let Some(e) = raw.elements {
            out.elements = e;
        }
        out.ambient_occlusion = out.ambient_occlusion && raw.ambient_occlusion;
        Some(out)
    }
}

fn vec3(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
}

fn parse_raw(json: &Value) -> Option<RawModel> {
    let textures = json
        .get("textures")
        .and_then(Value::as_object)
        .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    let elements = json.get("elements").and_then(Value::as_array).map(|a| a.iter().filter_map(parse_element).collect());
    Some(RawModel {
        parent: json.get("parent").and_then(Value::as_str).map(str::to_string),
        textures,
        elements,
        ambient_occlusion: json.get("ambientocclusion").and_then(Value::as_bool).unwrap_or(true),
    })
}

fn parse_element(e: &Value) -> Option<Element> {
    let from = vec3(e.get("from")?)?;
    let to = vec3(e.get("to")?)?;
    let rotation = e.get("rotation").and_then(|r| {
        Some(PartRotation {
            origin: vec3(r.get("origin")?)? / 16.0,
            axis: match r.get("axis")?.as_str()? {
                "x" => Axis::X,
                "y" => Axis::Y,
                "z" => Axis::Z,
                _ => return None,
            },
            angle: r.get("angle")?.as_f64()? as f32,
            rescale: r.get("rescale").and_then(Value::as_bool).unwrap_or(false),
        })
    });
    let mut faces = Vec::new();
    for (k, f) in e.get("faces")?.as_object()? {
        let Some(dir) = Facing::by_name(k) else { continue };
        let uv = match f.get("uv").and_then(Value::as_array) {
            Some(a) if a.len() == 4 => {
                [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0) as f32)
            }
            _ => default_uv(dir, from, to),
        };
        faces.push((
            dir,
            FaceDef {
                uv,
                texture: f.get("texture")?.as_str()?.to_string(),
                cullface: f.get("cullface").and_then(Value::as_str).and_then(Facing::by_name),
                rotation: f.get("rotation").and_then(Value::as_i64).unwrap_or(0) as i32,
                tint_index: f.get("tintindex").and_then(Value::as_i64).unwrap_or(-1) as i32,
            },
        ));
    }
    Some(Element { from, to, rotation, shade: e.get("shade").and_then(Value::as_bool).unwrap_or(true), faces })
}

/// `BlockPart.getFaceUvs` (1.8 uses the same mapping for down and up).
fn default_uv(f: Facing, from: Vec3, to: Vec3) -> [f32; 4] {
    match f {
        Facing::Down | Facing::Up => [from.x, from.z, to.x, to.z],
        Facing::North | Facing::South => [from.x, 16.0 - to.y, to.x, 16.0 - from.y],
        Facing::West | Facing::East => [from.z, 16.0 - to.y, to.z, 16.0 - from.y],
    }
}

#[derive(Clone, Debug)]
pub struct BakedQuad {
    /// Block-local positions (0–1 for a full cube).
    pub pos: [[f32; 3]; 4],
    /// Atlas UVs.
    pub uv: [[f32; 2]; 4],
    /// Facing derived from the vertex normal, like `getFacingFromVertexData`.
    pub face: Facing,
    pub cullface: Option<Facing>,
    pub tint_index: i32,
    /// Vanilla per-face brightness (0.5/0.6/0.8/1.0), or 1.0 when `shade` is false.
    pub shade: f32,
}

#[derive(Clone, Debug, Default)]
pub struct BakedModel {
    pub quads: Vec<BakedQuad>,
    pub ambient_occlusion: bool,
}

// Corner order per face, from `EnumFaceDirection`. Each entry picks (x, y, z) from
// min (false) or max (true).
const FACE_VERTS: [[[bool; 3]; 4]; 6] = [
    // DOWN
    [[false, false, true], [false, false, false], [true, false, false], [true, false, true]],
    // UP
    [[false, true, false], [false, true, true], [true, true, true], [true, true, false]],
    // NORTH
    [[true, true, false], [true, false, false], [false, false, false], [false, true, false]],
    // SOUTH
    [[false, true, true], [false, false, true], [true, false, true], [true, true, true]],
    // WEST
    [[false, true, false], [false, false, false], [false, false, true], [false, true, true]],
    // EAST
    [[true, true, true], [true, false, true], [true, false, false], [true, true, false]],
];

fn face_brightness(f: Facing) -> f32 {
    match f {
        Facing::Down => 0.5,
        Facing::Up => 1.0,
        Facing::North | Facing::South => 0.8,
        Facing::West | Facing::East => 0.6,
    }
}

/// `ModelRotation`: X applied first, then Y, both negated.
fn model_rotation(x: i32, y: i32) -> Option<Mat3> {
    if x == 0 && y == 0 {
        return None;
    }
    Some(Mat3::from_rotation_y((-y as f32).to_radians()) * Mat3::from_rotation_x((-x as f32).to_radians()))
}

/// `EnumFacing.rotateAround(X)`: east/west are unchanged.
fn rotate_around_x(f: Facing) -> Facing {
    match f {
        Facing::West | Facing::East => f,
        f => f.rotate_x(),
    }
}

/// `ModelRotation.rotateFace`.
fn rotate_face(f: Facing, qx: i32, qy: i32) -> Facing {
    let mut f = f;
    for _ in 0..qx {
        f = rotate_around_x(f);
    }
    if f.axis() != Axis::Y {
        for _ in 0..qy {
            f = f.rotate_y();
        }
    }
    f
}

/// `ModelRotation.rotateVertex`: where vertex `i` of a face is stored after rotation.
fn rotate_vertex(f: Facing, i: usize, qx: i32, qy: i32) -> usize {
    let mut j = i as i32;
    if f.axis() == Axis::X {
        j = (j + qx) % 4;
    }
    let mut g = f;
    for _ in 0..qx {
        g = rotate_around_x(g);
    }
    if g.axis() == Axis::Y {
        j = (j + qy) % 4;
    }
    j as usize
}

/// `FaceBakery.getFacingFromVertexData`.
fn facing_from_vertices(p: &[[f32; 3]; 4]) -> Facing {
    let (a, b, c) = (Vec3::from(p[0]), Vec3::from(p[1]), Vec3::from(p[2]));
    let n = (c - b).cross(a - b).normalize();
    let mut best = None;
    let mut best_dot = 0.0;
    for f in Facing::ALL {
        let (x, y, z) = f.offset();
        let d = n.dot(Vec3::new(x as f32, y as f32, z as f32));
        if d >= 0.0 && d > best_dot {
            best_dot = d;
            best = Some(f);
        }
    }
    best.unwrap_or(Facing::Up)
}

fn rotate_part(p: Vec3, r: &PartRotation) -> Vec3 {
    let rad = r.angle.to_radians();
    let (m, mut scale) = match r.axis {
        Axis::X => (Mat3::from_rotation_x(rad), Vec3::new(0.0, 1.0, 1.0)),
        Axis::Y => (Mat3::from_rotation_y(rad), Vec3::new(1.0, 0.0, 1.0)),
        Axis::Z => (Mat3::from_rotation_z(rad), Vec3::new(1.0, 1.0, 0.0)),
    };
    if r.rescale {
        let s = if r.angle.abs() == 22.5 {
            1.0 / (0.39269909262657166f64.cos() as f32) - 1.0
        } else {
            1.0 / (std::f64::consts::FRAC_PI_4.cos() as f32) - 1.0
        };
        scale = scale * s + Vec3::ONE;
    } else {
        scale = Vec3::ONE;
    }
    (m * (p - r.origin)) * scale + r.origin
}

pub fn bake(model: &UnbakedModel, variant: &Variant, atlas: &Atlas) -> BakedModel {
    let rot = model_rotation(variant.x, variant.y);
    // ModelRotation.quartersX/Y
    let (qx, qy) = ((variant.x / 90).abs(), (variant.y / 90).abs());
    let mut quads = Vec::new();
    for e in &model.elements {
        let min = e.from / 16.0;
        let max = e.to / 16.0;
        for (dir, face) in &e.faces {
            let sprite = atlas.sprite(&model.resolve_texture(&face.texture));
            let mut pos = [[0.0; 3]; 4];
            let mut uv = [[0.0; 2]; 4];
            for i in 0..4 {
                let c = FACE_VERTS[dir.index()][i];
                let mut p = Vec3::new(
                    if c[0] { max.x } else { min.x },
                    if c[1] { max.y } else { min.y },
                    if c[2] { max.z } else { min.z },
                );
                if let Some(r) = &e.rotation {
                    p = rotate_part(p, r);
                }
                let mut store = i;
                if let Some(m) = rot {
                    p = m * (p - Vec3::splat(0.5)) + Vec3::splat(0.5);
                    store = rotate_vertex(*dir, i, qx, qy);
                }
                pos[store] = p.into();
                // BlockFaceUV.func_178348_a / func_178346_b
                let k = (i as i32 + face.rotation / 90).rem_euclid(4);
                let u = if k == 0 || k == 1 { face.uv[0] } else { face.uv[2] };
                let v = if k == 0 || k == 3 { face.uv[1] } else { face.uv[3] };
                uv[store] = [sprite.u(u), sprite.v(v)];
            }
            quads.push(BakedQuad {
                face: facing_from_vertices(&pos),
                pos,
                uv,
                cullface: face.cullface.map(|f| rotate_face(f, qx, qy)),
                tint_index: face.tint_index,
                shade: if e.shade { face_brightness(rotate_face(*dir, qx, qy)) } else { 1.0 },
            });
        }
    }
    BakedModel { quads, ambient_occlusion: model.ambient_occlusion }
}

/// `WeightedBakedModel.getAlternativeModel` with `MathHelper.getPositionRandom`.
/// `weights` must be in vanilla's sorted order (see [`sort_weighted`]).
pub fn pick_weighted(weights: &[i32], x: i32, y: i32, z: i32) -> usize {
    let total: i32 = weights.iter().sum();
    if total <= 0 {
        return 0;
    }
    let rand = mc_core::math::coordinate_random(x, y, z);
    let mut r = ((rand as i32) >> 16).wrapping_abs() % total;
    for (i, w) in weights.iter().enumerate() {
        r -= w;
        if r < 0 {
            return i;
        }
    }
    0
}

/// Vanilla sorts alternatives by weight descending, then quad count ascending (stable).
pub fn sort_weighted(models: &mut [(i32, BakedModel)]) {
    models.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.quads.len().cmp(&b.1.quads.len())));
}
