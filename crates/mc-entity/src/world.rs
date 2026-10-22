//! The `World` queries entity movement depends on, written against a small trait so the same
//! code runs on the client (prediction) and on the server.
use mc_core::{math::floor_double, Aabb, BlockPos, Facing, Vec3};
use mc_data::{Material, StateId};

pub trait EntityWorld {
    /// `getBlockState`; unloaded or out-of-world positions read as air.
    fn state(&self, pos: BlockPos) -> StateId;
    /// `isChunkLoaded(x, z, allowEmpty = true)`
    fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool;
    /// `Block.addCollisionBoxesToList` for the block at `pos` (bounds state is the caller's).
    fn add_collision_boxes(&mut self, state: StateId, pos: BlockPos, mask: &Aabb, out: &mut Vec<Aabb>);
    /// Collision boxes of nearby entities (`getCollisionBoundingBox`, e.g. boats), excluding `self`.
    fn entity_boxes(&self, _bb: &Aabb, _out: &mut Vec<Aabb>) {}
}

/// `World.isBlockLoaded(pos)`
pub fn is_block_loaded(w: &impl EntityWorld, pos: BlockPos) -> bool {
    pos.y >= 0 && pos.y < 256 && pos.x.abs() < 30_000_000 && pos.z.abs() < 30_000_000 && w.is_chunk_loaded(pos.x >> 4, pos.z >> 4)
}

/// `World.isAreaLoaded(x1, y1, z1, x2, y2, z2, allowEmpty = true)`
pub fn is_area_loaded(w: &impl EntityWorld, x1: i32, y1: i32, z1: i32, x2: i32, y2: i32, z2: i32) -> bool {
    if y2 < 0 || y1 >= 256 {
        return false;
    }
    for cx in x1 >> 4..=x2 >> 4 {
        for cz in z1 >> 4..=z2 >> 4 {
            if !w.is_chunk_loaded(cx, cz) {
                return false;
            }
        }
    }
    true
}

/// `World.getCollidingBoundingBoxes(entity, bb)` (inside the world border).
pub fn colliding_boxes(w: &mut impl EntityWorld, bb: &Aabb) -> Vec<Aabb> {
    let mut list = Vec::new();
    let i = floor_double(bb.min_x);
    let j = floor_double(bb.max_x + 1.0);
    let k = floor_double(bb.min_y);
    let l = floor_double(bb.max_y + 1.0);
    let i1 = floor_double(bb.min_z);
    let j1 = floor_double(bb.max_z + 1.0);
    for x in i..j {
        for z in i1..j1 {
            if !is_block_loaded(w, BlockPos::new(x, 64, z)) {
                continue;
            }
            for y in k - 1..l {
                let pos = BlockPos::new(x, y, z);
                let state = w.state(pos);
                w.add_collision_boxes(state, pos, bb, &mut list);
            }
        }
    }
    let mut others = Vec::new();
    w.entity_boxes(&bb.expand(0.25, 0.25, 0.25), &mut others);
    list.extend(others.into_iter().filter(|o| o.intersects_with(bb)));
    list
}

/// `World.isAnyLiquid`
pub fn is_any_liquid(w: &impl EntityWorld, bb: &Aabb) -> bool {
    for x in floor_double(bb.min_x)..=floor_double(bb.max_x) {
        for y in floor_double(bb.min_y)..=floor_double(bb.max_y) {
            for z in floor_double(bb.min_z)..=floor_double(bb.max_z) {
                if w.state(BlockPos::new(x, y, z)).block().material.props().liquid {
                    return true;
                }
            }
        }
    }
    false
}

/// `World.isMaterialInBB`
pub fn is_material_in_bb(w: &impl EntityWorld, bb: &Aabb, m: Material) -> bool {
    for x in floor_double(bb.min_x)..floor_double(bb.max_x + 1.0) {
        for y in floor_double(bb.min_y)..floor_double(bb.max_y + 1.0) {
            for z in floor_double(bb.min_z)..floor_double(bb.max_z + 1.0) {
                if w.state(BlockPos::new(x, y, z)).block().material == m {
                    return true;
                }
            }
        }
    }
    false
}

