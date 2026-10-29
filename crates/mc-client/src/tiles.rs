//! Client tile entities that keep per-tile state: `TileEntityChest` / `TileEntityEnderChest`
//! (1.8.9). The drawing itself lives in `mc_render::tile`; this module owns the NBT the server
//! sends (`S35`), the `numPlayersUsing` counter set by block event 1, and `update()`'s lid
//! animation, including its two sound effects.
//!
//! `World.tickableTileEntities` is an `ArrayList` in `addTileEntity` order, so [`Tiles::order`]
//! preserves that order for ticking and drawing.
use mc_core::nbt::Compound;
use mc_core::random::JavaRandom;
use mc_core::{BlockPos, Vec3};
use mc_world::World;
use std::collections::HashMap;

/// `BlockChest.chestType` (0 normal, 1 trapped), or `BlockEnderChest`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Chest,
    Trapped,
    Ender,
}

impl Kind {
    /// The chest kind of a block state, if it is one.
    pub fn of(state: mc_data::StateId) -> Option<Kind> {
        match state.block().name {
            "minecraft:chest" => Some(Kind::Chest),
            "minecraft:trapped_chest" => Some(Kind::Trapped),
            "minecraft:ender_chest" => Some(Kind::Ender),
            _ => None,
        }
    }

    /// `TileEntityChest.getChestType`: `BlockChest.chestType`.
    fn chest_type(self) -> i32 {
        i32::from(self == Kind::Trapped)
    }
}

/// `EnumFacing.getIndex()` for the chest's `facing` property (`BlockChest.getMetaFromState`).
fn meta_of(state: mc_data::StateId) -> u8 {
    match state.value("facing") {
        Some("down") => 0,
        Some("up") => 1,
        Some("north") => 2,
        Some("south") => 3,
        Some("west") => 4,
        Some("east") => 5,
        // `BlockChest` only allows horizontal facings.
        _ => 2,
    }
}

/// `TileEntityChest.isChestAt`: a `BlockChest` of the same `chestType`. An ender chest is not a
/// `BlockChest`, so it never has a neighbour.
fn is_chest_at(world: &World, pos: BlockPos, kind: Kind) -> bool {
    if kind == Kind::Ender {
        return false;
    }
    Kind::of(world.state_at(pos.x, pos.y, pos.z)).is_some_and(|k| k != Kind::Ender && k.chest_type() == kind.chest_type())
}

/// The state `TileEntityChest`/`TileEntityEnderChest` keep for one block.
#[derive(Default)]
pub struct Chest {
    /// The last `S35` payload (inventory, custom name), for the opening GUI.
    pub nbt: Option<Compound>,
    /// `numPlayersUsing`, from `receiveClientEvent(1, n)`.
    pub num_players_using: i32,
    /// `lidAngle` / `prevLidAngle` in 0..=1.
    pub lid_angle: f32,
    pub prev_lid_angle: f32,
}

/// A `playSoundEffect` request produced by a tick.
pub struct Sound {
    pub name: &'static str,
    pub pos: Vec3,
    pub volume: f32,
    pub pitch: f32,
}

#[derive(Default)]
pub struct Tiles {
    map: HashMap<BlockPos, Chest>,
    order: Vec<BlockPos>,
}

impl Tiles {
    /// Adds a tile if the position does not have one yet (`World.addTileEntity`).
    pub fn insert(&mut self, pos: BlockPos) {
        if self.map.contains_key(&pos) {
            return;
        }
        self.map.insert(pos, Chest::default());
        self.order.push(pos);
    }

    /// `NetHandlerPlayClient.handleUpdateTileEntity`: the tile exists before its data arrives, so
    /// this also creates it.
    pub fn on_nbt(&mut self, pos: BlockPos, nbt: Option<Compound>) {
        self.insert(pos);
        if let Some(c) = self.map.get_mut(&pos) {
            c.nbt = nbt;
        }
    }

    /// Tiles already saved in a locally loaded world (`Chunk.tileEntities`), so a chest in a
    /// singleplayer save animates without the server's `S35`.
    pub fn seed_from_world(&mut self, world: &World) {
        for c in world.chunks.values() {
            for (pos, nbt) in &c.tile_entities {
                if Kind::of(world.state_at(pos.x, pos.y, pos.z)).is_some() {
                    self.on_nbt(*pos, Some(nbt.clone()));
                }
            }
        }
    }

