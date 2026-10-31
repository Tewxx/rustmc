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
use std::cell::RefCell;
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

fn is_rendered_tile(state: mc_data::StateId) -> bool {
    Kind::of(state).is_some() || matches!(state.block().name, "minecraft:standing_sign" | "minecraft:wall_sign" | "minecraft:skull" | "minecraft:standing_banner" | "minecraft:wall_banner")
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

/// `TileEntitySignRenderer`'s laid-out text for one sign. [`sign_line`] parses the chat JSON,
/// wraps the line and lays out every glyph, so vanilla's per-frame cost is paid once per edit.
struct SignLayout {
    /// The raw lines (`TileEntitySign.signText`) the glyphs were built from.
    text: [String; 4],
    /// `Settings.chat_colors` at build time: `split_text` strips `§` codes when it is off.
    chat_colors: bool,
    /// All four lines in line order, in sign-local space.
    glyphs: Vec<mc_render::tile::SignGlyph>,
}

#[derive(Default)]
pub struct Tiles {
    map: HashMap<BlockPos, Chest>,
    order: Vec<BlockPos>,
    /// [`Self::append_sign_glyphs`]' results, kept until the sign's text or column goes.
    sign_layouts: RefCell<HashMap<BlockPos, SignLayout>>,
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
                if is_rendered_tile(world.state_at(pos.x, pos.y, pos.z)) {
                    self.on_nbt(*pos, Some(nbt.clone()));
                }
            }
        }
    }

    /// `World.setBlockState` + `BlockContainer`: a locally placed container block gets a tile.
    pub fn placed(&mut self, pos: BlockPos, state: mc_data::StateId) {
        if is_rendered_tile(state) {
            self.insert(pos);
        }
    }

    /// `World.removeTileEntity`.
    pub fn remove(&mut self, pos: BlockPos) {
        if self.map.remove(&pos).is_some() {
            self.order.retain(|p| *p != pos);
        }
        self.sign_layouts.borrow_mut().remove(&pos);
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
        self.sign_layouts.borrow_mut().retain(|p, _| !(p.x >= x0 && p.x < x0 + 16 && p.z >= z0 && p.z < z0 + 16));
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
    pub fn frames(&self, world: &World, partial: f32, cam: [f32; 3], christmas: bool, color: &dyn Fn(u8, u8) -> [f32; 3], skins: &crate::skins::Skins) -> Vec<mc_render::tile::TileFrame> {
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
            if matches!(state.block().name, "minecraft:standing_sign" | "minecraft:wall_sign") {
                // `TileEntityRendererDispatcher.renderTileEntity`: `getCombinedLight(pos, 0)`.
                let (sky, block) = world.brightness_for_render(pos);
                out.push(TileFrame::Sign { pos: glam::Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                    standing: state.block().name == "minecraft:standing_sign", meta: state.meta(), light: color(sky as u8, block as u8), glyphs: Vec::new() });
                continue;
            }
            if matches!(state.block().name, "minecraft:standing_banner" | "minecraft:wall_banner") {
                let Some(nbt) = self.map.get(&pos).and_then(|te| te.nbt.as_ref()).or_else(|| world.tile_entity(pos)) else { continue };
                let base = nbt.get("Base").and_then(mc_core::nbt::Tag::as_i64).unwrap_or(0) as u8;
                let mut patterns = vec![("b".to_string(), base)];
                if let Some(list) = nbt.get("Patterns").and_then(mc_core::nbt::Tag::as_list) {
                    for entry in list.iter().filter_map(mc_core::nbt::Tag::as_compound) {
                        if let Some(id) = entry.get("Pattern").and_then(mc_core::nbt::Tag::as_str) {
                            let color = entry.get("Color").and_then(mc_core::nbt::Tag::as_i64).unwrap_or(0) as u8;
                            patterns.push((id.to_owned(), color));
                        }
                    }
                }
                let (sky, block) = world.brightness_for_render(pos);
                out.push(TileFrame::Banner { pos: glam::Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                    standing: state.block().name == "minecraft:standing_banner", meta: state.meta(), age: 0., patterns, light: color(sky as u8, block as u8) });
                continue;
            }
            if state.block().name == "minecraft:skull" {
                let nbt = self.map.get(&pos).and_then(|te| te.nbt.as_ref()).or_else(|| world.tile_entity(pos));
                let byte = |key| nbt.and_then(|nbt| nbt.get(key)).and_then(mc_core::nbt::Tag::as_i64).unwrap_or(0) as u8;
                let (sky, block) = world.brightness_for_render(pos);
                // TileEntitySkull.playerProfile ("Owner"), only for player skulls.
                let profile = nbt.filter(|_| byte("SkullType") == 3).and_then(|nbt| nbt.get("Owner")).and_then(mc_core::nbt::Tag::as_compound).and_then(crate::skins::skull_profile);
                let (skin, alex) = skins.skull_skin(profile.as_ref());
                out.push(TileFrame::Skull { pos: glam::Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                    facing: state.meta() & 7, rotation: byte("Rot"), kind: byte("SkullType"), light: color(sky as u8, block as u8), skin, alex });
                continue;
            }
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
            let (sky, block) = world.brightness_for_render(pos);
            out.push(TileFrame::Chest {
                pos: glam::Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                meta,
                large,
                skin,
                x_shift: meta == 2 && x_pos,
                z_shift: meta == 5 && z_pos,
                lid,
                light: color(sky as u8, block as u8),
            });
        }
        out
    }

    /// The four raw sign lines: the `S33PacketUpdateSign` text first, else the tile entity's
    /// `Text1..Text4`, else an empty JSON string (what `TileEntitySign.getSignText` starts with).
    fn sign_text<'a>(signs: &'a HashMap<BlockPos, [String; 4]>, world: &'a World, pos: BlockPos) -> [&'a str; 4] {
        if let Some(lines) = signs.get(&pos) {
            return std::array::from_fn(|line| lines[line].as_str());
        }
        const KEYS: [&str; 4] = ["Text1", "Text2", "Text3", "Text4"];
        let nbt = world.tile_entity(pos);
        std::array::from_fn(|line| nbt.and_then(|nbt| nbt.get(KEYS[line])).and_then(mc_core::nbt::Tag::as_str).unwrap_or("\"\""))
    }

    /// One sign's glyphs, appended to `out` in line order. A sign's text only changes on
    /// `S33PacketUpdateSign`/`Text1..Text4`, so the JSON parse, the wrap and the glyph layout are
    /// only redone when [`Self::sign_text`] no longer matches the cached lines.
    pub fn append_sign_glyphs(
        &self,
        pos: BlockPos,
        out: &mut Vec<mc_render::tile::SignGlyph>,
        world: &World,
        signs: &HashMap<BlockPos, [String; 4]>,
        font: &mut mc_gui::font::FontRenderer,
        lang: &mc_gui::lang::Lang,
        chat_colors: bool,
    ) {
        let raw = Self::sign_text(signs, world, pos);
        let mut layouts = self.sign_layouts.borrow_mut();
        let unchanged = layouts.get(&pos).is_some_and(|l| l.chat_colors == chat_colors && l.text.iter().zip(raw).all(|(a, b)| a.as_str() == b));
        if unchanged {
            let glyphs = &layouts[&pos].glyphs;
            out.reserve(glyphs.len());
            out.extend_from_slice(glyphs);
            return;
        }
        let mut glyphs = Vec::new();
        // `font.draw` picks `§k` glyphs from its RNG while drawing, so an obfuscated line has to
        // be laid out (and re-randomized) every frame like vanilla; it is never cached.
        let mut random = false;
        for line in 0..4 {
            let (mut line_glyphs, obfuscated) = sign_line(raw[line], line, font, lang, chat_colors);
            random |= obfuscated;
            glyphs.append(&mut line_glyphs);
        }
        if random {
            layouts.remove(&pos);
        }
        out.reserve(glyphs.len());
        out.extend_from_slice(&glyphs);
        if !random {
            layouts.insert(pos, SignLayout { text: raw.map(str::to_string), chat_colors, glyphs });
        }
    }

    /// Drops every cached sign layout. `Minecraft.refreshResources` reloads the font and the
    /// language, which the laid-out glyphs depend on; `reload_resources` calls this.
    pub fn clear_sign_layouts(&mut self) {
        self.sign_layouts.get_mut().clear();
    }
}

