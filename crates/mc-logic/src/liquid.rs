//! `BlockLiquid`, `BlockDynamicLiquid`, `BlockStaticLiquid`.

use crate::{flags, Level, Rng};
use mc_core::{BlockPos, Facing, JavaRandom};
use mc_data::{ids, Material, StateId};

#[inline]
fn level_of(s: StateId) -> i32 {
    // LEVEL is the only property and maps straight onto meta.
    s.meta() as i32
}

fn with_level(block: u16, level: i32) -> StateId {
    crate::st(block, level as u8)
}

fn material_of(block: u16) -> Material {
    if block == ids::WATER || block == ids::FLOWING_WATER {
        Material::Water
    } else {
        Material::Lava
    }
}

fn flowing(m: Material) -> u16 {
    if m == Material::Water { ids::FLOWING_WATER } else { ids::FLOWING_LAVA }
}

fn still(m: Material) -> u16 {
    if m == Material::Water { ids::WATER } else { ids::LAVA }
}

/// `tickRate`
pub fn tick_rate(l: &mut dyn Level, m: Material) -> i32 {
    match m {
        Material::Water => 5,
        Material::Lava => {
            if l.has_no_sky() {
                10
            } else {
                30
            }
        }
        _ => 0,
    }
}

/// `getLevel`: -1 unless the block at `pos` is this liquid's material.
fn get_level(l: &mut dyn Level, m: Material, pos: BlockPos) -> i32 {
    let s = l.state(pos);
    if s.block().material == m {
        level_of(s)
    } else {
        -1
    }
}

/// `triggerMixEffects`
fn trigger_mix_effects(l: &mut dyn Level, pos: BlockPos) {
    let r = l.rand();
    let pitch = 2.6 + (r.next_float() - r.next_float()) * 0.8;
    l.play_sound(pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5, "random.fizz", 0.5, pitch);
}

/// `checkForMixing`
pub fn check_for_mixing(l: &mut dyn Level, pos: BlockPos, state: StateId) -> bool {
    if state.block().material == Material::Lava {
        let mut flag = false;
        for f in Facing::ALL {
            if f != Facing::Down && l.material(pos.offset_facing(f)) == Material::Water {
                flag = true;
                break;
            }
        }
        if flag {
            let level = level_of(state);
            if level == 0 {
                l.set(pos, crate::st(ids::OBSIDIAN, 0));
                trigger_mix_effects(l, pos);
                return true;
            }
            if level <= 4 {
                l.set(pos, crate::st(ids::COBBLESTONE, 0));
                trigger_mix_effects(l, pos);
                return true;
            }
        }
    }
    false
}

pub fn on_block_added(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let id = state.block().id;
    if id == ids::FLOWING_WATER || id == ids::FLOWING_LAVA {
        if !check_for_mixing(l, pos, state) {
            let rate = tick_rate(l, material_of(id));
            l.schedule_update(pos, id, rate);
        }
    } else {
        check_for_mixing(l, pos, state);
    }
}

pub fn on_neighbor_block_change(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let id = state.block().id;
    if id == ids::WATER || id == ids::LAVA {
        if !check_for_mixing(l, pos, state) {
            // updateLiquid
            let m = material_of(id);
            let f = flowing(m);
            l.set_state(pos, with_level(f, level_of(state)), flags::UPDATE);
            let rate = tick_rate(l, m);
            l.schedule_update(pos, f, rate);
        }
    } else {
        check_for_mixing(l, pos, state);
    }
}

/// `BlockDynamicLiquid.isBlocked`
fn is_blocked(l: &mut dyn Level, pos: BlockPos) -> bool {
    let b = l.state(pos).block();
    if b.is_a("BlockDoor") || b.id == ids::STANDING_SIGN || b.id == ids::LADDER || b.id == ids::REEDS {
        return true;
    }
    if b.material == Material::Portal {
        true
    } else {
        b.material.props().blocks_movement
    }
}

/// `canFlowInto`
fn can_flow_into(l: &mut dyn Level, m: Material, pos: BlockPos, state: StateId) -> bool {
    let mat = state.block().material;
    mat != m && mat != Material::Lava && !is_blocked(l, pos)
}

/// `tryFlowInto`
fn try_flow_into(l: &mut dyn Level, m: Material, pos: BlockPos, state: StateId, level: i32) {
    if can_flow_into(l, m, pos, state) {
        if state.block().id != ids::AIR {
            if m == Material::Lava {
                trigger_mix_effects(l, pos);
            } else {
                l.drop_block_as_item(pos, state, 1.0, 0);
            }
        }
        l.set_state(pos, with_level(flowing(m), level), flags::DEFAULT);
    }
}

/// `func_176374_a`: distance to the nearest drop within 4 blocks.
fn flow_cost(l: &mut dyn Level, m: Material, pos: BlockPos, distance: i32, from: Facing) -> i32 {
    let mut i = 1000;
    for f in Facing::PLANE_HORIZONTAL {
        if f != from {
            let p = pos.offset_facing(f);
            let s = l.state(p);
            if !is_blocked(l, p) && (s.block().material != m || level_of(s) > 0) {
                if !is_blocked(l, p.down()) {
                    return distance;
                }
                if distance < 4 {
                    let j = flow_cost(l, m, p, distance + 1, f.opposite());
                    if j < i {
                        i = j;
                    }
                }
            }
        }
    }
    i
}

