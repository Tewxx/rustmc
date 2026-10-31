//! Local player simulation. Positions are feet coordinates; the camera is at eye height.
use mc_core::{math::floor_double, Aabb, BlockPos, Facing, Vec3};
use mc_data::geometry::{BlockGeometry, CollisionEntity};
use mc_data::StateId;
use mc_world::World;

pub use mc_entity::Keys;
use mc_entity::{EntityWorld, PlayerEntity};

/// `World` + `BlockGeometry` as the entity code's world view.
pub struct PhysWorld<'a> {
    pub world: &'a World,
    pub geometry: &'a mut BlockGeometry,
}

impl EntityWorld for PhysWorld<'_> {
    fn state(&self, pos: BlockPos) -> mc_data::StateId {
        self.world.state_at(pos.x, pos.y, pos.z)
    }
    fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.world.chunks.contains_key(&(cx, cz))
    }
    fn add_collision_boxes(&mut self, state: mc_data::StateId, pos: BlockPos, mask: &Aabb, out: &mut Vec<Aabb>) {
        self.geometry.add_collision_boxes(state, self.world, pos, mask, out, CollisionEntity::Other);
    }
}

/// The world as the remote non-living entities' client-side physics sees it.
pub struct ObjectPhys<'a> {
    pub phys: PhysWorld<'a>,
    /// Every player's feet position and eye height.
    pub players: Vec<(Vec3, f64)>,
}

impl crate::entities::ObjectWorld for ObjectPhys<'_> {
    fn ray(&mut self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let mut best: Option<(f64, Vec3)> = None;
        for x in from.x.min(to.x).floor() as i32..=from.x.max(to.x).floor() as i32 {
            for z in from.z.min(to.z).floor() as i32..=from.z.max(to.z).floor() as i32 {
                for y in from.y.min(to.y).floor() as i32..=from.y.max(to.y).floor() as i32 {
                    let state = self.phys.world.state_at(x, y, z);
                    if state.block().id == 0 {
                        continue;
                    }
                    let pos = BlockPos::new(x, y, z);
                    // ignoreBlockWithoutBoundingBox: only blocks with a collision box stop it.
                    let mut boxes = Vec::new();
                    let cell = Aabb::new(x as f64, y as f64, z as f64, x as f64 + 1., y as f64 + 1., z as f64 + 1.);
                    self.phys.geometry.add_collision_boxes(state, self.phys.world, pos, &cell, &mut boxes, CollisionEntity::Other);
                    if boxes.is_empty() {
                        continue;
                    }
                    if let Some(hit) = self.phys.geometry.ray_trace(state, self.phys.world, pos, from, to) {
                        let d = hit.position.square_distance_to(from);
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, hit.position));
                        }
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }

    fn collide(&mut self, bb: &Aabb, motion: &mut Vec3) -> (Aabb, bool) {
        let list = collisions(self.phys.world, self.phys.geometry, &bb.add_coord(motion.x, motion.y, motion.z));
        let dy = motion.y;
        let mut b = *bb;
        for c in &list {
            motion.y = c.calculate_y_offset(&b, motion.y);
        }
        b = b.offset(0.0, motion.y, 0.0);
        for c in &list {
            motion.x = c.calculate_x_offset(&b, motion.x);
        }
        b = b.offset(motion.x, 0.0, 0.0);
        for c in &list {
            motion.z = c.calculate_z_offset(&b, motion.z);
        }
        b = b.offset(0.0, 0.0, motion.z);
        (b, dy != motion.y && dy < 0.0)
    }

    fn in_water(&self, bb: &Aabb) -> bool {
        let bb = bb.expand(0.0, -0.4000000059604645, 0.0).contract(0.001, 0.001, 0.001);
        mc_entity::world::is_material_in_bb(&self.phys, &bb, mc_data::Material::Water)
    }

    fn slipperiness(&self, bb: &Aabb) -> f32 {
        let x = ((bb.min_x + bb.max_x) / 2.0).floor() as i32;
        let z = ((bb.min_z + bb.max_z) / 2.0).floor() as i32;
        self.phys.world.state_at(x, bb.min_y.floor() as i32 - 1, z).block().slipperiness
    }

    fn closest_player(&self, pos: Vec3, range: f64) -> Option<(Vec3, f64)> {
        self.players
            .iter()
            .map(|&(p, eye)| ((p.x - pos.x).powi(2) + (p.y - pos.y).powi(2) + (p.z - pos.z).powi(2), p, eye))
            .filter(|(d, _, _)| *d < range * range)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p, eye)| (p, eye))
    }
}

