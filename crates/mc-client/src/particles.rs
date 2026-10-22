//! `EffectRenderer` + `EntityFX`: the client particle simulation behind `S2APacketParticles`.
//!
//! Positions and motion are doubles like Java. Each type keeps the constructor constants and
//! `onUpdate` of its `net.minecraft.client.particle` class; rendering reproduces
//! `EntityFX.renderParticle`'s camera basis and UV layout. Types not listed in [`Kind`] are
//! not ported yet and are dropped instead of being approximated.
use crate::player::PhysWorld;
use glam::{DVec3, Vec3};
use mc_core::random::JavaRandom;
use mc_data::Material;
use mc_entity::world::colliding_boxes;
use mc_core::Aabb;
use mc_entity::EntityWorld;
use mc_world::World;
use std::collections::HashMap;

/// Source texture of a particle quad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tex {
    /// `particles.png`, indexed by 16×16 cells.
    Misc,
    /// `textures/entity/explosion.png`, a 4×4 animation sheet.
    Explosion,
    /// The block/item atlas with a stored sprite rectangle.
    Atlas,
}

/// `getBrightnessForRender` override.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bright {
    /// Terrain light at the particle position.
    Normal,
    /// `lightmap(240, 240)`: fireworks sparks.
    Full,
    /// `lightmap(0, 240)`: large explosion flashes.
    Block15,
}

/// The behaviours that differ from the `EntityFX` base update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Explosion,
    LargeExplosion,
    HugeExplosion,
    Firework,
    Bubble,
    Splash,
    Wake,
    Suspended,
    Aura,
    Rain,
    Crit { magic: bool },
    Smoke { large: bool },
    Spell { base: i32 },
    Reddust,
    Heart,
    Note,
    Portal,
    Enchant,
    Flame,
    Lava,
    Cloud,
    SnowShovel,
    Breaking,
    Digging { dust: bool },
    Barrier,
    Drop { water: bool },
    /// Placeholder for types that are not ported (never spawned).
    Unsupported,
}

pub struct Particle {
    pos: DVec3,
    prev: DVec3,
    motion: DVec3,
    age: i32,
    max_age: i32,
    /// Current scale at spawn; behaviour code may shrink it over time.
    scale: f32,
    /// `particleScale` before the type's own multipliers (flame, lava, smoke, …).
    base_scale: f32,
    gravity: f32,
    color: [f32; 3],
    alpha: f32,
    tex_x: i32,
    tex_y: i32,
    jitter: (f32, f32),
    size: f32,
    no_clip: bool,
    kind: Kind,
    anchor: DVec3,
    /// Sprite rectangle in atlas space for [`Tex::Atlas`].
    rect: Option<[f32; 4]>,
    on_ground: bool,
    dead: bool,
    /// `EntityDropParticleFX.bobTimer`.
    bob: i32,
}

/// `WorldClient.effectRenderer`'s particle list.
#[derive(Default)]
/// `EntityParticleEmitter`: a short-lived emitter that follows an entity (critical-hit bursts).
struct Emitter {
    entity: i32,
    particle: i32,
    age: i32,
}

#[derive(Default)]
pub struct Particles {
    list: Vec<Particle>,
    emitters: Vec<Emitter>,
}

impl Particles {
    pub fn clear(&mut self) {
        self.list.clear();
        self.emitters.clear();
        SPAWNS.with(|s| s.borrow_mut().clear());
    }

    /// `EffectRenderer.emitParticleAtEntity`: spawns the constructor burst immediately and
    /// keeps emitting for the remaining two ticks of `EntityParticleEmitter.lifetime`.
    pub fn emit_at_entity(&mut self, id: i32, pos: mc_core::Vec3, width: f64, height: f64, particle: i32, rand: &mut JavaRandom, models: &mc_render::models::BlockModels) {
        let e = Emitter { entity: id, particle, age: 1 };
        self.emitter_burst(&e, pos, width, height, rand, models);
        self.emitters.push(e);
    }

    fn emitter_burst(&mut self, e: &Emitter, pos: mc_core::Vec3, width: f64, height: f64, rand: &mut JavaRandom, models: &mc_render::models::BlockModels) {
        for _ in 0..16 {
            let d0 = (rand.next_float() * 2.0 - 1.0) as f64;
            let d1 = (rand.next_float() * 2.0 - 1.0) as f64;
            let d2 = (rand.next_float() * 2.0 - 1.0) as f64;
            if d0 * d0 + d1 * d1 + d2 * d2 <= 1.0 {
                let x = pos.x + d0 * width / 4.0;
                let y = pos.y + height / 2.0 + d1 * height / 4.0;
                let z = pos.z + d2 * width / 4.0;
                self.spawn(e.particle, x, y, z, d0, d1 + 0.2, d2, &[], rand, models);
            }
        }
    }

