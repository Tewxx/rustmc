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
}

impl ItemModel {
    pub fn transform(&self, t: TransformType) -> Mat4 {
        self.transforms.get(&t).copied().unwrap_or(Mat4::IDENTITY)
    }
}

#[derive(Default)]
pub struct ItemModels {
    by_name: HashMap<String, Arc<ItemModel>>,
    exact: HashMap<(u16, u16), String>,
    any: HashMap<u16, String>,
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
    ItemModel { quads, gui3d, builtin: m.built_in, transforms }
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
    let normal_m = m.inverse().transpose();
    // enableGUIStandardItemLighting: lights rotated by -30° Y then 165° X (in GUI eye space).
    let lights = crate::entity_model::lights_in(glam::Mat3::from_rotation_y(-30f32.to_radians()) * glam::Mat3::from_rotation_x(165f32.to_radians()));
    let mut quads: Vec<(f32, GuiQuad)> = Vec::new();
    for q in &model.quads {
        let p = q.pos.map(|v| m.transform_point3(Vec3::from(v)));
        let (nx, ny, nz) = q.face.offset();
        let n = normal_m.transform_vector3(Vec3::new(nx as f32, ny as f32, nz as f32)).normalize_or_zero();
        // Back faces and edge-on faces are culled.
        if n.z <= 1e-3 {
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
