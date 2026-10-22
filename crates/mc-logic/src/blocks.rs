//! Per-block behaviour, dispatched on block id the way vanilla dispatches on the `Block`
//! subclass. Blocks not listed use `Block`'s defaults (no-ops).

use crate::{flags, liquid, st, Level, Rng};
use mc_core::{BlockPos, Facing};
use mc_data::{ids, Material, StateId};

#[inline]
fn is_liquid_block(id: u16) -> bool {
    (ids::FLOWING_WATER..=ids::LAVA).contains(&id)
}

#[inline]
fn is_falling(id: u16) -> bool {
    id == ids::SAND || id == ids::GRAVEL || id == ids::ANVIL
}

/// `Block.canPlaceBlockAt` default: the target's material is replaceable.
pub fn replaceable_at(l: &mut dyn Level, pos: BlockPos) -> bool {
    l.material(pos).props().replaceable
}

/// `World.doesBlockHaveSolidTopSurface`
pub fn has_solid_top_surface(l: &mut dyn Level, pos: BlockPos) -> bool {
    let s = l.state(pos);
    let b = s.block();
    if b.material.props().opaque && b.full_cube {
        return true;
    }
    if b.is_a("BlockStairs") {
        return s.value("half") == Some("top");
    }
    if b.is_a("BlockSlab") {
        return s.value("half") == Some("top");
    }
    if b.is_a("BlockHopper") {
        return true;
    }
    if b.is_a("BlockSnow") {
        return s.get_int("layers") == 7;
    }
    false
}

/// `canPlaceBlockOn` of the `BlockBush` family.
pub fn bush_can_place_on(bush: u16, ground: StateId) -> bool {
    let g = ground.block().id;
    match bush {
        ids::DEADBUSH => g == ids::SAND || g == ids::HARDENED_CLAY || g == ids::STAINED_HARDENED_CLAY || g == ids::DIRT,
        ids::WATERLILY => g == ids::WATER,
        ids::BROWN_MUSHROOM | ids::RED_MUSHROOM => ground.block().full_block,
        ids::WHEAT | ids::CARROTS | ids::POTATOES | ids::PUMPKIN_STEM | ids::MELON_STEM => g == ids::FARMLAND,
        ids::NETHER_WART => g == ids::SOUL_SAND,
        _ => g == ids::GRASS || g == ids::DIRT || g == ids::FARMLAND,
    }
}

fn is_bush(id: u16) -> bool {
    matches!(
        id,
        ids::SAPLING | ids::TALLGRASS | ids::DEADBUSH | ids::YELLOW_FLOWER | ids::RED_FLOWER | ids::BROWN_MUSHROOM | ids::RED_MUSHROOM
            | ids::WHEAT | ids::CARROTS | ids::POTATOES | ids::PUMPKIN_STEM | ids::MELON_STEM | ids::NETHER_WART | ids::WATERLILY
            | ids::DOUBLE_PLANT
    )
}

/// `canBlockStay(world, pos, state)` for bushes, reeds and cacti.
pub fn can_block_stay(l: &mut dyn Level, pos: BlockPos, state: StateId) -> bool {
    let id = state.block().id;
    match id {
        ids::BROWN_MUSHROOM | ids::RED_MUSHROOM => {
            if !(0..256).contains(&pos.y) {
                return false;
            }
            let down = l.state(pos.down());
            let d = down.block().id;
            if d == ids::MYCELIUM {
                return true;
            }
            if d == ids::DIRT && down.value("variant") == Some("podzol") {
                return true;
            }
            l.light(pos) < 13 && bush_can_place_on(id, down)
        }
        ids::WATERLILY => {
            if !(0..256).contains(&pos.y) {
                return false;
            }
            let down = l.state(pos.down());
            down.block().material == Material::Water && down.meta() == 0
        }
        ids::DOUBLE_PLANT => {
            if state.value("half") == Some("upper") {
                l.block_id(pos.down()) == ids::DOUBLE_PLANT
            } else {
                let up = l.state(pos.up());
                up.block().id == ids::DOUBLE_PLANT && {
                    let down = l.state(pos.down());
                    bush_can_place_on(id, down)
                }
            }
        }
        ids::REEDS => reeds_can_place_at(l, pos),
        ids::CACTUS => {
            for f in Facing::PLANE_HORIZONTAL {
                if l.material(pos.offset_facing(f)).props().solid {
                    return false;
                }
            }
            let d = l.block_id(pos.down());
            d == ids::CACTUS || d == ids::SAND
        }
        _ => {
            let down = l.state(pos.down());
            bush_can_place_on(id, down)
        }
    }
}