/// `BlockLiquid.getLiquidHeightPercent`
pub fn liquid_height_percent(mut level: i32) -> f32 {
    if level >= 8 {
        level = 0;
    }
    (level + 1) as f32 / 9.0
}

fn level(s: StateId) -> i32 {
    s.get_int("level")
}

/// `BlockLiquid.getEffectiveFlowDecay`
fn effective_flow_decay(w: &impl EntityWorld, pos: BlockPos, m: Material) -> i32 {
    let s = w.state(pos);
    if s.block().material != m {
        -1
    } else {
        let i = level(s);
        if i >= 8 {
            0
        } else {
            i
        }
    }
}

/// `BlockLiquid.isBlockSolid`
fn liquid_block_solid(w: &impl EntityWorld, pos: BlockPos, side: Facing, m: Material) -> bool {
    let mat = w.state(pos).block().material;
    if mat == m {
        false
    } else if side == Facing::Up {
        true
    } else if mat == Material::Ice {
        false
    } else {
        mat.props().solid
    }
}

/// `BlockLiquid.getFlowVector`
pub fn flow_vector(w: &impl EntityWorld, pos: BlockPos, m: Material) -> Vec3 {
    let mut v = Vec3::new(0., 0., 0.);
    let i = effective_flow_decay(w, pos, m);
    for f in Facing::HORIZONTALS {
        let p = pos.offset_facing(f);
        let mut j = effective_flow_decay(w, p, m);
        if j < 0 {
            if !w.state(p).block().material.props().blocks_movement {
                j = effective_flow_decay(w, p.down(), m);
                if j >= 0 {
                    let k = j - (i - 8);
                    v = v.add(((p.x - pos.x) * k) as f64, ((p.y - pos.y) * k) as f64, ((p.z - pos.z) * k) as f64);
                }
            }
        } else {
            let l = j - i;
            v = v.add(((p.x - pos.x) * l) as f64, ((p.y - pos.y) * l) as f64, ((p.z - pos.z) * l) as f64);
        }
    }
    if level(w.state(pos)) >= 8 {
        for f in Facing::HORIZONTALS {
            let p = pos.offset_facing(f);
            if liquid_block_solid(w, p, f, m) || liquid_block_solid(w, p.up(), f, m) {
                v = v.normalize().add(0., -6., 0.);
                break;
            }
        }
    }
    v.normalize()
}

/// `World.handleMaterialAcceleration`: returns whether the box touches the material; pushes
/// `motion` along the summed flow when `pushed`.
pub fn handle_material_acceleration(w: &impl EntityWorld, bb: &Aabb, m: Material, motion: &mut Vec3, pushed: bool) -> bool {
    let i = floor_double(bb.min_x);
    let j = floor_double(bb.max_x + 1.0);
    let k = floor_double(bb.min_y);
    let l = floor_double(bb.max_y + 1.0);
    let i1 = floor_double(bb.min_z);
    let j1 = floor_double(bb.max_z + 1.0);
    if !is_area_loaded(w, i, k, i1, j, l, j1) {
        return false;
    }
    let mut flag = false;
    let mut v = Vec3::new(0., 0., 0.);
    for x in i..j {
        for y in k..l {
            for z in i1..j1 {
                let pos = BlockPos::new(x, y, z);
                let s = w.state(pos);
                if s.block().material == m {
                    let d0 = ((y + 1) as f32 - liquid_height_percent(level(s))) as f64;
                    // Vanilla compares the integer bound `l`, not the box top.
                    if l as f64 >= d0 {
                        flag = true;
                        v = v.add_vector(flow_vector(w, pos, m));
                    }
                }
            }
        }
    }
    if v.length() > 0.0 && pushed {
        let v = v.normalize();
        motion.x += v.x * 0.014;
        motion.y += v.y * 0.014;
        motion.z += v.z * 0.014;
    }
    flag
}
