use super::{
    bounds::{connections, piston_head},
    BlockGeometry, Bounds, CollisionEntity, RayHit,
};
use crate::{BlockAccess, StateId};
use mc_core::{aabb::HitFace, Aabb, Axis, BlockPos, Facing, Vec3};
use Bounds as B;
use Facing::{Down, East, North, South, Up, West};

impl BlockGeometry {
    pub fn selected_box(&mut self, state: StateId, world: &impl BlockAccess, pos: BlockPos) -> Aabb {
        let b = state.block();
        if b.class == "BlockCactus" {
            return Aabb::new(
                (pos.x as f32 + 0.0625) as f64,
                pos.y as f64,
                (pos.z as f32 + 0.0625) as f64,
                (pos.x.wrapping_add(1) as f32 - 0.0625) as f64,
                pos.y.wrapping_add(1) as f64,
                (pos.z.wrapping_add(1) as f32 - 0.0625) as f64,
            );
        }
        if b.class == "BlockCake" {
            return self.collision_box(world.state(pos), world, pos).unwrap();
        }
        if matches!(b.class, "BlockCocoa" | "BlockDoor" | "BlockLadder" | "BlockTrapDoor") || b.is_a("BlockBanner") || b.is_a("BlockSign") {
            self.update_bounds(state, world, pos);
        }
        self.bounds(state).world_box(pos)
    }

    /// `getCollisionBoundingBox`, including its mutations of the singleton's current bounds.
    pub fn collision_box(&mut self, state: StateId, world: &impl BlockAccess, pos: BlockPos) -> Option<Aabb> {
        let b = state.block();
        match b.class {
            "BlockAir" | "BlockFire" | "BlockLever" | "BlockTripWire" | "BlockTripWireHook" | "BlockVine" | "BlockWeb" | "BlockReed"
            | "BlockRedstoneWire" => return None,
            "BlockPistonMoving" => {
                let piston = world.moving_piston(pos)?;
                if piston.state.block().id == b.id || piston.state.block().material == crate::Material::Air {
                    return None;
                }
                let v = self.collision_box(piston.state, world, pos)?;
                let mut values = [v.min_x, v.min_y, v.min_z, v.max_x, v.max_y, v.max_z];
                let f = if piston.extending { 1. - piston.progress } else { piston.progress };
                let (x, y, z) = piston.facing.offset();
                for (axis, d) in [x, y, z].into_iter().enumerate() {
                    values[axis + if d < 0 { 0 } else { 3 }] -= (d as f32 * f) as f64;
                }
                return Some(Aabb::new(values[0], values[1], values[2], values[3], values[4], values[5]));
            }
            "BlockLilyPad" => return Some(self.bounds(state).world_box(pos)),
            "BlockCactus" => {
                return Some(Aabb::new(
                    (pos.x as f32 + 0.0625) as f64,
                    pos.y as f64,
                    (pos.z as f32 + 0.0625) as f64,
                    (pos.x.wrapping_add(1) as f32 - 0.0625) as f64,
                    (pos.y.wrapping_add(1) as f32 - 0.0625) as f64,
                    (pos.z.wrapping_add(1) as f32 - 0.0625) as f64,
                ))
            }
            "BlockCake" => {
                return Some(Aabb::new(
                    (pos.x as f32 + (1 + state.get_int("bites") * 2) as f32 / 16.) as f64,
                    pos.y as f64,
                    (pos.z as f32 + 0.0625) as f64,
                    (pos.x.wrapping_add(1) as f32 - 0.0625) as f64,
                    (pos.y as f32 + 0.5) as f64,
                    (pos.z.wrapping_add(1) as f32 - 0.0625) as f64,
                ))
            }
            "BlockFarmland" => {
                return Some(Aabb::new(
                    pos.x as f64,
                    pos.y as f64,
                    pos.z as f64,
                    pos.x.wrapping_add(1) as f64,
                    pos.y.wrapping_add(1) as f64,
                    pos.z.wrapping_add(1) as f64,
                ))
            }
            "BlockSoulSand" => {
                return Some(Aabb::new(
                    pos.x as f64,
                    pos.y as f64,
                    pos.z as f64,
                    pos.x.wrapping_add(1) as f64,
                    (pos.y.wrapping_add(1) as f32 - 0.125) as f64,
                    pos.z.wrapping_add(1) as f64,
                ))
            }
            "BlockSnow" => {
                let mut v = self.bounds(state).world_box(pos);
                v.max_y = (pos.y as f32 + (state.get_int("layers") - 1) as f32 * 0.125) as f64;
                return Some(Aabb::new(v.min_x, v.min_y, v.min_z, v.max_x, v.max_y, v.max_z));
            }
            "BlockFenceGate" => {
                if state.get_bool("open") {
                    return None;
                }
                return Some(if state.get_facing("facing").axis() == Axis::Z {
                    Aabb::new(
                        pos.x as f64,
                        pos.y as f64,
                        (pos.z as f32 + 0.375) as f64,
                        pos.x.wrapping_add(1) as f64,
                        (pos.y as f32 + 1.5) as f64,
                        (pos.z as f32 + 0.625) as f64,
                    )
                } else {
                    Aabb::new(
                        (pos.x as f32 + 0.375) as f64,
                        pos.y as f64,
                        pos.z as f64,
                        (pos.x as f32 + 0.625) as f64,
                        (pos.y as f32 + 1.5) as f64,
                        pos.z.wrapping_add(1) as f64,
                    )
                });
            }
            "BlockCocoa" | "BlockDoor" | "BlockLadder" | "BlockPistonBase" | "BlockSkull" | "BlockTrapDoor" => {
                self.update_bounds(state, world, pos);
            }
            "BlockWall" => {
                let mut v = self.update_bounds(state, world, pos);
                v.0[4] = 1.5;
                self.store(state, v);
            }
            _ if b.is_a("BlockBush")
                || b.is_a("BlockBasePressurePlate")
                || b.is_a("BlockButton")
                || b.is_a("BlockLiquid")
                || b.is_a("BlockRailBase")
                || b.is_a("BlockBanner")
                || b.is_a("BlockSign")
                || b.is_a("BlockTorch")
                || b.class == "BlockPortal" =>
            {
                return None
            }
            _ => {}
        }
        Some(self.bounds(state).world_box(pos))
    }

