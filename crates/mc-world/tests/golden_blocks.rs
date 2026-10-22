//! Checks the block table against `golden/blocks.json`, dumped from the real 1.8.9 classes by
//! `tools/extract/extract.ps1`. Skipped when the dump isn't present.

use mc_world::blocks::{self, Layer};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

fn golden() -> Option<HashMap<u64, Value>> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden/blocks.json");
    let Ok(text) = std::fs::read_to_string(&p) else {
        eprintln!("skipping: {} not found (run tools/extract/extract.ps1)", p.display());
        return None;
    };
    let all: Vec<Value> = serde_json::from_str(&text).unwrap();
    Some(all.into_iter().map(|b| (b["id"].as_u64().unwrap(), b)).collect())
}

#[test]
fn registry_has_every_vanilla_block() {
    let Some(g) = golden() else { return };
    assert_eq!(g.len(), 198);
    assert_eq!(g[&0]["name"], "minecraft:air");
    assert_eq!(g[&197]["name"], "minecraft:dark_oak_door");
}

#[test]
fn known_blocks_match_vanilla_properties() {
    let Some(g) = golden() else { return };
    let mut bad = Vec::new();
    for (id, meta, info) in blocks::all_known() {
        let v = &g[&(id as u64)];
        let name = v["name"].as_str().unwrap();
        let p = info.props;
        let mut check = |field: &str, ours: Value, theirs: &Value| {
            if &ours != theirs {
                bad.push(format!("{id}:{meta} {name}.{field}: ours {ours}, vanilla {theirs}"));
            }
        };
        check("light_opacity", p.opacity.into(), &v["light_opacity"]);
        check("light_value", p.emission.into(), &v["light_value"]);
        check("full_cube", p.full_cube.into(), &v["full_cube"]);
        check("block_normal_cube", p.normal_cube.into(), &v["block_normal_cube"]);
        check("translucent", p.translucent.into(), &v["translucent"]);
        // BlockLeaves switch opacity and layer with the graphics setting; the dump is fast graphics.
        if v["class"] != "BlockOldLeaf" && v["class"] != "BlockNewLeaf" {
            check("opaque_cube", info.opaque.into(), &v["opaque_cube"]);
            let layer = match v["layer"].as_str().unwrap() {
                "SOLID" => Layer::Solid,
                "CUTOUT" | "CUTOUT_MIPPED" => Layer::Cutout,
                _ => Layer::Translucent,
            };
            if layer != info.layer {
                bad.push(format!("{id}:{meta} {name}.layer: ours {:?}, vanilla {}", info.layer, v["layer"]));
            }
        }
        // Storage keeps raw id<<4|meta, but `ExtendedBlockStorage.get` only resolves pairs that
        // `getMetaFromState` produces (`Block.BLOCK_STATE_IDS`); anything else reads as air.
        let registered = v["states"].as_array().unwrap().iter().any(|s| s["meta"] == meta);
        if !registered {
            bad.push(format!("{id}:{meta} {name}: not a registered state in vanilla (reads as air)"));
        }
    }
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}
