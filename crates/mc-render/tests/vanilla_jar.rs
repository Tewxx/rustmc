//! Runs against the local 1.8.9.jar; skipped when it isn't present.

use mc_assets::AssetSource;
use mc_render::{mesh, models::BlockModels};
use mc_core::BlockPos;
use mc_data::{ids, Block, BlockAccess};
use mc_world::World;
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
fn every_mapped_state_bakes_with_real_textures() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let missing = models.atlas.sprite(mc_assets::atlas::MISSING);
    let mut bad = Vec::new();
    assert!(models.missing_locations().is_empty(), "missing locations: {:?}", models.missing_locations());
    for state in Block::all().iter().flat_map(Block::states).filter(|s| s.model_location().is_some()) {
        match models.get_state(state) {
            Some(e) if !e.models.is_empty() && e.models.iter().all(|(_, m)| !m.quads.is_empty()) => {
                let uses_missing = e.models.iter().flat_map(|(_, m)| &m.quads).any(|q| q.uv[0][0] >= missing.min_u && q.uv[0][0] < missing.max_u && q.uv[0][1] >= missing.min_v && q.uv[0][1] < missing.max_v);
                if uses_missing {
                    bad.push(format!("{state} uses missing texture"));
                }
            }
            _ => bad.push(format!("{state} has no model")),
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
    assert_eq!(mesh::build(&w, &models, 0, 0, true, true).indices.len() / 6, 6);
    c.set(1, 10, 0, 1, 0);
    w.insert(c);
    // Two touching stone blocks: 12 faces minus the 2 shared ones.
    assert_eq!(mesh::build(&w, &models, 0, 0, true, true).indices.len() / 6, 10);
}

#[test]
fn identical_glass_blocks_hide_their_shared_faces() {
    let Some(a)=assets() else{return};let models=BlockModels::load(&a);
    let mut world=World::default();let mut chunk=mc_world::chunk::Chunk::new(0,0);
    chunk.set(0,10,0,ids::GLASS,0);chunk.set(1,10,0,ids::GLASS,0);world.insert(chunk);
    assert_eq!(mesh::build(&world,&models,0,0,false,true).indices.len()/6,10);
}

#[test]
fn translucent_models_are_meshed_and_marked_for_the_sorted_pass() {
    let Some(a)=assets() else{return};let models=BlockModels::load(&a);
    let mut w=World::default();let mut c=mc_world::chunk::Chunk::new(0,0);
    c.set(0,10,0,ids::STAINED_GLASS,4);c.set(2,10,0,ids::STONE,0);w.insert(c);
    let result=mesh::build(&w,&models,0,0,false,true);
    assert_eq!(result.indices.len()/6,12);
    assert_eq!(result.vertices.chunks_exact(4).filter(|q|q[0].light[3]==255).count(),6);
    assert_eq!(result.vertices.chunks_exact(4).filter(|q|q[0].light[3]==0).count(),6);
}

#[test]
fn same_height_slabs_hide_shared_faces_even_across_different_materials() {
    let Some(a)=assets() else{return};let models=BlockModels::load(&a);
    let mut world=World::default();let mut chunk=mc_world::chunk::Chunk::new(0,0);
    chunk.set(0,10,0,ids::STONE_SLAB,0);chunk.set(1,10,0,ids::WOODEN_SLAB,0);world.insert(chunk.clone());
    assert_eq!(mesh::build(&world,&models,0,0,false,true).indices.len()/6,10);
    chunk.set(1,10,0,ids::WOODEN_SLAB,8);world.insert(chunk);
    assert_eq!(mesh::build(&world,&models,0,0,false,true).indices.len()/6,12);
}

#[test]
fn a_solid_block_above_a_bottom_slab_does_not_hide_its_inset_top_face() {
    let Some(a)=assets() else{return};let models=BlockModels::load(&a);
    let mut world=World::default();let mut chunk=mc_world::chunk::Chunk::new(0,0);
    chunk.set(0,10,0,ids::STONE_SLAB,0);chunk.set(0,11,0,ids::STONE,0);world.insert(chunk);
    let result=mesh::build(&world,&models,0,0,false,true);
    assert_eq!(result.indices.len()/6,12);
    assert!(result.vertices.chunks_exact(4).any(|quad|quad.iter().all(|v|v.pos[1]==10.5)));
}

#[test]
fn fast_leaves_occlude_neighbouring_stone_but_fancy_leaves_do_not() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let mut w = World::default();
    let mut c = mc_world::chunk::Chunk::new(0, 0);
    c.set(0, 10, 0, ids::LEAVES, 0);
    c.set(1, 10, 0, ids::STONE, 0);
    w.insert(c);
    let fast = mesh::build(&w, &models, 0, 0, true, false);
    let fancy = mesh::build(&w, &models, 0, 0, true, true);
    // Vanilla's leaves.json deliberately has no cullface on any quad: the leaf cube's
    // own faces all render in both modes, including the one against the stone.
    assert_eq!(fast.indices.len() / 6, 11);
    assert_eq!(fancy.indices.len() / 6, 12);
    assert!(fast.vertices.iter().all(|v| v.light[2] == 0));
    // Fancy leaves are CUTOUT_MIPPED.
    assert!(fancy.vertices.iter().any(|v| v.light[2] == 128));
}