    fn append_collision(&mut self, state: StateId, world: &impl BlockAccess, pos: BlockPos, mask: &Aabb, list: &mut Vec<Aabb>) {
        if let Some(aabb) = self.collision_box(state, world, pos) {
            if mask.intersects_with(&aabb) {
                list.push(aabb);
            }
        }
    }

    /// `addCollisionBoxesToList`, preserving both list order and post-call bounds.
    pub fn add_collision_boxes(
        &mut self,
        state: StateId,
        world: &impl BlockAccess,
        pos: BlockPos,
        mask: &Aabb,
        list: &mut Vec<Aabb>,
        entity: CollisionEntity,
    ) {
        let b = state.block();
        match b.class {
            "BlockEndPortal" => {}
            "BlockLilyPad" if entity == CollisionEntity::Boat => {}
            "BlockFence" => {
                let [n, s, w, e] = connections(state, world, pos);
                let (mut x, mut xx, mut z, mut zz) = (0.375, 0.625, if n { 0. } else { 0.375 }, if s { 1. } else { 0.625 });
                if n || s {
                    self.store(state, B::new(x, 0., z, xx, 1.5, zz));
                    self.append_collision(state, world, pos, mask, list);
                }
                z = 0.375;
                zz = 0.625;
                if w {
                    x = 0.;
                }
                if e {
                    xx = 1.;
                }
                if w || e || !n && !s {
                    self.store(state, B::new(x, 0., z, xx, 1.5, zz));
                    self.append_collision(state, world, pos, mask, list);
                }
                if n {
                    z = 0.;
                }
                if s {
                    zz = 1.;
                }
                self.store(state, B::new(x, 0., z, xx, 1., zz));
            }
            "BlockPane" | "BlockStainedGlassPane" => {
                let [n, s, w, e] = connections(state, world, pos);
                let any = n || s || w || e;
                let mut parts = [None, None];
                if w && e || !any {
                    parts[0] = Some(B::new(0., 0., 0.4375, 1., 1., 0.5625));
                } else if w {
                    parts[0] = Some(B::new(0., 0., 0.4375, 0.5, 1., 0.5625));
                } else if e {
                    parts[0] = Some(B::new(0.5, 0., 0.4375, 1., 1., 0.5625));
                }
                if n && s || !any {
                    parts[1] = Some(B::new(0.4375, 0., 0., 0.5625, 1., 1.));
                } else if n {
                    parts[1] = Some(B::new(0.4375, 0., 0., 0.5625, 1., 0.5));
                } else if s {
                    parts[1] = Some(B::new(0.4375, 0., 0.5, 0.5625, 1., 1.));
                }
                for part in parts.into_iter().flatten() {
                    self.store(state, part);
                    self.append_collision(state, world, pos, mask, list);
                }
            }
            "BlockBrewingStand" => {
                for part in [B::new(0.4375, 0., 0.4375, 0.5625, 0.875, 0.5625), B::new(0., 0., 0., 1., 0.125, 1.)] {
                    self.store(state, part);
                    self.append_collision(state, world, pos, mask, list);
                }
            }
            "BlockCauldron" | "BlockHopper" => {
                for part in [
                    B::new(0., 0., 0., 1., if b.class == "BlockHopper" { 0.625 } else { 0.3125 }, 1.),
                    B::new(0., 0., 0., 0.125, 1., 1.),
                    B::new(0., 0., 0., 1., 1., 0.125),
                    B::new(0.875, 0., 0., 1., 1., 1.),
                    B::new(0., 0., 0.875, 1., 1., 1.),
                ] {
                    self.store(state, part);
                    self.append_collision(state, world, pos, mask, list);
                }
                self.store(state, B::FULL);
            }
            "BlockEndPortalFrame" => {
                let base = B::new(0., 0., 0., 1., 0.8125, 1.);
                self.store(state, base);
                self.append_collision(state, world, pos, mask, list);
                if world.state(pos).get_bool("eye") {
                    self.store(state, B::new(0.3125, 0.8125, 0.3125, 0.6875, 1., 0.6875));
                    self.append_collision(state, world, pos, mask, list);
                }
                self.store(state, base);
            }
            "BlockPistonExtension" => {
                self.store(state, piston_head(state.get_facing("facing")));
                self.append_collision(state, world, pos, mask, list);
                let core = match state.get_facing("facing") {
                    Down => B::new(0.375, 0.25, 0.375, 0.625, 1., 0.625),
                    Up => B::new(0.375, 0., 0.375, 0.625, 0.75, 0.625),
                    North => B::new(0.25, 0.375, 0.25, 0.75, 0.625, 1.),
                    South => B::new(0.25, 0.375, 0., 0.75, 0.625, 0.75),
                    West => B::new(0.375, 0.25, 0.25, 0.625, 0.75, 1.),
                    East => B::new(0., 0.375, 0.25, 0.75, 0.625, 0.75),
                };
                self.store(state, core);
                self.append_collision(state, world, pos, mask, list);
                self.store(state, B::FULL);
            }
            "BlockPistonBase" => {
                self.store(state, B::FULL);
                self.append_collision(state, world, pos, mask, list);
            }
            "BlockStairs" => self.stair_collisions(state, world, pos, mask, list),
            _ if b.is_a("BlockSlab") => {
                self.update_bounds(state, world, pos);
                self.append_collision(state, world, pos, mask, list);
            }
            _ => self.append_collision(state, world, pos, mask, list),
        }
    }