    /// `World.setBlockState` + `BlockContainer`: a locally placed container block gets a tile.
    pub fn placed(&mut self, pos: BlockPos, state: mc_data::StateId) {
        if Kind::of(state).is_some() {
            self.insert(pos);
        }
    }

    /// `World.removeTileEntity`.
    pub fn remove(&mut self, pos: BlockPos) {
        if self.map.remove(&pos).is_some() {
            self.order.retain(|p| *p != pos);
        }
    }

    /// Drops every tile in a column that just unloaded.
    pub fn unload_chunk(&mut self, cx: i32, cz: i32) {
        let (x0, z0) = (cx << 4, cz << 4);
        self.order.retain(|p| {
            let inside = p.x >= x0 && p.x < x0 + 16 && p.z >= z0 && p.z < z0 + 16;
            if inside {
                self.map.remove(p);
            }
            !inside
        });
    }

    /// `TileEntity.receiveClientEvent` for event 1 (`BlockContainer.onBlockEventReceived`).
    pub fn receive_event(&mut self, pos: BlockPos, id: u8, param: u8) -> bool {
        if id != 1 {
            return false;
        }
        self.insert(pos);
        if let Some(c) = self.map.get_mut(&pos) {
            c.num_players_using = i32::from(param);
        }
        true
    }

    /// One client tick of `TileEntityChest.update` / `TileEntityEnderChest.update`.
    ///
    /// The `!worldObj.isRemote` branch of the chest (recounting nearby players every 200 ticks)
    /// is server-only and has no client-side effect, so it is not reproduced here.
    pub fn update(&mut self, world: &World, rand: &mut JavaRandom) -> Vec<Sound> {
        let mut sounds = Vec::new();
        for &pos in &self.order {
            let state = world.state_at(pos.x, pos.y, pos.z);
            let Some(kind) = Kind::of(state) else { continue };
            let x_neg = is_chest_at(world, pos.west(), kind);
            let z_neg = is_chest_at(world, pos.north(), kind);
            let x_pos = is_chest_at(world, pos.east(), kind);
            let z_pos = is_chest_at(world, pos.south(), kind);
            let Some(te) = self.map.get_mut(&pos) else { continue };
            let (fx, fy, fz) = (pos.x as f64, pos.y as f64, pos.z as f64);
            te.prev_lid_angle = te.lid_angle;
            if te.num_players_using > 0 && te.lid_angle == 0. && !z_neg && !x_neg {
                let mut sx = fx + 0.5;
                let mut sz = fz + 0.5;
                if z_pos {
                    sz += 0.5;
                }
                if x_pos {
                    sx += 0.5;
                }
                sounds.push(Sound {
                    name: "random.chestopen",
                    pos: Vec3::new(sx, fy + 0.5, sz),
                    volume: 0.5,
                    pitch: rand.next_float() * 0.1 + 0.9,
                });
            }
            if (te.num_players_using == 0 && te.lid_angle > 0.) || (te.num_players_using > 0 && te.lid_angle < 1.) {
                let f2 = te.lid_angle;
                if te.num_players_using > 0 {
                    te.lid_angle += 0.1;
                } else {
                    te.lid_angle -= 0.1;
                }
                if te.lid_angle > 1. {
                    te.lid_angle = 1.;
                }
                if te.lid_angle < 0.5 && f2 >= 0.5 && !z_neg && !x_neg {
                    let mut sx = fx + 0.5;
                    let mut sz = fz + 0.5;
                    if z_pos {
                        sz += 0.5;
                    }
                    if x_pos {
                        sx += 0.5;
                    }
                    sounds.push(Sound {
                        name: "random.chestclosed",
                        pos: Vec3::new(sx, fy + 0.5, sz),
                        volume: 0.5,
                        pitch: rand.next_float() * 0.1 + 0.9,
                    });
                }
                if te.lid_angle < 0. {
                    te.lid_angle = 0.;
                }
            }
        }
        sounds
    }

