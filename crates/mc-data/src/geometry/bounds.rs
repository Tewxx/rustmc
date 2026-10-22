use super::{BlockGeometry, Bounds};
use crate::actual_state::{fence_connects, pane_connects, wall_connects};
use crate::{ids, BlockAccess, StateId};
use mc_core::{Axis, BlockPos, Facing};

use Bounds as B;
use Facing::{Down, East, North, South, Up, West};

pub(super) fn connections(state: StateId, world: &impl BlockAccess, pos: BlockPos) -> [bool; 4] {
    let b = state.block();
    [North, South, West, East].map(|f| {
        let n = world.block(pos.offset_facing(f));
        if b.is_a("BlockPane") {
            pane_connects(b, n)
        } else if b.is_a("BlockWall") {
            wall_connects(b, n)
        } else {
            fence_connects(b, n)
        }
    })
}

fn wall_plate(facing: Facing, y: f32, yy: f32, thickness: f32) -> B {
    match facing {
        North => B::new(0., y, 1. - thickness, 1., yy, 1.),
        South => B::new(0., y, 0., 1., yy, thickness),
        West => B::new(1. - thickness, y, 0., 1., yy, 1.),
        _ => B::new(0., y, 0., thickness, yy, 1.),
    }
}

pub(super) fn piston_head(facing: Facing) -> Bounds {
    match facing {
        Down => B::new(0., 0., 0., 1., 0.25, 1.),
        Up => B::new(0., 0.75, 0., 1., 1., 1.),
        North => B::new(0., 0., 0., 1., 1., 0.25),
        South => B::new(0., 0., 0.75, 1., 1., 1.),
        West => B::new(0., 0., 0., 0.25, 1., 1.),
        East => B::new(0.75, 0., 0., 1., 1., 1.),
    }
}