    fn stair_collisions(&mut self, state: StateId, world: &impl BlockAccess, pos: BlockPos, mask: &Aabb, list: &mut Vec<Aabb>) {
        let s = world.state(pos);
        let facing = s.get_facing("facing");
        let top = s.expect("half") == "top";
        self.store(state, B::new(0., if top { 0.5 } else { 0. }, 0., 1., if top { 1. } else { 0.5 }, 1.));
        self.append_collision(state, world, pos, mask, list);
        let mut step = B::new(0., if top { 0. } else { 0.5 }, 0., 1., if top { 0.5 } else { 1. }, 1.);
        clip_half(&mut step, facing);
        let same = |p| {
            let n = world.state(p);
            n.block().is_a("BlockStairs") && n.value("half") == s.value("half") && n.value("facing") == s.value("facing")
        };
        let front = world.state(pos.offset_facing(facing));
        let outer = front.block().is_a("BlockStairs")
            && front.value("half") == s.value("half")
            && front.get_facing("facing").axis() != facing.axis()
            && !same(pos.offset_facing(front.get_facing("facing").opposite()));
        if outer {
            clip_half(&mut step, front.get_facing("facing"));
        }
        self.store(state, step);
        self.append_collision(state, world, pos, mask, list);
        if !outer {
            let back = world.state(pos.offset_facing(facing.opposite()));
            if back.block().is_a("BlockStairs")
                && back.value("half") == s.value("half")
                && back.get_facing("facing").axis() != facing.axis()
                && !same(pos.offset_facing(back.get_facing("facing")))
            {
                let mut corner = B::new(0., if top { 0. } else { 0.5 }, 0., 1., if top { 0.5 } else { 1. }, 1.);
                clip_half(&mut corner, facing.opposite());
                clip_half(&mut corner, back.get_facing("facing"));
                self.store(state, corner);
                self.append_collision(state, world, pos, mask, list);
            }
        }
        self.store(state, B::FULL);
    }