    /// The frames to draw. `partial` is the frame's partial tick, `christmas` selects the
    /// December textures (`TileEntityChestRenderer`'s calendar check) and `color` maps a
    /// combined light to the lightmap colour. `cam` is the interpolated view position:
    /// `TileEntityRendererDispatcher` skips tiles whose squared distance is 4096 or more
    /// (`TileEntity.getMaxRenderDistanceSquared`).
    #[allow(clippy::too_many_arguments)]
    pub fn frames(&self, world: &World, partial: f32, cam: [f32; 3], christmas: bool, color: &dyn Fn(u8, u8) -> [f32; 3]) -> Vec<mc_render::tile::TileFrame> {
        use mc_render::tile::{ChestSkin, TileFrame};
        let mut out = Vec::new();
        for &pos in &self.order {
            let dx = pos.x as f64 + 0.5 - cam[0] as f64;
            let dy = pos.y as f64 + 0.5 - cam[1] as f64;
            let dz = pos.z as f64 + 0.5 - cam[2] as f64;
            if dx * dx + dy * dy + dz * dz >= 4096. {
                continue;
            }
            let state = world.state_at(pos.x, pos.y, pos.z);
            let Some(kind) = Kind::of(state) else { continue };
            let Some(te) = self.map.get(&pos) else { continue };
            let x_neg = is_chest_at(world, pos.west(), kind);
            let z_neg = is_chest_at(world, pos.north(), kind);
            let x_pos = is_chest_at(world, pos.east(), kind);
            let z_pos = is_chest_at(world, pos.south(), kind);
            // `if (te.adjacentChestZNeg == null && te.adjacentChestXNeg == null)`: the other half
            // of a double chest is drawn by this one.
            if z_neg || x_neg {
                continue;
            }
            let lerp = |p: BlockPos| -> f32 {
                self.map.get(&p).map_or(0., |t| t.prev_lid_angle + (t.lid_angle - t.prev_lid_angle) * partial)
            };
            let mut lid = te.prev_lid_angle + (te.lid_angle - te.prev_lid_angle) * partial;
            if z_neg {
                lid = lid.max(lerp(pos.north()));
            }
            if x_neg {
                lid = lid.max(lerp(pos.west()));
            }
            let large = x_pos || z_pos;
            let skin = match kind {
                Kind::Ender => ChestSkin::Ender,
                _ if christmas => ChestSkin::Christmas,
                Kind::Trapped => ChestSkin::Trapped,
                Kind::Chest => ChestSkin::Normal,
            };
            let meta = meta_of(state);
            let (sky, block) = world.light(pos.x, pos.y, pos.z);
            out.push(TileFrame::Chest {
                pos: glam::Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                meta,
                large,
                skin,
                x_shift: meta == 2 && x_pos,
                z_shift: meta == 5 && z_pos,
                lid,
                light: color(sky, block),
            });
        }
        out
    }
}

/// `TileEntityChestRenderer`'s calendar check: December 24th-26th.
pub fn is_christmas(now: std::time::SystemTime) -> bool {
    // Days since the Unix epoch to a (month, day) pair, so the check needs no calendar crate.
    let secs = now.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let (_, month, day) = civil_from_days(days);
    month == 12 && (24..=26).contains(&day)
}