/// `BlockReed.canPlaceBlockAt`
pub fn reeds_can_place_at(l: &mut dyn Level, pos: BlockPos) -> bool {
    let d = l.block_id(pos.down());
    if d == ids::REEDS {
        return true;
    }
    if d != ids::GRASS && d != ids::DIRT && d != ids::SAND {
        return false;
    }
    for f in Facing::PLANE_HORIZONTAL {
        if l.material(pos.offset_facing(f).down()) == Material::Water {
            return true;
        }
    }
    false
}

/// `canPlaceBlockAt(world, pos)` for `block`.
pub fn can_place_block_at(l: &mut dyn Level, block: u16, pos: BlockPos) -> bool {
    match block {
        ids::REEDS => reeds_can_place_at(l, pos),
        ids::CACTUS => replaceable_at(l, pos) && can_block_stay(l, pos, st(ids::CACTUS, 0)),
        ids::PUMPKIN | ids::LIT_PUMPKIN => replaceable_at(l, pos) && has_solid_top_surface(l, pos.down()),
        ids::DOUBLE_PLANT => {
            replaceable_at(l, pos) && {
                let down = l.state(pos.down());
                bush_can_place_on(block, down)
            } && l.is_air(pos.up())
        }
        ids::SNOW_LAYER => {
            let down = l.state(pos.down());
            let b = down.block();
            if b.id == ids::ICE || b.id == ids::PACKED_ICE {
                return false;
            }
            if b.material == Material::Leaves {
                return true;
            }
            if b.id == ids::SNOW_LAYER && down.get_int("layers") >= 7 {
                return true;
            }
            b.opaque_cube && b.material.props().blocks_movement
        }
        id if is_bush(id) => {
            replaceable_at(l, pos) && {
                let down = l.state(pos.down());
                bush_can_place_on(id, down)
            }
        }
        _ => replaceable_at(l, pos),
    }
}

/// `BlockVine.canPlaceOn`
pub fn vine_can_place_on(b: &mc_data::Block) -> bool {
    b.full_cube && b.material.props().blocks_movement
}

/// `BlockVine.canPlaceBlockOnSide`
pub fn vine_can_place_on_side(l: &mut dyn Level, pos: BlockPos, side: Facing) -> bool {
    match side {
        Facing::Up => vine_can_place_on(l.state(pos.up()).block()),
        Facing::North | Facing::South | Facing::East | Facing::West => vine_can_place_on(l.state(pos.offset_facing(side.opposite())).block()),
        Facing::Down => false,
    }
}

fn vine_prop(f: Facing) -> &'static str {
    match f {
        Facing::North => "north",
        Facing::South => "south",
        Facing::East => "east",
        Facing::West => "west",
        _ => "up",
    }
}

/// `BlockVine.recheckGrownSides`
fn vine_recheck_grown_sides(l: &mut dyn Level, pos: BlockPos, state: StateId) -> bool {
    let original = state;
    let mut state = state;
    for f in Facing::PLANE_HORIZONTAL {
        let p = vine_prop(f);
        if state.get_bool(p) && !vine_can_place_on(l.state(pos.offset_facing(f)).block()) {
            let up = l.state(pos.up());
            if up.block().id != ids::VINE || !up.get_bool(p) {
                state = state.set_bool(p, false);
            }
        }
    }
    let grown = Facing::PLANE_HORIZONTAL.iter().filter(|f| state.get_bool(vine_prop(**f))).count();
    if grown == 0 {
        return false;
    }
    if original != state {
        l.set_state(pos, state, flags::UPDATE);
    }
    true
}

