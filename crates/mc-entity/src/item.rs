//! `EntityItem`: dropped item stacks.
use crate::world::*;
use mc_core::math::{self, floor_double};
use mc_core::random::JavaRandom;
use mc_core::{Aabb, BlockPos, Vec3};
use mc_data::Material;

#[derive(Clone, Debug)]
pub struct ItemEntity {
    pub id: u32,
    pub pos: Vec3,
    pub prev_pos: Vec3,
    pub last_tick_pos: Vec3,
    pub motion: Vec3,
    pub bb: Aabb,
    pub on_ground: bool,
    pub age: i32,
    pub pickup_delay: i32,
    pub hover_start: f32,
    pub ticks_existed: i32,
    pub dead: bool,
    /// Catalogue index and count of the stack.
    pub stack: (usize, u8),
    /// The stack's metadata and NBT (set by the owner; `stack.0` alone only picks the icon).
    pub damage: i16,
    pub tag: Option<mc_core::nbt::Interned>,
    in_water: bool,
}

fn item_bb(p: Vec3) -> Aabb {
    Aabb::new(p.x - 0.125, p.y, p.z - 0.125, p.x + 0.125, p.y + 0.25, p.z + 0.125)
}

impl ItemEntity {
    /// `new EntityItem(world, x, y, z, stack)`
    pub fn new(id: u32, pos: Vec3, stack: (usize, u8), rand: &mut JavaRandom) -> Self {
        // Entity constructor's rotationYaw draw, then EntityItem's own.
        let hover_start = (rand.next_double() * std::f64::consts::PI * 2.0) as f32;
        let _yaw = (rand.next_double() * 360.0) as f32;
        let motion = Vec3::new(
            (rand.next_double() * 0.20000000298023224 - 0.10000000149011612) as f32 as f64,
            0.20000000298023224,
            (rand.next_double() * 0.20000000298023224 - 0.10000000149011612) as f32 as f64,
        );
        Self {
            id,
            pos,
            prev_pos: pos,
            last_tick_pos: pos,
            motion,
            bb: item_bb(pos),
            on_ground: false,
            age: 0,
            pickup_delay: 0,
            hover_start,
            ticks_existed: 0,
            dead: false,
            stack,
            damage: 0,
            tag: None,
            in_water: false,
        }
    }

    /// `Block.spawnAsEntity`: random offset inside the block, default pickup delay.
    pub fn from_block(id: u32, pos: BlockPos, stack: (usize, u8), rand: &mut JavaRandom) -> Self {
        let d0 = (rand.next_float() * 0.5) as f64 + 0.25;
        let d1 = (rand.next_float() * 0.5) as f64 + 0.25;
        let d2 = (rand.next_float() * 0.5) as f64 + 0.25;
        let mut e = Self::new(id, Vec3::new(pos.x as f64 + d0, pos.y as f64 + d1, pos.z as f64 + d2), stack, rand);
        e.pickup_delay = 10;
        e
    }

    /// `EntityPlayer.dropItem(stack, false, true)` from a player at `pos` looking `yaw`/`pitch`.
    pub fn thrown(id: u32, pos: Vec3, eye_height: f32, yaw: f32, pitch: f32, stack: (usize, u8), rand: &mut JavaRandom) -> Self {
        let y = pos.y - 0.30000001192092896 + eye_height as f64;
        let mut e = Self::new(id, Vec3::new(pos.x, y, pos.z), stack, rand);
        e.pickup_delay = 40;
        let pi = std::f32::consts::PI;
        let f = 0.3f32;
        e.motion.x = (-math::sin(yaw / 180.0 * pi) * math::cos(pitch / 180.0 * pi) * f) as f64;
        e.motion.z = (math::cos(yaw / 180.0 * pi) * math::cos(pitch / 180.0 * pi) * f) as f64;
        e.motion.y = (-math::sin(pitch / 180.0 * pi) * f + 0.1) as f64;
        let f3 = rand.next_float() * pi * 2.0;
        let f = 0.02 * rand.next_float();
        e.motion.x += (math::cos(f3) * f) as f64;
        e.motion.y += ((rand.next_float() - rand.next_float()) * 0.1) as f64;
        e.motion.z += (math::sin(f3) * f) as f64;
        e
    }

    pub fn interpolated(&self, pt: f32) -> Vec3 {
        let (a, b) = (self.last_tick_pos, self.pos);
        Vec3::new(a.x + (b.x - a.x) * pt as f64, a.y + (b.y - a.y) * pt as f64, a.z + (b.z - a.z) * pt as f64)
    }