    #[allow(dead_code)]
    pub fn emitter_count(&self) -> usize {
        self.emitters.len()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// `EffectRenderer.spawnEffectParticle`: one packet particle. `args` are the packet's
    /// trailing ints (redstone colour, block/item ids for the deferred types).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        id: i32,
        x: f64,
        y: f64,
        z: f64,
        vx: f64,
        vy: f64,
        vz: f64,
        args: &[i32],
        rand: &mut JavaRandom,
        models: &mc_render::models::BlockModels,
    ) {
        let _ = args;
        let p = match id {
            0 => explosion(x, y, z, vx, vy, vz, rand),
            1 => large_explosion(x, y, z, vx, rand),
            2 => huge_explosion(x, y, z, rand),
            3 => firework(x, y, z, vx, vy, vz, rand),
            4 => bubble(x, y, z, vx, vy, vz, rand),
            5 => splash(x, y, z, vx, vy, vz, rand),
            6 => wake(x, y, z, vx, vy, vz, rand),
            7 => suspended(x, y, z, vx, vy, vz, rand),
            8 => suspended_depth(x, y, z, vx, vy, vz, rand),
            9 => crit(x, y, z, vx, vy, vz, rand, false),
            10 => crit(x, y, z, vx, vy, vz, rand, true),
            11 => smoke(x, y, z, vx, vy, vz, rand, 1.0),
            12 => smoke(x, y, z, vx, vy, vz, rand, 2.5),
            13 => spell(x, y, z, vx, vy, vz, rand, 128, None),
            14 => spell(x, y, z, vx, vy, vz, rand, 144, None),
            15 => spell(x, y, z, vx, vy, vz, rand, 128, Some([vx as f32, vy as f32, vz as f32])),
            16 => {
                let mut p = spell(x, y, z, vx, vy, vz, rand, 128, Some([vx as f32, vy as f32, vz as f32]));
                p.alpha = 0.15;
                p
            }
            17 => {
                let f = rand.next_float() * 0.5 + 0.35;
                spell(x, y, z, vx, vy, vz, rand, 144, Some([f, 0., f]))
            }
            18 => drop(x, y, z, rand, true),
            19 => drop(x, y, z, rand, false),
            20 => heart(x, y + 0.5, z, rand, true),
            21 => aura(x, y, z, vx, vy, vz, rand, true),
            22 => aura(x, y, z, vx, vy, vz, rand, false),
            23 => note(x, y, z, vx, rand),
            24 => portal(x, y, z, vx, vy, vz, rand),
            25 => enchant(x, y, z, vx, vy, vz, rand),
            26 => flame(x, y, z, vx, vy, vz, rand),
            27 => lava(x, y, z, rand),
            29 => cloud(x, y, z, vx, vy, vz, rand),
            30 => reddust(x, y, z, vx as f32, vy as f32, vz as f32, rand),
            31 => breaking(x, y, z, rand, models, "items/snowball"),
            32 => snow_shovel(x, y, z, vx, vy, vz, rand),
            33 => breaking(x, y, z, rand, models, "items/slimeball"),
            34 => heart(x, y, z, rand, false),
            35 => barrier(x, y, z, rand, models),
            37 => {
                let Some(state) = args.first().copied().and_then(mc_data::StateId::try_from_network) else { return };
                let Some(p) = block_crack(x, y, z, vx, vy, vz, rand, models, state, false) else { return };
                p
            }
            38 => {
                let Some(state) = args.first().copied().and_then(mc_data::StateId::try_from_network) else { return };
                let Some(p) = block_crack(x, y, z, vx, vy, vz, rand, models, state, true) else { return };
                p
            }
            39 => rain(x, y, z, rand),
            // Deferred, see `Particle` docs: 28 footstep, 36 item crack, 40 item take (no
            // factory in 1.8), 41 mob appearance.
            _ => return,
        };
        if p.kind != Kind::Unsupported {
            self.list.push(p);
        }
    }

    /// `EffectRenderer.updateEffects` (one client tick).
    pub fn tick(&mut self, world: &World, geometry: &mut mc_data::geometry::BlockGeometry, rand: &mut JavaRandom, player: Vec3, remotes: &HashMap<i32, crate::entities::Remote>, models: &mc_render::models::BlockModels) {
        let mut w = PhysWorld { world, geometry };
        let mut next: Vec<Particle> = Vec::with_capacity(self.list.len() + 8);
        for mut p in std::mem::take(&mut self.list) {
            p.tick(&mut w, rand, player);
            if !p.dead {
                next.push(p);
            }
        }
        self.list = next;
        self.take_spawns();
        // `EffectRenderer.updateEffects`: `EntityParticleEmitter`s follow their entity for 3 ticks.
        let mut emitters = Vec::new();
        for mut e in std::mem::take(&mut self.emitters) {
            if let Some(r) = remotes.get(&e.entity) {
                self.emitter_burst(&e, r.pos, r.width, r.height, rand, models);
            }
            e.age += 1;
            if e.age < 3 {
                emitters.push(e);
            }
        }
        self.emitters = emitters;
    }

