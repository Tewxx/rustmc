//! Replays every scene in `golden/actual_states.json` (random 3×3×3 neighbourhoods built and
//! evaluated by the real `getActualState`) and compares. Skipped when the dump isn't present.

use mc_core::BlockPos;
use mc_data::{Block, BlockAccess, StateId};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
struct Scene {
    states: HashMap<BlockPos, StateId>,
    pot: Option<(BlockPos, (i32, i32))>,
    comparators: HashMap<BlockPos, i32>,
}

impl BlockAccess for Scene {
    fn state(&self, pos: BlockPos) -> StateId {
        self.states.get(&pos).copied().unwrap_or(StateId::AIR)
    }
    fn flower_pot(&self, pos: BlockPos) -> Option<(i32, i32)> {
        self.pot.filter(|(p, _)| *p == pos).map(|(_, c)| c)
    }
    fn comparator_output(&self, pos: BlockPos) -> Option<i32> {
        self.comparators.get(&pos).copied()
    }
}

fn int(v: &Value) -> i32 {
    v.as_f64().unwrap() as i32
}

#[test]
fn every_scene_matches_vanilla() {
    let Some(p) = mc_testutil::golden("actual_states.json") else {
        return;
    };
    let text = std::fs::read_to_string(&p).unwrap();
    let scenes: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert!(!scenes.is_empty());

    let mut failures: BTreeMap<&str, (usize, String)> = BTreeMap::new();
    let mut per_block: BTreeMap<&str, usize> = BTreeMap::new();
    for sc in &scenes {
        let pos = sc["pos"].as_array().unwrap();
        let centre = BlockPos::new(int(&pos[0]), int(&pos[1]), int(&pos[2]));
        let c = StateId(int(&sc["c"]) as u16);
        let mut w = Scene::default();
        w.states.insert(centre, c);
        let mut offsets = Vec::with_capacity(26);
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy, dz) != (0, 0, 0) {
                        offsets.push(centre.offset(dx, dy, dz));
                    }
                }
            }
        }
        for (p, n) in offsets.iter().zip(sc["n"].as_array().unwrap()) {
            w.states.insert(*p, StateId(int(n) as u16));
        }
        if let Some(cmp) = sc.get("cmp") {
            for pair in cmp.as_array().unwrap() {
                w.comparators.insert(offsets[int(&pair[0]) as usize], int(&pair[1]));
            }
        }
        if let Some(pot) = sc.get("pot") {
            w.pot = Some((centre, (int(&pot[0]), int(&pot[1]))));
        }

        let want = StateId(int(&sc["out"]) as u16);
        let got = c.actual_state(&w, centre);
        let name = c.block().name;
        *per_block.entry(name).or_default() += 1;
        if got != want {
            let e = failures.entry(name).or_insert((0, String::new()));
            if e.0 == 0 {
                e.1 = format!("at {centre:?} stored {c}\n    vanilla {want}\n    ours    {got}");
            }
            e.0 += 1;
        }
    }

    // Every block that overrides getActualState has scenes, and no others do.
    let overriding: Vec<&str> = Block::all().iter().filter(|b| b.actual_state_override).map(|b| b.name).collect();
    assert_eq!(per_block.keys().copied().collect::<Vec<_>>(), {
        let mut v = overriding.clone();
        v.sort();
        v
    });

    if !failures.is_empty() {
        let report: Vec<String> =
            failures.iter().map(|(name, (n, first))| format!("{name}: {n}/{} wrong, first: {first}", per_block[name])).collect();
        panic!("{} blocks disagree with vanilla:\n{}", failures.len(), report.join("\n"));
    }
}

#[test]
fn blocks_without_override_return_themselves() {
    struct Empty;
    impl BlockAccess for Empty {
        fn state(&self, _: BlockPos) -> StateId {
            StateId::AIR
        }
    }
    for b in Block::all().iter().filter(|b| !b.actual_state_override) {
        for s in b.states() {
            assert_eq!(s.actual_state(&Empty, BlockPos::ORIGIN), s);
        }
    }
}