/// `TileEntitySignRenderer`'s line: the chat component wrapped to 90 pixels, centred and drawn
/// into glyph quads. The flag is `true` when the line is `§k`-obfuscated, whose glyph choice is
/// random per draw.
fn sign_line(raw: &str, line: usize, font: &mut mc_gui::font::FontRenderer, lang: &mc_gui::lang::Lang, chat_colors: bool) -> (Vec<mc_render::tile::SignGlyph>, bool) {
    let component = crate::chat::parse(raw, lang);
    let first = mc_gui::component::split_text(&component, 90, font, false, true, chat_colors)
        .into_iter().next().map_or(String::new(), |line| line.formatted());
    let random = first.contains("\u{a7}k") || first.contains("\u{a7}K");
    let mut batch = mc_gui::batch::GuiBatch::new();
    let x = -(font.width(&first) / 2) as f32;
    font.draw(&mut batch, &first, x, (line as i32 * 10 - 20) as f32, 0);
    let mut glyphs = Vec::new();
    for item in batch.items {
        let mc_gui::batch::Item::Draw(draw) = item else { continue };
        for quad in draw.vertices.chunks_exact(4) {
            glyphs.push(mc_render::tile::SignGlyph { pos: std::array::from_fn(|i| quad[i].pos),
                uv: std::array::from_fn(|i| quad[i].uv), color: quad[0].color, texture: draw.texture.clone() });
        }
    }
    (glyphs, random)
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
        let frames = tiles.frames(&world, 0., [4.5, 64.5, 1.5], false, &|_, _| [1.; 3], &Default::default());
        let got: Vec<(i32, bool, mc_render::tile::ChestSkin, u8)> = frames
            .iter()
            .filter_map(|f| match f {
                mc_render::tile::TileFrame::Chest { pos, large, skin, meta, .. } => Some((pos.x as i32, *large, *skin, *meta)),
                _ => None,
            })
            .collect();
        use mc_render::tile::ChestSkin::*;
        // The (3)+(4) pair: the west half has an +x neighbour and draws the wide sheet, the east
        // half is skipped by `adjacentChestXNeg != null`.
        assert_eq!(got, vec![(1, false, Normal, 3), (3, true, Normal, 2), (6, false, Trapped, 5), (8, false, Ender, 4)]);
        // `i == 2 && adjacentChestXPos != null` shifts the wide model by one block.
        assert!(frames.iter().any(|f| matches!(f, mc_render::tile::TileFrame::Chest { pos, x_shift: true, .. } if pos.x == 3.)));
    }

    #[test]
    fn sign_text_formats_centers_and_limits_the_first_line() {
        let Some(jar) = mc_testutil::jar() else { return };
        let mut assets = mc_assets::AssetSource::new(); assets.push(&jar).unwrap();
        let mut font = mc_gui::font::FontRenderer::load(&assets);
        let lang = mc_gui::lang::Lang::default();
        let glyphs = sign_line(r#"{"text":"A","color":"red"}"#, 1, &mut font, &lang, true).0;
        assert_eq!(glyphs.len(), 1);
        assert_eq!(glyphs[0].color, [255, 85, 85, 255]);
        assert!(glyphs[0].pos[0][0] < 0. && glyphs[0].pos[0][1] == -10.);
        let long = sign_line(&format!("\"{}\"", "A".repeat(100)), 0, &mut font, &lang, true).0;
        assert!(long.len() < 100 && !long.is_empty());
        let unicode = sign_line(r#"{"text":"中"}"#, 0, &mut font, &lang, true).0;
        assert_eq!(unicode[0].texture.as_deref(), Some("textures/font/unicode_page_4e.png"));
        // `§k` is reported so the caller knows the glyphs are random per draw.
        assert!(sign_line("\"\u{a7}k abc\"", 0, &mut font, &lang, true).1);
        assert!(!sign_line(r#"{"text":"plain"}"#, 0, &mut font, &lang, true).1);
    }

    #[test]
    fn banner_frame_keeps_server_patterns() {
        let mut world = World::default(); world.insert(mc_world::chunk::Chunk::new(0, 0));
        let p = BlockPos::new(1, 64, 1);
        let state = mc_data::Block::by_name("minecraft:standing_banner").unwrap().state_from_meta(7).unwrap();
        world.set_block_state(p, state);
        let pattern = mc_core::nbt::Tag::Compound([(String::from("Pattern"), mc_core::nbt::Tag::String("cr".into())),
            (String::from("Color"), mc_core::nbt::Tag::Int(4))].into_iter().collect());
        let mut tiles = Tiles::default(); tiles.on_nbt(p, Some([(String::from("Base"), mc_core::nbt::Tag::Int(2)),
            (String::from("Patterns"), mc_core::nbt::Tag::List(10, vec![pattern]))].into_iter().collect()));
        let frames = tiles.frames(&world, 0., [1.5, 64.5, 1.5], false, &|_, _| [1.;3], &Default::default());
        assert!(matches!(&frames[..], [mc_render::tile::TileFrame::Banner { meta: 7, patterns, .. }] if patterns == &vec![("b".to_string(), 2), ("cr".to_string(), 4)]));
    }

    #[test]
    fn skull_frame_uses_server_nbt_and_block_facing() {
        let mut world = World::default(); world.insert(mc_world::chunk::Chunk::new(0, 0));
        let p = BlockPos::new(1, 64, 1);
        let state = mc_data::Block::by_name("minecraft:skull").unwrap().state_from_meta(1).unwrap();
        world.set_block_state(p, state);
        let mut tiles = Tiles::default();
        tiles.on_nbt(p, Some([(String::from("SkullType"), mc_core::nbt::Tag::Byte(2)),
            (String::from("Rot"), mc_core::nbt::Tag::Byte(7))].into_iter().collect()));
        let frames = tiles.frames(&world, 0., [1.5, 64.5, 1.5], false, &|_, _| [1.;3], &Default::default());
        assert!(matches!(&frames[..], [mc_render::tile::TileFrame::Skull { facing: 1, rotation: 7, kind: 2, .. }]));
    }

    #[test]
    fn sign_frames_follow_block_state_and_tile_removal() {        let mut world = World::default(); world.insert(mc_world::chunk::Chunk::new(0, 0));
        let p = BlockPos::new(1, 64, 1);
        let state = mc_data::Block::by_name("minecraft:standing_sign").unwrap().state_from_meta(7).unwrap();
        world.set_block_state(p, state);
        let mut tiles = Tiles::default(); tiles.placed(p, state);
        let frames = tiles.frames(&world, 0., [1.5, 64.5, 1.5], false, &|_, _| [1.;3], &Default::default());
        assert!(matches!(&frames[..], [mc_render::tile::TileFrame::Sign { standing: true, meta: 7, .. }]));
        tiles.remove(p);
        assert!(tiles.frames(&world, 0., [1.5, 64.5, 1.5], false, &|_, _| [1.;3], &Default::default()).is_empty());
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
        assert_eq!(tiles.frames(&world, 0., [-62.0, 64.5, 1.5], false, &|_, _| [1.; 3], &Default::default()).len(), 1);
        // 64.5: outside (`>= 4096`).
        assert!(tiles.frames(&world, 0., [-63.0, 64.5, 1.5], false, &|_, _| [1.; 3], &Default::default()).is_empty());
    }

    /// `TileEntitySign.getSignText`: an absent line is the empty JSON string.
    #[test]
    fn missing_sign_lines_read_as_an_empty_json_string() {
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let signs = HashMap::new();
        assert_eq!(Tiles::sign_text(&signs, &world, BlockPos::new(1, 64, 1)), ["\"\"", "\"\"", "\"\"", "\"\""]);
    }

    /// A sign's glyphs are laid out once and copied out until its `S33` text or its
    /// `Text1..Text4` NBT changes; the packet's lines win over the tile entity, like
    /// `NetHandlerPlayClient.handleUpdateSign` replacing `signText`.
    #[test]
    fn sign_layout_is_cached_until_its_text_changes() {
        let Some(jar) = mc_testutil::jar() else { return };
        let mut assets = mc_assets::AssetSource::new(); assets.push(&jar).unwrap();
        let mut font = mc_gui::font::FontRenderer::load(&assets);
        let lang = mc_gui::lang::Lang::default();
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let p = BlockPos::new(1, 64, 1);
        let snapshot = |glyphs: &[mc_render::tile::SignGlyph]| -> Vec<([[f32; 2]; 4], [[f32; 2]; 4], [u8; 4], Option<String>)> {
            glyphs.iter().map(|g| (g.pos, g.uv, g.color, g.texture.clone())).collect()
        };
        let line = |text: &str| -> [String; 4] { std::array::from_fn(|i| if i == 0 { text.to_string() } else { "\"\"".to_string() }) };
        let mut signs = HashMap::new();
        signs.insert(p, line("\"abcd\""));

        let mut tiles = Tiles::default();
        let mut out = Vec::new();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert_eq!(out.len(), 4);
        let first = snapshot(&out);
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert_eq!(snapshot(&out), first, "an unchanged sign copies its cached glyphs");
        assert_eq!(tiles.sign_layouts.borrow().len(), 1);

        // Without `S33` lines the tile entity's `Text1..Text4` are used, and changing them rebuilds.
        signs.clear();
        let nbt = |text: &str| -> Compound { [(String::from("Text1"), mc_core::nbt::Tag::String(text.into()))].into_iter().collect() };
        world.chunks.get_mut(&(0, 0)).unwrap().tile_entities.insert(p, nbt("\"nbt\""));
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        let from_nbt = snapshot(&out);
        assert_ne!(from_nbt, first);
        world.chunks.get_mut(&(0, 0)).unwrap().tile_entities.insert(p, nbt("\"nbt longer\""));
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert_ne!(snapshot(&out), from_nbt, "changed NBT text rebuilds the layout");

        // `S33` text wins over NBT, and its own change invalidates too.
        signs.insert(p, line("\"s33\""));
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        let from_s33 = snapshot(&out);
        assert_ne!(from_s33, from_nbt, "the packet's lines win over the tile entity");
        signs.insert(p, line("\"s33 changed\""));
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert_ne!(snapshot(&out), from_s33);
        assert_eq!(tiles.sign_layouts.borrow().len(), 1, "one layout per position");

        // `chat_colors` is an input of `split_text`, so flipping it rebuilds too.
        assert!(tiles.sign_layouts.borrow()[&p].chat_colors);
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, false);
        assert!(!tiles.sign_layouts.borrow()[&p].chat_colors);

        // A pack reload and an unloaded column both drop the layouts.
        tiles.clear_sign_layouts();
        assert!(tiles.sign_layouts.borrow().is_empty());
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert!(!tiles.sign_layouts.borrow().is_empty());
        tiles.unload_chunk(0, 0);
        assert!(tiles.sign_layouts.borrow().is_empty());

        // `§k` glyphs are random per draw, so an obfuscated line must not be cached.
        signs.insert(p, line("\"\\u00a7k abc\""));
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert!(!out.is_empty());
        assert!(tiles.sign_layouts.borrow().get(&p).is_none(), "obfuscated text is laid out every frame");

        // Removing the tile drops a plain sign's cached layout.
        signs.insert(p, line("\"plain\""));
        out.clear();
        tiles.append_sign_glyphs(p, &mut out, &world, &signs, &mut font, &lang, true);
        assert!(tiles.sign_layouts.borrow().contains_key(&p));
        tiles.remove(p);
        assert!(tiles.sign_layouts.borrow().is_empty());
    }
}
