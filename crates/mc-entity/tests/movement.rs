//! Replays the scripted `EntityPlayerSP` traces from golden/movement.json (tools/extract
//! DumpMovement: the real vanilla tick path) through `PlayerEntity::tick`, comparing raw bits.
use mc_core::{Aabb, BlockPos, Vec3};
use mc_data::geometry::{BlockGeometry, CollisionEntity};
use mc_data::{BlockAccess, StateId};
use mc_entity::{EntityWorld, Keys, PlayerEntity};
use serde_json::Value;
use std::collections::HashMap;

struct Scene {
    states: HashMap<BlockPos, StateId>,
    geometry: BlockGeometry,
}
struct Blocks<'a>(&'a HashMap<BlockPos, StateId>);
impl BlockAccess for Blocks<'_> {
    fn state(&self, pos: BlockPos) -> StateId {
        self.0.get(&pos).copied().unwrap_or(StateId::AIR)
    }
}
impl EntityWorld for Scene {
    fn state(&self, pos: BlockPos) -> StateId {
        if pos.y < 0 || pos.y >= 256 {
            return StateId::AIR;
        }
        self.states.get(&pos).copied().unwrap_or(StateId::AIR)
    }
    fn is_chunk_loaded(&self, _cx: i32, _cz: i32) -> bool {
        true
    }
    fn add_collision_boxes(&mut self, state: StateId, pos: BlockPos, mask: &Aabb, out: &mut Vec<Aabb>) {
        self.geometry.add_collision_boxes(state, &Blocks(&self.states), pos, mask, out, CollisionEntity::Other);
    }
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| v.as_f64().unwrap() as i64)
}
fn d(v: &Value) -> f64 {
    f64::from_bits(i(v) as u64)
}

#[test]
fn movement_matches_java() {
    let Some(path) = mc_testutil::golden("movement.json") else {
        return;
    };
    let scenes: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut failures = Vec::new();
    let mut ticks = 0;
    for sc in &scenes {
        let name = sc["name"].as_str().unwrap();
        let mut world = Scene { states: HashMap::new(), geometry: BlockGeometry::new() };
        for b in sc["blocks"].as_array().unwrap() {
            let n: Vec<i64> = b.as_array().unwrap().iter().map(i).collect();
            for x in n[0]..=n[3] {
                for y in n[1]..=n[4] {
                    for z in n[2]..=n[5] {
                        let s = StateId(n[6] as u16);
                        let p = BlockPos::new(x as i32, y as i32, z as i32);
                        if s == StateId::AIR {
                            world.states.remove(&p);
                        } else {
                            world.states.insert(p, s);
                        }
                    }
                }
            }
        }
        let s = &sc["start"];
        let mut e = PlayerEntity::new(Vec3::new(d(&s[0]), d(&s[1]), d(&s[2])));
        e.yaw = f32::from_bits(i(&sc["yaw"]) as u32);
        let mode = sc["mode"].as_str().unwrap();
        e.creative = mode == "creative";
        e.allow_flying = mode != "survival";
        e.no_clip = mode == "spectator";
        e.flying = sc["flying"].as_bool().unwrap() || mode == "spectator";
        if let Some(m) = sc["motion0"].as_array().filter(|m| !m.is_empty()) {
            e.motion = Vec3::new(d(&m[0]), d(&m[1]), d(&m[2]));
        }
        let script = sc["script"].as_array().unwrap();
        let out = sc["out"].as_array().unwrap();
        for (t, (tk, want)) in script.iter().zip(out).enumerate() {
            ticks += 1;
            let k = i(&tk[0]);
            let has = |bit: i64| k & bit != 0;
            let keys = Keys {
                forward: has(1),
                back: has(2),
                left: has(4),
                right: has(8),
                jump: has(16),
                sneak: has(32),
                sprint: has(64),
                using_item: has(128),
            };
            let yb = i(&tk[1]);
            if yb > -(1 << 39) {
                e.yaw = f32::from_bits(yb as u32);
            }
            if tk.as_array().unwrap().len() == 5 {
                e.motion = Vec3::new(d(&tk[2]), d(&tk[3]), d(&tk[4]));
            }
            e.tick(&mut world, keys);
            let got_flags = e.on_ground as i64
                | (e.collided_horizontally as i64) << 1
                | (e.collided_vertically as i64) << 2
                | (e.sprinting as i64) << 3
                | (e.flying as i64) << 4
                | (e.in_water as i64) << 5;
            let got = [
                e.pos.x.to_bits() as i64,
                e.pos.y.to_bits() as i64,
                e.pos.z.to_bits() as i64,
                e.motion.x.to_bits() as i64,
                e.motion.y.to_bits() as i64,
                e.motion.z.to_bits() as i64,
                got_flags,
                e.fall_distance.to_bits() as i64,
            ];
            let w: Vec<i64> = want.as_array().unwrap().iter().map(i).collect();
            if got.as_slice() != w.as_slice() {
                let names = ["posX", "posY", "posZ", "motionX", "motionY", "motionZ", "flags", "fallDistance"];
                let mut diff = String::new();
                for (n, (g, v)) in names.iter().zip(got.iter().zip(&w)) {
                    if g != v {
                        let (gs, vs) = match *n {
                            "flags" => (format!("{g:#b}"), format!("{v:#b}")),
                            "fallDistance" => (f32::from_bits(*g as u32).to_string(), f32::from_bits(*v as u32).to_string()),
                            _ => (f64::from_bits(*g as u64).to_string(), f64::from_bits(*v as u64).to_string()),
                        };
                        diff += &format!(" {n}: ours {gs} java {vs};");
                    }
                }
                failures.push(format!("{name}: first divergence at tick {t} (keys {k:#x}):{diff}"));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{} of {} scenarios diverge ({ticks} ticks replayed):\n{}", failures.len(), scenes.len(), failures.join("\n"));
}