#[test]
fn unregistered_metadata_does_not_render_or_hide_its_neighbour() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let mut w = World::default();
    let mut c = mc_world::chunk::Chunk::new(0, 0);
    c.set(0, 10, 0, ids::STONE, 0);
    c.set(1, 10, 0, ids::GRASS, 1); // Not in BLOCK_STATE_IDS; vanilla reads air.
    w.insert(c);
    assert_eq!(mesh::build(&w, &models, 0, 0, false, true).indices.len() / 6, 6);
}

#[test]
fn fence_mesh_connects_to_a_neighbour_across_a_chunk_boundary() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let mut w = World::default();
    let mut left = mc_world::chunk::Chunk::new(0, 0);
    left.set(15, 10, 0, ids::FENCE, 0);
    w.insert(left);
    let unconnected = mesh::build(&w, &models, 0, 0, false, true);
    assert!(unconnected.vertices.iter().all(|v| v.pos[0] < 16.0));
    let mut right = mc_world::chunk::Chunk::new(1, 0);
    right.set(0, 10, 0, ids::FENCE, 0);
    w.insert(right);
    let connected = mesh::build(&w, &models, 0, 0, false, true);
    assert!(connected.vertices.iter().any(|v| v.pos[0] == 16.0));
    assert!(connected.indices.len() > unconnected.indices.len());
}

#[test]
fn snow_above_grass_selects_the_snowy_model() {
    let Some(a) = assets() else { return };
    let models = BlockModels::load(&a);
    let mut w = World::default();
    let mut c = mc_world::chunk::Chunk::new(0, 0);
    c.set(0, 10, 0, ids::GRASS, 0);
    w.insert(c.clone());
    let bare = mesh::build(&w, &models, 0, 0, false, true);
    c.set(0, 11, 0, ids::SNOW_LAYER, 0);
    w.insert(c);
    let snowy = mesh::build(&w, &models, 0, 0, false, true);
    let bare_side = bare.vertices.iter().filter(|v| v.pos[0] == 1.0 && v.pos[1] < 11.0).map(|v| v.uv).collect::<Vec<_>>();
    let snow_side = snowy.vertices.iter().filter(|v| v.pos[0] == 1.0 && v.pos[1] < 11.0).map(|v| v.uv).collect::<Vec<_>>();
    assert_ne!(bare_side, snow_side);
    let state = w.state(BlockPos::new(0, 10, 0));
    assert_eq!(state.actual_state(&w, BlockPos::new(0, 10, 0)).value("snowy"), Some("true"));
}

#[test]
fn absent_variant_uses_missing_model_instead_of_an_arbitrary_variant() {
    let Some(mut a) = assets() else { return };
    let pack = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-packs")
        .join(format!("wrong-variant-{}", std::process::id()));
    std::fs::create_dir_all(pack.join("assets/minecraft/blockstates")).unwrap();
    std::fs::write(pack.join("assets/minecraft/blockstates/stone.json"),
        r#"{"variants":{"wrong":{"model":"diamond_block"}}}"#).unwrap();
    a.push(&pack).unwrap();
    let models = BlockModels::load(&a);
    let stone = models.get(ids::STONE, 0).unwrap();
    assert_eq!(stone.models[0].1.quads.len(), 6);
    assert!(models.missing_locations().iter().any(|loc| loc.name == "minecraft:stone" && loc.variant == "normal"));
    let missing = models.atlas.sprite(mc_assets::atlas::MISSING);
    assert!(stone.models[0].1.quads.iter().all(|q| q.uv.iter().all(|uv|
        uv[0] >= missing.min_u && uv[0] <= missing.max_u && uv[1] >= missing.min_v && uv[1] <= missing.max_v)));
    assert!(models.get(ids::AIR, 0).is_none());
}

#[test]
fn unresolved_weighted_alternatives_are_skipped_like_vanilla() {
    let Some(mut a) = assets() else { return };
    let pack = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-packs")
        .join(format!("unresolved-alternative-{}", std::process::id()));
    std::fs::create_dir_all(pack.join("assets/minecraft/blockstates")).unwrap();
    std::fs::write(pack.join("assets/minecraft/blockstates/stone.json"),
        r#"{"variants":{"normal":[{"model":"rustmc_missing_test_model","weight":99},{"model":"diamond_block","weight":1}]}}"#).unwrap();
    a.push(&pack).unwrap();
    let models = BlockModels::load(&a);
    let stone = models.get(ids::STONE, 0).unwrap();
    assert_eq!(stone.weights, [1]);
    assert!(!models.missing_locations().iter().any(|loc| loc.name == "minecraft:stone" && loc.variant == "normal"));
    let diamond = models.get(ids::DIAMOND_BLOCK, 0).unwrap();
    assert_eq!(stone.models[0].1.quads[0].uv, diamond.models[0].1.quads[0].uv);
}
