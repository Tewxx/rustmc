//! Differential replay of ordered block geometry calls from the real 1.8.9 classes.
use mc_core::{Aabb, BlockPos, Facing, Vec3};
use mc_data::geometry::{BlockGeometry, CollisionEntity, MovingPiston};
use mc_data::{BlockAccess, StateId};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
struct Scene {
    states: HashMap<BlockPos, StateId>,
    piston: Option<(BlockPos, MovingPiston)>,
}
impl BlockAccess for Scene {
    fn state(&self, pos: BlockPos) -> StateId {
        self.states.get(&pos).copied().unwrap_or(StateId::AIR)
    }
    fn moving_piston(&self, pos: BlockPos) -> Option<MovingPiston> {
        self.piston.filter(|(p, _)| *p == pos).map(|(_, v)| v)
    }
}
fn int(v: &Value) -> i32 {
    v.as_f64().unwrap() as i32
}
fn bounds(v: &Value) -> [f64; 6] {
    std::array::from_fn(|i| v[i].as_f64().unwrap())
}
fn aabb(v: &Value) -> Aabb {
    let [min_x, min_y, min_z, max_x, max_y, max_z] = bounds(v);
    Aabb { min_x, min_y, min_z, max_x, max_y, max_z }
}
fn bits(b: Aabb) -> [u64; 6] {
    [b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z].map(f64::to_bits)
}
fn vec3(v: &Value) -> Vec3 {
    Vec3::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap())
}

#[test]
fn geometry_matches_java() {
    let Some(path) = mc_testutil::golden("geometry.json") else {
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap();
    let rows: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert!(rows.len() >= mc_data::block::STATE_COUNT);
    for (i, row) in rows.iter().take(mc_data::block::STATE_COUNT).enumerate() {
        assert_eq!(int(&row["c"]) as usize, i, "missing state in Java geometry dump");
    }
    let mut failures: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for (i, row) in rows.iter().enumerate() {
        let state = StateId(int(&row["c"]) as u16);
        let pos = BlockPos::new(int(&row["pos"][0]), int(&row["pos"][1]), int(&row["pos"][2]));
        let mut world = Scene::default();
        world.states.insert(pos, state);
        for n in row["n"].as_array().unwrap() {
            let f = Facing::from_index(int(&n[0]) as usize);
            let (x, y, z) = f.offset();
            let d = int(&n[1]);
            world.states.insert(pos.offset(x * d, y * d, z * d), StateId(int(&n[2]) as u16));
        }
        if let Some(p) = row["piston"].as_array() {
            world.piston = Some((
                pos,
                MovingPiston {
                    state: StateId(int(&p[0]) as u16),
                    facing: Facing::from_index(int(&p[1]) as usize),
                    progress: f32::from_bits(int(&p[2]) as u32),
                    extending: int(&p[3]) != 0,
                },
            ));
        }
        let mut geometry = BlockGeometry::new();
        let mut check = |op: &str, got: String, want: String| {
            if got != want {
                let entry = failures.entry(format!("{} {op}", state.block().class)).or_insert((0, String::new()));
                if entry.0 == 0 {
                    entry.1 = format!("scene {i} {state} at {pos:?}: ours {got}, Java {want}");
                }
                entry.0 += 1;
            }
        };
        for step in row["steps"].as_array().unwrap() {
            let op = step["op"].as_str().unwrap();
            match op {
                "update" => {
                    geometry.update_bounds(state, &world, pos);
                }
                "sides" => {
                    for fancy in [false, true] {
                        let flags = Facing::ALL
                            .into_iter()
                            .filter(|&f| geometry.should_side_be_rendered(state, &world, pos, f, fancy))
                            .fold(0, |v, f| v | 1 << f.index());
                        check("sides", flags.to_string(), int(&step[if fancy { "fancy" } else { "fast" }]).to_string());
                    }
                }
                "select" => {
                    let got = geometry.selected_box(state, &world, pos);
                    check("selected box", format!("{:?}", bits(got)), format!("{:?}", bits(aabb(&step["box"]))));
                }
                "collision" => {
                    let got = geometry.collision_box(state, &world, pos).map(bits);
                    let want = if step["box"].is_null() { None } else { Some(bits(aabb(&step["box"]))) };
                    check("collision box", format!("{got:?}"), format!("{want:?}"));
                }
                "boxes" => {
                    let mut boxes = Vec::new();
                    let entity = if step["boat"].as_bool().unwrap() { CollisionEntity::Boat } else { CollisionEntity::Other };
                    geometry.add_collision_boxes(state, &world, pos, &aabb(&step["mask"]), &mut boxes, entity);
                    let got = boxes.into_iter().map(bits).collect::<Vec<_>>();
                    let want = step["boxes"].as_array().unwrap().iter().map(|v| bits(aabb(v))).collect::<Vec<_>>();
                    check("collision list", format!("{got:?}"), format!("{want:?}"));
                }
                "ray" => {
                    let got = geometry
                        .ray_trace(state, &world, pos, vec3(&step["start"]), vec3(&step["end"]))
                        .map(|h| ([h.position.x, h.position.y, h.position.z].map(f64::to_bits), h.face.index()));
                    let want = if step["hit"].is_null() {
                        None
                    } else {
                        let v = vec3(&step["hit"]);
                        Some(([v.x, v.y, v.z].map(f64::to_bits), int(&step["hit"][3]) as usize))
                    };
                    check("ray hit", format!("{got:?}"), format!("{want:?}"));
                }
                _ => unreachable!(),
            }
            check(
                op,
                format!("{:?}", geometry.bounds(state).0.map(f64::to_bits)),
                format!("{:?}", bounds(&step["bounds"]).map(f64::to_bits)),
            );
        }
    }
    let report = failures.iter().map(|(k, (n, first))| format!("{k}: {n} mismatches, {first}")).collect::<Vec<_>>().join("\n");
    assert!(failures.is_empty(), "{report}");
}