    pub fn ray_trace(&mut self, state: StateId, world: &impl BlockAccess, pos: BlockPos, start: Vec3, end: Vec3) -> Option<RayHit> {
        let b = state.block();
        if b.class == "BlockPistonMoving" {
            return None;
        }
        if b.class == "BlockStairs" {
            const SKIP: [[u8; 2]; 8] = [[4, 5], [5, 7], [6, 7], [4, 6], [0, 1], [1, 3], [2, 3], [0, 2]];
            let s = world.state(pos);
            let index = s.get_facing("facing").horizontal_index() as usize + if s.expect("half") == "top" { 4 } else { 0 };
            let mut best = None;
            let mut distance = 0.;
            for i in 0..8 {
                self.stair_ray_pass[b.id as usize] = Some(i);
                if SKIP[index].contains(&i) {
                    continue;
                }
                self.update_bounds(state, world, pos);
                if let Some(hit) = trace_bounds(self.bounds(state), pos, start, end) {
                    let d = hit.position.square_distance_to(end);
                    if d > distance {
                        best = Some(hit);
                        distance = d;
                    }
                }
            }
            return best;
        }
        if b.is_a("BlockTorch") {
            let f = 0.15f32;
            let v = match world.state(pos).get_facing("facing") {
                East => B::new(0., 0.2, 0.5 - f, f * 2., 0.8, 0.5 + f),
                West => B::new(1. - f * 2., 0.2, 0.5 - f, 1., 0.8, 0.5 + f),
                South => B::new(0.5 - f, 0.2, 0., 0.5 + f, 0.8, f * 2.),
                North => B::new(0.5 - f, 0.2, 1. - f * 2., 0.5 + f, 0.8, 1.),
                _ => B::new(0.5 - 0.1, 0., 0.5 - 0.1, 0.5 + 0.1, 0.6, 0.5 + 0.1),
            };
            self.store(state, v);
        }
        self.update_bounds(state, world, pos);
        trace_bounds(self.bounds(state), pos, start, end)
    }
}

fn clip_half(bounds: &mut Bounds, facing: Facing) {
    match facing {
        North => bounds.0[5] = 0.5,
        South => bounds.0[2] = 0.5,
        West => bounds.0[3] = 0.5,
        East => bounds.0[0] = 0.5,
        _ => unreachable!(),
    }
}

fn trace_bounds(bounds: Bounds, pos: BlockPos, start: Vec3, end: Vec3) -> Option<RayHit> {
    let a = start.add(pos.x.wrapping_neg() as f64, pos.y.wrapping_neg() as f64, pos.z.wrapping_neg() as f64);
    let z = end.add(pos.x.wrapping_neg() as f64, pos.y.wrapping_neg() as f64, pos.z.wrapping_neg() as f64);
    bounds.raw_box().calculate_intercept(a, z).map(|(v, f)| RayHit {
        position: v.add(pos.x as f64, pos.y as f64, pos.z as f64),
        face: match f {
            HitFace::Down => Down,
            HitFace::Up => Up,
            HitFace::North => North,
            HitFace::South => South,
            HitFace::West => West,
            HitFace::East => East,
        },
    })
}
