//! Dense state lookup with one baked model per distinct vanilla model resource location.

use mc_assets::atlas::Atlas;
use mc_assets::blockstate::{BlockStateDef, Variant};
use mc_assets::colormap::ColorMap;
use mc_assets::model::{self, BakedModel, ModelLoader};
use mc_assets::AssetSource;
use mc_core::BlockPos;
use mc_data::biome::Biome;
use mc_data::color::{ColorAccess, Colorizer};
use mc_data::model_location::ModelLocation;
use mc_data::{Block, BlockAccess, StateId};
use mc_world::World;
use std::collections::HashMap;

pub struct BlockEntry {
    /// Weighted alternatives in vanilla's sorted order.
    pub models: Vec<(i32, BakedModel)>,
    pub weights: Vec<i32>,
}

pub struct BlockModels {
    pub atlas: Atlas,
    /// States differing only in ignored properties share the same model.
    entries: Vec<Option<usize>>,
    baked: Vec<BlockEntry>,
    /// `Block.getParticleTexture()` per state, in atlas UV space
    /// (`BlockModelShapes.getTexture`), used by block crack/dust and destroy particles.
    state_particles: Vec<Option<[f32; 4]>>,
    missing: Vec<ModelLocation>,
    pub colors: BlockColors,
    pub items: crate::items::ItemModels,
    /// `item_frame#normal` / `item_frame#map`: the entity model `RenderItemFrame` bakes
    /// outside the blockstate table.
    pub item_frame: Option<BakedModel>,
    pub item_frame_map: Option<BakedModel>,
}

/// `ModelBakery.LOCATIONS_BUILTIN_TEXTURES`
const BUILTIN_TEXTURES: &[&str] = &[
    "blocks/water_flow",
    "blocks/water_still",
    "blocks/lava_flow",
    "blocks/lava_still",
    "blocks/destroy_stage_0",
    "blocks/destroy_stage_1",
    "blocks/destroy_stage_2",
    "blocks/destroy_stage_3",
    "blocks/destroy_stage_4",
    "blocks/destroy_stage_5",
    "blocks/destroy_stage_6",
    "blocks/destroy_stage_7",
    "blocks/destroy_stage_8",
    "blocks/destroy_stage_9",
    "items/empty_armor_slot_helmet",
    "items/empty_armor_slot_chestplate",
    "items/empty_armor_slot_leggings",
    "items/empty_armor_slot_boots",
];

pub struct BlockColors {
    grass: Option<ColorMap>,
    foliage: Option<ColorMap>,
}

impl BlockColors {
    pub fn load(assets: &AssetSource) -> Self {
        Self { grass: ColorMap::load(assets, "grass"), foliage: ColorMap::load(assets, "foliage") }
    }
}

impl Colorizer for BlockColors {
    fn grass(&self, temperature: f64, rainfall: f64) -> u32 {
        self.grass.as_ref().map_or(0, |c| c.get(temperature, rainfall))
    }
    fn foliage(&self, temperature: f64, rainfall: f64) -> u32 {
        self.foliage.as_ref().map_or(0, |c| c.get(temperature, rainfall))
    }
}