impl BlockGeometry {
    /// `setBlockBoundsBasedOnState`, retaining the old bounds when the Java method is a no-op.
    pub fn update_bounds(&mut self, state: StateId, world: &impl BlockAccess, pos: BlockPos) -> Bounds {
        let b = state.block();
        let s = world.state(pos);
        let old = self.bounds(state);
        let v = match b.class {
            "BlockBed" => B::new(0., 0., 0., 1., 0.5625, 1.),
            "BlockAnvil" => {
                if s.get_facing("facing").axis() == Axis::X {
                    B::new(0., 0., 0.125, 1., 1., 0.875)
                } else {
                    B::new(0.125, 0., 0., 0.875, 1., 1.)
                }
            }
            "BlockButtonStone" | "BlockButtonWood" => {
                let t = if s.get_bool("powered") { 0.0625 } else { 0.125 };
                match s.get_facing("facing") {
                    East => B::new(0., 0.375, 0.3125, t, 0.625, 0.6875),
                    West => B::new(1. - t, 0.375, 0.3125, 1., 0.625, 0.6875),
                    South => B::new(0.3125, 0.375, 0., 0.6875, 0.625, t),
                    North => B::new(0.3125, 0.375, 1. - t, 0.6875, 0.625, 1.),
                    Up => B::new(0.3125, 0., 0.375, 0.6875, t, 0.625),
                    Down => B::new(0.3125, 1. - t, 0.375, 0.6875, 1., 0.625),
                }
            }
            "BlockPressurePlate" | "BlockPressurePlateWeighted" => {
                let powered = s.value("powered").map_or_else(|| s.get_int("power") > 0, |v| v == "true");
                B::new(0.0625, 0., 0.0625, 0.9375, if powered { 0.03125 } else { 0.0625 }, 0.9375)
            }
            "BlockCake" => B::new((1 + s.get_int("bites") * 2) as f32 / 16., 0., 0.0625, 0.9375, 0.5, 0.9375),
            "BlockCarpet" => B::new(0., 0., 0., 1., 0.0625, 1.),
            "BlockChest" => {
                let same = |p| world.block(p).id == b.id;
                if same(pos.north()) {
                    B::new(0.0625, 0., 0., 0.9375, 0.875, 0.9375)
                } else if same(pos.south()) {
                    B::new(0.0625, 0., 0.0625, 0.9375, 0.875, 1.)
                } else if same(pos.west()) {
                    B::new(0., 0., 0.0625, 0.9375, 0.875, 0.9375)
                } else if same(pos.east()) {
                    B::new(0.0625, 0., 0.0625, 1., 0.875, 0.9375)
                } else {
                    B::new(0.0625, 0., 0.0625, 0.9375, 0.875, 0.9375)
                }
            }
            "BlockCocoa" => {
                let j = (4 + s.get_int("age") * 2) as f32;
                let k = (5 + s.get_int("age") * 2) as f32;
                let f = j / 2.;
                let y = (12. - k) / 16.;
                match s.get_facing("facing") {
                    South => B::new((8. - f) / 16., y, (15. - j) / 16., (8. + f) / 16., 0.75, 0.9375),
                    North => B::new((8. - f) / 16., y, 0.0625, (8. + f) / 16., 0.75, (1. + j) / 16.),
                    West => B::new(0.0625, y, (8. - f) / 16., (1. + j) / 16., 0.75, (8. + f) / 16.),
                    _ => B::new((15. - j) / 16., y, (8. - f) / 16., 0.9375, 0.75, (8. + f) / 16.),
                }
            }
            "BlockDaylightDetector" => B::new(0., 0., 0., 1., 0.375, 1.),
            "BlockDoor" => {
                // combineMetadata deliberately reads metadata from neighbours of any block type.
                let top = s.meta() & 8 != 0;
                let lower = if top { world.state(pos.down()).meta() } else { s.meta() };
                let upper = if top { s.meta() } else { world.state(pos.up()).meta() };
                let facing = Facing::from_horizontal_index(lower as i32 & 3).rotate_y_ccw();
                let open = lower & 4 != 0;
                let hinge = upper & 1 != 0;
                let face = if !open {
                    facing
                } else if hinge {
                    facing.rotate_y_ccw()
                } else {
                    facing.rotate_y()
                };
                wall_plate(face, 0., 1., 0.1875)
            }
            "BlockDoublePlant" | "BlockHopper" => B::FULL,
            "BlockEndPortal" => B::new(0., 0., 0., 1., 0.0625, 1.),
            "BlockFence" | "BlockWall" => {
                let [n, s, w, e] = connections(state, world, pos);
                let (lo, hi) = if b.class == "BlockFence" { (0.375, 0.625) } else { (0.25, 0.75) };
                let (mut x, mut xx, mut z, mut zz, mut yy) =
                    (if w { 0. } else { lo }, if e { 1. } else { hi }, if n { 0. } else { lo }, if s { 1. } else { hi }, 1.);
                if b.class == "BlockWall" {
                    if n && s && !w && !e {
                        x = 0.3125;
                        xx = 0.6875;
                        yy = 0.8125;
                    } else if !n && !s && w && e {
                        z = 0.3125;
                        zz = 0.6875;
                        yy = 0.8125;
                    }
                }
                B::new(x, 0., z, xx, yy, zz)
            }
            "BlockFenceGate" => {
                if s.get_facing("facing").axis() == Axis::Z {
                    B::new(0., 0., 0.375, 1., 1., 0.625)
                } else {
                    B::new(0.375, 0., 0., 0.625, 1., 1.)
                }
            }
            "BlockLadder" if s.block().id == b.id => wall_plate(s.get_facing("facing"), 0., 1., 0.125),
            "BlockLever" | "BlockTripWireHook" => {
                let f = s.expect("facing");
                match f {
                    "east" => B::new(0., 0.2, 0.3125, 0.375, 0.8, 0.6875),
                    "west" => B::new(0.625, 0.2, 0.3125, 1., 0.8, 0.6875),
                    "south" => B::new(0.3125, 0.2, 0., 0.6875, 0.8, 0.375),
                    "north" => B::new(0.3125, 0.2, 0.625, 0.6875, 0.8, 1.),
                    "up_x" | "up_z" => B::new(0.25, 0., 0.25, 0.75, 0.6, 0.75),
                    _ => B::new(0.25, 0.4, 0.25, 0.75, 1., 0.75),
                }
            }
            "BlockPane" | "BlockStainedGlassPane" => {
                let [n, s, w, e] = connections(state, world, pos);
                let any = n || s || w || e;
                let (x, xx) = if w && e || !any {
                    (0., 1.)
                } else if w {
                    (0., 0.5625)
                } else if e {
                    (0.4375, 1.)
                } else {
                    (0.4375, 0.5625)
                };
                let (z, zz) = if n && s || !any {
                    (0., 1.)
                } else if n {
                    (0., 0.5625)
                } else if s {
                    (0.4375, 1.)
                } else {
                    (0.4375, 0.5625)
                };
                B::new(x, 0., z, xx, 1., zz)
            }
            "BlockPistonBase" => {
                let mut v = B::FULL;
                if s.block().id == b.id && s.get_bool("extended") {
                    let f = s.get_facing("facing");
                    let axis = match f.axis() {
                        Axis::X => 0,
                        Axis::Y => 1,
                        Axis::Z => 2,
                    };
                    if matches!(f, Down | North | West) {
                        v.0[axis] = 0.25;
                    } else {
                        v.0[axis + 3] = 0.75;
                    }
                }
                v
            }
            "BlockPistonExtension" => piston_head(s.get_facing("facing")),
            "BlockPistonMoving" => {
                if let Some(piston) = world.moving_piston(pos) {
                    let carried = piston.state.block();
                    if carried.id != b.id && carried.material != crate::Material::Air {
                        let bounds = self.update_bounds(piston.state, world, pos);
                        let mut f = if piston.extending { 1. - piston.progress } else { piston.progress };
                        if matches!(carried.id, ids::PISTON | ids::STICKY_PISTON) {
                            f = 0.;
                        }
                        let (x, y, z) = piston.facing.offset();
                        let delta = [x, y, z].map(|v| (v as f32 * f) as f64);
                        let mut out = bounds;
                        for axis in 0..3 {
                            out.0[axis] -= delta[axis];
                            out.0[axis + 3] -= delta[axis];
                        }
                        out
                    } else {
                        old
                    }
                } else {
                    old
                }
            }
            "BlockPortal" => {
                if s.expect("axis") == "x" {
                    B::new(0., 0., 0.375, 1., 1., 0.625)
                } else {
                    B::new(0.375, 0., 0., 0.625, 1., 1.)
                }
            }
            "BlockSkull" => match s.get_facing("facing") {
                North => B::new(0.25, 0.25, 0.5, 0.75, 0.75, 1.),
                South => B::new(0.25, 0.25, 0., 0.75, 0.75, 0.5),
                West => B::new(0.5, 0.25, 0.25, 1., 0.75, 0.75),
                East => B::new(0., 0.25, 0.25, 0.5, 0.75, 0.75),
                _ => B::new(0.25, 0., 0.25, 0.75, 0.5, 0.75),
            },
            "BlockSnow" => B::new(0., 0., 0., 1., s.get_int("layers") as f32 / 8., 1.),
            "BlockStairs" => match self.stair_ray_pass[b.id as usize] {
                None => B::FULL,
                Some(i) => B::new(
                    0.5 * (i % 2) as f32,
                    0.5 * (i / 4 % 2) as f32,
                    0.5 * (i / 2 % 2) as f32,
                    0.5 + 0.5 * (i % 2) as f32,
                    0.5 + 0.5 * (i / 4 % 2) as f32,
                    0.5 + 0.5 * (i / 2 % 2) as f32,
                ),
            },
            "BlockStem" => B::new(0.375, 0., 0.375, 0.625, (s.get_int("age") * 2 + 2) as f32 / 16., 0.625),
            "BlockTrapDoor" if s.block().id == b.id => {
                if s.get_bool("open") {
                    wall_plate(s.get_facing("facing"), 0., 1., 0.1875)
                } else if s.expect("half") == "top" {
                    B::new(0., 0.8125, 0., 1., 1., 1.)
                } else {
                    B::new(0., 0., 0., 1., 0.1875, 1.)
                }
            }
            "BlockTripWire" => {
                if !s.get_bool("suspended") {
                    B::new(0., 0., 0., 1., 0.09375, 1.)
                } else if !s.get_bool("attached") {
                    B::new(0., 0., 0., 1., 0.5, 1.)
                } else {
                    B::new(0., 0.0625, 0., 1., 0.15625, 1.)
                }
            }
            "BlockVine" => {
                let mut v = [1f32, 1., 1., 0., 0., 0.];
                let mut any = false;
                for f in [West, East, North, South] {
                    if !s.get_bool(f.name()) {
                        continue;
                    }
                    any = true;
                    match f {
                        West => {
                            v[3] = v[3].max(0.0625);
                            v[0] = 0.;
                            v[1] = 0.;
                            v[4] = 1.;
                            v[2] = 0.;
                            v[5] = 1.;
                        }
                        East => {
                            v[0] = v[0].min(0.9375);
                            v[3] = 1.;
                            v[1] = 0.;
                            v[4] = 1.;
                            v[2] = 0.;
                            v[5] = 1.;
                        }
                        North => {
                            v[5] = v[5].max(0.0625);
                            v[2] = 0.;
                            v[0] = 0.;
                            v[3] = 1.;
                            v[1] = 0.;
                            v[4] = 1.;
                        }
                        South => {
                            v[2] = v[2].min(0.9375);
                            v[5] = 1.;
                            v[0] = 0.;
                            v[3] = 1.;
                            v[1] = 0.;
                            v[4] = 1.;
                        }
                        _ => unreachable!(),
                    }
                }
                if !any && world.block(pos.up()).block_normal_cube {
                    v = [0., 0.9375, 0., 1., 1., 1.];
                }
                B(v.map(f64::from))
            }
            "BlockWallSign" => wall_plate(s.get_facing("facing"), 0.28125, 0.78125, 0.125),
            "BlockBannerHanging" => wall_plate(s.get_facing("facing"), 0., 0.78125, 0.125),
            _ if b.is_a("BlockSlab") => {
                if b.full_cube {
                    B::FULL
                } else if s.block().id != b.id {
                    old
                } else if s.expect("half") == "top" {
                    B::new(0., 0.5, 0., 1., 1., 1.)
                } else {
                    B::new(0., 0., 0., 1., 0.5, 1.)
                }
            }
            _ if b.is_a("BlockRailBase") => {
                B::new(0., 0., 0., 1., if s.block().id == b.id && s.expect("shape").starts_with("ascending_") { 0.625 } else { 0.125 }, 1.)
            }
            _ => old,
        };
        self.store(state, v)
    }