/// The local player: vanilla entity state plus client-only view state.
pub struct Player {
    pub e: PlayerEntity,
    pub dead: bool,
    pub perspective: u8,
    // ItemRenderer
    pub equipped_progress: f32,
    pub prev_equipped_progress: f32,
    /// `itemToRender` as (catalogue index, count).
    pub item_to_render: Option<(usize, u8)>,
    pub game_type: crate::level::GameType,
    /// `/spawnpoint` (a forced spawn: no bed needed).
    pub spawn_point: Option<BlockPos>,
}

impl std::ops::Deref for Player {
    type Target = PlayerEntity;
    fn deref(&self) -> &PlayerEntity {
        &self.e
    }
}
impl std::ops::DerefMut for Player {
    fn deref_mut(&mut self) -> &mut PlayerEntity {
        &mut self.e
    }
}

impl Player {
    pub fn new(pos: Vec3) -> Self {
        let mut e = PlayerEntity::new(pos);
        e.creative = true;
        e.disable_damage = true;
        e.allow_flying = true;
        Self {
            e,
            dead: false,
            perspective: 0,
            equipped_progress: 0.,
            prev_equipped_progress: 0.,
            item_to_render: None,
            game_type: crate::level::GameType::Creative,
            spawn_point: None,
        }
    }
    /// `EntityPlayerMP.setGameType` / `WorldSettings.GameType.configurePlayerCapabilities`.
    pub fn set_game_type(&mut self, g: crate::level::GameType) {
        use crate::level::GameType;
        self.game_type = g;
        self.e.disable_damage = matches!(g, GameType::Creative | GameType::Spectator);
        match g {
            GameType::Creative => {
                self.e.allow_flying = true;
                self.e.creative = true;
            }
            GameType::Spectator => {
                self.e.allow_flying = true;
                self.e.creative = false;
                self.e.flying = true;
            }
            _ => {
                self.e.allow_flying = false;
                self.e.creative = false;
                self.e.flying = false;
            }
        }
    }
    /// `PlayerCapabilities.allowEdit`
    pub fn allow_edit(&self) -> bool {
        !self.game_type.is_adventure()
    }
    pub fn bounds(&self) -> Aabb {
        self.e.bb
    }
    pub fn teleport(&mut self, pos: Vec3) {
        self.e.set_position(pos);
        self.e.motion = Vec3::new(0., 0., 0.);
        self.e.on_ground = false;
        self.e.fall_distance = 0.;
    }
    pub fn interpolated(&self, alpha: f32) -> glam::Vec3 {
        let (a, b) = (self.e.last_tick_pos, self.e.pos);
        glam::Vec3::new(
            (a.x + (b.x - a.x) * alpha as f64) as f32,
            (a.y + (b.y - a.y) * alpha as f64) as f32,
            (a.z + (b.z - a.z) * alpha as f64) as f32,
        )
    }
    pub fn tick(&mut self, world: &World, geometry: &mut BlockGeometry, keys: Keys) {
        // EntityPlayer.onUpdate: spectators pass through blocks.
        self.e.no_clip = self.game_type == crate::level::GameType::Spectator || self.e.mod_no_clip.unwrap_or(false);
        if self.dead {
            self.e.last_tick_pos = self.e.pos;
            return;
        }
        self.e.tick(&mut PhysWorld { world, geometry }, keys);
    }
    /// `ItemRenderer.updateEquippedItem`
    pub fn update_equipped(&mut self, current: Option<(usize, u8)>) {
        self.prev_equipped_progress = self.equipped_progress;
        let changed = current != self.item_to_render;
        let target = if changed { 0. } else { 1. };
        self.equipped_progress += (target - self.equipped_progress).clamp(-0.4, 0.4);
        if self.equipped_progress < 0.1 {
            self.item_to_render = current;
        }
    }
    pub fn reset_equipped_progress(&mut self) {
        self.equipped_progress = 0.;
    }
}