/// Howard Hinnant's `civil_from_days`: (year, month, day) for days since 1970-01-01, UTC.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_data::StateId;

    fn state(name: &str) -> StateId {
        facing(name, "north")
    }

    /// A block state of `name` with its `facing` property set to `facing`.
    fn facing(name: &str, dir: &str) -> StateId {
        mc_data::Block::by_name(name)
            .unwrap_or_else(|| panic!("{name}"))
            .states()
            .find(|s| s.value("facing") == Some(dir))
            .unwrap_or_else(|| panic!("{name} facing {dir}"))
    }

    #[test]
    fn chest_facing_maps_to_metadata() {
        assert_eq!(meta_of(state("minecraft:chest")), 2);
        assert_eq!(meta_of(state("minecraft:ender_chest")), 2);
        assert_eq!(Kind::of(state("minecraft:chest")), Some(Kind::Chest));
        assert_eq!(Kind::of(state("minecraft:trapped_chest")), Some(Kind::Trapped));
        assert_eq!(Kind::of(state("minecraft:ender_chest")), Some(Kind::Ender));
        assert_eq!(Kind::of(mc_data::Block::by_name("stone").unwrap().states().next().unwrap()), None);
    }

    /// `TileEntityChest.update`: 0.1 per tick, clamped, with `prevLidAngle` trailing by one.
    #[test]
    fn lid_opens_and_closes_in_tenths() {
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let pos = BlockPos::new(1, 64, 1);
        world.set_block_state(pos, state("minecraft:chest"));
        assert_eq!(Kind::of(world.state_at(pos.x, pos.y, pos.z)), Some(Kind::Chest));
        let mut tiles = Tiles::default();
        tiles.receive_event(pos, 1, 1);
        let mut rand = JavaRandom::new(1);
        let s = tiles.update(&world, &mut rand);
        assert_eq!(s.len(), 1, "opening plays random.chestopen");
        assert_eq!(s[0].name, "random.chestopen");
        let te = &tiles.map[&pos];
        assert!((te.lid_angle - 0.1).abs() < 1e-6);
        assert_eq!(te.prev_lid_angle, 0.);
        for _ in 0..20 {
            tiles.update(&world, &mut rand);
        }
        assert!((tiles.map[&pos].lid_angle - 1.).abs() < 1e-6);
        // Closing: 5 ticks to pass 0.5 → one closed sound.
        tiles.receive_event(pos, 1, 0);
        let mut closed = 0;
        for _ in 0..20 {
            closed += tiles.update(&world, &mut rand).iter().filter(|s| s.name == "random.chestclosed").count();
        }
        assert_eq!(closed, 1);
        assert_eq!(tiles.map[&pos].lid_angle, 0.);
    }

    #[test]
    fn civil_days_and_christmas() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(16_063), (2013, 12, 24));
        assert_eq!(civil_from_days(16_064), (2013, 12, 25));
        assert_eq!(civil_from_days(16_065), (2013, 12, 26));
        assert_eq!(civil_from_days(16_066), (2013, 12, 27));
        let at = |days: u64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(days * 86_400);
        assert!(!is_christmas(at(16_062)), "Dec 23");
        assert!(is_christmas(at(16_063)), "Dec 24");
        assert!(is_christmas(at(16_064)), "Dec 25");
        assert!(is_christmas(at(16_065)), "Dec 26");
        assert!(!is_christmas(at(16_066)), "Dec 27");
    }

    /// One flat world with the three chest kinds plus a double chest. Only the +x/+z half of a
    /// pair draws, and it draws the wide sheet.
    #[test]
    fn frames_cover_single_double_trapped_and_ender() {
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let cells = [
            (1, 64, 1, facing("minecraft:chest", "south")),
            (3, 64, 1, facing("minecraft:chest", "north")),
            (4, 64, 1, facing("minecraft:chest", "north")),
            (6, 64, 1, facing("minecraft:trapped_chest", "east")),
            (8, 64, 1, facing("minecraft:ender_chest", "west")),
        ];
        for (x, y, z, s) in cells {
            world.set_block_state(BlockPos::new(x, y, z), s);
        }

        let mut tiles = Tiles::default();
        for (x, y, z, _) in cells {
            tiles.on_nbt(BlockPos::new(x, y, z), None);
        }
        let frames = tiles.frames(&world, 0., [4.5, 64.5, 1.5], false, &|_, _| [1.; 3]);
        let got: Vec<(i32, bool, mc_render::tile::ChestSkin, u8)> = frames
            .iter()
            .map(|f| match f {
                mc_render::tile::TileFrame::Chest { pos, large, skin, meta, .. } => (pos.x as i32, *large, *skin, *meta),
            })
            .collect();
        use mc_render::tile::ChestSkin::*;
        // The (3)+(4) pair: the west half has an +x neighbour and draws the wide sheet, the east
        // half is skipped by `adjacentChestXNeg != null`.
        assert_eq!(got, vec![(1, false, Normal, 3), (3, true, Normal, 2), (6, false, Trapped, 5), (8, false, Ender, 4)]);
        // `i == 2 && adjacentChestXPos != null` shifts the wide model by one block.
        assert!(frames.iter().any(|f| matches!(f, mc_render::tile::TileFrame::Chest { pos, x_shift: true, .. } if pos.x == 3.)));
    }

    /// `TileEntityRendererDispatcher`: `getDistanceSq(...) < getMaxRenderDistanceSquared()`,
    /// which is 64 blocks (4096) for every tile entity in 1.8.9.
    #[test]
    fn tiles_stop_at_64_blocks() {
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        world.set_block_state(BlockPos::new(1, 64, 1), facing("minecraft:chest", "north"));
        let mut tiles = Tiles::default();
        tiles.on_nbt(BlockPos::new(1, 64, 1), None);
        // 63.5 blocks away in x: inside.
        assert_eq!(tiles.frames(&world, 0., [-62.0, 64.5, 1.5], false, &|_, _| [1.; 3]).len(), 1);
        // 64.5: outside (`>= 4096`).
        assert!(tiles.frames(&world, 0., [-63.0, 64.5, 1.5], false, &|_, _| [1.; 3]).is_empty());
    }
}