    /// Vanilla's neighbour-coordinate argument is derived from the block's own `pos` here.
    pub fn should_side_be_rendered(&self, state: StateId, world: &impl BlockAccess, pos: BlockPos, face: Facing, fancy: bool) -> bool {
        let b = state.block();
        let neighbour = pos.offset_facing(face);
        let n = world.state(neighbour);
        let v = self.bounds(state).0;
        let inset = match face {
            Down => v[1] > 0.,
            Up => v[4] < 1.,
            North => v[2] > 0.,
            South => v[5] < 1.,
            West => v[0] > 0.,
            East => v[3] < 1.,
        };
        let base = inset || !n.block().is_opaque_cube(fancy);
        match b.class {
            "BlockAnvil" | "BlockDragonEgg" | "BlockFence" | "BlockFenceGate" | "BlockHopper" | "BlockPistonExtension" => true,
            "BlockCarpet" | "BlockSnow" => face == Up || base,
            "BlockEndPortal" => face == Down && base,
            "BlockFarmland" => match face {
                Up => true,
                Down => base,
                _ => !n.block().is_opaque_cube(fancy) && n.block().id != ids::FARMLAND,
            },
            "BlockWall" => face != Down || base,
            "BlockPane" | "BlockStainedGlassPane" => n.block().id != b.id && base,
            "BlockOldLeaf" | "BlockNewLeaf" => !(!fancy && n.block().id == b.id) && base,
            "BlockDynamicLiquid" | "BlockStaticLiquid" => n.block().material != b.material && (face == Up || base),
            "BlockPortal" => {
                let axis = if n.block().id == b.id { Some(n.expect("axis")) } else { None };
                if axis == Some("z") && !matches!(face, East | West) || axis == Some("x") && !matches!(face, North | South) {
                    return false;
                }
                let edge = |f: Facing| {
                    world.block(neighbour.offset_facing(f)).id == b.id
                        && world.block(neighbour.offset_facing(f).offset_facing(f)).id != b.id
                };
                let x = edge(West) || edge(East) || axis == Some("x");
                let z = edge(North) || edge(South) || axis == Some("z");
                x && matches!(face, West | East) || z && matches!(face, North | South)
            }
            _ if b.is_a("BlockRedstoneDiode") => face.axis() != Axis::Y,
            _ if b.is_a("BlockBreakable") => {
                if matches!(b.id, ids::GLASS | ids::STAINED_GLASS) && world.state(pos) != n {
                    true
                } else {
                    n.block().id != b.id && base
                }
            }
            _ if b.is_a("BlockSlab") && !b.full_cube => {
                if face.axis() != Axis::Y && !base {
                    return false;
                }
                let is_slab = |s: StateId| matches!(s.block().id, ids::STONE_SLAB | ids::WOODEN_SLAB | ids::STONE_SLAB2);
                let top = is_slab(n) && n.expect("half") == "top";
                let own = world.state(pos);
                let own_top = is_slab(own) && own.expect("half") == "top";
                if own_top {
                    face == Down || face == Up && base || !is_slab(n) || !top
                } else {
                    face == Up || face == Down && base || !is_slab(n) || top
                }
            }
            _ => base,
        }
    }
}