pub fn collisions(world: &World, geometry: &mut BlockGeometry, mask: &Aabb) -> Vec<Aabb> {
    mc_entity::world::colliding_boxes(&mut PhysWorld { world, geometry }, mask)
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub pos: BlockPos,
    pub face: Facing,
    pub hit: Vec3,
}

/// `World.rayTraceBlocks(start, end, false, false, true)`, the block cursor ray that
/// `Entity.rayTrace` builds for `EntityRenderer.getMouseOver`. Steps one voxel boundary at a
/// time (vanilla's `int k1 = 200` guard) and calls the entered block's `collisionRayTrace`
/// against its selection box. `stopOnLiquid` is false, so liquids are passed through;
/// `ignoreBlockWithoutBoundingBox` is false, so `canCollideCheck` is the only filter.
/// `returnLastUncollidableBlock` is true in vanilla, but the `MovingObjectType.MISS` result it
/// can return has no [`Target`] representation, so only real block hits are reported.
pub fn target(world: &World, geometry: &mut BlockGeometry, start: Vec3, dir: glam::Vec3, reach: f64) -> Option<Target> {
    let end = Vec3::new(
        start.x + dir.x as f64 * reach,
        start.y + dir.y as f64 * reach,
        start.z + dir.z as f64 * reach,
    );
    ray_trace_blocks(world, geometry, start, end)
}

