//! Item models: `RenderItem.registerItems` mapping, `ItemModelGenerator` meshes for flat items,
//! camera transforms and `getColorFromItemStack` tints.
use glam::{Mat4, Vec3};
use mc_assets::atlas::Atlas;
use mc_assets::blockstate::Variant;
use mc_assets::model::{self, BakedQuad, ModelLoader, UnbakedModel};
use mc_data::color::Colorizer;
use mc_data::item::Item;
use mc_data::Block;
use std::collections::HashMap;
use std::sync::Arc;

/// `item_models.tsv`: `id meta model`, meta -1 for `ItemMeshDefinition` items.
const TABLE: &str = include_str!("item_models.tsv");
/// Extra variants `ModelBakery` loads for items whose model depends on use state.
const EXTRA: &[&str] = &["bow_pulling_0", "bow_pulling_1", "bow_pulling_2", "fishing_rod_cast", "bottle_drinkable", "bottle_splash"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransformType {
    None,
    ThirdPerson,
    FirstPerson,
    Head,
    Gui,
    Ground,
    Fixed,
}

pub struct ItemModel {
    /// Model-space quads (0–1 cube), atlas UVs.
    pub quads: Vec<BakedQuad>,
    /// `isGui3d`: false for `builtin/generated` models.
    pub gui3d: bool,
    /// `builtin/entity`: chests, skulls, banners (`TileEntityItemStackRenderer`).
    pub builtin: bool,
    pub transforms: HashMap<TransformType, Mat4>,
    pub(crate) bounds: [Vec3; 2],
}

impl ItemModel {
    pub fn transform(&self, t: TransformType) -> Mat4 {
        self.transforms.get(&t).copied().unwrap_or(Mat4::IDENTITY)
    }

    pub(crate) fn held_radius(&self) -> f32 {
        let extent = (self.bounds[0] - Vec3::splat(0.5)).abs().max((self.bounds[1] - Vec3::splat(0.5)).abs());
        let m = self.transform(TransformType::ThirdPerson);
        if m.x_axis.w != 0. || m.y_axis.w != 0. || m.z_axis.w != 0. || m.w_axis.w != 1. { return f32::INFINITY; }
        // Triangle inequality bounds any rotation, scale, translation or oversized pack model.
        2. * m.w_axis.truncate().length() + m.x_axis.truncate().length() * extent.x
            + m.y_axis.truncate().length() * extent.y + m.z_axis.truncate().length() * extent.z
    }
}

pub(crate) fn item_bounds(quads: &[BakedQuad]) -> [Vec3; 2] {
    let mut bounds = [Vec3::splat(0.5); 2];
    for p in quads.iter().flat_map(|q| q.pos) {
        let p = Vec3::from_array(p);
        if !p.is_finite() { return [Vec3::splat(f32::NEG_INFINITY), Vec3::splat(f32::INFINITY)]; }
        bounds[0] = bounds[0].min(p); bounds[1] = bounds[1].max(p);
    }
    bounds
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    #[test]
    fn held_radius_contains_oversized_mirrored_and_sheared_models() {
        let bounds = [Vec3::new(-20., -3., -7.), Vec3::new(6., 8., 9.)];
        let mut model = ItemModel { quads: Vec::new(), gui3d: true, builtin: false, transforms: HashMap::new(), bounds };
        let shear = Mat4::from_cols_array(&[-2., 0.3, 0., 0., 0.5, 0.7, -0.2, 0., 0., 0.5, 3., 0., 12., -8., 9., 1.]);
        for m in [Mat4::IDENTITY, Mat4::from_rotation_y(1.3), shear] {
            model.transforms.insert(TransformType::ThirdPerson, m);
            let radius = model.held_radius();
            let transform = Mat4::from_scale(Vec3::splat(2.)) * m * Mat4::from_scale(Vec3::splat(0.5))
                * Mat4::from_translation(Vec3::splat(-0.5));
            for corner in 0..8 {
                let p = Vec3::new(bounds[corner & 1].x, bounds[(corner >> 1) & 1].y, bounds[(corner >> 2) & 1].z);
                assert!(transform.transform_point3(p).length() <= radius);
            }
        }
        model.transforms.insert(TransformType::ThirdPerson, Mat4::perspective_rh(1., 1., 0.1, 100.));
        assert!(!model.held_radius().is_finite());
    }

    /// `RenderItem.setupGuiTransform` leaves `enableGUIStandardItemLighting`'s rotated lights
    /// on for 3D models, so the visible faces get different shades; `builtin/generated` flat
    /// models call `disableLighting()` and stay unlit.
    #[test]
    fn gui_item_lighting_shades_only_3d_models() {
        let quad = |face| BakedQuad {
            pos: [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
            uv: [[0.; 2]; 4],
            face,
            cullface: None,
            tint_index: -1,
            shade: 1.,
        };
        let model = |gui3d| ItemModel {
            quads: mc_core::Facing::ALL.into_iter().map(quad).collect(),
            gui3d,
            builtin: false,
            transforms: HashMap::new(),
            bounds: [Vec3::ZERO, Vec3::ONE],
        };
        let flat = gui_quads(&model(false), |_| 0xFFFFFF);
        assert!(!flat.is_empty());
        assert!(flat.iter().all(|q| q.shade == 1.0));
        let solid = gui_quads(&model(true), |_| 0xFFFFFF);
        assert!(!solid.is_empty());
        assert!(solid.iter().any(|q| q.shade > 0.9), "no fully lit GUI face");
        assert!(solid.iter().any(|q| q.shade < 0.6), "no dim GUI face");
        for q in &solid {
            let expected = (255. * q.shade) as u8;
            assert!(q.color.iter().all(|c| *c == [expected, expected, expected, 255]), "{:?}", q.color);
        }
    }
}

#[derive(Default)]
pub struct ItemModels {
    by_name: HashMap<String, Arc<ItemModel>>,
    exact: HashMap<(u16, u16), String>,
    any: HashMap<u16, String>,
    /// Model name -> `getParticleIcon` sprite (`ItemModelMesher`), kept beside the mesh
    /// because breaking particles only need the texture name.
    particles: HashMap<String, String>,
}

impl ItemModels {
    /// Texture names every item model needs, so they land in the shared atlas.
    pub fn textures(loader: &mut ModelLoader) -> Vec<String> {
        let mut out = Vec::new();
        for name in Self::names() {
            let Some(m) = loader.get(&format!("item/{name}")) else { continue };
            if m.generated {
                for i in 0..5 {
                    let t = m.resolve_texture(&format!("#layer{i}"));
                    if t == mc_assets::atlas::MISSING {
                        break;
                    }
                    out.push(t);
                }
            } else {
                out.extend(m.texture_names());
            }
            let particle = m.resolve_texture("#particle");
            if particle != mc_assets::atlas::MISSING {
                out.push(particle);
            }
        }
        out
    }

    fn names() -> impl Iterator<Item = String> {
        TABLE
            .lines()
            .filter_map(|l| l.split('\t').nth(2))
            .filter(|n| *n != "potion")
            .chain(EXTRA.iter().copied())
            .map(str::to_owned)
    }

    pub fn load(loader: &mut ModelLoader, atlas: &Atlas) -> Self {
        let mut out = Self::default();
        for line in TABLE.lines() {
            let f: Vec<_> = line.split('\t').collect();
            if f.len() != 3 {
                continue;
            }
            let (Ok(id), Ok(meta)) = (f[0].parse::<u16>(), f[1].parse::<i32>()) else { continue };
            if meta < 0 {
                out.any.insert(id, f[2].to_owned());
            } else {
                out.exact.insert((id, meta as u16), f[2].to_owned());
            }
        }
        for name in Self::names() {
            if out.by_name.contains_key(&name) {
                continue;
            }
            let Some(m) = loader.get(&format!("item/{name}")).cloned() else { continue };
            out.particles.insert(name.clone(), particle_sprite(&m));
            out.by_name.insert(name, Arc::new(bake_item(&m, atlas)));
        }
        out
    }

    /// `ItemModelMesher.getItemModel`: exact (id, meta), else the mesh definition, else meta 0.
    pub fn model_name(&self, id: u16, meta: u16) -> Option<&str> {
        if let Some(n) = self.exact.get(&(id, meta)) {
            return Some(n);
        }
        if let Some(n) = self.any.get(&id) {
            if n == "potion" {
                // ItemPotion.isSplash: (meta & 16384) != 0
                return Some(if meta & 16384 != 0 { "bottle_splash" } else { "bottle_drinkable" });
            }
            return Some(n);
        }
        // Damageable items register meta 0 only; any damage value uses it.
        self.exact.get(&(id, 0)).map(String::as_str)
    }

    pub fn get(&self, id: u16, meta: u16) -> Option<&Arc<ItemModel>> {
        self.by_name.get(self.model_name(id, meta)?)
    }

    pub fn by_name(&self, name: &str) -> Option<&Arc<ItemModel>> {
        self.by_name.get(name)
    }

    /// `ItemModelMesher.getParticleIcon`: the atlas sprite an item crack particle draws with.
    pub fn sprite_name(&self, id: u16, meta: u16) -> Option<&str> {
        let name = self.particles.get(self.model_name(id, meta)?)?;
        (!name.is_empty() && name != mc_assets::atlas::MISSING).then_some(name.as_str())
    }
}

/// `getParticleIcon`'s texture: `builtin/generated` items take layer0, block models `#particle`.
fn particle_sprite(m: &UnbakedModel) -> String {
    if m.generated {
        return m.resolve_texture("#layer0");
    }
    let particle = m.resolve_texture("#particle");
    if particle == mc_assets::atlas::MISSING { m.resolve_texture("#layer0") } else { particle }
}

fn bake_item(m: &UnbakedModel, atlas: &Atlas) -> ItemModel {
    let identity = Variant { model: String::new(), x: 0, y: 0, uvlock: false, weight: 1 };
    let (quads, gui3d) = if m.generated {
        match model::generate_item_model(m, atlas) {
            Some(g) => (model::bake(&g, &identity, atlas).quads, false),
            None => (Vec::new(), false),
        }
    } else {
        (model::bake(m, &identity, atlas).quads, true)
    };
    let mut transforms = HashMap::new();
    for (key, t) in [
        ("thirdperson", TransformType::ThirdPerson),
        ("firstperson", TransformType::FirstPerson),
        ("head", TransformType::Head),
        ("gui", TransformType::Gui),
        ("ground", TransformType::Ground),
        ("fixed", TransformType::Fixed),
    ] {
        if let Some(d) = m.display.get(key) {
            transforms.insert(t, d.matrix());
        }
    }
    let bounds = item_bounds(&quads);
    ItemModel { quads, gui3d, builtin: m.built_in, transforms, bounds }
}

/// `Item.getColorFromItemStack(stack, tintIndex)` for the items whose color doesn't come from NBT.
pub fn item_color(id: u16, meta: u16, tint_index: i32, colors: &impl Colorizer) -> u32 {
    let Some(item) = Item::by_id(id) else { return 0xFFFFFF };
    let block_color = || {
        item.block
            .and_then(Block::by_name)
            .and_then(|b| b.state_from_meta(meta as u8 & 15))
            .map_or(0xFFFFFF, |s| s.render_color(colors))
    };
    let class = item.classes.first().copied().unwrap_or("");
    match class {
        "ItemColored" | "ItemLeaves" | "ItemLilyPad" => block_color(),
        // GRASS = 2, FERN = 3
        "ItemDoublePlant" if matches!(meta & 7, 2 | 3) => colors.grass(0.5, 1.0),
        "ItemArmor" if item.name == "minecraft:leather_helmet" || item.name.starts_with("minecraft:leather_") => {
            // ItemArmor without display.color NBT: 10511680 for the base layer.
            if tint_index > 0 {
                0xFFFFFF
            } else {
                10511680
            }
        }
        "ItemPotion" if tint_index <= 0 => potion_color(meta),
        _ => 0xFFFFFF,
    }
    .into_color()
}

trait IntoColor {
    fn into_color(self) -> u32;
}
impl IntoColor for u32 {
    fn into_color(self) -> u32 {
        self & 0xFFFFFF
    }
}

/// `PotionHelper.getLiquidColor`; only the water bottle (no effects) is ported so far.
pub fn potion_color(meta: u16) -> u32 {
    mc_data::potion::meta_color(meta as i32)
}

/// Projects an item model into 16×16 GUI space (`RenderItem.renderItemIntoGUI`): positions in
/// GUI pixels relative to the slot corner, front faces only, back-to-front, lit like vanilla.
pub struct GuiQuad {
    pub pos: [[f32; 2]; 4],
    pub uv: [[f32; 2]; 4],
    pub color: [[u8; 4]; 4],
    /// The quad's tint index (-1 untinted) and lighting, for per-stack recoloring.
    pub tint_index: i32,
    pub shade: f32,
}

pub fn gui_quads(model: &ItemModel, tint: impl Fn(i32) -> u32) -> Vec<GuiQuad> {
    // setupGuiTransform (without the slot translation) → GUI display transform → renderItem.
    let base = Mat4::from_translation(Vec3::new(8., 8., 0.))
        * Mat4::from_scale(Vec3::new(1., 1., -1.))
        * Mat4::from_scale(Vec3::splat(0.5))
        * if model.gui3d {
            Mat4::from_scale(Vec3::splat(40.)) * Mat4::from_rotation_x(210f32.to_radians()) * Mat4::from_rotation_y(-135f32.to_radians())
        } else {
            Mat4::from_scale(Vec3::splat(64.)) * Mat4::from_rotation_x(180f32.to_radians())
        };
    let m = base * model.transform(TransformType::Gui) * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::splat(-0.5));
    let normal_m = glam::Mat3::from_mat4(m).inverse().transpose();
    // enableGUIStandardItemLighting: lights rotated by -30° Y then 165° X (in GUI eye space).
    let lights = crate::entity_model::lights_in(glam::Mat3::from_rotation_y(-30f32.to_radians()) * glam::Mat3::from_rotation_x(165f32.to_radians()));
    let mut quads: Vec<(f32, GuiQuad)> = Vec::new();
    for q in &model.quads {
        let p = q.pos.map(|v| m.transform_point3(Vec3::from(v)));
        let (nx, ny, nz) = q.face.offset();
        let n = crate::entity_model::gl_normal(normal_m, Vec3::new(nx as f32, ny as f32, nz as f32), false);
        // Back faces and edge-on faces are culled (direction only).
        if n.normalize_or_zero().z <= 1e-3 {
            continue;
        }
        let c = if q.tint_index >= 0 { tint(q.tint_index) } else { 0xFFFFFF };
        let shade = if model.gui3d { crate::entity_model::standard_lighting(n, lights) } else { 1.0 };
        let ch = |v: u32| ((v & 255) as f32 * shade) as u8;
        let color = [ch(c >> 16), ch(c >> 8), ch(c), 255];
        let depth = p.iter().map(|v| v.z).sum::<f32>();
        quads.push((depth, GuiQuad { pos: p.map(|v| [v.x, v.y]), uv: q.uv, color: [color; 4], tint_index: q.tint_index, shade }));
    }
    // Larger z is closer to the viewer after scale(1,1,-1); draw far faces first.
    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
    quads.into_iter().map(|(_, q)| q).collect()
}