/// `Block.onBlockAdded`
pub fn on_block_added(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let id = state.block().id;
    match id {
        _ if is_liquid_block(id) => liquid::on_block_added(l, pos, state),
        _ if is_falling(id) => l.schedule_update(pos, id, 2),
        ids::DRAGON_EGG => l.schedule_update(pos, id, 5),
        // BlockFire.onBlockAdded (no nether portal yet)
        ids::FIRE => {
            if !has_solid_top_surface(l, pos.down()) && !fire_neighbor_can_catch(l, pos) {
                l.set_air(pos);
            } else {
                let d = 30 + l.rng(Rng::World).next_int_bounded(10);
                l.schedule_update(pos, id, d);
            }
        }
        ids::CHEST | ids::TRAPPED_CHEST => {
            check_for_surrounding_chests(l, pos, state);
            for f in Facing::PLANE_HORIZONTAL {
                let p = pos.offset_facing(f);
                let s = l.state(p);
                if s.block().id == id {
                    check_for_surrounding_chests(l, p, s);
                }
            }
        }
        _ => {}
    }
}

/// `Block.breakBlock`
pub fn break_block(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let id = state.block().id;
    let r = match id {
        ids::LEAVES | ids::LEAVES2 => 1,
        ids::LOG | ids::LOG2 => 4,
        _ => return,
    };
    let j = r + 1;
    if l.is_area_loaded(pos.offset(-j, -j, -j), pos.offset(j, j, j)) {
        // BlockPos.getAllInBox order: x fastest, then y, then z (BlockLeaves uses x, y, z loops
        // in the other nesting, but every update here is independent).
        for dz in -r..=r {
            for dy in -r..=r {
                for dx in -r..=r {
                    let p = pos.offset(dx, dy, dz);
                    let s = l.state(p);
                    if s.block().material == Material::Leaves && !s.get_bool("check_decay") {
                        l.set_state(p, s.set_bool("check_decay", true), flags::NO_RERENDER);
                    }
                }
            }
        }
    }
}

/// `Block.onNeighborBlockChange`
pub fn on_neighbor_block_change(l: &mut dyn Level, pos: BlockPos, state: StateId, _neighbor: u16) {
    let id = state.block().id;
    match id {
        _ if is_liquid_block(id) => liquid::on_neighbor_block_change(l, pos, state),
        _ if is_falling(id) => l.schedule_update(pos, id, 2),
        ids::DRAGON_EGG => l.schedule_update(pos, id, 5),
        ids::DOUBLE_PLANT => double_plant_check_and_drop(l, pos, state),
        ids::REEDS => {
            if !can_block_stay(l, pos, state) {
                l.drop_block_as_item(pos, state, 1.0, 0);
                l.set_air(pos);
            }
        }
        ids::CACTUS => {
            if !can_block_stay(l, pos, state) {
                destroy_block(l, pos, true);
            }
        }
        ids::SNOW_LAYER => {
            if !can_place_block_at(l, ids::SNOW_LAYER, pos) {
                l.drop_block_as_item(pos, state, 1.0, 0);
                l.set_air(pos);
            }
        }
        ids::VINE => {
            if !l.is_remote() && !vine_recheck_grown_sides(l, pos, state) {
                l.drop_block_as_item(pos, state, 1.0, 0);
                l.set_air(pos);
            }
        }
        _ if is_bush(id) => {
            // BlockBush.checkAndDropBlock
            if !can_block_stay(l, pos, state) {
                l.drop_block_as_item(pos, state, 1.0, 0);
                l.set_state(pos, StateId::AIR, flags::DEFAULT);
            }
        }
        _ => {}
    }
}

/// `BlockDoublePlant.checkAndDropBlock`
fn double_plant_check_and_drop(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    if can_block_stay(l, pos, state) {
        return;
    }
    let upper = state.value("half") == Some("upper");
    let top = if upper { pos } else { pos.up() };
    let bottom = if upper { pos.down() } else { pos };
    let top_block = if upper { ids::DOUBLE_PLANT } else { l.block_id(top) };
    let bottom_block = if upper { l.block_id(bottom) } else { ids::DOUBLE_PLANT };
    if top_block == ids::DOUBLE_PLANT {
        l.set_state(top, StateId::AIR, flags::UPDATE);
    }
    if bottom_block == ids::DOUBLE_PLANT {
        l.set_state(bottom, StateId::AIR, flags::DEFAULT);
        if !upper {
            l.drop_block_as_item(bottom, state, 1.0, 0);
        }
    }
}

