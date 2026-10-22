//! Differential color tests using Java results and the real vanilla colormap resources.

use mc_assets::AssetSource;
use mc_core::BlockPos;
use mc_data::biome::Biome;
use mc_data::color::{ColorAccess, Colorizer};
use mc_data::{Block, BlockAccess, StateId};
use mc_render::models::BlockColors;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

fn inputs(name: &str) -> Option<(BlockColors, Vec<Value>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let jar = root.join("../../../MCP-919/jars/versions/1.8.9/1.8.9.jar");
    let path = root.join(format!("../../golden/{name}.json"));
    if !jar.is_file() || !path.is_file() {
        eprintln!("skipping: requires 1.8.9.jar and {} (run tools/extract/extract.ps1)", path.display());
        return None;
    }
    let mut assets = AssetSource::new();
    assets.push(&jar).unwrap();
    Some((BlockColors::load(&assets), serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()))
}

fn pos(row: &Value) -> BlockPos {
    BlockPos::new(row[0].as_i64().unwrap() as i32, row[1].as_i64().unwrap() as i32, row[2].as_i64().unwrap() as i32)
}

#[test]
fn every_biome_color_and_temperature_matches_java() {
    let Some((colors, rows)) = inputs("biomes") else { return };
    assert_eq!(rows.len(), Biome::all().len());
    for row in rows {
        let biome = Biome::by_id(row["id"].as_u64().unwrap() as u8).unwrap();
        assert_eq!(biome.water_color as u64, row["water"].as_u64().unwrap());
        for sample in row["samples"].as_array().unwrap() {
            let pos = pos(sample);
            assert_eq!(biome.temperature_at(pos).to_bits(), sample[3].as_i64().unwrap() as u32, "{} {pos:?}", biome.name);
            assert_eq!(biome.grass_color(pos, &colors) as u64, sample[4].as_u64().unwrap(), "{} {pos:?} grass", biome.name);
            assert_eq!(biome.foliage_color(pos, &colors) as u64, sample[5].as_u64().unwrap(), "{} {pos:?} foliage", biome.name);
        }
    }
}

struct Scene<'a> {
    colors: &'a BlockColors,
    states: HashMap<BlockPos, StateId>,
    biomes: HashMap<BlockPos, u8>,
    pot: Option<(i32, i32)>,
}
impl BlockAccess for Scene<'_> {
    fn state(&self, pos: BlockPos) -> StateId {
        self.states.get(&pos).copied().unwrap_or(StateId::AIR)
    }
    fn flower_pot(&self, _pos: BlockPos) -> Option<(i32, i32)> {
        self.pot
    }
}
impl Scene<'_> {
    fn biome(&self, pos: BlockPos) -> &'static Biome {
        Biome::by_id(*self.biomes.get(&pos).unwrap_or(&1)).unwrap()
    }
}
impl ColorAccess for Scene<'_> {
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

#[test]
fn every_block_color_matches_java_in_mixed_biomes() {
    let Some((colors, rows)) = inputs("colors") else { return };
    let states: HashMap<_, _> = Block::all().iter().flat_map(Block::states).map(|s| (s.to_string(), s)).collect();
    assert_eq!(rows.len(), states.len());
    for row in rows {
        let state = states[row["state"].as_str().unwrap()];
        let pos = pos(&row["pos"]);
        let mut scene = Scene { colors: &colors, states: HashMap::from([(pos, state)]), biomes: HashMap::new(), pot: None };
        for (i, id) in row["biomes"].as_array().unwrap().iter().enumerate() {
            scene.biomes.insert(pos.offset((i % 3) as i32 - 1, 0, (i / 3) as i32 - 1), id.as_u64().unwrap() as u8);
        }
        if let Some(below) = row["below"].as_str() {
            scene.states.insert(pos.down(), states[below]);
        }
        if let Some(pot) = row["pot"].as_array() {
            scene.pot = Some((pot[0].as_i64().unwrap() as i32, pot[1].as_i64().unwrap() as i32));
        }
        assert_eq!(state.block().block_color(&colors) as u64, row["block_color"].as_u64().unwrap(), "{state} block color");
        assert_eq!(state.render_color(&colors) as u64, row["render_color"].as_u64().unwrap(), "{state} render color");
        assert_eq!(state.block().color_multiplier(&scene, pos) as u64, row["multiplier"].as_u64().unwrap(), "{state} multiplier");
    }
}

#[test]
fn java_float_climate_is_widened_after_narrowing() {
    let Some((colors, _)) = inputs("biomes") else { return };
    let plains = Biome::by_id(1).unwrap();
    assert_eq!(plains.grass_color(BlockPos::new(0, 64, 0), &colors), colors.grass(0.8f32 as f64, 0.4f32 as f64));
}
