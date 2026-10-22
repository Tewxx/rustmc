//! Bakes every block the id table knows into one atlas + per-(id, meta) model lists.

use mc_assets::atlas::Atlas;
use mc_assets::blockstate::{BlockStateDef, Variant};
use mc_assets::colormap::ColorMap;
use mc_assets::model::{self, BakedModel, ModelLoader};
use mc_assets::AssetSource;
use mc_world::blocks::{self, BlockInfo, Tint};
use std::collections::HashMap;

pub struct BlockEntry {
    pub info: BlockInfo,
    /// Weighted alternatives in vanilla's sorted order.
    pub models: Vec<(i32, BakedModel)>,
    pub weights: Vec<i32>,
}

pub struct BlockModels {
    pub atlas: Atlas,
    entries: HashMap<(u16, u8), BlockEntry>,
    grass: u32,
    foliage: u32,
}

// Plains biome climate; real biomes come with chunk biome data later.
const PLAINS_TEMP: f64 = 0.8;
const PLAINS_RAIN: f64 = 0.4;

impl BlockModels {
    pub fn load(assets: &AssetSource) -> Self {
        let mut loader = ModelLoader::new(assets);
        let mut states: HashMap<String, Option<BlockStateDef>> = HashMap::new();
        let mut pending: Vec<((u16, u8), BlockInfo, Vec<Variant>)> = Vec::new();
        let mut textures = Vec::new();

        for (id, meta, info) in blocks::all_known() {
            let def = states.entry(info.blockstate.clone()).or_insert_with(|| BlockStateDef::load(assets, &info.blockstate));
            let Some(def) = def else { continue };
            // Fall back to the first variant when our key guess differs from the file.
            let Some(variants) = def.variants.get(&info.variant).or_else(|| def.variants.values().next()) else { continue };
            for v in variants {
                if let Some(m) = loader.get(&format!("block/{}", v.model)) {
                    textures.extend(m.texture_names());
                }
            }
            pending.push(((id, meta), info, variants.clone()));
        }

        let atlas = Atlas::build(assets, textures);
        let mut entries = HashMap::new();
        for (key, info, variants) in pending {
            let mut models: Vec<(i32, BakedModel)> = variants
                .iter()
                .filter_map(|v| loader.get(&format!("block/{}", v.model)).map(|m| (v.weight, model::bake(m, v, &atlas))))
                .collect();
            model::sort_weighted(&mut models);
            let weights = models.iter().map(|(w, _)| *w).collect();
            entries.insert(key, BlockEntry { info, models, weights });
        }

        let grass = ColorMap::load(assets, "grass").map_or(0x91BD59, |c| c.get(PLAINS_TEMP, PLAINS_RAIN));
        let foliage = ColorMap::load(assets, "foliage").map_or(0x77AB2F, |c| c.get(PLAINS_TEMP, PLAINS_RAIN));
        Self { atlas, entries, grass, foliage }
    }

    pub fn get(&self, id: u16, meta: u8) -> Option<&BlockEntry> {
        self.entries.get(&(id, meta))
    }

    pub fn tint_color(&self, tint: Tint) -> u32 {
        match tint {
            Tint::None => 0xFFFFFF,
            Tint::Grass => self.grass,
            Tint::Foliage => self.foliage,
            Tint::Fixed(c) => c,
        }
    }

    pub fn is_opaque(&self, id: u16, meta: u8) -> bool {
        self.entries.get(&(id, meta)).is_some_and(|e| e.info.opaque)
    }
}