/// `World.destroyBlock(pos, dropBlock)`
pub fn destroy_block(l: &mut dyn Level, pos: BlockPos, drop: bool) -> bool {
    let s = l.state(pos);
    if s.block().material == Material::Air {
        return false;
    }
    if drop {
        l.drop_block_as_item(pos, s, 1.0, 0);
    }
    l.set_state(pos, StateId::AIR, flags::DEFAULT)
}

/// `Block.updateTick`
pub fn update_tick(l: &mut dyn Level, pos: BlockPos, state: StateId, rng: Rng) {
    let id = state.block().id;
    match id {
        ids::FLOWING_WATER | ids::FLOWING_LAVA => liquid::update_tick_dynamic(l, pos, state, rng),
        ids::WATER | ids::LAVA => liquid::update_tick_static(l, pos, state, rng),
        ids::FIRE => fire_tick(l, pos, state),
        _ => {}
    }
}

/// `BlockChest.correctFacing`
pub fn chest_correct_facing(l: &mut dyn Level, pos: BlockPos, state: StateId) -> StateId {
    let id = state.block().id;
    let mut facing: Option<Facing> = None;
    for f in Facing::PLANE_HORIZONTAL {
        let s = l.state(pos.offset_facing(f));
        if s.block().id == id {
            return state;
        }
        if s.block().full_block {
            if facing.is_some() {
                facing = None;
                break;
            }
            facing = Some(f);
        }
    }
    if let Some(f) = facing {
        return state.set("facing", f.opposite().name());
    }
    let mut f2 = state.get_facing("facing");
    if l.state(pos.offset_facing(f2)).block().full_block {
        f2 = f2.opposite();
    }
    if l.state(pos.offset_facing(f2)).block().full_block {
        f2 = f2.rotate_y();
    }
    if l.state(pos.offset_facing(f2)).block().full_block {
        f2 = f2.opposite();
    }
    state.set("facing", f2.name())
}

/// `BlockChest.checkForSurroundingChests`
pub fn check_for_surrounding_chests(l: &mut dyn Level, pos: BlockPos, state: StateId) -> StateId {
    if l.is_remote() {
        return state;
    }
    let id = state.block().id;
    let n = l.state(pos.north());
    let s = l.state(pos.south());
    let w = l.state(pos.west());
    let e = l.state(pos.east());
    let mut facing = state.get_facing("facing");
    let (bn, bs, bw, be) = (n.block(), s.block(), w.block(), e.block());
    if bn.id != id && bs.id != id {
        let flag = bn.full_block;
        let flag1 = bs.full_block;
        if bw.id == id || be.id == id {
            let p1 = if bw.id == id { pos.west() } else { pos.east() };
            let s6 = l.state(p1.north()).block();
            let s7 = l.state(p1.south()).block();
            facing = Facing::South;
            let f2 = if bw.id == id { w.get_facing("facing") } else { e.get_facing("facing") };
            if f2 == Facing::North {
                facing = Facing::North;
            }
            if (flag || s6.full_block) && !flag1 && !s7.full_block {
                facing = Facing::South;
            }
            if (flag1 || s7.full_block) && !flag && !s6.full_block {
                facing = Facing::North;
            }
        }
    } else {
        let p = if bn.id == id { pos.north() } else { pos.south() };
        let s4 = l.state(p.west()).block();
        let s5 = l.state(p.east()).block();
        facing = Facing::East;
        let f1 = if bn.id == id { n.get_facing("facing") } else { s.get_facing("facing") };
        if f1 == Facing::West {
            facing = Facing::West;
        }
        if (bw.full_block || s4.full_block) && !be.full_block && !s5.full_block {
            facing = Facing::East;
        }
        if (be.full_block || s5.full_block) && !bw.full_block && !s4.full_block {
            facing = Facing::West;
        }
    }
    let state = state.set("facing", facing.name());
    l.set_state(pos, state, flags::DEFAULT);
    state
}

