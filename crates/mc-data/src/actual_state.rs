//! `Block.getActualState`: the parts of a state that are not stored in metadata but derived from
//! neighbours (fence connections, stair shapes, door halves, …) or tile entities (flower pots).
//!
//! Dispatch is on the vanilla class name; only classes that override `getActualState` reach it
//! (`Block::actual_state_override`). Verified against `golden/actual_states.json`.

use crate::block::{ids, Block, StateId};
use crate::material::Material;
use mc_core::{Axis, BlockPos, Facing};

/// Read access to a world, the subset of `IBlockAccess` block logic needs.
pub trait BlockAccess {
    /// `getBlockState`; unloaded or out-of-world positions read as air.
    fn state(&self, pos: BlockPos) -> StateId;

    /// Contents of a `TileEntityFlowerPot` at `pos` as `(item id, data)`, if there is one.
    fn flower_pot(&self, _pos: BlockPos) -> Option<(i32, i32)> {
        None
    }

    /// `TileEntityComparator.getOutputSignal` at `pos`, if there is a comparator tile entity.
    fn comparator_output(&self, _pos: BlockPos) -> Option<i32> {
        None
    }

    fn moving_piston(&self, _pos: BlockPos) -> Option<crate::geometry::MovingPiston> {
        None
    }

    fn block(&self, pos: BlockPos) -> &'static Block {
        self.state(pos).block()
    }

    /// `isAirBlock`: the block's material is air.
    fn is_air(&self, pos: BlockPos) -> bool {
        self.block(pos).material == Material::Air
    }
}

impl StateId {
    /// `getActualState(state, world, pos)`. `self` is the state stored at `pos`.
    pub fn actual_state(self, world: &impl BlockAccess, pos: BlockPos) -> StateId {
        let b = self.block();
        if !b.actual_state_override {
            return self;
        }
        match b.class {
            "BlockGrass" | "BlockMycelium" => self.set_bool("snowy", snow_on_top(world, pos)),
            "BlockDirt" if self.expect("variant") == "podzol" => self.set_bool("snowy", snow_on_top(world, pos)),
            "BlockDirt" => self,
            "BlockBed" => bed(self, world, pos),
            "BlockDoor" => door(self, world, pos),
            "BlockDoublePlant" => double_plant(self, world, pos),
            "BlockStem" => stem(self, world, pos),
            "BlockVine" => self.set_bool("up", world.block(pos.up()).block_normal_cube),
            "BlockFlowerPot" => self.set("contents", flower_pot_contents(world.flower_pot(pos))),
            "BlockFence" => connect4(self, |f| fence_connects(b, world.block(pos.offset_facing(f)))),
            "BlockFenceGate" => fence_gate(self, world, pos),
            "BlockPane" | "BlockStainedGlassPane" => connect4(self, |f| pane_connects(b, world.block(pos.offset_facing(f)))),
            "BlockWall" => connect4(self, |f| wall_connects(b, world.block(pos.offset_facing(f)))).set_bool("up", !world.is_air(pos.up())),
            "BlockTripWire" => connect4(self, |f| tripwire_connects(self, world.state(pos.offset_facing(f)), f)),
            "BlockTripWireHook" => self.set_bool("suspended", !has_solid_top(world.state(pos.down()))),
            "BlockRedstoneRepeater" => self.set_bool("locked", repeater_side_power(self, world, pos) > 0),
            "BlockFire" => fire(self, world, pos),
            "BlockStairs" => stairs(self, world, pos),
            "BlockRedstoneWire" => redstone_wire(self, world, pos),
            other => unreachable!("{other} overrides getActualState but has no port"),
        }
    }
}

/// Snow block or snow layer directly above (grass, mycelium, podzol).
fn snow_on_top(world: &impl BlockAccess, pos: BlockPos) -> bool {
    matches!(world.block(pos.up()).id, ids::SNOW | ids::SNOW_LAYER)
}

/// Sets `north`/`east`/`south`/`west` from a per-side predicate.
fn connect4(state: StateId, mut connects: impl FnMut(Facing) -> bool) -> StateId {
    let mut s = state;
    for f in [Facing::North, Facing::East, Facing::South, Facing::West] {
        s = s.set_bool(f.name(), connects(f));
    }
    s
}

/// Opaque full cube that isn't a gourd: what fences and walls attach to besides their own kind.
fn solid_attachable(n: &Block) -> bool {
    n.material.props().opaque && n.full_cube && n.material != Material::Gourd
}

/// `BlockFence.canConnectTo`: fences of the same material, any fence gate, or a solid block.
pub(crate) fn fence_connects(fence: &Block, n: &Block) -> bool {
    if n.id == ids::BARRIER {
        return false;
    }
    if (n.is_a("BlockFence") && n.material == fence.material) || n.is_a("BlockFenceGate") {
        return true;
    }
    solid_attachable(n)
}