    /// `EntityItem.onUpdate`
    pub fn tick(&mut self, w: &mut impl EntityWorld, rand: &mut JavaRandom) {
        self.last_tick_pos = self.pos;
        self.ticks_existed += 1;
        self.prev_pos = self.pos;
        self.handle_water(w);
        if self.pickup_delay > 0 && self.pickup_delay != 32767 {
            self.pickup_delay -= 1;
        }
        self.motion.y -= 0.03999999910593033;
        let center_y = (self.bb.min_y + self.bb.max_y) / 2.0;
        let no_clip = self.push_out_of_blocks(w, self.pos.x, center_y, self.pos.z, rand);
        self.move_entity(w, no_clip);
        let moved = self.prev_pos.x as i32 != self.pos.x as i32
            || self.prev_pos.y as i32 != self.pos.y as i32
            || self.prev_pos.z as i32 != self.pos.z as i32;
        if moved || self.ticks_existed % 25 == 0 {
            let p = BlockPos::new(floor_double(self.pos.x), floor_double(self.pos.y), floor_double(self.pos.z));
            if w.state(p).block().material == Material::Lava {
                self.motion.y = 0.20000000298023224;
                self.motion.x = ((rand.next_float() - rand.next_float()) * 0.2) as f64;
                self.motion.z = ((rand.next_float() - rand.next_float()) * 0.2) as f64;
            }
        }
        let mut f = 0.98f32;
        if self.on_ground {
            let p = BlockPos::new(floor_double(self.pos.x), floor_double(self.bb.min_y) - 1, floor_double(self.pos.z));
            f = w.state(p).block().slipperiness * 0.98;
        }
        self.motion.x *= f as f64;
        self.motion.y *= 0.9800000190734863;
        self.motion.z *= f as f64;
        if self.on_ground {
            self.motion.y *= -0.5;
        }
        self.age += 1;
        self.handle_water(w);
        if self.age >= 6000 {
            self.dead = true;
        }
    }

    fn handle_water(&mut self, w: &impl EntityWorld) {
        let bb = self.bb.expand(0.0, -0.4000000059604645, 0.0).contract(0.001, 0.001, 0.001);
        self.in_water = handle_material_acceleration(w, &bb, Material::Water, &mut self.motion, true);
    }

    /// `Entity.pushOutOfBlocks`
    fn push_out_of_blocks(&mut self, w: &mut impl EntityWorld, x: f64, y: f64, z: f64, rand: &mut JavaRandom) -> bool {
        let pos = BlockPos::new(floor_double(x), floor_double(y), floor_double(z));
        let (d0, d1, d2) = (x - pos.x as f64, y - pos.y as f64, z - pos.z as f64);
        let boxes = colliding_boxes(w, &self.bb);
        if boxes.is_empty() && !full_cube(w, pos) {
            return false;
        }
        let mut i = 3;
        let mut d3 = 9999.0;
        let cands = [
            (!full_cube(w, pos.offset(-1, 0, 0)), d0, 0),
            (!full_cube(w, pos.offset(1, 0, 0)), 1.0 - d0, 1),
            (!full_cube(w, pos.up()), 1.0 - d1, 3),
            (!full_cube(w, pos.offset(0, 0, -1)), d2, 4),
            (!full_cube(w, pos.offset(0, 0, 1)), 1.0 - d2, 5),
        ];
        for (open, d, idx) in cands {
            if open && d < d3 {
                d3 = d;
                i = idx;
            }
        }
        let f = (rand.next_float() * 0.2 + 0.1) as f64;
        match i {
            0 => self.motion.x = -f,
            1 => self.motion.x = f,
            3 => self.motion.y = f,
            4 => self.motion.z = -f,
            5 => self.motion.z = f,
            _ => {}
        }
        true
    }

    /// `Entity.moveEntity` without the player-only parts.
    fn move_entity(&mut self, w: &mut impl EntityWorld, no_clip: bool) {
        let (mut x, mut y, mut z) = (self.motion.x, self.motion.y, self.motion.z);
        if no_clip {
            self.bb = self.bb.offset(x, y, z);
        } else {
            let (d3, d4, d5) = (x, y, z);
            let list = colliding_boxes(w, &self.bb.add_coord(x, y, z));
            for b in &list {
                y = b.calculate_y_offset(&self.bb, y);
            }
            self.bb = self.bb.offset(0.0, y, 0.0);
            for b in &list {
                x = b.calculate_x_offset(&self.bb, x);
            }
            self.bb = self.bb.offset(x, 0.0, 0.0);
            for b in &list {
                z = b.calculate_z_offset(&self.bb, z);
            }
            self.bb = self.bb.offset(0.0, 0.0, z);
            self.on_ground = d4 != y && d4 < 0.0;
            if d3 != x {
                self.motion.x = 0.0;
            }
            if d5 != z {
                self.motion.z = 0.0;
            }
            if d4 != y {
                self.motion.y = 0.0;
            }
        }
        self.pos = Vec3::new((self.bb.min_x + self.bb.max_x) / 2.0, self.bb.min_y, (self.bb.min_z + self.bb.max_z) / 2.0);
    }
}

/// `World.isBlockFullCube`
fn full_cube(w: &impl EntityWorld, p: BlockPos) -> bool {
    // Collision box of average edge length >= 1: air and passable blocks have none.
    let b = w.state(p).block();
    b.full_cube && b.material.props().blocks_movement
}
