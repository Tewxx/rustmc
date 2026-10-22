//! Runs against the local 1.8.9.jar; skipped when it isn't present.

use mc_assets::AssetSource;
use mc_render::{mesh, models::BlockModels};
use mc_world::{blocks, World};
use std::path::Path;

const JAR: &str = "../../../MCP-919/jars/versions/1.8.9/1.8.9.jar";

fn assets() -> Option<AssetSource> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(JAR);
    if !p.is_file() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    let mut a = AssetSource::new();
    a.push(&p).unwrap();
    Some(a)
}

#[test]
fn every_known_block_bakes_with_real_textures() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let missing = models.atlas.sprite(mc_assets::atlas::MISSING);
    let mut bad = Vec::new();
    for (id, meta, info) in blocks::all_known() {
        match models.get(id, meta) {
            Some(e) if !e.models.is_empty() && e.models.iter().all(|(_, m)| !m.quads.is_empty()) => {
                let uses_missing = e.models.iter().flat_map(|(_, m)| &m.quads).any(|q| q.uv[0][0] >= missing.min_u && q.uv[0][0] < missing.max_u && q.uv[0][1] >= missing.min_v && q.uv[0][1] < missing.max_v);
                if uses_missing {
                    bad.push(format!("{id}:{meta} {} uses missing texture", info.blockstate));
                }
            }
            _ => bad.push(format!("{id}:{meta} {} [{}] has no model", info.blockstate, info.variant)),
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn full_cube_neighbours_are_culled() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let mut w = World::default();
    let mut c = mc_world::chunk::Chunk::new(0, 0);
    c.set(0, 10, 0, 1, 0);
    w.insert(c.clone());
    assert_eq!(mesh::build(&w, &models, 0, 0, true).indices.len() / 6, 6);
    c.set(1, 10, 0, 1, 0);
    w.insert(c);
    // Two touching stone blocks: 12 faces minus the 2 shared ones.
    assert_eq!(mesh::build(&w, &models, 0, 0, true).indices.len() / 6, 10);
}
