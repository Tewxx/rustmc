//! Entities the server tracks for us: other players (`EntityOtherPlayerMP`) and the rest by
//! kind, moved with vanilla's 3-step position interpolation (`setPositionAndRotation2`).
use mc_core::{Aabb, Vec3};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Player,
    /// `SpawnMob` type id.
    Mob(u8),
    /// `SpawnObject` type id (2 = item, waiting for its stack in metadata).
    Object(i8),
    /// `S11PacketSpawnExperienceOrb` (its own spawn packet, not `SpawnObject`).
    XpOrb,
}

pub struct Remote {
    pub id: i32,
    pub kind: Kind,
    /// `SpawnObject`'s data int: item frame facing, falling-block state id, potion damage,
    /// fishing-rod owner.
    pub data: i32,
    /// `EntityTNTPrimed.fuse` (`SpawnObject` carries no fuse; the client starts at 80).
    pub fuse: i32,
    /// `serverPosX/Y/Z`: fixed point ×32.
    pub server: (i32, i32, i32),
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub yaw: f32,
    pub prev_yaw: f32,
    pub pitch: f32,
    pub prev_pitch: f32,
    pub head_yaw: f32,
    pub prev_head_yaw: f32,
    pub body_yaw: f32,
    pub prev_body_yaw: f32,
    // interpolation target
    target: Vec3,
    target_yaw: f32,
    target_pitch: f32,
    increments: i32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub prev_limb_amount: f32,
    pub ticks_existed: i32,
    pub swing_ticks: i32,
    pub swinging: bool,
    pub prev_swing: f32,
    pub swing: f32,
    pub sneaking: bool,
    pub sprinting: bool,
    /// Data watcher flag 4 (`isEating`: eating, drinking, blocking or drawing a bow).
    pub eating: bool,
    /// `EntityOtherPlayerMP.isItemInUse` and `itemInUseCount`.
    pub in_use: bool,
    pub use_count: i32,
    pub on_ground: bool,
    /// Held item (`EntityEquipment` slot 0) as (catalogue index, count).
    pub held: Option<(usize, u8)>,
    /// The full held stack (damage value and NBT pick the model, colors and glint).
    pub held_stack: Option<mc_gui::ItemStack>,
    /// `EntityEquipment` slots 1-4 (boots .. helmet).
    pub armor: [Option<mc_gui::ItemStack>; 4],
    pub width: f64,
    pub height: f64,
    pub hurt_time: i32,
    /// Profile name (players), from the tab list.
    pub name: String,
    pub uuid: Option<mc_protocol::Uuid>,
    /// The data watcher as last synced (`DataWatcher.updateWatchedObjectsFromList`).
    pub meta: HashMap<u8, mc_protocol::MetaValue>,
    /// `EntityLivingBase.activePotionsMap` (`S1D`/`S1E`) for living remotes.
    pub effects: mc_entity::effects::Effects,
    /// `EntityLiving.getLeashedToEntity()`: the leash holder's entity id (`S1BPacketEntityAttach`
    /// with leash=1); `None` when not leashed.
    pub leash_holder: Option<i32>,
    /// Set by the client before each tick (`EntitySquid.isInWater`).
    pub in_water: bool,
    pub anim: MobAnim,
    /// `motionX/Y/Z`, from the spawn packet and `S12PacketEntityVelocity`; non-living entities
    /// move themselves with it between the server's (sparse) position updates.
    pub motion: Vec3,
    /// `EntityArrow.inGround` / `arrowShake`.
    pub in_ground: bool,
    pub shake: i32,
}

/// What the non-living entities' client-side `onUpdate` asks of the world.
pub trait ObjectWorld {
    /// `World.rayTraceBlocks(from, to, false, true, false)`: the first block with a collision box.
    fn ray(&mut self, from: Vec3, to: Vec3) -> Option<Vec3>;
    /// `Entity.moveEntity` against block collision boxes: the moved box and `onGround`;
    /// `motion` is clipped to what was actually moved.
    fn collide(&mut self, bb: &Aabb, motion: &mut Vec3) -> (Aabb, bool);
    /// `Entity.isInWater`.
    fn in_water(&self, bb: &Aabb) -> bool;
    /// `slipperiness` of the block under the box.
    fn slipperiness(&self, bb: &Aabb) -> f32;
    /// `World.getClosestPlayerToEntity(entity, range)`: the closest player's feet and eye height.
    fn closest_player(&self, pos: Vec3, range: f64) -> Option<(Vec3, f64)>;
}

/// A world with nothing in it (unit tests).
#[cfg(test)]
pub struct NoWorld;

#[cfg(test)]
impl ObjectWorld for NoWorld {
    fn ray(&mut self, _: Vec3, _: Vec3) -> Option<Vec3> {
        None
    }
    fn collide(&mut self, bb: &Aabb, motion: &mut Vec3) -> (Aabb, bool) {
        (bb.offset(motion.x, motion.y, motion.z), false)
    }
    fn in_water(&self, _: &Aabb) -> bool {
        false
    }
    fn slipperiness(&self, _: &Aabb) -> f32 {
        0.6
    }
    fn closest_player(&self, _: Vec3, _: f64) -> Option<(Vec3, f64)> {
        None
    }
}