/// `getPossibleFlowDirections`, in `EnumSet` (ordinal) order: N, S, W, E.
fn possible_flow_directions(l: &mut dyn Level, m: Material, pos: BlockPos) -> Vec<Facing> {
    let mut i = 1000;
    let mut set: Vec<Facing> = Vec::new();
    for f in Facing::PLANE_HORIZONTAL {
        let p = pos.offset_facing(f);
        let s = l.state(p);
        if !is_blocked(l, p) && (s.block().material != m || level_of(s) > 0) {
            let j = if is_blocked(l, p.down()) { flow_cost(l, m, p, 1, f.opposite()) } else { 0 };
            if j < i {
                set.clear();
            }
            if j <= i {
                set.push(f);
                i = j;
            }
        }
    }
    set.sort_by_key(|f| f.index());
    set
}

/// `BlockDynamicLiquid.updateTick`. `rng` is the random the tick was given.
pub fn update_tick_dynamic(l: &mut dyn Level, pos: BlockPos, mut state: StateId, rng: Rng) {
    let id = state.block().id;
    let m = material_of(id);
    let mut i = level_of(state);
    let j = if m == Material::Lava && !l.water_vaporizes() { 2 } else { 1 };
    let mut k = tick_rate(l, m);
    if i > 0 {
        let mut lv = -100;
        let mut sources = 0;
        for f in Facing::PLANE_HORIZONTAL {
            // checkAdjacentBlock
            let a = get_level(l, m, pos.offset_facing(f));
            if a >= 0 {
                if a == 0 {
                    sources += 1;
                }
                let a = if a >= 8 { 0 } else { a };
                lv = if lv >= 0 && a >= lv { lv } else { a };
            }
        }
        let mut i1 = lv + j;
        if i1 >= 8 || lv < 0 {
            i1 = -1;
        }
        let up = get_level(l, m, pos.up());
        if up >= 0 {
            i1 = if up >= 8 { up } else { up + 8 };
        }
        if sources >= 2 && m == Material::Water {
            let below = l.state(pos.down());
            if below.block().material.props().solid {
                i1 = 0;
            } else if below.block().material == m && level_of(below) == 0 {
                i1 = 0;
            }
        }
        if m == Material::Lava && i < 8 && i1 < 8 && i1 > i && l.rng(rng).next_int_bounded(4) != 0 {
            k *= 4;
        }
        if i1 == i {
            place_static(l, pos, state);
        } else {
            i = i1;
            if i1 < 0 {
                l.set_air(pos);
            } else {
                state = with_level(id, i1);
                l.set_state(pos, state, flags::UPDATE);
                l.schedule_update(pos, id, k);
                l.notify_neighbors(pos, id);
            }
        }
    } else {
        place_static(l, pos, state);
    }
    let below_pos = pos.down();
    let below = l.state(below_pos);
    if can_flow_into(l, m, below_pos, below) {
        if m == Material::Lava && l.material(below_pos) == Material::Water {
            l.set(below_pos, crate::st(ids::STONE, 0));
            trigger_mix_effects(l, below_pos);
            return;
        }
        if i >= 8 {
            try_flow_into(l, m, below_pos, below, i);
        } else {
            try_flow_into(l, m, below_pos, below, i + 8);
        }
    } else if i >= 0 && (i == 0 || is_blocked(l, below_pos)) {
        let set = possible_flow_directions(l, m, pos);
        let mut k1 = i + j;
        if i >= 8 {
            k1 = 1;
        }
        if k1 >= 8 {
            return;
        }
        for f in set {
            let p = pos.offset_facing(f);
            let s = l.state(p);
            try_flow_into(l, m, p, s, k1);
        }
    }
}

/// `placeStaticBlock`
fn place_static(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let m = material_of(state.block().id);
    l.set_state(pos, with_level(still(m), level_of(state)), flags::UPDATE);
}

/// `BlockStaticLiquid.updateTick` (lava setting things on fire).
pub fn update_tick_static(l: &mut dyn Level, pos: BlockPos, state: StateId, rng: Rng) {
    if state.block().material != Material::Lava || !l.game_rule("doFireTick") {
        return;
    }
    let i = l.rng(rng).next_int_bounded(3);
    if i > 0 {
        let mut p = pos;
        for _ in 0..i {
            let dx = l.rng(rng).next_int_bounded(3) - 1;
            let dz = l.rng(rng).next_int_bounded(3) - 1;
            p = p.offset(dx, 1, dz);
            let b = l.state(p).block();
            if b.material == Material::Air {
                if surrounding_flammable(l, p) {
                    l.set(p, crate::st(ids::FIRE, 0));
                    return;
                }
            } else if b.material.props().blocks_movement {
                return;
            }
        }
    } else {
        for _ in 0..3 {
            let dx = l.rng(rng).next_int_bounded(3) - 1;
            let dz = l.rng(rng).next_int_bounded(3) - 1;
            let p = pos.offset(dx, 0, dz);
            if l.is_air(p.up()) && l.material(p).props().can_burn {
                l.set(p.up(), crate::st(ids::FIRE, 0));
            }
        }
    }
}

fn surrounding_flammable(l: &mut dyn Level, pos: BlockPos) -> bool {
    Facing::ALL.iter().any(|f| l.material(pos.offset_facing(*f)).props().can_burn)
}

#[allow(dead_code)]
fn _assert_object_safe(_: &mut dyn Level, _: &mut JavaRandom) {}