/// `BlockWall.canConnectTo`: the same wall block, any fence gate, or a solid block.
pub(crate) fn wall_connects(wall: &Block, n: &Block) -> bool {
    if n.id == ids::BARRIER {
        return false;
    }
    if n.id == wall.id || n.is_a("BlockFenceGate") {
        return true;
    }
    solid_attachable(n)
}

/// `BlockPane.canPaneConnectToBlock`.
pub(crate) fn pane_connects(pane: &Block, n: &Block) -> bool {
    n.full_block
        || n.id == pane.id
        || matches!(n.id, ids::GLASS | ids::STAINED_GLASS | ids::STAINED_GLASS_PANE)
        || n.is_a("BlockPane")
}

/// `BlockTripWire.isConnectedTo`: hooks facing back at the wire, or wire with the same suspension.
fn tripwire_connects(wire: StateId, n: StateId, dir: Facing) -> bool {
    match n.block().id {
        ids::TRIPWIRE_HOOK => n.get_facing("facing") == dir.opposite(),
        ids::TRIPWIRE => wire.get_bool("suspended") == n.get_bool("suspended"),
        _ => false,
    }
}

/// `World.doesBlockHaveSolidTopSurface`.
pub fn has_solid_top(s: StateId) -> bool {
    let b = s.block();
    if b.material.props().opaque && b.full_cube {
        return true;
    }
    if b.is_a("BlockStairs") || b.is_a("BlockSlab") {
        return s.expect("half") == "top";
    }
    if b.is_a("BlockHopper") {
        return true;
    }
    // Snow layers count only at exactly 7 layers (not 8), as in vanilla.
    b.is_a("BlockSnow") && s.get_int("layers") == 7
}

fn bed(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    if state.expect("part") != "foot" {
        return state;
    }
    let head = world.state(pos.offset_facing(state.get_facing("facing")));
    if head.block().id == state.block().id {
        state.set_bool("occupied", head.get_bool("occupied"))
    } else {
        state
    }
}

/// Each door half stores half the properties; the actual state merges in the other half's.
fn door(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    let id = state.block().id;
    if state.expect("half") == "lower" {
        let upper = world.state(pos.up());
        if upper.block().id == id {
            return state.set("hinge", upper.expect("hinge")).set("powered", upper.expect("powered"));
        }
    } else {
        let lower = world.state(pos.down());
        if lower.block().id == id {
            return state.set("facing", lower.expect("facing")).set("open", lower.expect("open"));
        }
    }
    state
}

fn double_plant(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    if state.expect("half") == "upper" {
        let lower = world.state(pos.down());
        if lower.block().id == state.block().id {
            return state.set("variant", lower.expect("variant"));
        }
    }
    state
}

/// Points at the first adjacent fruit in N, E, S, W order, else up.
fn stem(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    let crop = state.block().stem_crop;
    let towards = Facing::PLANE_HORIZONTAL.into_iter().find(|&f| Some(world.block(pos.offset_facing(f)).id) == crop);
    state.set("facing", towards.unwrap_or(Facing::Up).name())
}

/// `in_wall` is only ever switched on here; it stays as stored otherwise.
fn fence_gate(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    let sides = match state.get_facing("facing").axis() {
        Axis::Z => [Facing::West, Facing::East],
        Axis::X => [Facing::North, Facing::South],
        Axis::Y => return state,
    };
    if sides.iter().any(|&f| world.block(pos.offset_facing(f)).id == ids::COBBLESTONE_WALL) {
        state.set_bool("in_wall", true)
    } else {
        state
    }
}

/// `BlockFlowerPot` contents from the pot's item and data. Only item-block items show; items
/// below 256 are exactly the `ItemBlock`s, whose block shares their id.
fn flower_pot_contents(pot: Option<(i32, i32)>) -> &'static str {
    let Some((item, data)) = pot else { return "empty" };
    if !(0..256).contains(&item) {
        return "empty";
    }
    // Out-of-range data falls back to the first variant (`byMetadata`/`getType` clamp to 0).
    let clamp = |n: usize| if (0..n as i32).contains(&data) { data as usize } else { 0 };
    match item as u16 {
        ids::SAPLING => ["oak_sapling", "spruce_sapling", "birch_sapling", "jungle_sapling", "acacia_sapling", "dark_oak_sapling"][clamp(6)],
        ids::TALLGRASS => match data {
            0 => "dead_bush",
            2 => "fern",
            _ => "empty",
        },
        ids::YELLOW_FLOWER => "dandelion",
        ids::RED_FLOWER => {
            ["rose", "blue_orchid", "allium", "houstonia", "red_tulip", "orange_tulip", "white_tulip", "pink_tulip", "oxeye_daisy"][clamp(9)]
        }
        ids::RED_MUSHROOM => "mushroom_red",
        ids::BROWN_MUSHROOM => "mushroom_brown",
        ids::DEADBUSH => "dead_bush",
        ids::CACTUS => "cactus",
        _ => "empty",
    }
}