fn ray_trace_blocks(world: &World, geometry: &mut BlockGeometry, start: Vec3, end: Vec3) -> Option<Target> {
    if [start.x, start.y, start.z, end.x, end.y, end.z].iter().any(|v| v.is_nan()) {
        return None;
    }
    // End cell (`i, j, k`) and current cell (`l, i1, j1`), both `MathHelper.floor_double`.
    let (end_x, end_y, end_z) = (floor_double(end.x), floor_double(end.y), floor_double(end.z));
    let (mut x, mut y, mut z) = (floor_double(start.x), floor_double(start.y), floor_double(start.z));
    let mut p = start;
    let mut pos = BlockPos::new(x, y, z);
    let mut state = world.state_at(x, y, z);
    // The start block is tested before the walk, so a ray that starts inside a selection box
    // reports it even when the end position is in the same cell.
    if can_collide_check(state) {
        if let Some(hit) = geometry.ray_trace(state, world, pos, p, end) {
            return Some(Target { pos, face: hit.face, hit: hit.position });
        }
    }
    for _ in 0..200 {
        if p.x.is_nan() || p.y.is_nan() || p.z.is_nan() {
            return None;
        }
        if x == end_x && y == end_y && z == end_z {
            return None;
        }
        // The next boundary of the current cell on each axis; 999 when that axis is already
        // aligned with the end cell.
        let (mut x_plane, mut y_plane, mut z_plane) = (999., 999., 999.);
        let (mut moves_x, mut moves_y, mut moves_z) = (true, true, true);
        if end_x > x {
            x_plane = x as f64 + 1.;
        } else if end_x < x {
            x_plane = x as f64;
        } else {
            moves_x = false;
        }
        if end_y > y {
            y_plane = y as f64 + 1.;
        } else if end_y < y {
            y_plane = y as f64;
        } else {
            moves_y = false;
        }
        if end_z > z {
            z_plane = z as f64 + 1.;
        } else if end_z < z {
            z_plane = z as f64;
        } else {
            moves_z = false;
        }
        // Fraction of the remaining vector at which each boundary is crossed.
        let (dx, dy, dz) = (end.x - p.x, end.y - p.y, end.z - p.z);
        let (mut t_x, mut t_y, mut t_z) = (999., 999., 999.);
        if moves_x {
            t_x = (x_plane - p.x) / dx;
        }
        if moves_y {
            t_y = (y_plane - p.y) / dy;
        }
        if moves_z {
            t_z = (z_plane - p.z) / dz;
        }
        // Java tests for -0.0, which also catches 0.0: a boundary exactly at `p` must step
        // into the next cell instead of dividing by a zero-length remaining vector.
        if t_x == 0.0 {
            t_x = -1.0e-4;
        }
        if t_y == 0.0 {
            t_y = -1.0e-4;
        }
        if t_z == 0.0 {
            t_z = -1.0e-4;
        }
        // Cross the nearest boundary; ties go to x, then y, then z like vanilla.
        let face;
        if t_x < t_y && t_x < t_z {
            face = if end_x > x { Facing::West } else { Facing::East };
            p = Vec3::new(x_plane, p.y + dy * t_x, p.z + dz * t_x);
        } else if t_y < t_z {
            face = if end_y > y { Facing::Down } else { Facing::Up };
            p = Vec3::new(p.x + dx * t_y, y_plane, p.z + dz * t_y);
        } else {
            face = if end_z > z { Facing::North } else { Facing::South };
            p = Vec3::new(p.x + dx * t_z, p.y + dy * t_z, z_plane);
        }
        // The stepped coordinate lies exactly on the boundary, which `floor_double` counts as
        // the cell being entered; stepping towards -x/-y/-z must move one cell further.
        x = floor_double(p.x) - if face == Facing::East { 1 } else { 0 };
        y = floor_double(p.y) - if face == Facing::Up { 1 } else { 0 };
        z = floor_double(p.z) - if face == Facing::South { 1 } else { 0 };
        pos = BlockPos::new(x, y, z);
        state = world.state_at(x, y, z);
        if can_collide_check(state) {
            // `collisionRayTrace` receives the boundary point `p`, not the original start.
            if let Some(hit) = geometry.ray_trace(state, world, pos, p, end) {
                return Some(Target { pos, face: hit.face, hit: hit.position });
            }
        }
    }
    None
}

/// `Block.canCollideCheck(state, hitIfLiquid = false)`. Only `BlockAir`, non-collidable blocks
/// (fire) and liquids answer false; a liquid would need `hitIfLiquid` and level 0, which the
/// cursor ray never passes.
fn can_collide_check(state: StateId) -> bool {
    let b = state.block();
    b.id != 0 && !b.material.props().liquid && b.collidable
}