/// The projectiles' rotation update: face along the motion, eased by 0.2 from the previous
/// rotation (`EntityThrowable`, `EntityArrow`, `EntityEnderEye`, `EntityFireworkRocket`).
fn face_motion(m: Vec3, yaw: &mut f32, pitch: &mut f32, prev_yaw: &mut f32, prev_pitch: &mut f32) {
    let f = (m.x * m.x + m.z * m.z).sqrt() as f32;
    let ry = (m.x.atan2(m.z) * 180.0 / std::f64::consts::PI) as f32;
    let rp = (m.y.atan2(f as f64) * 180.0 / std::f64::consts::PI) as f32;
    while rp - *prev_pitch < -180.0 {
        *prev_pitch -= 360.0;
    }
    while rp - *prev_pitch >= 180.0 {
        *prev_pitch += 360.0;
    }
    while ry - *prev_yaw < -180.0 {
        *prev_yaw -= 360.0;
    }
    while ry - *prev_yaw >= 180.0 {
        *prev_yaw += 360.0;
    }
    *pitch = *prev_pitch + (rp - *prev_pitch) * 0.2;
    *yaw = *prev_yaw + (ry - *prev_yaw) * 0.2;
}

fn centre(bb: &Aabb) -> Vec3 {
    Vec3::new((bb.min_x + bb.max_x) / 2.0, bb.min_y, (bb.min_z + bb.max_z) / 2.0)
}

/// Client-side state the mob renderers read, advanced the way the vanilla entity classes
/// advance it on a `WorldClient`.
#[derive(Clone, Debug)]
pub struct MobAnim {
    pub death_time: i32,
    /// `EntityBodyHelper`
    rotation_ticks: i32,
    prev_head: f32,
    /// `EntityCreeper.timeSinceIgnited` / `lastActiveTime`
    pub since_ignited: i32,
    pub last_active: i32,
    /// `EntityChicken.wingRotation`, `field_70888_h`, `destPos`, `field_70884_g`
    pub wing: f32,
    pub prev_wing: f32,
    pub dest_pos: f32,
    pub prev_dest_pos: f32,
    wing_delta: f32,
    /// `EntitySlime.squishFactor` / `prevSquishFactor`
    pub squish: f32,
    pub prev_squish: f32,
    squish_amount: f32,
    was_on_ground: bool,
    /// `EntitySquid`
    pub squid_rotation: f32,
    rotation_velocity: f32,
    pub tentacle: f32,
    pub last_tentacle: f32,
    pub squid_pitch: f32,
    pub prev_squid_pitch: f32,
    pub squid_yaw: f32,
    pub prev_squid_yaw: f32,
    squid_spin: f32,
    /// `EntityIronGolem.attackTimer` / `holdRoseTick`
    pub attack_timer: i32,
    pub hold_rose: i32,
}

impl Default for MobAnim {
    fn default() -> Self {
        Self {
            death_time: 0,
            rotation_ticks: 0,
            prev_head: 0.,
            since_ignited: 0,
            last_active: 0,
            wing: 0.,
            prev_wing: 0.,
            dest_pos: 0.,
            prev_dest_pos: 0.,
            wing_delta: 1.,
            squish: 0.,
            prev_squish: 0.,
            squish_amount: 0.,
            was_on_ground: false,
            squid_rotation: 0.,
            // 1 / (rand.nextFloat() + 1) * 0.2 at its mean.
            rotation_velocity: 0.135,
            tentacle: 0.,
            last_tentacle: 0.,
            squid_pitch: 0.,
            prev_squid_pitch: 0.,
            squid_yaw: 0.,
            prev_squid_yaw: 0.,
            squid_spin: 0.,
            attack_timer: 0,
            hold_rose: 0,
        }
    }
}

/// `EntityList` mob ids with their `setSize` (width, height).
pub fn mob_size(kind: u8) -> (f64, f64) {
    match kind {
        51 | 54 | 57 | 66 => (0.6, 1.95),
        52 => (1.4, 0.9),
        53 => (3.6, 10.8),
        56 => (4.0, 4.0),
        58 => (0.6, 2.9),
        59 => (0.7, 0.5),
        60 | 67 => (0.4, 0.3),
        63 => (16.0, 8.0),
        64 => (0.9, 3.5),
        65 => (0.5, 0.9),
        68 => (0.85, 0.85),
        90 => (0.9, 0.9),
        91 | 92 | 96 => (0.9, 1.3),
        93 => (0.4, 0.7),
        94 => (0.95, 0.95),
        95 => (0.6, 0.8),
        97 => (0.7, 1.9),
        98 | 101 => (0.6, 0.7),
        99 => (1.4, 2.9),
        100 => (1.4_f32 as f64, 1.6_f32 as f64),
        _ => (0.6, 1.8),
    }
}