/// `Block.randomTick` for the blocks whose random tick does something.
pub fn random_tick(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let id = state.block().id;
    match id {
        ids::GRASS | ids::MYCELIUM => spread_tick(l, pos, id),
        // BlockIce: melts when block light > 11 - lightOpacity(3)
        ids::ICE => {
            if l.light_for(false, pos) > 11 - state.block().light_opacity as i32 {
                if l.water_vaporizes() {
                    l.set_air(pos);
                } else {
                    l.drop_block_as_item(pos, state, 1.0, 0);
                    l.set(pos, st(ids::WATER, 0));
                }
            }
        }
        // BlockSnow
        ids::SNOW_LAYER => {
            if l.light_for(false, pos) > 11 {
                l.drop_block_as_item(pos, state, 1.0, 0);
                l.set_air(pos);
            }
        }
        ids::REEDS => {
            let below = l.state(pos.down()).block().id;
            if below == ids::REEDS || check_for_drop(l, pos, state) {
                grow_column(l, pos, state, false);
            }
        }
        ids::CACTUS => grow_column(l, pos, state, true),
        ids::WHEAT | ids::CARROTS | ids::POTATOES => crop_tick(l, pos, state),
        ids::FARMLAND => farmland_tick(l, pos, state),
        ids::LEAVES | ids::LEAVES2 => leaves_tick(l, pos, state),
        ids::VINE => vine_tick(l, pos, state),
        ids::SAPLING => {
            // BlockSapling.updateTick: BlockBush check, then grow at light >= 9 with 1/7.
            if !can_block_stay(l, pos, state) {
                l.drop_block_as_item(pos, state, 1.0, 0);
                l.set_state(pos, StateId::AIR, flags::DEFAULT);
            } else if l.light(pos.up()) >= 9 && l.rng(Rng::World).next_int_bounded(7) == 0 {
                if state.get_int("stage") == 0 {
                    l.set_state(pos, state.cycle("stage").unwrap_or(state), flags::NO_RERENDER);
                } else {
                    l.grow_tree(pos, state);
                }
            }
        }
        _ => update_tick(l, pos, state, Rng::World),
    }
}

/// `BlockReed.checkForDrop`
fn check_for_drop(l: &mut dyn Level, pos: BlockPos, state: StateId) -> bool {
    if reeds_can_place_at(l, pos) {
        return true;
    }
    l.drop_block_as_item(pos, state, 1.0, 0);
    l.set_air(pos);
    false
}

/// `BlockReed.updateTick` / `BlockCactus.updateTick`: up to three tall, `age` 0..15.
fn grow_column(l: &mut dyn Level, pos: BlockPos, state: StateId, cactus: bool) {
    let up = pos.up();
    if !l.is_air(up) {
        return;
    }
    let id = state.block().id;
    let mut i = 1;
    while l.state(pos.offset(0, -i, 0)).block().id == id {
        i += 1;
    }
    if i < 3 {
        let age = state.get_int("age");
        if age == 15 {
            let top = state.block().default_state;
            l.set(up, top);
            let reset = state.set("age", "0");
            l.set_state(pos, reset, flags::NO_RERENDER);
            if cactus {
                on_neighbor_block_change(l, up, top, id);
            }
        } else {
            l.set_state(pos, state.set("age", &(age + 1).to_string()), flags::NO_RERENDER);
        }
    }
}

/// `BlockGrass.updateTick` / `BlockMycelium.updateTick`
fn spread_tick(l: &mut dyn Level, pos: BlockPos, id: u16) {
    let up = pos.up();
    if l.light(up) < 4 && l.state(up).block().light_opacity > 2 {
        l.set(pos, st(ids::DIRT, 0));
    } else if l.light(up) >= 9 {
        for _ in 0..4 {
            let r = l.rng(Rng::World);
            let (dx, dy, dz) = (r.next_int_bounded(3) - 1, r.next_int_bounded(5) - 3, r.next_int_bounded(3) - 1);
            let p = pos.offset(dx, dy, dz);
            let above = l.state(p.up()).block();
            let s = l.state(p);
            if s == st(ids::DIRT, 0) && l.light(p.up()) >= 4 && above.light_opacity <= 2 {
                l.set(p, st(id, 0));
            }
        }
    }
}

