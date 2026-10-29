//! Checks the registry, through its public API, against the dumps from the real 1.8.9 classes
//! (`tools/extract/extract.ps1`). Also catches a `generated/` that is stale relative to `golden/`.
//! Skipped when the dumps aren't present.

use mc_data::{Block, Material, RenderLayer, StateId};
use serde_json::Value;
use std::collections::BTreeMap;

fn golden(name: &str) -> Option<Value> {
    let p = mc_testutil::golden(name)?;
    let text = std::fs::read_to_string(&p).unwrap();
    Some(serde_json::from_str(&text).unwrap())
}

fn props_of(v: &Value) -> BTreeMap<String, String> {
    v.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect()
}

fn props(s: StateId) -> BTreeMap<String, String> {
    s.properties().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn find(block: &Block, want: &Value) -> StateId {
    let want = props_of(want);
    block.states().find(|&s| props(s) == want).unwrap_or_else(|| panic!("{}: no state {want:?}", block.name))
}

fn layer(name: &str) -> RenderLayer {
    match name {
        "SOLID" => RenderLayer::Solid,
        "CUTOUT_MIPPED" => RenderLayer::CutoutMipped,
        "CUTOUT" => RenderLayer::Cutout,
        _ => RenderLayer::Translucent,
    }
}

#[test]
fn every_block_and_state_matches() {
    let Some(blocks) = golden("blocks.json") else { return };
    let blocks = blocks.as_array().unwrap();
    assert_eq!(Block::all().len(), blocks.len());
    let mut total = 0;
    for g in blocks {
        let id = g["id"].as_u64().unwrap() as u16;
        let b = Block::by_id(id).unwrap();
        let name = g["name"].as_str().unwrap();
        assert_eq!(b.name, name);
        assert_eq!(Block::by_name(name).map(|x| x.id), Some(id));
        assert_eq!(b.class, g["class"]);
        assert_eq!(b.unlocalized_name, g["unlocalized_name"]);
        assert_eq!(b.material.java_name(), g["material"]);

        // Static properties. Floats were dumped widened to double, so `as f32` is exact.
        let f = |k: &str| g[k].as_f64().unwrap() as f32;
        let flag = |k: &str| g[k].as_bool().unwrap();
        assert_eq!(b.hardness, f("hardness"), "{name}");
        assert_eq!(b.resistance, f("resistance"), "{name}");
        assert_eq!(b.slipperiness, f("slipperiness"), "{name}");
        assert_eq!(b.ao_light_value, f("ao_light_value"), "{name}");
        assert_eq!(b.light_opacity as u64, g["light_opacity"].as_u64().unwrap(), "{name}");
        assert_eq!(b.light_value as u64, g["light_value"].as_u64().unwrap(), "{name}");
        assert_eq!(b.is_opaque_cube(false), flag("opaque_cube"), "{name}");
        assert_eq!(b.is_opaque_cube(true), flag("opaque_cube_fancy"), "{name}");
        assert_eq!(b.render_layer(false), layer(g["layer"].as_str().unwrap()), "{name}");
        assert_eq!(b.render_layer(true), layer(g["layer_fancy"].as_str().unwrap()), "{name}");
        let flags = [
            (b.full_cube, "full_cube"),
            (b.full_block, "full_block"),
            (b.normal_cube, "normal_cube"),
            (b.block_normal_cube, "block_normal_cube"),
            (b.translucent, "translucent"),
            (b.use_neighbor_brightness, "use_neighbor_brightness"),
            (b.ticks_randomly, "ticks_randomly"),
            (b.collidable, "collidable"),
            (b.enable_stats, "enable_stats"),
            (b.has_tile_entity, "has_tile_entity"),
            (b.can_provide_power, "can_provide_power"),
            (b.comparator_override, "comparator_override"),
            (b.requires_updates, "requires_updates"),
            (b.can_spawn_in, "can_spawn_in"),
        ];
        for (ours, k) in flags {
            assert_eq!(ours, flag(k), "{name}.{k}");
        }
        assert_eq!(b.mobility as i64, g["mobility"].as_i64().unwrap(), "{name}");
        assert_eq!(b.render_type as i64, g["render_type"].as_i64().unwrap(), "{name}");
        let snd = &g["sound"];
        assert_eq!(b.sound.name, snd["name"]);
        assert_eq!(b.sound.volume, snd["volume"].as_f64().unwrap() as f32);
        assert_eq!(b.sound.frequency, snd["frequency"].as_f64().unwrap() as f32);
        assert_eq!(b.sound.break_sound, snd["break"]);
        assert_eq!(b.sound.step_sound, snd["step"]);
        assert_eq!(b.sound.place_sound, snd["place"]);
        let bounds: Vec<f64> = g["default_bounds"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(b.default_bounds.to_vec(), bounds, "{name}");

        // Property definitions, in order.
        let defs = g["properties"].as_array().unwrap();
        assert_eq!(b.properties.len(), defs.len(), "{name}");
        for (p, d) in b.properties.iter().zip(defs) {
            assert_eq!(p.name, d["name"]);
            let vals: Vec<&str> = d["values"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
            assert_eq!(p.values, vals.as_slice(), "{name}.{}", p.name);
        }

        // States in getValidStates order, with their metas.
        let states = g["states"].as_array().unwrap();
        assert_eq!(b.state_count as usize, states.len(), "{name}");
        for (s, gs) in b.states().zip(states) {
            assert_eq!(props(s), props_of(&gs["props"]), "{name}");
            assert_eq!(s.block().id, id);
            assert_eq!(s.meta() as u64, gs["meta"].as_u64().unwrap(), "{s}");
            assert_eq!(s.raw(), id << 4 | s.meta() as u16);
        }
        assert_eq!(b.default_state, find(b, &g["default_state"]), "{name} default");
        for (m, gs) in g["state_from_meta"].as_array().unwrap().iter().enumerate() {
            let ours = b.state_from_meta(m as u8);
            if gs.is_object() {
                assert_eq!(ours, Some(find(b, gs)), "{name} meta {m}");
            } else {
                assert_eq!(ours, None, "{name} meta {m} throws in vanilla ({gs})");
            }
        }
        total += states.len();
    }
    assert_eq!(total, mc_data::block::STATE_COUNT);
}

#[test]
fn storage_and_network_ids_match() {
    let Some(ids) = golden("state_ids.json") else { return };
    let mut registered = [false; 4096];
    for e in ids["by_raw"].as_array().unwrap() {
        let raw = e["raw"].as_u64().unwrap() as u16;
        registered[raw as usize] = true;
        let b = Block::by_id(e["block"].as_u64().unwrap() as u16).unwrap();
        assert_eq!(StateId::try_from_raw(raw), Some(find(b, &e["props"])), "raw {raw}");
    }
    for (raw, &r) in registered.iter().enumerate() {
        if !r {
            assert_eq!(StateId::try_from_raw(raw as u16), None, "raw {raw} must be unregistered");
            assert_eq!(StateId::from_raw(raw as u16), StateId::AIR);
        }
    }
    for e in ids["by_state"].as_array().unwrap() {
        let b = Block::by_id(e["block"].as_u64().unwrap() as u16).unwrap();
        let s = find(b, &e["props"]);
        assert_eq!(s.network_id() as i64, e["raw"].as_i64().unwrap(), "{s}");
    }
}

#[test]
fn materials_match() {
    let Some(mats) = golden("materials.json") else { return };
    let mats = mats.as_array().unwrap();
    assert_eq!(Material::ALL.len(), mats.len());
    for (m, g) in Material::ALL.iter().zip(mats) {
        assert_eq!(m.java_name(), g["name"]);
        let p = m.props();
        let flag = |k: &str| g[k].as_bool().unwrap();
        assert_eq!(
            (p.liquid, p.solid, p.blocks_light, p.blocks_movement, p.can_burn, p.replaceable, p.opaque, p.tool_not_required),
            (
                flag("liquid"),
                flag("solid"),
                flag("blocks_light"),
                flag("blocks_movement"),
                flag("can_burn"),
                flag("replaceable"),
                flag("opaque"),
                flag("tool_not_required")
            ),
            "{}",
            m.java_name()
        );
        assert_eq!(p.mobility as u64, g["mobility"].as_u64().unwrap());
        assert_eq!(p.map_color as u64, g["map_color"].as_u64().unwrap());
    }
}