impl BlockModels {
    pub fn load(assets: &AssetSource) -> Self {
        let mut loader = ModelLoader::new(assets);
        let mut definitions: HashMap<String, Option<BlockStateDef>> = HashMap::new();
        let mut locations = HashMap::new();
        let mut entries = vec![None; mc_data::block::STATE_COUNT];
        let mut pending: Vec<Option<Vec<Variant>>> = Vec::new();
        let mut particle_models: Vec<Option<String>> = Vec::new();
        let mut textures = Vec::new();
        let mut missing = Vec::new();
        for state in Block::all().iter().flat_map(Block::states) {
            let Some(loc) = state.model_location() else { continue };
            let index = *locations.entry(loc.clone()).or_insert_with(|| {
                let def = definitions
                    .entry(loc.name.clone())
                    .or_insert_with(|| BlockStateDef::load(assets, loc.name.trim_start_matches("minecraft:")));
                let variants = def.as_ref().and_then(|d| d.variants.get(&loc.variant));
                let mut resolved = Vec::new();
                let mut first_model = None;
                if let Some(variants) = variants {
                    for v in variants {
                        if let Some(m) = loader.get(&format!("block/{}", v.model)) {
                            if first_model.is_none() {
                                first_model = Some(format!("block/{}", v.model));
                            }
                            textures.extend(m.texture_names());
                            resolved.push(v.clone());
                        }
                    }
                }
                // Vanilla uses the missing model for absent keys; choosing a different variant
                // silently renders a valid-looking but incorrect block.
                if resolved.is_empty() {
                    missing.push(loc.clone());
                }
                // ModelBakery skips unresolved alternatives; the entire location falls back
                // to the missing model only when none resolved.
                let present = !resolved.is_empty();
                pending.push(if present { Some(resolved) } else { None });
                particle_models.push(if present { first_model } else { None });
                pending.len() - 1
            });
            entries[state.0 as usize] = Some(index);
        }
        textures.extend(crate::items::ItemModels::textures(&mut loader));
        // `RenderItemFrame`'s two models are not blockstates; their textures still belong in
        // the block atlas.
        for name in ["block/item_frame", "block/item_frame_map"] {
            if let Some(m) = loader.get(name) {
                textures.extend(m.texture_names());
            }
        }
        textures.extend(BUILTIN_TEXTURES.iter().map(|t| t.to_string()));
        // Block particle textures (breaking particles, `getParticleTexture`).
        let atlas = Atlas::build(assets, textures);
        let items = crate::items::ItemModels::load(&mut loader, &atlas);
        let identity = Variant { model: String::new(), x: 0, y: 0, uvlock: false, weight: 1 };
        let item_frame = loader.get("block/item_frame").map(|m| model::bake(m, &identity, &atlas));
        let item_frame_map = loader.get("block/item_frame_map").map(|m| model::bake(m, &identity, &atlas));
        let baked = pending
            .into_iter()
            .map(|variants| {
                let mut models = match variants {
                    Some(variants) => variants
                        .iter()
                        .map(|v| {
                            let m = loader.get(&format!("block/{}", v.model)).unwrap();
                            (v.weight, model::bake(m, v, &atlas))
                        })
                        .collect(),
                    None => vec![(1, model::missing_model(&atlas))],
                };
                model::sort_weighted(&mut models);
                let weights = models.iter().map(|(w, _)| *w).collect();
                BlockEntry { models, weights }
            })
            .collect();
        // `BlockModelShapes.getTexture(state)`: the model's `particle` texture. Models that do
        // not declare one resolve to the missing sprite, like vanilla.
        let mut state_particles = vec![None; mc_data::block::STATE_COUNT];
        for state in Block::all().iter().flat_map(Block::states) {
            let Some(loc) = entries[state.0 as usize].and_then(|i| particle_models.get(i)).cloned().flatten() else { continue };
            let Some(m) = loader.get(&loc) else { continue };
            let s = atlas.sprite(&m.resolve_texture("#particle"));
            state_particles[state.0 as usize] = Some([s.min_u, s.min_v, s.max_u, s.max_v]);
        }
        Self {
            atlas,
            entries,
            baked,
            state_particles,
            missing,
            colors: BlockColors::load(assets),
            items,
            item_frame,
            item_frame_map,
        }
    }

    pub fn get_state(&self, state: StateId) -> Option<&BlockEntry> {
        self.entries[state.0 as usize].map(|i| &self.baked[i])
    }

    pub fn get(&self, id: u16, meta: u8) -> Option<&BlockEntry> {
        if id > 0xFFF || meta > 15 {
            return None;
        }
        self.get_state(StateId::from_raw(id << 4 | meta as u16))
    }

    pub fn missing_locations(&self) -> &[ModelLocation] {
        &self.missing
    }

    /// `Block.getParticleTexture()` for a state, in atlas UV space.
    pub fn particle_uv(&self, state: StateId) -> Option<[f32; 4]> {
        self.state_particles.get(state.0 as usize).copied().flatten()
    }

    pub fn tint_color(&self, world: &World, state: StateId, pos: BlockPos) -> u32 {
        state.block().color_multiplier(&ColorWorld { world, colors: &self.colors }, pos)
    }
}

struct ColorWorld<'a> {
    world: &'a World,
    colors: &'a BlockColors,
}

impl BlockAccess for ColorWorld<'_> {
    fn state(&self, pos: BlockPos) -> StateId {
        self.world.state(pos)
    }
    fn flower_pot(&self, pos: BlockPos) -> Option<(i32, i32)> {
        self.world.flower_pot(pos)
    }
    fn comparator_output(&self, pos: BlockPos) -> Option<i32> {
        self.world.comparator_output(pos)
    }
}

impl ColorWorld<'_> {
    fn biome(&self, pos: BlockPos) -> &'static Biome {
        Biome::by_id(self.world.biome_id(pos)).unwrap_or_else(|| Biome::by_id(1).unwrap())
    }
}

impl ColorAccess for ColorWorld<'_> {
    fn grass_color(&self, pos: BlockPos) -> u32 {
        self.biome(pos).grass_color(pos, self.colors)
    }
    fn foliage_color(&self, pos: BlockPos) -> u32 {
        self.biome(pos).foliage_color(pos, self.colors)
    }
    fn water_color(&self, pos: BlockPos) -> u32 {
        self.biome(pos).water_color
    }
}