/// `BlockCrops.updateTick` (after `BlockBush.checkAndDropBlock`).
fn crop_tick(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    if !can_block_stay(l, pos, state) {
        l.drop_block_as_item(pos, state, 1.0, 0);
        l.set_state(pos, StateId::AIR, flags::DEFAULT);
        return;
    }
    if l.light(pos.up()) >= 9 {
        let age = state.get_int("age");
        if age < 7 {
            let f = growth_chance(l, pos, state.block().id);
            if l.rng(Rng::World).next_int_bounded((25.0 / f) as i32 + 1) == 0 {
                l.set_state(pos, state.set("age", &(age + 1).to_string()), flags::UPDATE);
            }
        }
    }
}

/// `BlockCrops.getGrowthChance`
fn growth_chance(l: &mut dyn Level, pos: BlockPos, id: u16) -> f32 {
    let mut f = 1.0f32;
    let down = pos.down();
    for i in -1..=1 {
        for j in -1..=1 {
            let s = l.state(down.offset(i, 0, j));
            let mut f1 = 0.0;
            if s.block().id == ids::FARMLAND {
                f1 = 1.0;
                if s.get_int("moisture") > 0 {
                    f1 = 3.0;
                }
            }
            if i != 0 || j != 0 {
                f1 /= 4.0;
            }
            f += f1;
        }
    }
    let mut is = |dx: i32, dz: i32| l.state(pos.offset(dx, 0, dz)).block().id == id;
    let ew = is(-1, 0) || is(1, 0);
    let ns = is(0, -1) || is(0, 1);
    if ew && ns {
        f /= 2.0;
    } else if is(-1, -1) || is(1, -1) || is(1, 1) || is(-1, 1) {
        f /= 2.0;
    }
    f
}

/// `BlockFarmland.updateTick` (rain on the farmland is not modelled: no weather in `Level`).
fn farmland_tick(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    let moisture = state.get_int("moisture");
    let mut water = false;
    'w: for x in -4..=4 {
        for y in 0..=1 {
            for z in -4..=4 {
                if l.state(pos.offset(x, y, z)).block().material == Material::Water {
                    water = true;
                    break 'w;
                }
            }
        }
    }
    if !water {
        if moisture > 0 {
            l.set_state(pos, state.set("moisture", &(moisture - 1).to_string()), flags::UPDATE);
        } else {
            let up = l.state(pos.up()).block().id;
            if !matches!(up, ids::WHEAT | ids::CARROTS | ids::POTATOES | ids::PUMPKIN_STEM | ids::MELON_STEM) {
                l.set(pos, st(ids::DIRT, 0));
            }
        }
    } else if moisture < 7 {
        l.set_state(pos, state.set("moisture", "7"), flags::UPDATE);
    }
}

/// `BlockLeaves.updateTick`: decays unless a log is within 4 leaf steps.
fn leaves_tick(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    if !(state.get_bool("check_decay") && state.get_bool("decayable")) {
        return;
    }
    if !l.is_area_loaded(pos.offset(-5, -5, -5), pos.offset(5, 5, 5)) {
        return;
    }
    let idx = |x: i32, y: i32, z: i32| ((x + 16) * 1024 + (y + 16) * 32 + z + 16) as usize;
    let mut surr = vec![0i32; 32768];
    for x in -4..=4 {
        for y in -4..=4 {
            for z in -4..=4 {
                let b = l.state(pos.offset(x, y, z)).block();
                surr[idx(x, y, z)] = if matches!(b.id, ids::LOG | ids::LOG2) {
                    0
                } else if b.material == Material::Leaves {
                    -2
                } else {
                    -1
                };
            }
        }
    }
    for i in 1..=4 {
        for x in -4..=4 {
            for y in -4..=4 {
                for z in -4..=4 {
                    if surr[idx(x, y, z)] == i - 1 {
                        for (dx, dy, dz) in [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)] {
                            let k = idx(x + dx, y + dy, z + dz);
                            if surr[k] == -2 {
                                surr[k] = i;
                            }
                        }
                    }
                }
            }
        }
    }
    if surr[idx(0, 0, 0)] >= 0 {
        l.set_state(pos, state.set_bool("check_decay", false), flags::NO_RERENDER);
    } else {
        l.drop_block_as_item(pos, state, 1.0, 0);
        l.set_air(pos);
    }
}