    /// Builds the camera-facing quads for one frame. Positions are world-space; `light`
    /// maps a packed sky/block pair to an RGB colour.
    pub fn quads(&mut self, cam: Vec3, yaw: f32, pitch: f32, pt: f32, world: &World, light: impl Fn(u8, u8) -> [f32; 3]) -> Vec<mc_render::avatar::ParticleQuad> {
        let (sy, cy) = (mc_core::math::sin(yaw.to_radians()), mc_core::math::cos(yaw.to_radians()));
        let (sp, cp) = (mc_core::math::sin(pitch.to_radians()), mc_core::math::cos(pitch.to_radians()));
        let right = Vec3::new(cy, 0., sy);
        let up = Vec3::new(-sy * sp, cp, cy * sp);
        let mut out: Vec<mc_render::avatar::ParticleQuad> = Vec::with_capacity(self.list.len());
        for p in &mut self.list {
            let Some((tex, uv, color, half)) = p.render_data(pt) else { continue };
            let c = p.interpolated(pt);
            let center = Vec3::new(c.x as f32, c.y as f32, c.z as f32);
            let h = Vec3::splat(half);
            let pos = [center - (right + up) * h, center - right * h + up * h, center + (right + up) * h, center + right * h - up * h];
            let (sky, block) = world.light(c.x.floor() as i32, c.y.floor() as i32, c.z.floor() as i32);
            let lm = match p.bright() {
                Bright::Full => light(15, 15),
                Bright::Block15 => light(0, 15),
                Bright::Normal => {
                    let boost = p.light_boost();
                    light(sky, block.saturating_add(boost).min(15))
                }
            };
            let tex = match tex {
                Tex::Misc => mc_render::avatar::ParticleTex::Misc,
                Tex::Explosion => mc_render::avatar::ParticleTex::Explosion,
                Tex::Atlas => mc_render::avatar::ParticleTex::Block,
            };
            out.push(mc_render::avatar::ParticleQuad { pos, uv, color, light: lm, tex });
        }
        // Vanilla draws without sorting; alpha-blended quads read best back to front and this
        // pass shares the opaque depth write (see `AvatarRenderer::encode`).
        out.sort_by(|a, b| {
            let da = a.pos[0].distance_squared(cam);
            let db = b.pos[0].distance_squared(cam);
            db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }
}

impl Particle {
    fn bright(&self) -> Bright {
        match self.kind {
            Kind::LargeExplosion | Kind::Firework => Bright::Full,
            Kind::Lava | Kind::Drop { water: false } => Bright::Block15,
            _ => Bright::Normal,
        }
    }

    /// The `getBrightnessForRender` block-light bump of flame, portal and enchantment particles.
    fn light_boost(&self) -> u8 {
        let t = (self.age as f32 / self.max_age.max(1) as f32).clamp(0.0, 1.0);
        match self.kind {
            Kind::Flame => (t * 15.0) as u8,
            Kind::Portal | Kind::Enchant => ((t * t * t * t) * 15.0) as u8,
            _ => 0,
        }
    }

    fn interpolated(&self, pt: f32) -> DVec3 {
        self.prev + (self.pos - self.prev) * pt as f64
    }

    fn bb(&self) -> Aabb {
        let h = self.size as f64 * 0.5;
        Aabb::new(self.pos.x - h, self.pos.y, self.pos.z - h, self.pos.x + h, self.pos.y + self.size as f64, self.pos.z + h)
    }

    fn set_bb_center(&mut self, bb: &Aabb) {
        self.pos.x = (bb.min_x + bb.max_x) * 0.5;
        self.pos.y = bb.min_y;
        self.pos.z = (bb.min_z + bb.max_z) * 0.5;
    }

    /// `Entity.moveEntity` without step assist (`EntityFX` has no step height).
    fn move_entity(&mut self, w: &mut impl EntityWorld, x: f64, y: f64, z: f64) {
        if self.no_clip {
            self.pos += DVec3::new(x, y, z);
            return;
        }
        let bb = self.bb();
        let mut y = y;
        for b in colliding_boxes(w, &bb.offset(0., y, 0.)) {
            y = b.calculate_y_offset(&bb, y);
        }
        let bb = bb.offset(0., y, 0.);
        let mut x = x;
        for b in colliding_boxes(w, &bb.offset(x, 0., 0.)) {
            x = b.calculate_x_offset(&bb, x);
        }
        let bb = bb.offset(x, 0., 0.);
        let mut z = z;
        for b in colliding_boxes(w, &bb.offset(0., 0., z)) {
            z = b.calculate_z_offset(&bb, z);
        }
        let bb = bb.offset(0., 0., z);
        self.on_ground = y != 0. && y < 0.;
        self.set_bb_center(&bb);
    }

    fn tick(&mut self, w: &mut PhysWorld, rand: &mut JavaRandom, player: Vec3) {
        self.prev = self.pos;
        let mut base = |p: &mut Particle| {
            if p.age >= p.max_age {
                p.dead = true;
            }
            p.age += 1;
            p.motion.y -= 0.04 * p.gravity as f64;
            p.move_entity(w, p.motion.x, p.motion.y, p.motion.z);
            p.motion *= 0.9800000190734863;
            if p.on_ground {
                p.motion.x *= 0.699999988079071;
                p.motion.z *= 0.699999988079071;
            }
        };
        match self.kind {
            Kind::Explosion => {
                base(self);
                self.set_index(7 - self.age * 8 / self.max_age.max(1));
                self.motion.y += 0.004;
                self.motion *= 0.8999999761581421;
            }
            Kind::LargeExplosion => {
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::HugeExplosion => {
                for _ in 0..6 {
                    let p = large_explosion(
                        self.pos.x + (rand.next_double() - rand.next_double()) * 4.0,
                        self.pos.y + (rand.next_double() - rand.next_double()) * 4.0,
                        self.pos.z + (rand.next_double() - rand.next_double()) * 4.0,
                        self.age as f64 / self.max_age.max(1) as f64,
                        rand,
                    );
                    self.spawns_hack(p);
                }
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Firework => {
                if self.age >= self.max_age {
                    self.dead = true;
                }
                self.age += 1;
                if self.age > self.max_age / 2 {
                    self.alpha = 1.0 - (self.age - self.max_age / 2) as f32 / self.max_age as f32;
                }
                self.set_index(160 + (7 - self.age * 8 / self.max_age.max(1)));
                self.motion.y -= 0.004;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9100000262260437;
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
            }
            Kind::Bubble => {
                self.motion.y += 0.002;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.8500000238418579;
                let m = w.state(mc_core::BlockPos::new(self.pos.x.floor() as i32, self.pos.y.floor() as i32, self.pos.z.floor() as i32)).block().material;
                if m != Material::Water {
                    self.dead = true;
                }
                self.max_age -= 1;
                if self.max_age <= 0 {
                    self.dead = true;
                }
            }
            Kind::Suspended => {
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                let m = w.state(mc_core::BlockPos::new(self.pos.x.floor() as i32, self.pos.y.floor() as i32, self.pos.z.floor() as i32)).block().material;
                if m != Material::Water {
                    self.dead = true;
                }
                self.max_age -= 1;
                if self.max_age <= 0 {
                    self.dead = true;
                }
            }
            Kind::Wake => {
                self.motion.y -= self.gravity as f64;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9800000190734863;
                let i = 60 - self.max_age;
                let f = i as f32 * 0.001;
                self.size = f;
                self.set_index(19 + i % 4);
                self.max_age -= 1;
                if self.max_age <= 0 {
                    self.dead = true;
                }
            }
            Kind::Aura => {
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.99;
                self.max_age -= 1;
                if self.max_age <= 0 {
                    self.dead = true;
                }
            }
            Kind::Crit { .. } => {
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.color[1] *= 0.96;
                self.color[2] *= 0.9;
                self.motion *= 0.699999988079071;
                self.motion.y -= 0.019999999552965164;
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Smoke { .. } | Kind::Spell { .. } | Kind::Reddust => {
                self.set_index(match self.kind {
                    Kind::Spell { base } => base + (7 - self.age * 8 / self.max_age.max(1)),
                    _ => 7 - self.age * 8 / self.max_age.max(1),
                });
                if matches!(self.kind, Kind::Smoke { .. } | Kind::Spell { .. }) {
                    self.motion.y += 0.004;
                }
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                if self.pos.y == self.prev.y {
                    self.motion.x *= 1.1;
                    self.motion.z *= 1.1;
                }
                self.motion *= 0.9599999785423279;
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Flame => {
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9599999785423279;
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Lava => {
                if self.age >= self.max_age {
                    self.dead = true;
                }
                self.age += 1;
                let f = self.age as f32 / self.max_age.max(1) as f32;
                if rand.next_float() > f {
                    let p = smoke(self.pos.x, self.pos.y, self.pos.z, self.motion.x, self.motion.y, self.motion.z, rand, 1.0);
                    self.spawns_hack(p);
                }
                self.motion.y -= 0.03;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9990000128746033;
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
            }
            Kind::Heart | Kind::Note => {
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                if self.pos.y == self.prev.y {
                    self.motion.x *= 1.1;
                    self.motion.z *= 1.1;
                }
                self.motion *= if matches!(self.kind, Kind::Heart) { 0.8600000143051147 } else { 0.6600000262260437 };
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Portal => {
                let f = self.age as f32 / self.max_age.max(1) as f32;
                let f = -f + f * f * 2.0;
                let f = 1.0 - f;
                self.pos = self.anchor + self.motion * f as f64 + DVec3::new(0., (1.0 - f) as f64, 0.);
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Enchant => {
                let f = self.age as f32 / self.max_age.max(1) as f32;
                let f = 1.0 - f;
                let f1 = 1.0 - f;
                let f1 = f1 * f1;
                let f1 = f1 * f1;
                self.pos = self.anchor + self.motion * f as f64 - DVec3::new(0., (f1 * 1.2) as f64, 0.);
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Cloud => {
                if self.age >= self.max_age {
                    self.dead = true;
                }
                self.age += 1;
                self.set_index(7 - self.age * 8 / self.max_age.max(1));
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9599999785423279;
                // `getClosestPlayerToEntity(this, 2.0)`: only the local player is available here.
                let dx = self.pos.x - player.x as f64;
                let dz = self.pos.z - player.z as f64;
                if dx * dx + dz * dz < 4.0 && self.pos.y > player.y as f64 {
                    self.pos.y += (player.y as f64 - self.pos.y) * 0.2;
                }
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
            }
            Kind::SnowShovel => {
                self.set_index(7 - self.age * 8 / self.max_age.max(1));
                self.motion.y -= 0.03;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9900000095367432;
                if self.on_ground {
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                self.age += 1;
                if self.age >= self.max_age {
                    self.dead = true;
                }
            }
            Kind::Breaking | Kind::Digging { .. } | Kind::Barrier => base(self),
            Kind::Drop { water } => {
                if water {
                    self.color = [0.2, 0.3, 1.0];
                } else {
                    self.color = [1.0, 16.0 / (40 - self.bob + 16) as f32, 4.0 / (40 - self.bob + 8) as f32];
                }
                self.motion.y -= self.gravity as f64;
                if self.bob > 0 {
                    self.motion *= 0.02;
                    self.set_index(113);
                } else {
                    self.set_index(112);
                }
                self.bob -= 1;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9800000190734863;
                self.max_age -= 1;
                if self.max_age <= 0 {
                    self.dead = true;
                }
                if self.on_ground {
                    if water {
                        self.dead = true;
                        let p = splash(self.pos.x, self.pos.y, self.pos.z, 0., 0., 0., rand);
                        self.spawns_hack(p);
                    } else {
                        self.set_index(114);
                    }
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                let below = w.state(mc_core::BlockPos::new(self.pos.x.floor() as i32, (self.pos.y - 1.0).floor() as i32, self.pos.z.floor() as i32)).block();
                if below.material.props().liquid || below.material.props().solid {
                    let top = (self.pos.y - 1.0).floor() + below.default_bounds[4] as f64;
                    if self.pos.y < top {
                        self.dead = true;
                    }
                }
            }
            Kind::Rain => {
                self.motion.y -= self.gravity as f64;
                self.move_entity(w, self.motion.x, self.motion.y, self.motion.z);
                self.motion *= 0.9800000190734863;
                self.max_age -= 1;
                if self.max_age <= 0 {
                    self.dead = true;
                }
                if self.on_ground {
                    if rand.next_double() < 0.5 {
                        self.dead = true;
                    }
                    self.motion.x *= 0.699999988079071;
                    self.motion.z *= 0.699999988079071;
                }
                let pos = mc_core::BlockPos::new(self.pos.x.floor() as i32, self.pos.y.floor() as i32, self.pos.z.floor() as i32);
                let b = w.state(pos).block();
                if b.material.props().liquid || b.material.props().solid {
                    let top = pos.y as f64 + b.default_bounds[4] as f64;
                    if self.pos.y < top {
                        self.dead = true;
                    }
                }
            }
            Kind::Splash => base(self),
            Kind::Unsupported => self.dead = true,
        }
    }

    /// Deferred spawns are appended by the tick, which cannot borrow the manager list.
    fn spawns_hack(&mut self, p: Particle) {
        SPAWNS.with(|s| s.borrow_mut().push(p));
    }

    fn set_index(&mut self, index: i32) {
        self.tex_x = index % 16;
        self.tex_y = index / 16;
    }

    /// `EntityFX.renderParticle`'s UVs, colour, half-size and texture source. Mutable because
    /// flame/lava/portal shrink `particleScale` in render, like the Java classes.
    fn render_data(&mut self, pt: f32) -> Option<(Tex, [[f32; 2]; 4], [f32; 4], f32)> {
        let t = ((self.age as f32 + pt) / self.max_age.max(1) as f32).clamp(0.0, 1.0);
        let (u0, v0, u1, v1);
        let mut color = [self.color[0], self.color[1], self.color[2], self.alpha];
        let half;
        let mut tex = Tex::Misc;
        match self.kind {
            Kind::LargeExplosion => {
                let i = ((self.age as f32 + pt) * 15.0 / self.max_age.max(1) as f32) as i32;
                if i > 15 {
                    return None;
                }
                let f = (i % 4) as f32 / 4.0;
                let f1 = f + 0.24975;
                let f2 = (i / 4) as f32 / 4.0;
                let f3 = f2 + 0.24975;
                (u0, v0, u1, v1) = (f, f2, f1, f3);
                half = 2.0 * (self.base_scale * (1.0 - self.anchor.x as f32 * 0.5));
                tex = Tex::Explosion;
                color = [self.color[0], self.color[1], self.color[2], 1.0];
            }
            Kind::HugeExplosion => return None,
            _ => {
                match self.kind {
                    Kind::Flame => self.scale = self.base_scale * (1.0 - t * t * 0.5),
                    Kind::Lava => self.scale = self.base_scale * (1.0 - t * t),
                    Kind::Portal => {
                        let f = 1.0 - t;
                        self.scale = self.base_scale * (1.0 - f * f);
                    }
                    Kind::Smoke { .. } | Kind::Spell { .. } | Kind::Reddust | Kind::Cloud | Kind::SnowShovel | Kind::Heart | Kind::Note | Kind::Crit { .. } => {
                        self.scale = self.base_scale * (t * 32.0).clamp(0.0, 1.0);
                    }
                    _ => {}
                }
                half = if self.kind == Kind::Barrier { 0.5 } else { 0.1 * self.scale };
                if let Some(r) = self.rect {
                    let jx = self.jitter.0 / 4.0;
                    let jy = self.jitter.1 / 4.0;
                    if self.kind == Kind::Barrier {
                        (u0, v0, u1, v1) = (r[0], r[1], r[2], r[3]);
                    } else {
                        (u0, v0, u1, v1) = (
                            r[0] + jx * (r[2] - r[0]),
                            r[1] + jy * (r[3] - r[1]),
                            r[0] + (jx + 0.25) * (r[2] - r[0]),
                            r[1] + (jy + 0.25) * (r[3] - r[1]),
                        );
                    }
                    tex = Tex::Atlas;
                } else {
                    u0 = self.tex_x as f32 / 16.0;
                    v0 = self.tex_y as f32 / 16.0;
                    u1 = u0 + 0.0624375;
                    v1 = v0 + 0.0624375;
                }
            }
        }
        Some((tex, [[u1, v1], [u1, v0], [u0, v0], [u0, v1]], color, half))
    }
}

thread_local! {
    static SPAWNS: std::cell::RefCell<Vec<Particle>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Drains particles spawned by running particle updates (`HugeExplosion`, lava, trails).
impl Particles {
    fn take_spawns(&mut self) {
        SPAWNS.with(|s| {
            let mut b = s.borrow_mut();
            if !b.is_empty() {
                self.list.append(&mut b);
            }
        });
    }
}

// ---------------------------------------------------------------- constructors

fn base(rand: &mut JavaRandom, x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64) -> Particle {
    let mut p = base_at(rand, x, y, z);
    let mut mx = vx + (rand.next_float() as f64 * 2.0 - 1.0) * 0.4000000059604645;
    let mut my = vy + (rand.next_float() as f64 * 2.0 - 1.0) * 0.4000000059604645;
    let mut mz = vz + (rand.next_float() as f64 * 2.0 - 1.0) * 0.4000000059604645;
    let f = (rand.next_float() + rand.next_float() + 1.0) as f64 * 0.15;
    let f1 = (mx * mx + my * my + mz * mz).sqrt();
    mx = mx / f1 * f * 0.4000000059604645;
    my = my / f1 * f * 0.4000000059604645 + 0.10000000149011612;
    mz = mz / f1 * f * 0.4000000059604645;
    p.motion = DVec3::new(mx, my, mz);
    p
}

fn base_at(rand: &mut JavaRandom, x: f64, y: f64, z: f64) -> Particle {
    // `EntityFX`'s constructor order: jitterX, jitterY, scale, maxAge.
    let jitter = (rand.next_float() * 3.0, rand.next_float() * 3.0);
    let scale = (rand.next_float() * 0.5 + 0.5) * 2.0;
    let max_age = (4.0 / (rand.next_float() * 0.9 + 0.1)) as i32;
    Particle {
        pos: DVec3::new(x, y, z),
        prev: DVec3::new(x, y, z),
        motion: DVec3::ZERO,
        age: 0,
        max_age,
        scale,
        base_scale: scale,
        gravity: 0.0,
        color: [1.0, 1.0, 1.0],
        alpha: 1.0,
        tex_x: 0,
        tex_y: 0,
        jitter,
        size: 0.2,
        no_clip: false,
        kind: Kind::Unsupported,
        anchor: DVec3::new(x, y, z),
        rect: None,
        on_ground: false,
        dead: false,
        bob: 0,
    }
}

fn explosion(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    p.motion = DVec3::new(
        vx + (rand.next_double() * 2.0 - 1.0) * 0.05,
        vy + (rand.next_double() * 2.0 - 1.0) * 0.05,
        vz + (rand.next_double() * 2.0 - 1.0) * 0.05,
    );
    let f = rand.next_float() * 0.3 + 0.7;
    p.color = [f, f, f];
    p.scale = rand.next_float() * rand.next_float() * 6.0 + 1.0;
    p.base_scale = p.scale;
    p.max_age = (16.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32 + 2;
    p.kind = Kind::Explosion;
    p
}

fn large_explosion(x: f64, y: f64, z: f64, arg: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base_at(rand, x, y, z);
    p.max_age = 6 + rand.next_int_bounded(4);
    let f = rand.next_float() * 0.6 + 0.4;
    p.color = [f, f, f];
    p.base_scale = 1.0;
    p.anchor = DVec3::new(arg, 0., 0.);
    p.kind = Kind::LargeExplosion;
    p
}

fn huge_explosion(x: f64, y: f64, z: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base_at(rand, x, y, z);
    p.kind = Kind::HugeExplosion;
    p.max_age = 8;
    p
}

fn firework(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base_at(rand, x, y, z);
    p.motion = DVec3::new(vx, vy, vz);
    p.scale *= 0.75;
    p.base_scale = p.scale;
    p.max_age = 48 + rand.next_int_bounded(12);
    p.alpha = 0.99;
    p.no_clip = false;
    p.kind = Kind::Firework;
    p
}

fn bubble(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    p.color = [1., 1., 1.];
    p.set_index(32);
    p.size = 0.02;
    p.scale *= rand.next_float() * 0.6 + 0.2;
    p.base_scale = p.scale;
    p.motion = DVec3::new(
        vx * 0.2 + (rand.next_float() as f64 * 2.0 - 1.0) * 0.02,
        vy * 0.2 + (rand.next_float() as f64 * 2.0 - 1.0) * 0.02,
        vz * 0.2 + (rand.next_float() as f64 * 2.0 - 1.0) * 0.02,
    );
    p.max_age = (8.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.kind = Kind::Bubble;
    p
}

fn splash(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base_at(rand, x, y, z);
    p.gravity = 0.04;
    p.tex_x += 1;
    if vy == 0.0 && (vx != 0.0 || vz != 0.0) {
        p.motion = DVec3::new(vx, vy + 0.1, vz);
    }
    p.kind = Kind::Splash;
    p
}

fn wake(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.3;
    p.motion.y = rand.next_float() as f64 * 0.2 + 0.1;
    p.color = [1., 1., 1.];
    p.set_index(19);
    p.size = 0.01;
    p.max_age = (8.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.gravity = 0.0;
    p.motion = DVec3::new(vx, vy, vz);
    p.kind = Kind::Wake;
    p
}

fn suspended(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y - 0.125, z, vx, vy, vz);
    p.color = [0.4, 0.4, 0.7];
    p.set_index(0);
    p.size = 0.01;
    p.scale *= rand.next_float() * 0.6 + 0.2;
    p.base_scale = p.scale;
    p.motion = DVec3::ZERO;
    p.max_age = (16.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.kind = Kind::Suspended;
    p
}

fn suspended_depth(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = aura(x, y, z, vx, vy, vz, rand, false);
    p.max_age = (8.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p
}

fn crit(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom, magic: bool) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.1;
    p.motion += DVec3::new(vx, vy, vz) * 0.4;
    let f = rand.next_float() * 0.3 + 0.6;
    p.color = [f, f, f];
    p.scale *= 0.75;
    p.base_scale = p.scale;
    p.max_age = (6.0 / (rand.next_float() as f64 * 0.8 + 0.6)) as i32;
    p.no_clip = false;
    p.set_index(65);
    // `EntityCrit2FX` calls onUpdate() once at the end of its constructor (age 1, first move).
    p.age = 1;
    p.pos += p.motion;
    p.tick_crit_ctor();
    if magic {
        p.color = [p.color[0] * 0.3, p.color[1] * 0.8, p.color[2]];
        p.tex_x += 1;
    }
    p.kind = Kind::Crit { magic };
    p
}

impl Particle {
    fn tick_crit_ctor(&mut self) {
        self.color[1] *= 0.96;
        self.color[2] *= 0.9;
        self.motion *= 0.699999988079071;
        self.motion.y -= 0.019999999552965164;
    }
}

fn smoke(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom, m: f32) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.1;
    p.motion += DVec3::new(vx, vy, vz);
    let f = rand.next_float() * 0.3;
    p.color = [f, f, f];
    p.scale *= 0.75;
    p.scale *= m;
    p.base_scale = p.scale;
    p.max_age = (8.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.max_age = (p.max_age as f32 * m) as i32;
    p.no_clip = false;
    p.kind = Kind::Smoke { large: m > 1.0 };
    p
}

fn spell(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom, base_index: i32, tint: Option<[f32; 3]>) -> Particle {
    let sx = 0.5 - rand.next_double();
    let sz = 0.5 - rand.next_double();
    let mut p = base(rand, x, y, z, sx, vy, sz);
    p.motion.y *= 0.2;
    if vx == 0.0 && vz == 0.0 {
        p.motion.x *= 0.1;
        p.motion.z *= 0.1;
    }
    p.scale *= 0.75;
    p.base_scale = p.scale;
    p.max_age = (8.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.no_clip = false;
    if let Some(c) = tint {
        p.color = c;
    }
    p.kind = Kind::Spell { base: base_index };
    p
}

fn reddust(x: f64, y: f64, z: f64, r: f32, g: f32, b: f32, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.1;
    let g = if g == 0.0 { 1.0 } else { g };
    let f = rand.next_float() * 0.4 + 0.6;
    p.color = [
        (rand.next_float() * 0.2 + 0.8) * r * f,
        (rand.next_float() * 0.2 + 0.8) * g * f,
        (rand.next_float() * 0.2 + 0.8) * b * f,
    ];
    p.scale *= 0.75;
    p.base_scale = p.scale;
    p.max_age = (8.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.no_clip = false;
    p.kind = Kind::Reddust;
    p
}

fn heart(x: f64, y: f64, z: f64, rand: &mut JavaRandom, angry: bool) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.01;
    p.motion.y += 0.1;
    p.scale *= 0.75;
    p.scale *= 2.0;
    p.base_scale = p.scale;
    p.max_age = 16;
    p.no_clip = false;
    p.set_index(if angry { 81 } else { 80 });
    if angry {
        p.color = [1., 1., 1.];
    }
    p.kind = Kind::Heart;
    p
}

fn aura(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom, happy: bool) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    let f = rand.next_float() * 0.1 + 0.2;
    p.color = if happy { [1., 1., 1.] } else { [f, f, f] };
    p.set_index(if happy { 82 } else { 0 });
    p.size = 0.02;
    p.scale *= rand.next_float() * 0.6 + 0.5;
    p.base_scale = p.scale;
    p.motion *= 0.02;
    p.max_age = (20.0 / (rand.next_float() as f64 * 0.8 + 0.2)) as i32;
    p.no_clip = true;
    p.kind = Kind::Aura;
    p
}

fn note(x: f64, y: f64, z: f64, vx: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.01;
    p.motion.y += 0.2;
    let f = vx as f32;
    p.color = [
        mc_core::math::sin((f + 0.0) * std::f32::consts::PI * 2.0) * 0.65 + 0.35,
        mc_core::math::sin((f + 0.33333334) * std::f32::consts::PI * 2.0) * 0.65 + 0.35,
        mc_core::math::sin((f + 0.6666667) * std::f32::consts::PI * 2.0) * 0.65 + 0.35,
    ];
    p.scale *= 0.75;
    p.scale *= 2.0;
    p.base_scale = p.scale;
    p.max_age = 6;
    p.no_clip = false;
    p.set_index(64);
    p.kind = Kind::Note;
    p
}

fn portal(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    p.motion = DVec3::new(vx, vy, vz);
    p.anchor = DVec3::new(x, y, z);
    p.pos = p.anchor;
    let f = rand.next_float() * 0.6 + 0.4;
    p.scale = rand.next_float() * 0.2 + 0.5;
    p.base_scale = p.scale;
    p.color = [f * 0.9, f * 0.3, f];
    p.max_age = (rand.next_double() * 10.0) as i32 + 40;
    p.no_clip = true;
    p.set_index((rand.next_double() * 8.0) as i32);
    p.kind = Kind::Portal;
    p
}

fn enchant(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    p.motion = DVec3::new(vx, vy, vz);
    p.anchor = DVec3::new(x, y, z);
    p.pos = p.anchor + p.motion;
    p.prev = p.pos;
    let f = rand.next_float() * 0.6 + 0.4;
    p.scale = rand.next_float() * 0.5 + 0.2;
    p.base_scale = p.scale;
    p.color = [f * 0.9, f * 0.9, f];
    p.max_age = (rand.next_double() * 10.0) as i32 + 30;
    p.no_clip = true;
    p.set_index((rand.next_double() * 26.0) as i32 + 1 + 224);
    p.kind = Kind::Enchant;
    p
}

fn flame(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    p.motion = p.motion * 0.01 + DVec3::new(vx, vy, vz);
    let d = (rand.next_float() - rand.next_float()) as f64 * 0.05;
    p.pos += DVec3::new(d, d, d);
    p.base_scale = p.scale;
    p.color = [1., 1., 1.];
    p.max_age = (8.0 / (rand.next_double() * 0.8 + 0.2)) as i32 + 4;
    p.no_clip = true;
    p.set_index(48);
    p.kind = Kind::Flame;
    p
}

fn lava(x: f64, y: f64, z: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.8;
    p.motion.y = rand.next_float() as f64 * 0.4 + 0.05;
    p.color = [1., 1., 1.];
    p.scale *= rand.next_float() * 2.0 + 0.2;
    p.base_scale = p.scale;
    p.max_age = (16.0 / (rand.next_double() * 0.8 + 0.2)) as i32;
    p.no_clip = false;
    p.set_index(49);
    p.kind = Kind::Lava;
    p
}

fn cloud(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    let f = 2.5f32;
    p.motion *= 0.1;
    p.motion += DVec3::new(vx, vy, vz);
    let g = 1.0 - rand.next_float() * 0.3;
    p.color = [g, g, g];
    p.scale *= 0.75;
    p.scale *= f;
    p.base_scale = p.scale;
    p.max_age = (8.0 / (rand.next_double() * 0.8 + 0.3)) as i32;
    p.max_age = (p.max_age as f32 * f) as i32;
    p.no_clip = false;
    p.kind = Kind::Cloud;
    p
}

fn snow_shovel(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    p.motion *= 0.1;
    p.motion += DVec3::new(vx, vy, vz);
    let f = 1.0 - rand.next_float() * 0.3;
    p.color = [f, f, f];
    p.scale *= 0.75;
    p.base_scale = p.scale;
    p.max_age = (8.0 / (rand.next_double() * 0.8 + 0.2)) as i32;
    p.no_clip = false;
    p.kind = Kind::SnowShovel;
    p
}

/// `EntityDiggingFX` / `EntityBlockDustFX`: a block's particle sprite cut into 1/4×1/4 cells.
#[allow(clippy::too_many_arguments)]
fn block_crack(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom, models: &mc_render::models::BlockModels, state: mc_data::StateId, dust: bool) -> Option<Particle> {
    let rect = models.particle_uv(state)?;
    let tint = state.render_color(&models.colors);
    Some(digging(x, y, z, vx, vy, vz, rand, rect, tint, dust))
}

/// `EntityDiggingFX` with an already chosen atlas rect and `getRenderColor`/`colorMultiplier`.
#[allow(clippy::too_many_arguments)]
fn digging(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, rand: &mut JavaRandom, rect: [f32; 4], tint: u32, exact_motion: bool) -> Particle {
    let mut p = base(rand, x, y, z, vx, vy, vz);
    if exact_motion {
        p.motion = DVec3::new(vx, vy, vz);
    }
    p.gravity = 1.0;
    p.scale /= 2.0;
    p.base_scale = p.scale;
    p.rect = Some(rect);
    let f = 0.6;
    p.color = [
        f * (tint >> 16 & 255) as f32 / 255.,
        f * (tint >> 8 & 255) as f32 / 255.,
        f * (tint & 255) as f32 / 255.,
    ];
    p.no_clip = false;
    p.kind = Kind::Digging { dust: exact_motion };
    p
}

impl Particles {
    /// `EffectRenderer.addBlockDestroyEffects`: 4×4×4 digging particles with outward velocity.
    pub fn block_destroy(&mut self, state: mc_data::StateId, pos: mc_core::BlockPos, world: &World, models: &mc_render::models::BlockModels, rand: &mut JavaRandom) {
        if state.block().material == Material::Air {
            return;
        }
        let state = state.actual_state(world, pos);
        let Some(rect) = models.particle_uv(state) else { return };
        // `EntityDiggingFX.setBlockPos`: grass keeps its own (untinted) particle texture.
        let tint = if state.block().class == "BlockGrass" { 0xffffff } else { models.tint_color(world, state, pos) };
        for j in 0..4 {
            for k in 0..4 {
                for l in 0..4 {
                    let x = pos.x as f64 + (j as f64 + 0.5) / 4.0;
                    let y = pos.y as f64 + (k as f64 + 0.5) / 4.0;
                    let z = pos.z as f64 + (l as f64 + 0.5) / 4.0;
                    let vx = x - pos.x as f64 - 0.5;
                    let vy = y - pos.y as f64 - 0.5;
                    let vz = z - pos.z as f64 - 0.5;
                    let p = digging(x, y, z, vx, vy, vz, rand, rect, tint, false);
                    self.list.push(p);
                }
            }
        }
    }

    /// `EffectRenderer.addBlockHitEffects`: one digging particle on the struck face, slowed to
    /// `multiplyVelocity(0.2)` and shrunk by `multipleParticleScaleBy(0.6)`.
    pub fn block_hit(&mut self, state: mc_data::StateId, pos: mc_core::BlockPos, face: mc_core::Facing, world: &World, models: &mc_render::models::BlockModels, rand: &mut JavaRandom) {
        if state.block().material == Material::Air {
            return;
        }
        let Some(rect) = models.particle_uv(state) else { return };
        let b = state.block().default_bounds;
        let (min_x, min_y, min_z, max_x, max_y, max_z) = (b[0] as f64, b[1] as f64, b[2] as f64, b[3] as f64, b[4] as f64, b[5] as f64);
        let f = 0.1;
        let mut x = pos.x as f64 + rand.next_double() * (max_x - min_x - f * 2.0) + f + min_x;
        let mut y = pos.y as f64 + rand.next_double() * (max_y - min_y - f * 2.0) + f + min_y;
        let mut z = pos.z as f64 + rand.next_double() * (max_z - min_z - f * 2.0) + f + min_z;
        match face {
            mc_core::Facing::Down => y = pos.y as f64 + min_y - f,
            mc_core::Facing::Up => y = pos.y as f64 + max_y + f,
            mc_core::Facing::North => z = pos.z as f64 + min_z - f,
            mc_core::Facing::South => z = pos.z as f64 + max_z + f,
            mc_core::Facing::West => x = pos.x as f64 + min_x - f,
            mc_core::Facing::East => x = pos.x as f64 + max_x + f,
        }
        let tint = if state.block().class == "BlockGrass" { 0xffffff } else { models.tint_color(world, state, pos) };
        let mut p = digging(x, y, z, 0., 0., 0., rand, rect, tint, false);
        p.motion = DVec3::new(p.motion.x * 0.2, (p.motion.y - 0.1) * 0.2 + 0.1, p.motion.z * 0.2);
        p.size *= 0.6;
        p.scale *= 0.6;
        p.base_scale = p.scale;
        self.list.push(p);
    }
}

fn breaking(x: f64, y: f64, z: f64, rand: &mut JavaRandom, models: &mc_render::models::BlockModels, sprite: &str) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    let s = models.atlas.sprite(sprite);
    p.rect = Some([s.min_u, s.min_v, s.max_u, s.max_v]);
    p.color = [1., 1., 1.];
    p.gravity = 1.0;
    p.scale /= 2.0;
    p.base_scale = p.scale;
    p.kind = Kind::Breaking;
    p
}

fn barrier(x: f64, y: f64, z: f64, rand: &mut JavaRandom, models: &mc_render::models::BlockModels) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    let s = models.atlas.sprite("items/barrier");
    p.rect = Some([s.min_u, s.min_v, s.max_u, s.max_v]);
    p.color = [1., 1., 1.];
    p.motion = DVec3::ZERO;
    p.gravity = 0.0;
    p.max_age = 80;
    p.kind = Kind::Barrier;
    p
}

fn drop(x: f64, y: f64, z: f64, rand: &mut JavaRandom, water: bool) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion = DVec3::ZERO;
    p.color = if water { [0., 0., 1.] } else { [1., 0., 0.] };
    p.set_index(113);
    p.size = 0.01;
    p.gravity = 0.06;
    p.bob = 40;
    p.max_age = (64.0 / (rand.next_double() * 0.8 + 0.2)) as i32;
    p.kind = Kind::Drop { water };
    p
}

fn rain(x: f64, y: f64, z: f64, rand: &mut JavaRandom) -> Particle {
    let mut p = base(rand, x, y, z, 0., 0., 0.);
    p.motion *= 0.3;
    p.motion.y = rand.next_float() as f64 * 0.2 + 0.1;
    p.color = [1., 1., 1.];
    p.set_index(19 + rand.next_int_bounded(4));
    p.size = 0.01;
    p.gravity = 0.06;
    p.max_age = (8.0 / (rand.next_double() * 0.8 + 0.2)) as i32;
    p.kind = Kind::Rain;
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_data::geometry::BlockGeometry;

    fn phys<'a>(world: &'a World, geometry: &'a mut BlockGeometry) -> PhysWorld<'a> {
        PhysWorld { world, geometry }
    }

    #[test]
    fn quads_use_the_vanilla_camera_basis_and_uv_order() {
        let mut rand = JavaRandom::new(7);
        let mut ps = Particles::default();
        ps.list.push(explosion(0., 0.5, 0., 0., 0., 0., &mut rand));
        let q = ps.quads(Vec3::ZERO, 0., 0., 0., &World::default(), |_, _| [1., 1., 1.]);
        assert_eq!(q.len(), 1);
        let p = &q[0];
        // At yaw 0 / pitch 0 the quad spans +X (right) and +Y (up), centred on the particle.
        assert!(p.pos[0].x < p.pos[2].x && p.pos[0].y < p.pos[2].y);
        assert!(p.pos[0].x + p.pos[2].x < 1e-5);
        assert!(p.pos[0].y + p.pos[2].y < 1.1);
        // Java emits (maxU,maxV), (maxU,minV), (minU,minV), (minU,maxV).
        assert!(p.uv[0][0] > p.uv[2][0] && p.uv[0][1] > p.uv[2][1]);
        assert_eq!(p.tex, mc_render::avatar::ParticleTex::Misc);
    }

    #[test]
    fn smoke_animates_its_texture_and_dies() {
        let mut rand = JavaRandom::new(1);
        let mut ps = Particles::default();
        ps.list.push(smoke(0., 1., 0., 0., 0., 0., &mut rand, 1.0));
        let world = World::default();
        let mut geometry = BlockGeometry::new();
        let mut w = phys(&world, &mut geometry);
        let mut r = JavaRandom::new(2);
        ps.list[0].tick(&mut w, &mut r, Vec3::ZERO);
        assert_eq!(ps.list[0].tex_y * 16 + ps.list[0].tex_x, 7);
        for _ in 0..200 {
            ps.list[0].tick(&mut w, &mut r, Vec3::ZERO);
        }
        assert!(ps.list[0].dead);
    }

    #[test]
    fn huge_explosion_spawns_large_explosions() {
        let mut rand = JavaRandom::new(3);
        let mut ps = Particles::default();
        ps.list.push(huge_explosion(0., 0.5, 0., &mut rand));
        let world = World::default();
        let mut geometry = BlockGeometry::new();
        let mut w = phys(&world, &mut geometry);
        ps.list[0].tick(&mut w, &mut rand, Vec3::ZERO);
        ps.take_spawns();
        let large = ps.list.iter().filter(|p| p.kind == Kind::LargeExplosion).count();
        assert_eq!(large, 6);
    }

    #[test]
    fn large_explosion_frame_uvs_come_from_the_animation_sheet() {
        let mut rand = JavaRandom::new(4);
        let mut ps = Particles::default();
        let mut p = large_explosion(0., 0.5, 0., 0.0, &mut rand);
        p.age = 0;
        ps.list.push(p);
        let q = ps.quads(Vec3::ZERO, 0., 0., 0.5, &World::default(), |_, _| [1., 1., 1.]);
        assert_eq!(q[0].tex, mc_render::avatar::ParticleTex::Explosion);
        assert!(q[0].uv[0][0] <= 0.25 && q[0].uv[0][1] <= 0.25);
    }
}
