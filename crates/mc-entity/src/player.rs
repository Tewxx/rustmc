//! `EntityPlayerSP` movement and animation state: a port of the `Entity`, `EntityLivingBase`,
//! `EntityPlayer` and `EntityPlayerSP` tick paths that run for the local player.
use crate::world::*;
use mc_core::math::{self, floor_double, wrap_angle_to_180_float};
use mc_core::{Aabb, BlockPos, Vec3};
use mc_data::Material;

pub const WIDTH: f32 = 0.6;
pub const HEIGHT: f32 = 1.8;
const SWING_END: i32 = 6;

/// `MovementInputFromOptions` key state for one tick.
#[derive(Clone, Copy, Default, Debug)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    /// `keyBindSprint.isKeyDown()`
    pub sprint: bool,
    pub using_item: bool,
}

#[derive(Clone, Debug)]
pub struct PlayerEntity {
    pub pos: Vec3,
    pub prev_pos: Vec3,
    /// `lastTickPos`, used for render interpolation.
    pub last_tick_pos: Vec3,
    pub motion: Vec3,
    pub bb: Aabb,
    pub yaw: f32,
    pub pitch: f32,
    pub prev_yaw: f32,
    pub prev_pitch: f32,
    pub on_ground: bool,
    pub collided_horizontally: bool,
    pub collided_vertically: bool,
    pub fall_distance: f32,
    pub in_water: bool,
    in_web: bool,
    pub no_clip: bool,
    pub ticks_existed: i32,
    pub step_height: f32,
    pub distance_walked: f32,
    pub prev_distance_walked: f32,
    pub distance_walked_on_step: f32,

    // MovementInput
    pub move_forward: f32,
    pub move_strafe: f32,
    pub jump_key: bool,
    pub sneak_key: bool,
    // EntityLivingBase
    pub move_forward_e: f32,
    pub move_strafe_e: f32,
    pub is_jumping: bool,
    jump_ticks: i32,
    ai_move_speed: f32,
    jump_movement_factor: f32,
    pub sprinting: bool,
    sprint_toggle_timer: i32,
    fly_toggle_timer: i32,
    pub food_level: i32,
    // PlayerCapabilities
    pub flying: bool,
    pub allow_flying: bool,
    pub creative: bool,
    pub fly_speed: f32,
    pub walk_speed: f32,

    // Animation
    pub swing_progress_int: i32,
    pub is_swing_in_progress: bool,
    pub swing_progress: f32,
    pub prev_swing_progress: f32,
    pub render_yaw_offset: f32,
    pub prev_render_yaw_offset: f32,
    pub rotation_yaw_head: f32,
    pub prev_rotation_yaw_head: f32,
    pub limb_swing: f32,
    pub limb_swing_amount: f32,
    pub prev_limb_swing_amount: f32,
    pub camera_yaw: f32,
    pub prev_camera_yaw: f32,
    pub camera_pitch: f32,
    pub prev_camera_pitch: f32,
    pub render_arm_yaw: f32,
    pub render_arm_pitch: f32,
    pub prev_render_arm_yaw: f32,
    pub prev_render_arm_pitch: f32,
    pub fov_modifier_hand: f32,
    pub prev_fov_modifier_hand: f32,
    /// Abilities changed by the player (double-tap jump): send `C13PacketPlayerAbilities`.
    pub abilities_dirty: bool,
}

fn player_bb(p: Vec3) -> Aabb {
    let w = (WIDTH / 2.0) as f64;
    Aabb::new(p.x - w, p.y, p.z - w, p.x + w, p.y + HEIGHT as f64, p.z + w)
}