/// `BlockVine.updateTick`
fn vine_tick(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    if l.rng(Rng::World).next_int_bounded(4) != 0 {
        return;
    }
    let mut j = 5;
    let mut crowded = false;
    'scan: for x in -4..=4 {
        for z in -4..=4 {
            for y in -1..=1 {
                if l.state(pos.offset(x, y, z)).block().id == ids::VINE {
                    j -= 1;
                    if j <= 0 {
                        crowded = true;
                        break 'scan;
                    }
                }
            }
        }
    }
    let f = Facing::ALL[l.rng(Rng::World).next_int_bounded(6) as usize];
    let up = pos.up();
    let horizontal = !matches!(f, Facing::Up | Facing::Down);
    if f == Facing::Up && pos.y < 255 && l.is_air(up) {
        if !crowded {
            let mut st = state;
            for h in Facing::PLANE_HORIZONTAL {
                let drop = l.rng(Rng::World).next_boolean();
                if drop || !vine_can_place_on(l.state(up.offset_facing(h)).block()) {
                    st = st.set_bool(vine_prop(h), false);
                }
            }
            if Facing::PLANE_HORIZONTAL.iter().any(|&h| st.get_bool(vine_prop(h))) {
                l.set_state(up, st, flags::UPDATE);
            }
        }
    } else if horizontal && !state.get_bool(vine_prop(f)) {
        if !crowded {
            let p3 = pos.offset_facing(f);
            let b1 = l.state(p3).block();
            let def = state.block().default_state;
            if b1.material == Material::Air {
                let (f1, f2) = (f.rotate_y(), f.rotate_y_ccw());
                let (fl1, fl2) = (state.get_bool(vine_prop(f1)), state.get_bool(vine_prop(f2)));
                let (p4, p5) = (p3.offset_facing(f1), p3.offset_facing(f2));
                let can = |l: &mut dyn Level, p: BlockPos| vine_can_place_on(l.state(p).block());
                if fl1 && can(l, p4) {
                    l.set_state(p3, def.set_bool(vine_prop(f1), true), flags::UPDATE);
                } else if fl2 && can(l, p5) {
                    l.set_state(p3, def.set_bool(vine_prop(f2), true), flags::UPDATE);
                } else if fl1 && l.is_air(p4) && can(l, pos.offset_facing(f1)) {
                    l.set_state(p4, def.set_bool(vine_prop(f.opposite()), true), flags::UPDATE);
                } else if fl2 && l.is_air(p5) && can(l, pos.offset_facing(f2)) {
                    l.set_state(p5, def.set_bool(vine_prop(f.opposite()), true), flags::UPDATE);
                } else if can(l, p3.up()) {
                    l.set_state(p3, def, flags::UPDATE);
                }
            } else if b1.material.props().opaque && b1.full_cube {
                l.set_state(pos, state.set_bool(vine_prop(f), true), flags::UPDATE);
            }
        }
    } else if pos.y > 1 {
        let p2 = pos.down();
        let s2 = l.state(p2);
        if s2.block().material == Material::Air {
            let mut st = state;
            for h in Facing::PLANE_HORIZONTAL {
                if l.rng(Rng::World).next_boolean() {
                    st = st.set_bool(vine_prop(h), false);
                }
            }
            if Facing::PLANE_HORIZONTAL.iter().any(|&h| st.get_bool(vine_prop(h))) {
                l.set_state(p2, st, flags::UPDATE);
            }
        } else if s2.block().id == ids::VINE {
            let mut st = s2;
            for h in Facing::PLANE_HORIZONTAL {
                if l.rng(Rng::World).next_boolean() && state.get_bool(vine_prop(h)) {
                    st = st.set_bool(vine_prop(h), true);
                }
            }
            if st != s2 {
                l.set_state(p2, st, flags::UPDATE);
            }
        }
    }
}

