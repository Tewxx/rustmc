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
}

pub struct Remote {
    pub id: i32,
    pub kind: Kind,
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
    pub on_ground: bool,
    /// Held item (`EntityEquipment` slot 0) as (catalogue index, count).
    pub held: Option<(usize, u8)>,
    pub width: f64,
    pub height: f64,
    pub hurt_time: i32,
    /// Profile name (players), from the tab list.
    pub name: String,
    pub uuid: Option<mc_protocol::Uuid>,
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

pub fn angle(b: i8) -> f32 {
    b as f32 * 360.0 / 256.0
}

impl Remote {
    pub fn new(id: i32, kind: Kind, x: i32, y: i32, z: i32, yaw: i8, pitch: i8) -> Self {
        let pos = Vec3::new(x as f64 / 32.0, y as f64 / 32.0, z as f64 / 32.0);
        let (w, h) = match &kind {
            Kind::Player => (0.6, 1.8),
            Kind::Object(2) => (0.25, 0.25),
            _ => (0.6, 1.8),
        };
        let (yw, pt) = (angle(yaw), angle(pitch));
        Self {
            id,
            kind,
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
            on_ground: false,
            held: None,
            width: w,
            height: h,
            hurt_time: 0,
            name: String::new(),
            uuid: None,
        }
    }

    pub fn bb(&self) -> Aabb {
        let r = self.width / 2.0;
        Aabb::new(self.pos.x - r, self.pos.y, self.pos.z - r, self.pos.x + r, self.pos.y + self.height, self.pos.z + r)
    }

    /// `setPositionAndRotation2(x, y, z, yaw, pitch, 3, false)`
    pub fn move_to(&mut self, yaw: f32, pitch: f32) {
        self.target = Vec3::new(self.server.0 as f64 / 32.0, self.server.1 as f64 / 32.0, self.server.2 as f64 / 32.0);
        self.target_yaw = yaw;
        self.target_pitch = pitch;
        self.increments = 3;
    }

    pub fn swing_arm(&mut self) {
        // EntityLivingBase.swingItem (getArmSwingAnimationEnd = 6 without potions)
        if !self.swinging || self.swing_ticks >= 3 || self.swing_ticks < 0 {
            self.swing_ticks = -1;
            self.swinging = true;
        }
    }

    pub fn tick(&mut self) {
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
        if self.swinging {
            self.swing_ticks += 1;
            if self.swing_ticks >= 6 {
                self.swing_ticks = 0;
                self.swinging = false;
            }
        } else {
            self.swing_ticks = 0;
        }
        self.swing = self.swing_ticks as f32 / 6.0;
        if self.kind != Kind::Player {
            self.body_yaw = self.yaw;
            return;
        }
        // EntityLivingBase.onUpdate: body yaw follows movement, clamped to the head.
        let dx = self.pos.x - self.prev_pos.x;
        let dz = self.pos.z - self.prev_pos.z;
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
        // EntityOtherPlayerMP.onUpdate: limb swing
        self.prev_limb_amount = self.limb_amount;
        let mut f = (dx * dx + dz * dz).sqrt() as f32 * 4.0;
        if f > 1.0 {
            f = 1.0;
        }
        self.limb_amount += (f - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
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

#[derive(Default)]
pub struct Entities {
    pub map: HashMap<i32, Remote>,
}

impl Entities {
    pub fn tick(&mut self) {
        for e in self.map.values_mut() {
            e.tick();
        }
    }

    /// `EntityRenderer.getMouseOver`'s entity pass: the closest entity whose box (grown by
    /// `getCollisionBorderSize` 0.1) the look ray hits within `reach`.
    pub fn ray(&self, eye: Vec3, look: Vec3, reach: f64) -> Option<(i32, f64)> {
        let end = Vec3::new(eye.x + look.x * reach, eye.y + look.y * reach, eye.z + look.z * reach);
        let mut best: Option<(i32, f64)> = None;
        for e in self.map.values() {
            if matches!(e.kind, Kind::Object(_)) {
                continue;
            }
            let bb = e.bb().expand(0.1, 0.1, 0.1);
            let d = if bb.is_vec_inside(eye) {
                0.0
            } else if let Some((hit, _)) = bb.calculate_intercept(eye, end) {
                ((hit.x - eye.x).powi(2) + (hit.y - eye.y).powi(2) + (hit.z - eye.z).powi(2)).sqrt()
            } else {
                continue;
            };
            if d <= reach && best.is_none_or(|(_, b)| d < b) {
                best = Some((e.id, d));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_over_three_ticks() {
        let mut e = Remote::new(1, Kind::Player, 0, 0, 0, 0, 0);
        e.server = (96, 0, 0);
        e.move_to(0., 0.);
        e.tick();
        assert!((e.pos.x - 1.0).abs() < 1e-9);
        e.tick();
        e.tick();
        assert!((e.pos.x - 3.0).abs() < 1e-9);
        assert!(e.limb_amount > 0.);
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