impl PlayerEntity {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            prev_pos: pos,
            last_tick_pos: pos,
            motion: Vec3::new(0., 0., 0.),
            bb: player_bb(pos),
            yaw: 0.,
            pitch: 0.,
            prev_yaw: 0.,
            prev_pitch: 0.,
            on_ground: false,
            collided_horizontally: false,
            collided_vertically: false,
            fall_distance: 0.,
            in_water: false,
            in_web: false,
            no_clip: false,
            ticks_existed: 0,
            step_height: 0.6,
            distance_walked: 0.,
            prev_distance_walked: 0.,
            distance_walked_on_step: 0.,
            move_forward: 0.,
            move_strafe: 0.,
            jump_key: false,
            sneak_key: false,
            move_forward_e: 0.,
            move_strafe_e: 0.,
            is_jumping: false,
            jump_ticks: 0,
            ai_move_speed: 0.1,
            jump_movement_factor: 0.02,
            sprinting: false,
            sprint_toggle_timer: 0,
            fly_toggle_timer: 0,
            food_level: 20,
            flying: false,
            allow_flying: false,
            creative: false,
            fly_speed: 0.05,
            walk_speed: 0.1,
            swing_progress_int: 0,
            is_swing_in_progress: false,
            swing_progress: 0.,
            prev_swing_progress: 0.,
            render_yaw_offset: 0.,
            prev_render_yaw_offset: 0.,
            rotation_yaw_head: 0.,
            prev_rotation_yaw_head: 0.,
            limb_swing: 0.,
            limb_swing_amount: 0.,
            prev_limb_swing_amount: 0.,
            camera_yaw: 0.,
            prev_camera_yaw: 0.,
            camera_pitch: 0.,
            prev_camera_pitch: 0.,
            render_arm_yaw: 0.,
            render_arm_pitch: 0.,
            prev_render_arm_yaw: 0.,
            prev_render_arm_pitch: 0.,
            fov_modifier_hand: 1.,
            prev_fov_modifier_hand: 1.,
            abilities_dirty: false,
        }
    }

    /// `setPositionAndUpdate` / `S08PacketPlayerPosLook` handling.
    pub fn set_position(&mut self, p: Vec3) {
        self.pos = p;
        self.prev_pos = p;
        self.last_tick_pos = p;
        self.bb = player_bb(p);
    }

    pub fn is_sneaking(&self) -> bool {
        self.sneak_key
    }

    /// `EntityPlayer.getEyeHeight`
    pub fn eye_height(&self) -> f32 {
        if self.is_sneaking() {
            1.62 - 0.08
        } else {
            1.62
        }
    }

    /// Value of the movement speed attribute (walk speed, ×1.3 while sprinting).
    fn movement_speed_attr(&self) -> f64 {
        let base = self.walk_speed as f64;
        if self.sprinting {
            base * (1.0 + 0.30000001192092896)
        } else {
            base
        }
    }

    /// `EntityLivingBase.swingItem` (client half; the caller sends `C0APacketAnimation`).
    pub fn swing_item(&mut self) {
        if !self.is_swing_in_progress || self.swing_progress_int >= SWING_END / 2 || self.swing_progress_int < 0 {
            self.swing_progress_int = -1;
            self.is_swing_in_progress = true;
        }
    }

    /// `getSwingProgress(partialTicks)`
    pub fn swing_progress(&self, pt: f32) -> f32 {
        let mut f = self.swing_progress - self.prev_swing_progress;
        if f < 0.0 {
            f += 1.0;
        }
        self.prev_swing_progress + f * pt
    }

    fn update_arm_swing_progress(&mut self) {
        if self.is_swing_in_progress {
            self.swing_progress_int += 1;
            if self.swing_progress_int >= SWING_END {
                self.swing_progress_int = 0;
                self.is_swing_in_progress = false;
            }
        } else {
            self.swing_progress_int = 0;
        }
        self.swing_progress = self.swing_progress_int as f32 / SWING_END as f32;
    }

    /// `AbstractClientPlayer.getFovModifier` (no bow).
    fn fov_modifier(&self) -> f32 {
        let mut f = 1.0f32;
        if self.flying {
            f *= 1.1;
        }
        f = (f as f64 * ((self.movement_speed_attr() / self.walk_speed as f64 + 1.0) / 2.0)) as f32;
        if self.walk_speed == 0.0 || f.is_nan() || f.is_infinite() {
            f = 1.0;
        }
        f
    }

    /// One client tick: `World.updateEntity` → `EntityPlayerSP.onUpdate` up to (not including)
    /// `onUpdateWalkingPlayer`, plus `EntityRenderer.updateFovModifierHand`.
    pub fn tick(&mut self, w: &mut impl EntityWorld, keys: Keys) {
        self.last_tick_pos = self.pos;
        self.ticks_existed += 1;
        // Entity.onEntityUpdate
        self.prev_distance_walked = self.distance_walked;
        self.prev_pos = self.pos;
        self.prev_pitch = self.pitch;
        self.prev_yaw = self.yaw;
        self.handle_water_movement(w);
        // EntityLivingBase.onEntityUpdate
        self.prev_swing_progress = self.swing_progress;
        self.prev_render_yaw_offset = self.render_yaw_offset;
        self.prev_rotation_yaw_head = self.rotation_yaw_head;

        self.on_living_update_sp(w, keys);
        self.update_body_yaw();

        // EntityRenderer.updateFovModifierHand
        self.prev_fov_modifier_hand = self.fov_modifier_hand;
        self.fov_modifier_hand += (self.fov_modifier() - self.fov_modifier_hand) * 0.5;
        self.fov_modifier_hand = self.fov_modifier_hand.clamp(0.1, 1.5);
    }

    fn handle_water_movement(&mut self, w: &impl EntityWorld) {
        let bb = self.bb.expand(0.0, -0.4000000059604645, 0.0).contract(0.001, 0.001, 0.001);
        if handle_material_acceleration(w, &bb, Material::Water, &mut self.motion, !self.flying) {
            self.fall_distance = 0.0;
            self.in_water = true;
        } else {
            self.in_water = false;
        }
    }

    fn is_in_lava(&self, w: &impl EntityWorld) -> bool {
        is_material_in_bb(w, &self.bb.expand(-0.10000000149011612, -0.4000000059604645, -0.10000000149011612), Material::Lava)
    }

    pub fn set_sprinting(&mut self, v: bool) {
        self.sprinting = v;
    }

    /// `EntityPlayerSP.onLivingUpdate`
    fn on_living_update_sp(&mut self, w: &mut impl EntityWorld, keys: Keys) {
        if self.sprint_toggle_timer > 0 {
            self.sprint_toggle_timer -= 1;
        }
        let flag = self.jump_key;
        let flag1 = self.sneak_key;
        let f = 0.8f32;
        let flag2 = self.move_forward >= f;
        // MovementInputFromOptions.updatePlayerMoveState
        self.move_strafe = 0.0;
        self.move_forward = 0.0;
        if keys.forward {
            self.move_forward += 1.0;
        }
        if keys.back {
            self.move_forward -= 1.0;
        }
        if keys.left {
            self.move_strafe += 1.0;
        }
        if keys.right {
            self.move_strafe -= 1.0;
        }
        self.jump_key = keys.jump;
        self.sneak_key = keys.sneak;
        if self.sneak_key {
            self.move_strafe = (self.move_strafe as f64 * 0.3) as f32;
            self.move_forward = (self.move_forward as f64 * 0.3) as f32;
        }
        if keys.using_item {
            self.move_strafe *= 0.2;
            self.move_forward *= 0.2;
            self.sprint_toggle_timer = 0;
        }
        let half = WIDTH as f64 * 0.35;
        let y = self.bb.min_y + 0.5;
        self.push_out_of_blocks(w, self.pos.x - half, y, self.pos.z + half);
        self.push_out_of_blocks(w, self.pos.x - half, y, self.pos.z - half);
        self.push_out_of_blocks(w, self.pos.x + half, y, self.pos.z - half);
        self.push_out_of_blocks(w, self.pos.x + half, y, self.pos.z + half);
        let flag3 = self.food_level > 6 || self.allow_flying;
        if self.on_ground && !flag1 && !flag2 && self.move_forward >= f && !self.sprinting && flag3 && !keys.using_item {
            if self.sprint_toggle_timer <= 0 && !keys.sprint {
                self.sprint_toggle_timer = 7;
            } else {
                self.set_sprinting(true);
            }
        }
        if !self.sprinting && self.move_forward >= f && flag3 && !keys.using_item && keys.sprint {
            self.set_sprinting(true);
        }
        if self.sprinting && (self.move_forward < f || self.collided_horizontally || !flag3) {
            self.set_sprinting(false);
        }
        if self.allow_flying {
            if self.no_clip {
                if !self.flying {
                    self.flying = true;
                    self.abilities_dirty = true;
                }
            } else if !flag && self.jump_key {
                if self.fly_toggle_timer == 0 {
                    self.fly_toggle_timer = 7;
                } else {
                    self.flying = !self.flying;
                    self.abilities_dirty = true;
                    self.fly_toggle_timer = 0;
                }
            }
        }
        if self.flying {
            if self.sneak_key {
                self.motion.y -= (self.fly_speed * 3.0) as f64;
            }
            if self.jump_key {
                self.motion.y += (self.fly_speed * 3.0) as f64;
            }
        }
        self.on_living_update_player(w);
        if self.on_ground && self.flying && !self.no_clip {
            self.flying = false;
            self.abilities_dirty = true;
        }
    }

    /// `EntityPlayer.onLivingUpdate`
    fn on_living_update_player(&mut self, w: &mut impl EntityWorld) {
        if self.fly_toggle_timer > 0 {
            self.fly_toggle_timer -= 1;
        }
        self.prev_camera_yaw = self.camera_yaw;
        self.prev_camera_pitch = self.camera_pitch;
        self.on_living_update_base(w);
        self.jump_movement_factor = 0.02;
        if self.sprinting {
            self.jump_movement_factor = (self.jump_movement_factor as f64 + 0.02 * 0.3) as f32;
        }
        self.ai_move_speed = self.movement_speed_attr() as f32;
        let mut f = math::sqrt_double(self.motion.x * self.motion.x + self.motion.z * self.motion.z);
        let mut f1 = ((-self.motion.y * 0.20000000298023224).atan() * 15.0) as f32;
        if f > 0.1 {
            f = 0.1;
        }
        if !self.on_ground {
            f = 0.0;
        }
        if self.on_ground {
            f1 = 0.0;
        }
        self.camera_yaw += (f - self.camera_yaw) * 0.4;
        self.camera_pitch += (f1 - self.camera_pitch) * 0.8;
    }

    /// `EntityLivingBase.onLivingUpdate`
    fn on_living_update_base(&mut self, w: &mut impl EntityWorld) {
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        if self.motion.x.abs() < 0.005 {
            self.motion.x = 0.0;
        }
        if self.motion.y.abs() < 0.005 {
            self.motion.y = 0.0;
        }
        if self.motion.z.abs() < 0.005 {
            self.motion.z = 0.0;
        }
        // EntityPlayerSP.updateEntityActionState → EntityPlayer → EntityLivingBase
        self.update_arm_swing_progress();
        self.rotation_yaw_head = self.yaw;
        self.move_strafe_e = self.move_strafe;
        self.move_forward_e = self.move_forward;
        self.is_jumping = self.jump_key;
        self.prev_render_arm_yaw = self.render_arm_yaw;
        self.prev_render_arm_pitch = self.render_arm_pitch;
        self.render_arm_pitch = (self.render_arm_pitch as f64 + (self.pitch - self.render_arm_pitch) as f64 * 0.5) as f32;
        self.render_arm_yaw = (self.render_arm_yaw as f64 + (self.yaw - self.render_arm_yaw) as f64 * 0.5) as f32;

        if self.is_jumping {
            if self.in_water || self.is_in_lava(w) {
                self.motion.y += 0.03999999910593033;
            } else if self.on_ground && self.jump_ticks == 0 {
                self.jump();
                self.jump_ticks = 10;
            }
        } else {
            self.jump_ticks = 0;
        }
        self.move_strafe_e *= 0.98;
        self.move_forward_e *= 0.98;
        self.move_entity_with_heading_player(w, self.move_strafe_e, self.move_forward_e);
    }

    fn jump(&mut self) {
        self.motion.y = 0.41999998688697815;
        if self.sprinting {
            let f = self.yaw * 0.017453292;
            self.motion.x -= (math::sin(f) * 0.2) as f64;
            self.motion.z += (math::cos(f) * 0.2) as f64;
        }
    }

    /// `EntityPlayer.moveEntityWithHeading`
    fn move_entity_with_heading_player(&mut self, w: &mut impl EntityWorld, strafe: f32, forward: f32) {
        if self.flying {
            let d3 = self.motion.y;
            let f = self.jump_movement_factor;
            self.jump_movement_factor = self.fly_speed * if self.sprinting { 2.0 } else { 1.0 };
            self.move_entity_with_heading(w, strafe, forward);
            self.motion.y = d3 * 0.6;
            self.jump_movement_factor = f;
        } else {
            self.move_entity_with_heading(w, strafe, forward);
        }
    }

    fn move_flying(&mut self, mut strafe: f32, mut forward: f32, friction: f32) {
        let mut f = strafe * strafe + forward * forward;
        if f >= 1.0e-4 {
            f = math::sqrt_float(f);
            if f < 1.0 {
                f = 1.0;
            }
            f = friction / f;
            strafe *= f;
            forward *= f;
            let f1 = math::sin(self.yaw * std::f32::consts::PI / 180.0);
            let f2 = math::cos(self.yaw * std::f32::consts::PI / 180.0);
            self.motion.x += (strafe * f2 - forward * f1) as f64;
            self.motion.z += (forward * f2 + strafe * f1) as f64;
        }
    }

    fn ground_slipperiness(&self, w: &impl EntityWorld) -> f32 {
        if self.on_ground {
            let pos = BlockPos::new(floor_double(self.pos.x), floor_double(self.bb.min_y) - 1, floor_double(self.pos.z));
            w.state(pos).block().slipperiness * 0.91
        } else {
            0.91
        }
    }

    fn is_on_ladder(&self, w: &impl EntityWorld) -> bool {
        if self.no_clip {
            return false;
        }
        let pos = BlockPos::new(floor_double(self.pos.x), floor_double(self.bb.min_y), floor_double(self.pos.z));
        matches!(w.state(pos).block().name, "minecraft:ladder" | "minecraft:vine")
    }

    /// `isOffsetPositionInLiquid`: true when the offset box is free of blocks and liquid.
    fn is_offset_position_in_liquid(&self, w: &mut impl EntityWorld, x: f64, y: f64, z: f64) -> bool {
        let bb = self.bb.offset(x, y, z);
        colliding_boxes(w, &bb).is_empty() && !is_any_liquid(w, &bb)
    }

    /// `EntityLivingBase.moveEntityWithHeading`
    fn move_entity_with_heading(&mut self, w: &mut impl EntityWorld, strafe: f32, forward: f32) {
        if self.in_water && !self.flying {
            let d0 = self.pos.y;
            self.move_flying(strafe, forward, 0.02);
            self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
            self.motion.x *= 0.800000011920929;
            self.motion.y *= 0.800000011920929;
            self.motion.z *= 0.800000011920929;
            self.motion.y -= 0.02;
            if self.collided_horizontally
                && self.is_offset_position_in_liquid(w, self.motion.x, self.motion.y + 0.6000000238418579 - self.pos.y + d0, self.motion.z)
            {
                self.motion.y = 0.30000001192092896;
            }
        } else if self.is_in_lava(w) && !self.flying {
            let d1 = self.pos.y;
            self.move_flying(strafe, forward, 0.02);
            self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
            self.motion.x *= 0.5;
            self.motion.y *= 0.5;
            self.motion.z *= 0.5;
            self.motion.y -= 0.02;
            if self.collided_horizontally
                && self.is_offset_position_in_liquid(w, self.motion.x, self.motion.y + 0.6000000238418579 - self.pos.y + d1, self.motion.z)
            {
                self.motion.y = 0.30000001192092896;
            }
        } else {
            let f4 = self.ground_slipperiness(w);
            let f = 0.16277136 / (f4 * f4 * f4);
            let f5 = if self.on_ground { self.ai_move_speed * f } else { self.jump_movement_factor };
            self.move_flying(strafe, forward, f5);
            let f4 = self.ground_slipperiness(w);
            if self.is_on_ladder(w) {
                let f6 = 0.15f32 as f64;
                self.motion.x = self.motion.x.clamp(-f6, f6);
                self.motion.z = self.motion.z.clamp(-f6, f6);
                self.fall_distance = 0.0;
                if self.motion.y < -0.15 {
                    self.motion.y = -0.15;
                }
                if self.is_sneaking() && self.motion.y < 0.0 {
                    self.motion.y = 0.0;
                }
            }
            self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
            if self.collided_horizontally && self.is_on_ladder(w) {
                self.motion.y = 0.2;
            }
            // worldObj.isRemote: the client player hovers in unloaded chunks.
            let probe = BlockPos::new(self.pos.x as i32, 0, self.pos.z as i32);
            if !is_block_loaded(w, probe) {
                self.motion.y = if self.pos.y > 0.0 { -0.1 } else { 0.0 };
            } else {
                self.motion.y -= 0.08;
            }
            self.motion.y *= 0.9800000190734863;
            self.motion.x *= f4 as f64;
            self.motion.z *= f4 as f64;
        }
        self.prev_limb_swing_amount = self.limb_swing_amount;
        let d2 = self.pos.x - self.prev_pos.x;
        let d3 = self.pos.z - self.prev_pos.z;
        let mut f7 = math::sqrt_double(d2 * d2 + d3 * d3) * 4.0;
        if f7 > 1.0 {
            f7 = 1.0;
        }
        self.limb_swing_amount += (f7 - self.limb_swing_amount) * 0.4;
        self.limb_swing += self.limb_swing_amount;
    }

    fn reset_position_to_bb(&mut self) {
        self.pos = Vec3::new((self.bb.min_x + self.bb.max_x) / 2.0, self.bb.min_y, (self.bb.min_z + self.bb.max_z) / 2.0);
    }

    /// `Entity.moveEntity`
    pub fn move_entity(&mut self, w: &mut impl EntityWorld, mut x: f64, mut y: f64, mut z: f64) {
        if self.no_clip {
            self.bb = self.bb.offset(x, y, z);
            self.reset_position_to_bb();
            return;
        }
        let (d0, d1, d2) = (self.pos.x, self.pos.y, self.pos.z);
        if self.in_web {
            self.in_web = false;
            x *= 0.25;
            y *= 0.05000000074505806;
            z *= 0.25;
            self.motion = Vec3::new(0., 0., 0.);
        }
        let mut d3 = x;
        let d4 = y;
        let mut d5 = z;
        let flag = self.on_ground && self.is_sneaking();
        if flag {
            let d6 = 0.05;
            while x != 0.0 && colliding_boxes(w, &self.bb.offset(x, -1.0, 0.0)).is_empty() {
                x = step_toward_zero(x, d6);
                d3 = x;
            }
            while z != 0.0 && colliding_boxes(w, &self.bb.offset(0.0, -1.0, z)).is_empty() {
                z = step_toward_zero(z, d6);
                d5 = z;
            }
            while x != 0.0 && z != 0.0 && colliding_boxes(w, &self.bb.offset(x, -1.0, z)).is_empty() {
                x = step_toward_zero(x, d6);
                d3 = x;
                z = step_toward_zero(z, d6);
                d5 = z;
            }
        }
        let list1 = colliding_boxes(w, &self.bb.add_coord(x, y, z));
        let bb1 = self.bb;
        for b in &list1 {
            y = b.calculate_y_offset(&self.bb, y);
        }
        self.bb = self.bb.offset(0.0, y, 0.0);
        let flag1 = self.on_ground || d4 != y && d4 < 0.0;
        for b in &list1 {
            x = b.calculate_x_offset(&self.bb, x);
        }
        self.bb = self.bb.offset(x, 0.0, 0.0);
        for b in &list1 {
            z = b.calculate_z_offset(&self.bb, z);
        }
        self.bb = self.bb.offset(0.0, 0.0, z);

        if self.step_height > 0.0 && flag1 && (d3 != x || d5 != z) {
            let (d11, d7, d8) = (x, y, z);
            let bb3 = self.bb;
            self.bb = bb1;
            y = self.step_height as f64;
            let list = colliding_boxes(w, &self.bb.add_coord(d3, y, d5));
            let mut bb4 = self.bb;
            let bb5 = bb4.add_coord(d3, 0.0, d5);
            let mut d9 = y;
            for b in &list {
                d9 = b.calculate_y_offset(&bb5, d9);
            }
            bb4 = bb4.offset(0.0, d9, 0.0);
            let mut d15 = d3;
            for b in &list {
                d15 = b.calculate_x_offset(&bb4, d15);
            }
            bb4 = bb4.offset(d15, 0.0, 0.0);
            let mut d16 = d5;
            for b in &list {
                d16 = b.calculate_z_offset(&bb4, d16);
            }
            bb4 = bb4.offset(0.0, 0.0, d16);
            let mut bb14 = self.bb;
            let mut d17 = y;
            for b in &list {
                d17 = b.calculate_y_offset(&bb14, d17);
            }
            bb14 = bb14.offset(0.0, d17, 0.0);
            let mut d18 = d3;
            for b in &list {
                d18 = b.calculate_x_offset(&bb14, d18);
            }
            bb14 = bb14.offset(d18, 0.0, 0.0);
            let mut d19 = d5;
            for b in &list {
                d19 = b.calculate_z_offset(&bb14, d19);
            }
            bb14 = bb14.offset(0.0, 0.0, d19);
            let d20 = d15 * d15 + d16 * d16;
            let d10 = d18 * d18 + d19 * d19;
            if d20 > d10 {
                x = d15;
                z = d16;
                y = -d9;
                self.bb = bb4;
            } else {
                x = d18;
                z = d19;
                y = -d17;
                self.bb = bb14;
            }
            for b in &list {
                y = b.calculate_y_offset(&self.bb, y);
            }
            self.bb = self.bb.offset(0.0, y, 0.0);
            if d11 * d11 + d8 * d8 >= x * x + z * z {
                x = d11;
                y = d7;
                z = d8;
                self.bb = bb3;
            }
        }
        self.reset_position_to_bb();
        self.collided_horizontally = d3 != x || d5 != z;
        self.collided_vertically = d4 != y;
        self.on_ground = self.collided_vertically && d4 < 0.0;

        let mut pos = BlockPos::new(floor_double(self.pos.x), floor_double(self.pos.y - 0.20000000298023224), floor_double(self.pos.z));
        let mut block = w.state(pos).block();
        if block.material == Material::Air {
            let below = w.state(pos.down()).block();
            if matches!(below.class, "BlockFence" | "BlockWall" | "BlockFenceGate") {
                block = below;
                pos = pos.down();
            }
        }
        let _ = pos;
        // updateFallState (damage is the server's)
        if self.on_ground {
            self.fall_distance = 0.0;
        } else if y < 0.0 {
            self.fall_distance = (self.fall_distance as f64 - y) as f32;
        }
        if d3 != x {
            self.motion.x = 0.0;
        }
        if d5 != z {
            self.motion.z = 0.0;
        }
        if d4 != y {
            // Block.onLanded
            match block.name {
                "minecraft:slime" if !self.is_sneaking() => {
                    if self.motion.y < 0.0 {
                        self.motion.y = -self.motion.y;
                    }
                }
                "minecraft:bed" if !self.is_sneaking() => {
                    if self.motion.y < 0.0 {
                        self.motion.y = -self.motion.y * 0.6600000262260437;
                    }
                }
                _ => self.motion.y = 0.0,
            }
        }
        if !flag {
            let d12 = self.pos.x - d0;
            let d14 = self.pos.z - d2;
            let d13 = if block.name == "minecraft:ladder" { self.pos.y - d1 } else { 0.0 };
            if self.on_ground && block.name == "minecraft:slime" && self.motion.y.abs() < 0.1 && !self.is_sneaking() {
                let d = 0.4 + self.motion.y.abs() * 0.2;
                self.motion.x *= d;
                self.motion.z *= d;
            }
            self.distance_walked = (self.distance_walked as f64 + math::sqrt_double(d12 * d12 + d14 * d14) as f64 * 0.6) as f32;
            self.distance_walked_on_step =
                (self.distance_walked_on_step as f64 + math::sqrt_double(d12 * d12 + d13 * d13 + d14 * d14) as f64 * 0.6) as f32;
        }
        self.do_block_collisions(w);
    }

    /// `Entity.doBlockCollisions`: webs and soul sand.
    fn do_block_collisions(&mut self, w: &impl EntityWorld) {
        let a = BlockPos::new(floor_double(self.bb.min_x + 0.001), floor_double(self.bb.min_y + 0.001), floor_double(self.bb.min_z + 0.001));
        let b = BlockPos::new(floor_double(self.bb.max_x - 0.001), floor_double(self.bb.max_y - 0.001), floor_double(self.bb.max_z - 0.001));
        if !is_area_loaded(w, a.x, a.y, a.z, b.x, b.y, b.z) {
            return;
        }
        for x in a.x..=b.x {
            for y in a.y..=b.y {
                for z in a.z..=b.z {
                    match w.state(BlockPos::new(x, y, z)).block().name {
                        "minecraft:web" => {
                            self.in_web = true;
                            self.fall_distance = 0.0;
                        }
                        "minecraft:soul_sand" => {
                            self.motion.x *= 0.4;
                            self.motion.z *= 0.4;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// `EntityPlayerSP.pushOutOfBlocks`
    fn push_out_of_blocks(&mut self, w: &impl EntityWorld, x: f64, y: f64, z: f64) {
        if self.no_clip {
            return;
        }
        let pos = BlockPos::new(floor_double(x), floor_double(y), floor_double(z));
        let d0 = x - pos.x as f64;
        let d1 = z - pos.z as f64;
        let open = |p: BlockPos| !w.state(p).block().normal_cube && !w.state(p.up()).block().normal_cube;
        if !open(pos) {
            let mut i = -1;
            let mut d2 = 9999.0;
            if open(pos.offset(-1, 0, 0)) && d0 < d2 {
                d2 = d0;
                i = 0;
            }
            if open(pos.offset(1, 0, 0)) && 1.0 - d0 < d2 {
                d2 = 1.0 - d0;
                i = 1;
            }
            if open(pos.offset(0, 0, -1)) && d1 < d2 {
                d2 = d1;
                i = 4;
            }
            if open(pos.offset(0, 0, 1)) && 1.0 - d1 < d2 {
                i = 5;
            }
            let f = 0.1f32 as f64;
            match i {
                0 => self.motion.x = -f,
                1 => self.motion.x = f,
                4 => self.motion.z = -f,
                5 => self.motion.z = f,
                _ => {}
            }
        }
    }

    /// Body yaw at the end of `EntityLivingBase.onUpdate`.
    fn update_body_yaw(&mut self) {
        let d0 = self.pos.x - self.prev_pos.x;
        let d1 = self.pos.z - self.prev_pos.z;
        let f = (d0 * d0 + d1 * d1) as f32;
        let mut f1 = self.render_yaw_offset;
        if f > 0.0025000002 {
            f1 = (math::atan2(d1, d0) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
        }
        if self.swing_progress > 0.0 {
            f1 = self.yaw;
        }
        // updateDistance
        let f = wrap_angle_to_180_float(f1 - self.render_yaw_offset);
        self.render_yaw_offset += f * 0.3;
        let mut f1 = wrap_angle_to_180_float(self.yaw - self.render_yaw_offset);
        if f1 < -75.0 {
            f1 = -75.0;
        }
        if f1 >= 75.0 {
            f1 = 75.0;
        }
        self.render_yaw_offset = self.yaw - f1;
        if f1 * f1 > 2500.0 {
            self.render_yaw_offset += f1 * 0.2;
        }
        // Keep prev values continuous across the ±180 wrap.
        while self.render_yaw_offset - self.prev_render_yaw_offset < -180.0 {
            self.prev_render_yaw_offset -= 360.0;
        }
        while self.render_yaw_offset - self.prev_render_yaw_offset >= 180.0 {
            self.prev_render_yaw_offset += 360.0;
        }
        while self.rotation_yaw_head - self.prev_rotation_yaw_head < -180.0 {
            self.prev_rotation_yaw_head -= 360.0;
        }
        while self.rotation_yaw_head - self.prev_rotation_yaw_head >= 180.0 {
            self.prev_rotation_yaw_head += 360.0;
        }
    }

    /// `Entity.setAngles`: mouse look, pitch clamped to ±90.
    pub fn turn(&mut self, dyaw: f32, dpitch: f32) {
        let f = self.pitch;
        let f1 = self.yaw;
        self.yaw = (self.yaw as f64 + dyaw as f64 * 0.15) as f32;
        self.pitch = (self.pitch as f64 - dpitch as f64 * 0.15) as f32;
        self.pitch = self.pitch.clamp(-90.0, 90.0);
        self.prev_pitch += self.pitch - f;
        self.prev_yaw += self.yaw - f1;
    }
}

fn step_toward_zero(v: f64, d: f64) -> f64 {
    if v < d && v >= -d {
        0.0
    } else if v > 0.0 {
        v - d
    } else {
        v + d
    }
}