fn wrap(mut a: f32) -> f32 {
    a %= 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// `EntityBodyHelper.computeAngleWithBound`
fn bound(a: f32, b: f32, max: f32) -> f32 {
    let mut f = wrap(a - b);
    if f < -max {
        f = -max;
    }
    if f >= max {
        f = max;
    }
    a - f
}

pub fn angle(b: i8) -> f32 {
    b as f32 * 360.0 / 256.0
}

/// `SpawnObject` types: `(width, height)` from each entity's `setSize`.
pub fn object_size(kind: i8) -> (f64, f64) {
    match kind {
        1 => (1.375, 0.6),          // EntityBoat
        2 => (0.25, 0.25),          // EntityItem
        10 => (0.98, 0.7),          // EntityMinecart
        50 => (0.98, 0.98),         // EntityTNTPrimed
        51 => (2.0, 2.0),           // EntityEnderCrystal
        60 => (0.5, 0.5),           // EntityArrow
        61 => (0.25, 0.25),         // EntitySnowball
        62 => (0.25, 0.25),         // EntityEgg
        63 => (1.0, 1.0),           // EntityLargeFireball
        64 => (0.3125, 0.3125),     // EntitySmallFireball
        65 => (0.25, 0.25),         // EntityEnderPearl
        66 => (0.3125, 0.3125),     // EntityWitherSkull
        70 => (0.98, 0.98),         // EntityFallingBlock
        71 => (0.5, 0.5),           // EntityItemFrame
        72 => (0.25, 0.25),         // EntityEnderEye
        73 => (0.25, 0.25),         // EntityPotion
        75 => (0.25, 0.25),         // EntityExpBottle
        76 => (0.25, 0.25),         // EntityFireworkRocket
        77 => (0.5, 0.5),           // EntityLeashKnot
        78 => (0.5, 1.975),         // EntityArmorStand
        90 => (0.25, 0.25),         // EntityFishHook
        _ => (0.6, 0.6),
    }
}

impl Remote {
    pub fn new(id: i32, kind: Kind, x: i32, y: i32, z: i32, yaw: i8, pitch: i8) -> Self {
        let pos = Vec3::new(x as f64 / 32.0, y as f64 / 32.0, z as f64 / 32.0);
        let (w, h) = match &kind {
            Kind::Player => (0.6, 1.8),
            Kind::Mob(k) => mob_size(*k),
            Kind::Object(k) => object_size(*k),
            Kind::XpOrb => (0.5, 0.5),
        };
        let (yw, pt) = (angle(yaw), angle(pitch));
        let fuse = if matches!(&kind, Kind::Object(50)) { 80 } else { 0 };
        Self {
            id,
            kind,
            data: 0,
            fuse,
            server: (x, y, z),
            pos,
            prev_pos: pos,
            yaw: yw,
            prev_yaw: yw,
            pitch: pt,
            prev_pitch: pt,
            head_yaw: yw,
            prev_head_yaw: yw,
            body_yaw: yw,
            prev_body_yaw: yw,
            target: pos,
            target_yaw: yw,
            target_pitch: pt,
            increments: 0,
            limb_swing: 0.,
            limb_amount: 0.,
            prev_limb_amount: 0.,
            ticks_existed: 0,
            swing_ticks: 0,
            swinging: false,
            prev_swing: 0.,
            swing: 0.,
            sneaking: false,
            sprinting: false,
            eating: false,
            in_use: false,
            use_count: 0,
            on_ground: false,
            held: None,
            held_stack: None,
            armor: [None; 4],
            width: w,
            height: h,
            hurt_time: 0,
            name: String::new(),
            uuid: None,
            meta: HashMap::new(),
            effects: mc_entity::effects::Effects::default(),
            leash_holder: None,
            in_water: false,
            anim: MobAnim::default(),
            motion: Vec3::new(0., 0., 0.),
            in_ground: false,
            shake: 0,
        }
    }

    /// How many ticks `setPositionAndRotation2` spreads a move over; `None` for the entities
    /// that keep `Entity.setPositionAndRotation2` and snap to the new position.
    fn interpolation_steps(&self) -> Option<i32> {
        match self.kind {
            // EntityLivingBase / EntityOtherPlayerMP (armor stands are living), EntityFishHook.
            Kind::Player | Kind::Mob(_) | Kind::Object(78) | Kind::Object(90) => Some(3),
            // EntityMinecart: turnProgress = increments + 2.
            Kind::Object(10) => Some(5),
            // EntityBoat: boatPosRotationIncrements = increments + 5.
            Kind::Object(1) => Some(8),
            _ => None,
        }
    }

    /// `Entity.setVelocity` (`EntityArrow` also faces the new motion while it has no rotation).
    pub fn set_velocity(&mut self, m: Vec3) {
        self.motion = m;
        if matches!(self.kind, Kind::Object(60)) && self.prev_pitch == 0. && self.prev_yaw == 0. {
            let f = (m.x * m.x + m.z * m.z).sqrt();
            self.yaw = (m.x.atan2(m.z) * 180.0 / std::f64::consts::PI) as f32;
            self.pitch = (m.y.atan2(f) * 180.0 / std::f64::consts::PI) as f32;
            self.prev_yaw = self.yaw;
            self.prev_pitch = self.pitch;
        }
    }

    /// `entity instanceof EntityLivingBase`: players, mobs and armor stands. Objects and XP
    /// orbs never take potion effects.
    pub fn is_living(&self) -> bool {
        matches!(self.kind, Kind::Player | Kind::Mob(_) | Kind::Object(78))
    }

    pub fn meta_byte(&self, i: u8) -> i8 {
        match self.meta.get(&i) {
            Some(mc_protocol::MetaValue::Byte(b)) => *b,
            _ => 0,
        }
    }

    pub fn meta_int(&self, i: u8) -> i32 {
        match self.meta.get(&i) {
            Some(mc_protocol::MetaValue::Int(v)) => *v,
            _ => 0,
        }
    }

    pub fn meta_float(&self, i: u8) -> Option<f32> {
        match self.meta.get(&i) {
            Some(mc_protocol::MetaValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn meta_short(&self, i: u8) -> i16 {
        match self.meta.get(&i) {
            Some(mc_protocol::MetaValue::Short(v)) => *v,
            _ => 0,
        }
    }

    pub fn meta_rot(&self, i: u8) -> Option<(f32, f32, f32)> {
        match self.meta.get(&i) {
            Some(mc_protocol::MetaValue::Rotations(x, y, z)) => Some((*x, *y, *z)),
            _ => None,
        }
    }

    pub fn meta_string(&self, i: u8) -> Option<&str> {
        match self.meta.get(&i) {
            Some(mc_protocol::MetaValue::String(s)) => Some(s),
            _ => None,
        }
    }

    /// `Entity.isInvisible()` (flags byte bit 0x20).
    pub fn invisible(&self) -> bool {
        self.meta_byte(0) & 0x20 != 0
    }

    /// `Entity.getAlwaysRenderNameTagForRender()`: byte 3 for living entities,
    /// `customNameVisible` for the rest.
    pub fn name_visible(&self) -> bool {
        self.meta_byte(3) != 0
    }

    /// `Entity.getCustomNameTag()`.
    pub fn custom_name(&self) -> Option<&str> {
        self.meta_string(2).filter(|s| !s.is_empty())
    }

    /// `getHealth() > 0` (an unsynced health counts as alive).
    pub fn alive(&self) -> bool {
        self.meta_float(6).is_none_or(|h| h > 0.)
    }

    /// `EntityAgeable.isChild` (growing age byte < 0) / `EntityZombie.isChild` (byte 12 = 1).
    pub fn is_child(&self) -> bool {
        self.meta_byte(12) < 0 || matches!(self.kind, Kind::Mob(54 | 57)) && self.meta_byte(12) == 1
    }

    /// `EntityLivingBase.handleStatusUpdate` and the mob overrides.
    pub fn status(&mut self, id: i8) {
        match (id, &self.kind) {
            (2, _) => {
                self.limb_amount = 1.5;
                self.hurt_time = 10;
            }
            (3, _) => {
                self.meta.insert(6, mc_protocol::MetaValue::Float(0.));
            }
            (4, Kind::Mob(99)) => self.anim.attack_timer = 10,
            (11, Kind::Mob(99)) => self.anim.hold_rose = 400,
            (34, Kind::Mob(99)) => self.anim.hold_rose = 0,
            (19, Kind::Mob(94)) => self.anim.squid_rotation = 0.,
            _ => {}
        }
    }

    pub fn bb(&self) -> Aabb {
        let r = self.width / 2.0;
        Aabb::new(self.pos.x - r, self.pos.y, self.pos.z - r, self.pos.x + r, self.pos.y + self.height, self.pos.z + r)
    }

    /// `setPositionAndRotation2(x, y, z, yaw, pitch, 3, _)`. A teleport within 1/32 block
    /// (1/64 vertically) keeps the current position (`handleEntityTeleport`).
    pub fn move_to(&mut self, yaw: f32, pitch: f32, teleport: bool) {
        let mut t = Vec3::new(self.server.0 as f64 / 32.0, self.server.1 as f64 / 32.0, self.server.2 as f64 / 32.0);
        if teleport && (self.pos.x - t.x).abs() < 0.03125 && (self.pos.y - t.y).abs() < 0.015625 && (self.pos.z - t.z).abs() < 0.03125 {
            t = self.pos;
        }
        match self.interpolation_steps() {
            Some(n) => {
                self.target = t;
                self.target_yaw = yaw;
                self.target_pitch = pitch;
                self.increments = n;
            }
            None => {
                // Entity.setPosition + setRotation.
                self.pos = t;
                self.yaw = yaw % 360.0;
                self.pitch = pitch % 360.0;
            }
        }
    }

    /// The non-living entities' own `onUpdate` movement, which the client runs too.
    pub fn physics(&mut self, w: &mut impl ObjectWorld) {
        let k = match self.kind {
            Kind::Object(k) => k,
            Kind::XpOrb => return self.xp_physics(w),
            _ => return,
        };
        match k {
            // EntityThrowable: snowball, egg, ender pearl, splash potion, bottle o' enchanting.
            // The client's block ray only feeds onImpact; the throwable flies on until the
            // server removes it.
            61 | 62 | 65 | 73 | 75 => {
                self.pos = Vec3::new(self.pos.x + self.motion.x, self.pos.y + self.motion.y, self.pos.z + self.motion.z);
                face_motion(self.motion, &mut self.yaw, &mut self.pitch, &mut self.prev_yaw, &mut self.prev_pitch);
                let drag = if w.in_water(&self.bb()) { 0.8f32 } else { 0.99 } as f64;
                let gravity = match k {
                    73 => 0.05f32,
                    75 => 0.07,
                    _ => 0.03,
                } as f64;
                let m = self.motion;
                self.motion = Vec3::new(m.x * drag, m.y * drag - gravity, m.z * drag);
            }
            // EntityEnderEye: the steering toward the stronghold is server-side only.
            72 => {
                self.pos = Vec3::new(self.pos.x + self.motion.x, self.pos.y + self.motion.y, self.pos.z + self.motion.z);
                face_motion(self.motion, &mut self.yaw, &mut self.pitch, &mut self.prev_yaw, &mut self.prev_pitch);
            }
            // EntityFireworkRocket
            76 => {
                self.motion = Vec3::new(self.motion.x * 1.15, self.motion.y + 0.04, self.motion.z * 1.15);
                let mut m = self.motion;
                let (bb, _) = w.collide(&self.bb(), &mut m);
                self.pos = centre(&bb);
                face_motion(self.motion, &mut self.yaw, &mut self.pitch, &mut self.prev_yaw, &mut self.prev_pitch);
            }
            // EntityFireball (large, small, wither skull): the client has no acceleration, so
            // the spawn motion just decays by the motion factor.
            63 | 64 | 66 => {
                self.pos = Vec3::new(self.pos.x + self.motion.x, self.pos.y + self.motion.y, self.pos.z + self.motion.z);
                let f = if w.in_water(&self.bb()) { 0.8f32 } else if k == 66 { 0.73 } else { 0.95 } as f64;
                self.motion = Vec3::new(self.motion.x * f, self.motion.y * f, self.motion.z * f);
            }
            60 => self.arrow_physics(w),
            // EntityTNTPrimed / EntityFallingBlock
            50 | 70 => {
                self.motion.y -= 0.03999999910593033;
                let mut m = self.motion;
                let (bb, ground) = w.collide(&self.bb(), &mut m);
                self.pos = centre(&bb);
                self.on_ground = ground;
                self.clip_motion(m);
                let d = 0.9800000190734863;
                self.motion = Vec3::new(self.motion.x * d, self.motion.y * d, self.motion.z * d);
                if ground {
                    let up = if k == 50 { -0.5 } else { 1.0 };
                    self.motion = Vec3::new(self.motion.x * 0.699999988079071, self.motion.y * up, self.motion.z * 0.699999988079071);
                }
            }
            _ => {}
        }
    }

    /// `Entity.moveEntity` zeroes each motion component a collision cut short.
    fn clip_motion(&mut self, moved: Vec3) {
        if moved.x != self.motion.x {
            self.motion.x = 0.;
        }
        if moved.y != self.motion.y {
            self.motion.y = 0.;
        }
        if moved.z != self.motion.z {
            self.motion.z = 0.;
        }
    }

    /// `EntityArrow.onUpdate` on the client: it flies until its ray meets a block, then sticks.
    fn arrow_physics(&mut self, w: &mut impl ObjectWorld) {
        if self.prev_pitch == 0. && self.prev_yaw == 0. {
            self.set_velocity(self.motion);
        }
        if self.shake > 0 {
            self.shake -= 1;
        }
        if self.in_ground {
            return;
        }
        let end = Vec3::new(self.pos.x + self.motion.x, self.pos.y + self.motion.y, self.pos.z + self.motion.z);
        if let Some(hit) = w.ray(self.pos, end) {
            // The block branch of the impact: back off 0.05 along the motion and stick.
            self.motion = Vec3::new(hit.x - self.pos.x, hit.y - self.pos.y, hit.z - self.pos.z);
            let len = (self.motion.x.powi(2) + self.motion.y.powi(2) + self.motion.z.powi(2)).sqrt() as f32 as f64;
            if len > 0. {
                let k = 0.05000000074505806 / len;
                self.pos = Vec3::new(self.pos.x - self.motion.x * k, self.pos.y - self.motion.y * k, self.pos.z - self.motion.z * k);
            }
            self.in_ground = true;
            self.shake = 7;
        }
        self.pos = Vec3::new(self.pos.x + self.motion.x, self.pos.y + self.motion.y, self.pos.z + self.motion.z);
        face_motion(self.motion, &mut self.yaw, &mut self.pitch, &mut self.prev_yaw, &mut self.prev_pitch);
        let drag = if w.in_water(&self.bb()) { 0.6f32 } else { 0.99 } as f64;
        let m = self.motion;
        self.motion = Vec3::new(m.x * drag, m.y * drag - 0.05000000074505806, m.z * drag);
    }

    /// `EntityXPOrb.onUpdate` (the lava bounce and the pickup are the server's business).
    fn xp_physics(&mut self, w: &mut impl ObjectWorld) {
        self.motion.y -= 0.029999999329447746;
        let d0 = 8.0;
        if let Some((p, eye)) = w.closest_player(self.pos, d0) {
            let d1 = (p.x - self.pos.x) / d0;
            let d2 = (p.y + eye - self.pos.y) / d0;
            let d3 = (p.z - self.pos.z) / d0;
            let d4 = (d1 * d1 + d2 * d2 + d3 * d3).sqrt();
            let mut d5 = 1.0 - d4;
            if d5 > 0.0 {
                d5 *= d5;
                self.motion = Vec3::new(self.motion.x + d1 / d4 * d5 * 0.1, self.motion.y + d2 / d4 * d5 * 0.1, self.motion.z + d3 / d4 * d5 * 0.1);
            }
        }
        let mut m = self.motion;
        let (bb, ground) = w.collide(&self.bb(), &mut m);
        self.pos = centre(&bb);
        self.on_ground = ground;
        self.clip_motion(m);
        let f = if ground { w.slipperiness(&bb) * 0.98 } else { 0.98 } as f64;
        self.motion = Vec3::new(self.motion.x * f, self.motion.y * 0.9800000190734863, self.motion.z * f);
        if ground {
            self.motion.y *= -0.8999999761581421;
        }
    }

    pub fn swing_arm(&mut self) {
        // EntityLivingBase.swingItem (haste/mining fatigue change getArmSwingAnimationEnd)
        if !self.swinging || self.swing_ticks >= self.effects.swing_end() / 2 || self.swing_ticks < 0 {
            self.swing_ticks = -1;
            self.swinging = true;
        }
    }

    pub fn tick(&mut self) {
        let mob = match self.kind {
            Kind::Mob(m) => Some(m),
            _ => None,
        };
        // EntityTNTPrimed.onUpdate counts the fuse down on the client too (the spawn packet
        // carries no fuse, so it starts at 80).
        if matches!(self.kind, Kind::Object(50)) && self.fuse > 0 {
            self.fuse -= 1;
        }
        // EntityCreeper.onUpdate runs before the living update.
        if mob == Some(50) && self.alive() {
            let state = if self.meta_byte(18) != 0 { 1 } else { self.meta_byte(16) as i32 };
            let a = &mut self.anim;
            a.last_active = a.since_ignited;
            a.since_ignited = (a.since_ignited + state).clamp(0, 30);
        }
        // EntitySlime.onUpdate: squishFactor eases first, and prevSquishFactor copies the new
        // value (so the renderer's interpolation is a no-op, as in vanilla).
        if matches!(mob, Some(55 | 62)) {
            let a = &mut self.anim;
            a.squish += (a.squish_amount - a.squish) * 0.5;
            a.prev_squish = a.squish;
        }
        if mob.is_some() && !self.alive() {
            // onDeathUpdate
            self.anim.death_time += 1;
        }
        self.ticks_existed += 1;
        self.prev_pos = self.pos;
        self.prev_yaw = self.yaw;
        self.prev_pitch = self.pitch;
        self.prev_head_yaw = self.head_yaw;
        self.prev_body_yaw = self.body_yaw;
        self.prev_swing = self.swing;
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        // EntityLivingBase.onUpdate -> updatePotionEffects: the client only counts the
        // durations down; expired effects stay until `S1E` removes them.
        if self.is_living() {
            self.effects.tick_client();
        }
        // EntityOtherPlayerMP.onLivingUpdate
        if self.increments > 0 {
            let n = self.increments as f64;
            self.pos = Vec3::new(
                self.pos.x + (self.target.x - self.pos.x) / n,
                self.pos.y + (self.target.y - self.pos.y) / n,
                self.pos.z + (self.target.z - self.pos.z) / n,
            );
            self.yaw += wrap(self.target_yaw - self.yaw) / self.increments as f32;
            self.pitch += (self.target_pitch - self.pitch) / self.increments as f32;
            self.increments -= 1;
        }
        // updateArmSwingProgress
        let swing_end = self.effects.swing_end();
        if self.swinging {
            self.swing_ticks += 1;
            if self.swing_ticks >= swing_end {
                self.swing_ticks = 0;
                self.swinging = false;
            }
        } else {
            self.swing_ticks = 0;
        }
        self.swing = self.swing_ticks as f32 / swing_end as f32;
        if matches!(self.kind, Kind::Object(_) | Kind::XpOrb) {
            self.body_yaw = self.yaw;
            return;
        }
        let dx = self.pos.x - self.prev_pos.x;
        let dz = self.pos.z - self.prev_pos.z;
        // moveEntityWithHeading: limb swing
        self.prev_limb_amount = self.limb_amount;
        let mut f = (dx * dx + dz * dz).sqrt() as f32 * 4.0;
        if f > 1.0 {
            f = 1.0;
        }
        self.limb_amount += (f - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
        if let Some(m) = mob {
            self.mob_living_update(m, dx, dz);
            // EntityLiving.func_110146_f: EntityBodyHelper.updateRenderAngles
            let a = &mut self.anim;
            if dx * dx + dz * dz > 2.500000277905201E-7 {
                self.body_yaw = self.yaw;
                self.head_yaw = bound(self.body_yaw, self.head_yaw, 75.);
                a.prev_head = self.head_yaw;
                a.rotation_ticks = 0;
            } else {
                let mut f = 75.;
                if (self.head_yaw - a.prev_head).abs() > 15. {
                    a.rotation_ticks = 0;
                    a.prev_head = self.head_yaw;
                } else {
                    a.rotation_ticks += 1;
                    if a.rotation_ticks > 10 {
                        f = (1. - (a.rotation_ticks - 10) as f32 / 10.).max(0.) * 75.;
                    }
                }
                self.body_yaw = bound(self.head_yaw, self.body_yaw, f);
            }
            if matches!(m, 55 | 62) {
                let a = &mut self.anim;
                if self.on_ground && !a.was_on_ground {
                    a.squish_amount = -0.5;
                } else if !self.on_ground && a.was_on_ground {
                    a.squish_amount = 1.;
                }
                a.was_on_ground = self.on_ground;
                // alterSquishAmount
                a.squish_amount *= if m == 62 { 0.9 } else { 0.6 };
            }
            return;
        }
        // EntityLivingBase.onUpdate: body yaw follows movement, clamped to the head.
        let d2 = (dx * dx + dz * dz) as f32;
        let mut f3 = self.body_yaw;
        if d2 > 0.0025000002 {
            f3 = (dz.atan2(dx) as f32) * 180.0 / std::f32::consts::PI - 90.0;
        }
        if self.swing > 0.0 {
            f3 = self.yaw;
        }
        let f = wrap(f3 - self.body_yaw);
        self.body_yaw += f * 0.3;
        let mut f1 = wrap(self.head_yaw - self.body_yaw);
        f1 = f1.clamp(-75.0, 75.0);
        self.body_yaw = self.head_yaw - f1;
        if f1 * f1 > 2500.0 {
            self.body_yaw += f1 * 0.2;
        }
    }

    /// The mob classes' `onLivingUpdate` additions that run on the client.
    fn mob_living_update(&mut self, m: u8, dx: f64, dz: f64) {
        let a = &mut self.anim;
        match m {
            93 => {
                a.prev_wing = a.wing;
                a.prev_dest_pos = a.dest_pos;
                a.dest_pos = (a.dest_pos as f64 + if self.on_ground { -1. } else { 4. } * 0.3) as f32;
                a.dest_pos = a.dest_pos.clamp(0., 1.);
                if !self.on_ground && a.wing_delta < 1. {
                    a.wing_delta = 1.;
                }
                a.wing_delta = (a.wing_delta as f64 * 0.9) as f32;
                a.wing += a.wing_delta * 2.;
            }
            94 => {
                use std::f32::consts::PI;
                a.prev_squid_pitch = a.squid_pitch;
                a.prev_squid_yaw = a.squid_yaw;
                a.last_tentacle = a.tentacle;
                a.squid_rotation += a.rotation_velocity;
                if a.squid_rotation as f64 > std::f64::consts::PI * 2. {
                    a.squid_rotation = PI * 2.;
                }
                if self.in_water {
                    if a.squid_rotation < PI {
                        let f = a.squid_rotation / PI;
                        a.tentacle = mc_core::math::sin(f * f * PI) * PI * 0.25;
                        if f as f64 > 0.75 {
                            a.squid_spin = 1.;
                        } else {
                            a.squid_spin *= 0.8;
                        }
                    } else {
                        a.tentacle = 0.;
                        a.squid_spin *= 0.99;
                    }
                    // The client's motion is what the position did this tick.
                    let my = self.pos.y - self.prev_pos.y;
                    let f1 = (dx * dx + dz * dz).sqrt() as f32;
                    a.squid_yaw = (a.squid_yaw as f64 + std::f64::consts::PI * a.squid_spin as f64 * 1.5) as f32;
                    a.squid_pitch += (-(f1.atan2(my as f32)) * 180. / PI - a.squid_pitch) * 0.1;
                } else {
                    a.tentacle = mc_core::math::sin(a.squid_rotation).abs() * PI * 0.25;
                    a.squid_pitch = (a.squid_pitch as f64 + (-90. - a.squid_pitch) as f64 * 0.02) as f32;
                }
            }
            99 => {
                if a.attack_timer > 0 {
                    a.attack_timer -= 1;
                }
                if a.hold_rose > 0 {
                    a.hold_rose -= 1;
                }
            }
            _ => {}
        }
    }

    pub fn interpolated(&self, pt: f32) -> Vec3 {
        let pt = pt as f64;
        Vec3::new(
            self.prev_pos.x + (self.pos.x - self.prev_pos.x) * pt,
            self.prev_pos.y + (self.pos.y - self.prev_pos.y) * pt,
            self.prev_pos.z + (self.pos.z - self.prev_pos.z) * pt,
        )
    }
}

#[derive(Clone, Debug)]
/// `EntityPainting`: the hanging block position, facing yaw and `EnumArt` ordinal.
pub struct Painting {
    pub id: i32,
    pub pos: Vec3,
    pub yaw: f32,
    pub art: usize,
}

/// `EntityLightningBolt` (`S2CPacketSpawnGlobalEntity`, type 1).
#[derive(Clone, Debug)]
pub struct Lightning {
    pub id: i32,
    /// `RenderLightningBolt` needs the strike position; the bolt itself is not ported yet.
    #[allow(dead_code)]
    pub pos: Vec3,
    /// `EntityLightningBolt.ticksExisted`; the bolt is removed after 4 client ticks.
    pub ticks: i32,
}

#[derive(Default)]
pub struct Entities {
    pub map: HashMap<i32, Remote>,
}

impl Entities {
    pub fn tick(&mut self, w: &mut impl ObjectWorld) {
        for e in self.map.values_mut() {
            e.tick();
            e.physics(w);
        }
    }

    /// `EntityRenderer.getMouseOver`'s entity pass: the closest entity whose box (grown by
    /// `getCollisionBorderSize` 0.1) the look ray hits within `reach`.
    pub fn ray(&self, eye: Vec3, look: Vec3, reach: f64) -> Option<(i32, f64, Vec3)> {
        let end = Vec3::new(eye.x + look.x * reach, eye.y + look.y * reach, eye.z + look.z * reach);
        let mut best: Option<(i32, f64, Vec3)> = None;
        for e in self.map.values() {
            if matches!(e.kind, Kind::Object(k) if k != 78) {
                continue;
            }
            if matches!(e.kind, Kind::Object(78)) && matches!(e.meta.get(&10), Some(mc_protocol::MetaValue::Byte(v)) if v & 16 != 0) {
                continue;
            }
            let border = 0.1_f32 as f64;
            let bb = e.bb().expand(border, border, border);
            let (d, hit) = if bb.is_vec_inside(eye) {
                (0.0, bb.calculate_intercept(eye, end).map_or(eye, |(hit, _)| hit))
            } else if let Some((hit, _)) = bb.calculate_intercept(eye, end) {
                (((hit.x - eye.x).powi(2) + (hit.y - eye.y).powi(2) + (hit.z - eye.z).powi(2)).sqrt(), hit)
            } else {
                continue;
            };
            if d <= reach && best.is_none_or(|(_, b, _)| d < b) {
                best = Some((e.id, d, hit));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_sizes_follow_set_size() {
        assert_eq!(object_size(2), (0.25, 0.25));
        assert_eq!(object_size(78), (0.5, 1.975));
        assert_eq!(object_size(60), (0.5, 0.5));
        assert_eq!(object_size(50), (0.98, 0.98));
        assert_eq!(object_size(71), (0.5, 0.5));
        assert_eq!(object_size(90), (0.25, 0.25));
    }

    #[test]
    fn tnt_starts_with_the_vanilla_fuse() {
        let mut tnt = Remote::new(1, Kind::Object(50), 0, 0, 0, 0, 0);
        assert_eq!(tnt.fuse, 80);
        tnt.tick();
        assert_eq!(tnt.fuse, 79);
        let arrow = Remote::new(2, Kind::Object(60), 0, 0, 0, 0, 0);
        assert_eq!(arrow.fuse, 0);
    }

    #[test]
    fn metadata_accessors() {
        let mut r = Remote::new(3, Kind::Object(78), 0, 0, 0, 0, 0);
        r.meta.insert(0, mc_protocol::MetaValue::Byte(0x20));
        r.meta.insert(2, mc_protocol::MetaValue::String("hello".into()));
        r.meta.insert(3, mc_protocol::MetaValue::Byte(1));
        assert!(r.invisible());
        assert_eq!(r.custom_name(), Some("hello"));
        assert!(r.name_visible());
        r.meta.insert(12, mc_protocol::MetaValue::Rotations(1., 2., 3.));
        assert_eq!(r.meta_rot(12), Some((1., 2., 3.)));
        assert_eq!(r.meta_rot(11), None);
    }

    #[test]
    fn remote_potion_effects_tick_down_and_stay_until_removed() {
        use mc_entity::effects::{PotionEffect, SPEED};
        let mut e = Remote::new(7, Kind::Player, 0, 0, 0, 0, 0);
        e.effects.add(PotionEffect { id: SPEED, duration: 2, amplifier: 1, ambient: false, show_particles: true, duration_max: false });
        e.tick();
        assert_eq!(e.effects.get(SPEED).unwrap().duration, 1);
        e.tick();
        assert_eq!(e.effects.get(SPEED).unwrap().duration, 0);
        // The client keeps expired effects; only `S1E` removes them.
        e.tick();
        assert!(e.effects.has(SPEED));
    }

    #[test]
    fn only_living_remotes_tick_effects() {
        use mc_entity::effects::{PotionEffect, SPEED};
        let mut orb = Remote::new(8, Kind::XpOrb, 0, 0, 0, 0, 0);
        orb.effects.add(PotionEffect { id: SPEED, duration: 5, amplifier: 0, ambient: false, show_particles: true, duration_max: false });
        orb.tick();
        assert_eq!(orb.effects.get(SPEED).unwrap().duration, 5);
        assert!(Remote::new(9, Kind::Mob(50), 0, 0, 0, 0, 0).is_living());
        assert!(!Remote::new(10, Kind::Object(2), 0, 0, 0, 0, 0).is_living());
    }

    #[test]
    fn interpolates_over_three_ticks() {
        let mut e = Remote::new(1, Kind::Player, 0, 0, 0, 0, 0);
        e.server = (96, 0, 0);
        e.move_to(0., 0., false);
        e.tick();
        assert!((e.pos.x - 1.0).abs() < 1e-9);
        e.tick();
        e.tick();
        assert!((e.pos.x - 3.0).abs() < 1e-9);
        assert!(e.limb_amount > 0.);
    }

    #[test]
    fn thrown_pearl_flies_between_updates() {
        let mut e = Remote::new(1, Kind::Object(65), 0, 0, 0, 0, 0);
        e.set_velocity(Vec3::new(1.0, 0.5, 0.0));
        let mut w = NoWorld;
        e.tick();
        e.physics(&mut w);
        assert!((e.pos.x - 1.0).abs() < 1e-9 && (e.pos.y - 0.5).abs() < 1e-9);
        assert!((e.motion.y - (0.5 * 0.99f32 as f64 - 0.03f32 as f64)).abs() < 1e-9);
        // A position update snaps instead of interpolating.
        e.server = (320, 0, 0);
        e.move_to(0., 0., false);
        assert!((e.pos.x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn ray_hits_closest() {
        let mut es = Entities::default();
        es.map.insert(1, Remote::new(1, Kind::Player, 32 * 3, 0, 0, 0, 0));
        es.map.insert(2, Remote::new(2, Kind::Player, 32 * 2, 0, 0, 0, 0));
        let hit = es.ray(Vec3::new(0., 1., 0.), Vec3::new(1., 0., 0.), 3.0);
        assert_eq!(hit.map(|h| h.0), Some(2));
    }
}