fn flammability(l: &mut dyn Level, pos: BlockPos) -> i32 {
    l.state(pos).block().fire_flammability as i32
}

/// `BlockFire.canNeighborCatchFire`
fn fire_neighbor_can_catch(l: &mut dyn Level, pos: BlockPos) -> bool {
    Facing::ALL.iter().any(|&f| l.state(pos.offset_facing(f)).block().fire_encouragement > 0)
}

/// `BlockFire.getNeighborEncouragement`
fn fire_neighbor_encouragement(l: &mut dyn Level, pos: BlockPos) -> i32 {
    if !l.is_air(pos) {
        return 0;
    }
    Facing::ALL.iter().map(|&f| l.state(pos.offset_facing(f)).block().fire_encouragement as i32).max().unwrap_or(0)
}

/// `BlockFire.catchOnFire` (rain and TNT priming are not modelled here).
fn fire_catch(l: &mut dyn Level, pos: BlockPos, chance: i32, age: i32) {
    let i = flammability(l, pos);
    if l.rng(Rng::World).next_int_bounded(chance) < i {
        if l.rng(Rng::World).next_int_bounded(age + 10) < 5 {
            let j = (age + l.rng(Rng::World).next_int_bounded(5) / 4).min(15);
            l.set_state(pos, st(ids::FIRE, 0).set("age", &j.to_string()), flags::DEFAULT);
        } else {
            l.set_air(pos);
        }
    }
}

/// `BlockFire.updateTick` in the overworld, not raining, difficulty normal.
fn fire_tick(l: &mut dyn Level, pos: BlockPos, state: StateId) {
    if !l.game_rule("doFireTick") {
        return;
    }
    if !has_solid_top_surface(l, pos.down()) && !fire_neighbor_can_catch(l, pos) {
        l.set_air(pos);
    }
    let infinite = l.state(pos.down()).block().id == ids::NETHERRACK;
    let age = state.get_int("age");
    if age < 15 {
        let na = age + l.rng(Rng::World).next_int_bounded(3) / 2;
        l.set_state(pos, state.set("age", &na.to_string()), flags::NO_RERENDER);
    }
    let d = 30 + l.rng(Rng::World).next_int_bounded(10);
    l.schedule_update(pos, ids::FIRE, d);
    if !infinite {
        if !fire_neighbor_can_catch(l, pos) {
            if !has_solid_top_surface(l, pos.down()) || age > 3 {
                l.set_air(pos);
            }
            return;
        }
        if l.state(pos.down()).block().fire_encouragement == 0 && age == 15 && l.rng(Rng::World).next_int_bounded(4) == 0 {
            l.set_air(pos);
            return;
        }
    }
    fire_catch(l, pos.offset(1, 0, 0), 300, age);
    fire_catch(l, pos.offset(-1, 0, 0), 300, age);
    fire_catch(l, pos.down(), 250, age);
    fire_catch(l, pos.up(), 250, age);
    fire_catch(l, pos.offset(0, 0, -1), 300, age);
    fire_catch(l, pos.offset(0, 0, 1), 300, age);
    const DIFFICULTY: i32 = 2;
    for x in -1..=1 {
        for z in -1..=1 {
            for y in -1..=4 {
                if x == 0 && y == 0 && z == 0 {
                    continue;
                }
                let mut l1 = 100;
                if y > 1 {
                    l1 += (y - 1) * 100;
                }
                let bp = pos.offset(x, y, z);
                let enc = fire_neighbor_encouragement(l, bp);
                if enc > 0 {
                    let j2 = (enc + 40 + DIFFICULTY * 7) / (age + 30);
                    if j2 > 0 && l.rng(Rng::World).next_int_bounded(l1) <= j2 {
                        let k2 = (age + l.rng(Rng::World).next_int_bounded(5) / 4).min(15);
                        l.set_state(bp, state.set("age", &k2.to_string()), flags::DEFAULT);
                    }
                }
            }
        }
    }
}