/// `BlockRedstoneRepeater.isLocked` → `getPowerOnSides`: strongest power from diodes on either side.
fn repeater_side_power(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> i32 {
    let facing = state.get_facing("facing");
    [facing.rotate_y(), facing.rotate_y_ccw()]
        .into_iter()
        .map(|side| {
            let p = pos.offset_facing(side);
            let n = world.state(p);
            // `canPowerSide`: only repeaters and comparators lock a repeater.
            if is_diode(n.block()) { diode_strong_power(n, world, p, side) } else { 0 }
        })
        .max()
        .unwrap()
}

/// `BlockRedstoneDiode.isRedstoneRepeaterBlockID`.
fn is_diode(b: &Block) -> bool {
    matches!(b.id, ids::UNPOWERED_REPEATER | ids::POWERED_REPEATER | ids::UNPOWERED_COMPARATOR | ids::POWERED_COMPARATOR)
}

/// Diode `getStrongPower` (= `getWeakPower`) towards `side`.
fn diode_strong_power(s: StateId, world: &impl BlockAccess, pos: BlockPos, side: Facing) -> i32 {
    let b = s.block();
    let comparator = b.is_a("BlockRedstoneComparator");
    let powered = b.diode_powered || (comparator && s.get_bool("powered"));
    if !powered || s.get_facing("facing") != side {
        return 0;
    }
    if comparator { world.comparator_output(pos).unwrap_or(0) } else { 15 }
}

fn fire(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    let catches = |p: BlockPos| world.block(p).fire_encouragement > 0;
    if has_solid_top(world.state(pos.down())) || catches(pos.down()) {
        return state.block().default_state;
    }
    let (x, y, z) = (pos.x, pos.y, pos.z);
    // Java `/` truncates toward zero, like Rust's; the parity flags depend on it for negative coords.
    let alt = (x + y + z) & 1 == 1;
    let flip = (x / 2 + y / 2 + z / 2) & 1 == 1;
    let upper = if catches(pos.up()) { if alt { 1 } else { 2 } } else { 0 };
    connect4(state, |f| catches(pos.offset_facing(f)))
        .set("upper", &upper.to_string())
        .set_bool("flip", flip)
        .set_bool("alt", alt)
}

/// Stair shape. A stair in front, turned sideways, makes an outer corner; failing that, one
/// behind makes an inner corner. Either is cancelled by a matching stair alongside.
fn stairs(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    // Vanilla reads the stored state again here rather than the argument.
    let me = world.state(pos);
    let facing = me.get_facing("facing");
    let half = me.expect("half");
    let top = half == "top";

    // `func_176307_f` / `func_176305_g`: 0 straight, 1 right, 2 left.
    let corner = |neighbour: BlockPos, check_towards: bool| -> u8 {
        let n = world.state(neighbour);
        if !n.block().is_a("BlockStairs") || n.expect("half") != half {
            return 0;
        }
        let nf = n.get_facing("facing");
        let side = if check_towards { nf } else { nf.opposite() };
        let same = |p: BlockPos| {
            let s = world.state(p);
            s.block().is_a("BlockStairs") && s.expect("half") == half && s.get_facing("facing") == facing
        };
        let (ccw, cw) = (facing.rotate_y_ccw(), facing.rotate_y());
        if (nf == ccw || nf == cw) && !same(pos.offset_facing(side)) {
            let right = (nf == ccw) == top;
            if right { 1 } else { 2 }
        } else {
            0
        }
    };

    // `func_176306_h` is true unless the outer-corner condition holds.
    let outer = corner(pos.offset_facing(facing), false);
    let shape = if outer != 0 {
        ["straight", "outer_right", "outer_left"][outer as usize]
    } else {
        ["straight", "inner_right", "inner_left"][corner(pos.offset_facing(facing.opposite()), true) as usize]
    };
    state.set("shape", shape)
}

/// `BlockRedstoneWire.canConnectTo`; `side` is `None` for the up/down checks.
fn wire_connects(s: StateId, side: Option<Facing>) -> bool {
    let b = s.block();
    match b.id {
        ids::REDSTONE_WIRE => true,
        ids::UNPOWERED_REPEATER | ids::POWERED_REPEATER => {
            let f = s.get_facing("facing");
            side == Some(f) || side == Some(f.opposite())
        }
        _ => b.can_provide_power && side.is_some(),
    }
}

fn redstone_wire(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> StateId {
    let roof_blocks = world.block(pos.up()).block_normal_cube;
    let attach = |dir: Facing| -> &'static str {
        let p = pos.offset_facing(dir);
        let n = world.state(p);
        let n_cube = n.block().block_normal_cube;
        let side = wire_connects(n, Some(dir)) || (!n_cube && wire_connects(world.state(p.down()), None));
        if side {
            "side"
        } else if !roof_blocks && n_cube && wire_connects(world.state(p.up()), None) {
            "up"
        } else {
            "none"
        }
    };
    let mut s = state;
    for dir in [Facing::West, Facing::East, Facing::North, Facing::South] {
        s = s.set(dir.name(), attach(dir));
    }
    s
}
