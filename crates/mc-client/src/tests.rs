use super::*;
#[test]
fn gameplay_break_place_pick_and_commands_share_the_world() {
    let Some(jar) = find_jar() else {
        return;
    };
    let mut assets = AssetSource::new();
    assets.push(&jar).unwrap();
    let models = BlockModels::load(&assets);
    let mut world = World::default();
    let mut c = mc_world::chunk::Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 0, z, 1, 0);
        }
    }
    for y in 1..4 {
        c.set(8, y, 10, 1, 0);
    }
    world.insert(c);
    let mut app = App::new(assets, models, world, (8, 8), Settings::default());
    app.in_game = true;
    app.tick_accum = 1.;
    app.entity.yaw = 0.;
    app.entity.pitch = 20.;
    app.sync_view();
    app.send_chat("/give Player stone 64");
    assert_eq!(app.ui.inventory[0].unwrap().count, 64);
    assert_eq!(app.target.unwrap().pos, BlockPos::new(8, 2, 10));
    app.interact(MouseButton::Left);
    assert_eq!(app.world.get(8, 2, 10), (0, 0));
    app.world.set_block(BlockPos::new(8, 2, 10), 1, 0);
    app.sync_view();
    app.interact(MouseButton::Right);
    assert_eq!(app.world.get(8, 2, 9), (1, 0));
    app.sync_view();
    app.interact(MouseButton::Right);
    assert_eq!(app.world.get(8, 2, 8), (0, 0), "placement must not intersect the player");
    let p = BlockPos::new(7, 1, 9);
    app.world.set_block(p, 5, 2);
    app.target = Some(player::Target {
        pos: p,
        face: Facing::North,
        hit: mc_core::Vec3::new(7.5, 1.5, 9.),
    });
    app.player.selected_slot = 3;
    app.interact(MouseButton::Middle);
    let held = &app.catalogue.items[app.ui.inventory[3].unwrap().item];
    assert_eq!((held.block, held.meta), (5, 2));
    app.send_chat("/tp ~1 ~ ~");
    assert_eq!(app.entity.pos.x, app.spawn.x + 1.);

    // Item use: eating finishes after 32 ticks and consumes one apple; a sword blocks until
    // released; a thrown snowball is used up at once.
    app.send_chat("/gamemode survival");
    app.target = None;
    app.pointed_entity = None;
    app.player.food = 10;
    app.ui.inventory[4] = Some(app.catalogue.stack(app.catalogue.find("apple", 0).unwrap(), 3));
    app.player.selected_slot = 4;
    app.right_click_mouse();
    assert_eq!(app.item_in_use_count, 32);
    for _ in 0..32 {
        app.tick_item_in_use();
    }
    assert!(app.item_in_use.is_none());
    assert_eq!(app.ui.inventory[4].unwrap().count, 2);
    assert_eq!(app.player.food, 14);
    app.ui.inventory[5] = Some(app.catalogue.stack(app.catalogue.find("diamond_sword", 0).unwrap(), 1));
    app.player.selected_slot = 5;
    app.right_click_mouse();
    assert!(app.item_in_use.is_some());
    app.tick_item_in_use();
    assert_eq!(app.item_in_use_count, 71999);
    app.on_stopped_using_item();
    assert!(app.item_in_use.is_none());
    app.ui.inventory[6] = Some(app.catalogue.stack(app.catalogue.find("snowball", 0).unwrap(), 2));
    app.player.selected_slot = 6;
    app.right_click_mouse();
    assert_eq!(app.ui.inventory[6].unwrap().count, 1);
}

#[test]
fn block_break_effects_spawn_a_4x4x4_grid() {
    let Some(jar) = find_jar() else {
        return;
    };
    let mut assets = AssetSource::new();
    assets.push(&jar).unwrap();
    let models = BlockModels::load(&assets);
    let state = mc_data::StateId::try_from_network(1).unwrap();
    assert!(models.particle_uv(state).is_some(), "stone has a particle texture");
    let mut particles = crate::particles::Particles::default();
    let mut rand = mc_core::random::JavaRandom::new(1);
    particles.block_destroy(state, BlockPos::new(0, 0, 0), &World::default(), &models, &mut rand);
    assert_eq!(particles.len(), 64);
}

#[test]
fn crit_emitter_bursts_once_and_expires_after_three_ticks() {
    let Some(jar) = find_jar() else {
        return;
    };
    let mut assets = AssetSource::new();
    assets.push(&jar).unwrap();
    let models = BlockModels::load(&assets);
    let mut particles = crate::particles::Particles::default();
    let mut rand = mc_core::random::JavaRandom::new(9);
    particles.emit_at_entity(1, mc_core::Vec3::new(0., 0., 0.), 0.6, 1.8, 9, &mut rand, &models);
    let n = particles.len();
    assert!(n > 0 && n <= 16, "{n} crit particles in the constructor burst");
    assert_eq!(particles.emitter_count(), 1);
    let world = World::default();
    let mut geometry = BlockGeometry::new();
    let remotes = std::collections::HashMap::new();
    particles.tick(&world, &mut geometry, &mut rand, Vec3::ZERO, &remotes, &models);
    assert_eq!(particles.emitter_count(), 1);
    particles.tick(&world, &mut geometry, &mut rand, Vec3::ZERO, &remotes, &models);
    assert_eq!(particles.emitter_count(), 0);
}

/// Rendering must be a pure function of world + settings: same seed twice gives identical
/// blocks, light and per-section mesh sizes, whatever the thread count.
#[test]
fn seeded_demo_world_is_deterministic() {
    use std::hash::{Hash, Hasher};
    let mk = || Args {
        play: false, jar: None, packs: vec![], world: None, radius: 8, pos: None, screenshot: None,
        screen: "hud".into(), bench: None, seed: Some(12345), connect: None, name: "Player".into(),
        online: false, pack_mode: None,
    };
    let hashes = |w: &World| {
        let mut keys: Vec<_> = w.chunks.keys().copied().collect();
        keys.sort();
        keys.iter().map(|k| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for s in w.chunks[k].sections.iter().flatten() {
                s.blocks.hash(&mut h);
                s.sky_light.hash(&mut h);
                s.block_light.hash(&mut h);
            }
            (*k, h.finish())
        }).collect::<Vec<_>>()
    };
    let (w1, ..) = crate::world_load::load_world(&mk());
    let (w2, ..) = crate::world_load::load_world(&mk());
    assert_eq!(hashes(&w1), hashes(&w2), "blocks/light differ between identical loads");
    let Some(jar) = find_jar() else { return };
    let mut assets = AssetSource::new();
    assets.push(&jar).unwrap();
    let models = BlockModels::load(&assets);
    let mut jobs = vec![];
    for (&(cx, cz), c) in &w1.chunks {
        for sy in 0..16 {
            if c.sections[sy].is_some() {
                jobs.push((cx, sy as i32, cz));
            }
        }
    }
    jobs.sort();
    let counts = |w: &World| {
        crate::world_load::mesh_parallel(w, &models, &jobs, true, true).iter().map(|(k, m)| (*k, m.indices.len())).collect::<Vec<_>>()
    };
    let a = counts(&w1);
    assert_eq!(a, counts(&w2));
    let mut g = mc_data::geometry::BlockGeometry::new();
    let serial: Vec<_> = jobs.iter().map(|&(x, sy, z)| ((x, sy, z), mc_render::mesh::build_section(&w1, &models, x, sy, z, true, true, &mut g).indices.len())).collect();
    assert_eq!(a, serial, "parallel and serial meshing differ");
}