/// The AABB scan this replaced, kept to cross-check the vanilla walk in tests.
#[cfg(test)]
fn target_aabb_scan(world: &World, geometry: &mut BlockGeometry, start: Vec3, dir: glam::Vec3, reach: f64) -> Option<Target> {
    let end = Vec3::new(
        start.x + dir.x as f64 * reach,
        start.y + dir.y as f64 * reach,
        start.z + dir.z as f64 * reach,
    );
    let mut best = None;
    let mut distance = f64::INFINITY;
    for x in start.x.min(end.x).floor() as i32..=start.x.max(end.x).floor() as i32 {
        for z in start.z.min(end.z).floor() as i32..=start.z.max(end.z).floor() as i32 {
            for y in start.y.min(end.y).floor() as i32..=start.y.max(end.y).floor() as i32 {
                let state = world.state_at(x, y, z);
                if state.block().id == 0 || state.block().material.props().liquid {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                if let Some(hit) = geometry.ray_trace(state, world, pos, start, end) {
                    let d = (hit.position.x - start.x).powi(2) + (hit.position.y - start.y).powi(2) + (hit.position.z - start.z).powi(2);
                    if d < distance {
                        distance = d;
                        best = Some(Target {
                            pos,
                            face: hit.face,
                            hit: hit.position,
                        });
                    }
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::random::JavaRandom;
    use mc_data::Block;
    use mc_world::chunk::Chunk;

    fn block(name: &str) -> &'static Block {
        Block::by_name(name).unwrap_or_else(|| panic!("missing block {name}"))
    }

    fn stone() -> StateId {
        block("stone").default_state
    }

    /// A one-chunk world from `(local x, y, local z, state)` pairs.
    fn world_with(blocks: &[(usize, i32, usize, StateId)]) -> World {
        let mut world = World::default();
        let mut c = Chunk::new(0, 0);
        for &(x, y, z, state) in blocks {
            c.set_state_raw(x, y, z, state);
        }
        world.insert(c);
        world
    }

    /// Full cubes, stairs, slabs, fences, panes, thin boxes, liquids and air. Fire is left out:
    /// `canCollideCheck` rejects it, which the old scan ignored (covered by the liquid test).
    fn random_palette() -> Vec<StateId> {
        let mut out = Vec::new();
        for name in [
            "air", "stone", "glass", "planks", "stone_slab", "wooden_slab", "double_stone_slab", "sandstone_stairs",
            "stone_brick_stairs", "fence", "fence_gate", "cobblestone_wall", "iron_bars", "torch", "redstone_torch",
            "water", "flowing_water", "lava", "chest", "bed", "wooden_door", "ladder", "vine", "cactus", "snow_layer",
            "carpet", "farmland", "soul_sand", "brewing_stand", "cauldron", "hopper", "lever", "stone_pressure_plate",
            "standing_sign", "wall_sign", "anvil", "piston_extension", "skull", "cocoa", "end_portal_frame", "trapdoor",
        ] {
            out.extend(block(name).states());
        }
        out
    }

    /// Four chunks filled with random states above a solid stone floor.
    fn random_grid(seed: i64) -> World {
        let palette = random_palette();
        let mut rand = JavaRandom::new(seed);
        let mut world = World::default();
        for cx in -1..=0 {
            for cz in -1..=0 {
                let mut c = Chunk::new(cx, cz);
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..16 {
                            let state = if y == 0 {
                                stone()
                            } else if rand.next_int_bounded(2) == 0 {
                                StateId::AIR
                            } else {
                                palette[rand.next_int_bounded(palette.len() as i32) as usize]
                            };
                            c.set_state_raw(x, y, z, state);
                        }
                    }
                }
                world.insert(c);
            }
        }
        world
    }

    /// The vanilla walk must select the same block/face as the old AABB scan. Hit positions are
    /// compared with a tolerance because the walk hands `collisionRayTrace` the boundary point
    /// (as vanilla does) while the scan passed the original eye position.
    #[test]
    fn dda_matches_the_old_aabb_scan() {
        let world = random_grid(0x5EED);
        let mut geometry = BlockGeometry::new();
        let mut rand = JavaRandom::new(0xC0FFEE);
        let (mut hits, mut misses, mut ties) = (0, 0, 0);
        for i in 0..4000 {
            let start = Vec3::new(
                -15.5 + rand.next_double() * 31.0,
                0.05 + rand.next_double() * 14.9,
                -15.5 + rand.next_double() * 31.0,
            );
            let mut dir = glam::Vec3::new(
                (rand.next_double() - 0.5) as f32,
                (rand.next_double() - 0.5) as f32,
                (rand.next_double() - 0.5) as f32,
            )
            .normalize();
            if dir == glam::Vec3::ZERO {
                dir = glam::Vec3::X;
            }
            let reach = 1.0 + rand.next_double() * 5.0;
            let old = target_aabb_scan(&world, &mut geometry, start, dir, reach);
            let new = target(&world, &mut geometry, start, dir, reach);
            let d = |p: Vec3, q: Vec3| (p.x - q.x).abs().max((p.y - q.y).abs()).max((p.z - q.z).abs());
            let dist = |h: Vec3| (h.x - start.x).powi(2) + (h.y - start.y).powi(2) + (h.z - start.z).powi(2);
            match (old, new) {
                (Some(a), Some(b)) => {
                    // The scan tested every cell of the bounding box and kept the closest hit, so
                    // it can never find a hit beyond the walk's first one.
                    assert!(
                        dist(a.hit) <= dist(b.hit) + 1e-7,
                        "ray {i}: walk skipped a nearer hit: start {start:?} dir {dir:?} reach {reach} old {a:?} new {b:?}"
                    );
                    if a.pos == b.pos {
                        assert_eq!(a.face, b.face, "ray {i}: start {start:?} dir {dir:?} old {a:?} new {b:?}");
                        assert!(d(a.hit, b.hit) < 1e-9, "ray {i}: {:?} vs {:?}", a.hit, b.hit);
                    } else {
                        // A hit on a cell boundary belongs to the boxes on both sides: the walk
                        // names the cell it just entered, while the scan names whichever of the
                        // two shares the face it iterated first. The hit point itself is equal.
                        assert!(d(a.hit, b.hit) < 1e-9, "ray {i}: start {start:?} dir {dir:?} old {a:?} new {b:?}");
                        assert_eq!(a.pos.offset_facing(a.face), b.pos, "ray {i}: old {a:?} new {b:?}");
                        assert_eq!(b.pos.offset_facing(b.face), a.pos, "ray {i}: old {a:?} new {b:?}");
                        ties += 1;
                    }
                    hits += 1;
                }
                (None, None) => misses += 1,
                (a, b) => panic!("ray {i}: start {start:?} dir {dir:?} reach {reach}: old {a:?}, new {b:?}"),
            }
        }
        assert!(hits > 2000, "only {hits} hit rays");
        assert!(misses > 200, "only {misses} missing rays");
        assert!(ties < hits / 2, "{ties} of {hits} hits changed block across a shared face");
    }

    #[test]
    fn targets_a_full_block_face() {
        let world = world_with(&[(0, 0, 2, stone())]);
        let mut geometry = BlockGeometry::new();
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 2), Facing::North));
        assert_eq!(t.hit, Vec3::new(0.5, 0.5, 2.0));
    }

    /// `rayTraceBlocks` tests the start block before stepping, so a ray whose eye is inside a
    /// block reports that block's exit face even though another block is straight ahead.
    #[test]
    fn starts_inside_a_block_and_reports_its_exit_face() {
        let world = world_with(&[(0, 0, 0, stone()), (3, 0, 0, stone())]);
        let mut geometry = BlockGeometry::new();
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::X, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 0), Facing::East));
        assert_eq!(t.hit, Vec3::new(1.0, 0.5, 0.5));
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), -glam::Vec3::X, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 0), Facing::West));
        assert_eq!(t.hit, Vec3::new(0.0, 0.5, 0.5));
    }

    #[test]
    fn the_walk_stops_at_the_reach_limit() {
        let world = world_with(&[(0, 0, 4, stone())]);
        let mut geometry = BlockGeometry::new();
        assert!(target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 3.0).is_none());
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 3.6).unwrap();
        assert_eq!((t.pos, t.hit), (BlockPos::new(0, 0, 4), Vec3::new(0.5, 0.5, 4.0)));
    }

    #[test]
    fn same_start_and_end_cell_only_tests_the_start_block() {
        let world = world_with(&[(0, 0, 0, stone())]);
        let mut geometry = BlockGeometry::new();
        assert!(target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::X, 0.25).is_none());
        assert!(target(&world, &mut geometry, Vec3::new(0.1, 0.1, 0.1), glam::Vec3::ZERO, 5.0).is_none());
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::X, 2.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 0), Facing::East));
    }

    /// `stopOnLiquid=false` passes through water/lava and `canCollideCheck` rejects fire.
    #[test]
    fn liquids_are_passed_through_and_fire_is_not_targetable() {
        let world = world_with(&[
            (0, 0, 1, block("fire").default_state),
            (0, 0, 2, block("water").default_state),
            (0, 0, 3, block("lava").default_state),
            (0, 0, 4, stone()),
        ]);
        let mut geometry = BlockGeometry::new();
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 4), Facing::North));
        assert_eq!(t.hit, Vec3::new(0.5, 0.5, 4.0));
        let water = world_with(&[(0, 0, 2, block("water").default_state)]);
        assert!(target(&water, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 5.0).is_none());
        let fire = world_with(&[(0, 0, 1, block("fire").default_state)]);
        assert!(target(&fire, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 3.0).is_none());
    }

    #[test]
    fn slabs_use_the_half_of_the_selection_box() {
        let mut geometry = BlockGeometry::new();
        let bottom = world_with(&[(0, 0, 2, block("stone_slab").default_state.set("half", "bottom"))]);
        assert!(target(&bottom, &mut geometry, Vec3::new(0.5, 0.75, 0.5), glam::Vec3::Z, 5.0).is_none());
        let t = target(&bottom, &mut geometry, Vec3::new(0.5, 0.25, 0.5), glam::Vec3::Z, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 2), Facing::North));
        assert_eq!(t.hit, Vec3::new(0.5, 0.25, 2.0));

        let top = world_with(&[(0, 0, 2, block("stone_slab").default_state.set("half", "top"))]);
        assert!(target(&top, &mut geometry, Vec3::new(0.5, 0.25, 0.5), glam::Vec3::Z, 5.0).is_none());
        let t = target(&top, &mut geometry, Vec3::new(0.5, 0.75, 0.5), glam::Vec3::Z, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 2), Facing::North));
        assert_eq!(t.hit, Vec3::new(0.5, 0.75, 2.0));
    }

    /// `BlockStairs.collisionRayTrace` ray-tests the eight half-blocks, skipping the two that
    /// the facing leaves empty above the bottom slab.
    #[test]
    fn stairs_use_the_ray_trace_passes() {
        let mut geometry = BlockGeometry::new();
        let state = block("sandstone_stairs").default_state.set("facing", "east").set("half", "bottom");
        let world = world_with(&[(0, 0, 2, state)]);
        let t = target(&world, &mut geometry, Vec3::new(0.75, 0.75, 0.5), glam::Vec3::Z, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 2), Facing::North));
        assert_eq!(t.hit, Vec3::new(0.75, 0.75, 2.0));
        assert!(target(&world, &mut geometry, Vec3::new(0.25, 0.75, 0.5), glam::Vec3::Z, 5.0).is_none());
    }

    /// Fences ray-trace their 1.0-high `setBlockBoundsBasedOnState` post, not the 1.5-high
    /// collision shape.
    #[test]
    fn fences_use_the_post_selection_box() {
        let world = world_with(&[(0, 0, 2, block("fence").default_state)]);
        let mut geometry = BlockGeometry::new();
        let t = target(&world, &mut geometry, Vec3::new(0.5, 0.5, 0.5), glam::Vec3::Z, 5.0).unwrap();
        assert_eq!((t.pos, t.face), (BlockPos::new(0, 0, 2), Facing::North));
        assert_eq!(t.hit, Vec3::new(0.5, 0.5, 2.375));
        assert!(target(&world, &mut geometry, Vec3::new(0.25, 0.5, 0.5), glam::Vec3::Z, 5.0).is_none());
    }
}

