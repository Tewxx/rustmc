//! Local player simulation. Positions are feet coordinates; the camera is at eye height.
use mc_core::{Aabb, BlockPos, Facing, Vec3};
use mc_data::geometry::{BlockGeometry, CollisionEntity};
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
        self.e.no_clip = self.game_type == crate::level::GameType::Spectator;
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

pub fn target(world: &World, geometry: &mut BlockGeometry, start: Vec3, dir: glam::Vec3, reach: f64) -> Option<Target> {
    let end = Vec3::new(
        start.x + dir.x as f64 * reach,
        start.y + dir.y as f64 * reach,
        start.z + dir.z as f64 * reach,
    );
    let mut best = None;
    let mut distance = f64::INFINITY;
    // The short selection ray's bounding grid also covers boundary-aligned rays without DDA epsilon drift.
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

