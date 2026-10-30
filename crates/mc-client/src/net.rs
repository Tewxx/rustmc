//! The multiplayer side of the client: `NetHandlerPlayClient` packet plumbing, the
//! `Chunk.fillChunk` conversion, and `EntityPlayerSP.onUpdateWalkingPlayer`'s movement packets.
use mc_core::Vec3;
use mc_protocol::chunk::WireChunk;
use mc_protocol::net::{login_with_socket, Writer};
use mc_protocol::packets::*;
use mc_world::chunk::{Chunk, Section};
use std::sync::mpsc::{self, Receiver};

/// What the reader thread hands the game loop.
pub enum Incoming {
    Packet(Box<PlayClientbound>),
    /// The connection ended; the reason as chat JSON or plain text.
    Closed(String),
}

pub struct Net {
    writer: Writer,
    rx: Receiver<Incoming>,
    pub entity_id: i32,
    /// Our profile UUID from `LoginSuccess`.
    pub uuid: mc_protocol::Uuid,
    // EntityPlayerSP
    last_x: f64,
    last_y: f64,
    last_z: f64,
    last_yaw: f32,
    last_pitch: f32,
    position_update_ticks: i32,
    server_sprint: bool,
    server_sneak: bool,
    /// `PlayerControllerMP.currentPlayerItem`
    pub current_item: i32,
    pub closed: Option<String>,
    /// `NetHandlerPlayClient.doneLoadingTerrain`: the first `S08` snaps `prevPos`.
    done_loading_terrain: bool,
}

/// Closing the socket also ends the reader thread, so a dropped connection leaks nothing.
impl Drop for Net {
    fn drop(&mut self) {
        self.writer.shutdown();
    }
}

impl Net {
    /// `GuiConnecting`: blocks until login succeeds, then reads packets on a thread.
    /// `stage` flips to "Logging in..." once the server asks for authentication.
    pub fn connect(host: &str, port: u16, name: &str, session: Option<&crate::auth::Session>, stage: &std::sync::atomic::AtomicU8, socket: &std::sync::Mutex<Option<std::net::TcpStream>>, cancel: &std::sync::atomic::AtomicBool) -> Result<Net, String> {
        let mut join = |hash: &str| {
            stage.store(mc_gui::screens::STAGE_LOGGING_IN, std::sync::atomic::Ordering::Relaxed);
            match session {
            Some(s) => crate::auth::join_server(s, hash),
            None => Err("This server is online-mode. Sign in with --online (Microsoft account), or join an offline-mode server with --name <name>.".into()),
            }
        };
        let (mut reader, writer, ok) = login_with_socket(host, port, name, Some(&mut join), |stream| {
            let mut slot = socket.lock().unwrap();
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "connection cancelled"));
            }
            *slot = Some(stream.try_clone()?);
            stage.store(mc_gui::screens::STAGE_LOGGING_IN, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }).map_err(|e| match e {
            mc_protocol::net::LoginError::Kicked(json) => json,
            mc_protocol::net::LoginError::Io(e) if e.to_string().contains("lookup address") || e.to_string().contains("not known") => "Unknown host".into(),
            mc_protocol::net::LoginError::Io(e) => format!("Could not connect to {host}:{port}: {e}"),
            // `UnknownHostException`
            mc_protocol::net::LoginError::Protocol(s) if s.starts_with("Unknown host") => "Unknown host".into(),
            mc_protocol::net::LoginError::Auth(s) | mc_protocol::net::LoginError::Protocol(s) => s,
        })?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("Netty Client IO".into())
            .spawn(move || loop {
                // A panic here would drop the channel and leave the game waiting forever.
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reader.recv_play()))
                    .unwrap_or_else(|_| Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Internal Exception: packet handling panicked")));
                match r {
                    Ok(p) => {
                        if tx.send(Incoming::Packet(Box::new(p))).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let msg = if e.kind() == std::io::ErrorKind::UnexpectedEof { "End of stream".to_string() } else { e.to_string() };
                        let _ = tx.send(Incoming::Closed(msg));
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Net {
            writer,
            rx,
            entity_id: 0,
            uuid: parse_uuid(&ok.uuid),
            last_x: 0.,
            last_y: 0.,
            last_z: 0.,
            last_yaw: 0.,
            last_pitch: 0.,
            position_update_ticks: 0,
            server_sprint: false,
            server_sneak: false,
            current_item: 0,
            closed: None,
            done_loading_terrain: false,
        })
    }

    pub fn send(&mut self, p: PlayServerbound) {
        if self.closed.is_none() {
            if let Err(e) = self.writer.send_play(&p) {
                self.closed = Some(e.to_string());
            }
        }
    }

    /// Everything received since the last call.
    pub fn poll(&mut self) -> Vec<Incoming> {
        self.rx.try_iter().collect()
    }

    pub fn disconnect(&mut self) {
        self.writer.shutdown();
    }

    /// `NetHandlerPlayClient.handleJoinGame`'s replies: brand and client settings.
    pub fn send_join_replies(&mut self, locale: &str, view_distance: i8, chat_mode: i8, chat_colors: bool, skin_parts: u8) {
        let mut brand = Vec::new();
        let b = b"vanilla";
        brand.push(b.len() as u8);
        brand.extend_from_slice(b);
        self.send(PlayServerbound::PluginMessageC(PluginMessageC { channel: "MC|Brand".into(), data: brand }));
        self.send_settings(locale, view_distance, chat_mode, chat_colors, skin_parts);
    }

    /// `GameSettings.sendSettingsToServer` (C15).
    pub fn send_settings(&mut self, locale: &str, view_distance: i8, chat_mode: i8, chat_colors: bool, skin_parts: u8) {
        self.send(PlayServerbound::ClientSettings(ClientSettings { locale: locale.into(), view_distance, chat_mode, chat_colors, skin_parts }));
    }

    /// `handlePlayerPosLook`: applies the (possibly relative) position and answers with C06.
    ///
    /// Every axis the packet gives as absolute also zeroes that axis of the player's motion
    /// (`NetHandlerPlayClient.handlePlayerPosLook`), so a teleport does not carry momentum.
    /// On the first teleport after joining, `prev_pos` is snapped to the new position like
    /// vanilla's `!doneLoadingTerrain` branch, which keeps the camera from interpolating from
    /// wherever the client happened to be.
    pub fn handle_pos_look(&mut self, p: &PlayerPosLookS, pos: &mut Vec3, motion: &mut Vec3, prev_pos: &mut Vec3, yaw: &mut f32, pitch: &mut f32) {
        let relative = |bit: u8| p.flags & bit != 0;
        let x = if relative(1) { pos.x + p.x } else { p.x };
        let y = if relative(2) { pos.y + p.y } else { p.y };
        let z = if relative(4) { pos.z + p.z } else { p.z };
        if !relative(1) {
            motion.x = 0.;
        }
        if !relative(2) {
            motion.y = 0.;
        }
        if !relative(4) {
            motion.z = 0.;
        }
        let pt = if p.flags & 16 != 0 { *pitch + p.pitch } else { p.pitch };
        let yw = if p.flags & 8 != 0 { *yaw + p.yaw } else { p.yaw };
        // A hostile server can send NaN/infinite/astronomical values; the player's yaw-wrapping
        // loops never finish for those (a freeze), so keep everything in a sane range.
        let coord = |v: f64, old: f64| if v.is_finite() { v.clamp(-3.0e7, 3.0e7) } else { old };
        let (x, y, z) = (coord(x, pos.x), coord(y, pos.y), coord(z, pos.z));
        let pt = if pt.is_finite() { pt.clamp(-90.0, 90.0) } else { *pitch };
        let yw = if !yw.is_finite() { *yaw } else if yw.abs() > 36000.0 { yw % 360.0 } else { yw };
        *pos = Vec3::new(x, y, z);
        *yaw = yw;
        *pitch = pt;
        // The reply uses getEntityBoundingBox().minY, which equals posY right after setPosition.
        self.send(PlayServerbound::PlayerPosLook(PlayerPosLook { x, y, z, yaw: yw, pitch: pt, on_ground: false }));
        if !self.done_loading_terrain {
            *prev_pos = *pos;
            self.done_loading_terrain = true;
        }
    }

    /// `EntityPlayerSP.onUpdateWalkingPlayer`
    ///
    /// The sprint/sneak `EntityAction`s are sent before the view-entity check, so they still go
    /// out while the `S43` camera watches another entity. A riding player sends the
    /// `C06(motionX, -999, motionZ)` form instead of its own position and does not refresh the
    /// reported position, exactly like vanilla.
    #[allow(clippy::too_many_arguments)]
    pub fn update_walking(
        &mut self,
        sprinting: bool,
        sneaking: bool,
        current_view: bool,
        riding: Option<(f64, f64)>,
        x: f64,
        min_y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
    ) {
        if sprinting != self.server_sprint {
            let action = if sprinting { EntityActionKind::StartSprinting } else { EntityActionKind::StopSprinting };
            self.send(PlayServerbound::EntityAction(EntityAction { entity_id: self.entity_id, action, aux: 0 }));
            self.server_sprint = sprinting;
        }
        if sneaking != self.server_sneak {
            let action = if sneaking { EntityActionKind::StartSneaking } else { EntityActionKind::StopSneaking };
            self.send(PlayServerbound::EntityAction(EntityAction { entity_id: self.entity_id, action, aux: 0 }));
            self.server_sneak = sneaking;
        }
        if !current_view {
            return;
        }
        let (d0, d1, d2) = (x - self.last_x, min_y - self.last_y, z - self.last_z);
        let d3 = (yaw - self.last_yaw) as f64;
        let d4 = (pitch - self.last_pitch) as f64;
        let mut moved = d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4 || self.position_update_ticks >= 20;
        let rotated = d3 != 0.0 || d4 != 0.0;
        if let Some((mx, mz)) = riding {
            self.send(PlayServerbound::PlayerPosLook(PlayerPosLook { x: mx, y: -999.0, z: mz, yaw, pitch, on_ground }));
            moved = false;
        } else {
            let p = match (moved, rotated) {
                (true, true) => PlayServerbound::PlayerPosLook(PlayerPosLook { x, y: min_y, z, yaw, pitch, on_ground }),
                (true, false) => PlayServerbound::PlayerPosition(PlayerPosition { x, y: min_y, z, on_ground }),
                (false, true) => PlayServerbound::PlayerLook(PlayerLook { yaw, pitch, on_ground }),
                (false, false) => PlayServerbound::PlayerGround(PlayerGround { on_ground }),
            };
            self.send(p);
        }
        self.position_update_ticks += 1;
        if moved {
            self.last_x = x;
            self.last_y = min_y;
            self.last_z = z;
            self.position_update_ticks = 0;
        }
        if rotated {
            self.last_yaw = yaw;
            self.last_pitch = pitch;
        }
    }

    /// `EntityPlayerSP.onUpdate` uses this path while riding instead of onUpdateWalkingPlayer.
    pub fn update_riding(
        &mut self,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
        strafe: f32,
        forward: f32,
        jump: bool,
        sneak: bool,
    ) {
        self.send(PlayServerbound::PlayerLook(PlayerLook {
            yaw,
            pitch,
            on_ground,
        }));
        self.send(PlayServerbound::SteerVehicle(SteerVehicle {
            strafe,
            forward,
            flags: (jump as u8) | ((sneak as u8) << 1),
        }));
    }

    /// `PlayerControllerMP.syncCurrentPlayItem`
    pub fn sync_current_item(&mut self, slot: i32) {
        if slot != self.current_item {
            self.current_item = slot;
            self.send(PlayServerbound::HeldItemChangeC(HeldItemChangeC { slot: slot as i16 }));
        }
    }
}

/// A dashed (or plain) hex UUID string.
pub fn parse_uuid(s: &str) -> mc_protocol::Uuid {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return mc_protocol::Uuid(0, 0);
    }
    mc_protocol::Uuid(u64::from_str_radix(&hex[..16], 16).unwrap_or(0), u64::from_str_radix(&hex[16..], 16).unwrap_or(0))
}

fn unpack(n: &[u8; 2048], out: &mut [u8; 4096]) {
    for (i, v) in out.iter_mut().enumerate() {
        *v = (n[i >> 1] >> ((i & 1) * 4)) & 15;
    }
}

/// `Chunk.fillChunk`: a full packet replaces every section (unmasked ones become empty); a
/// partial one only overwrites the masked sections of the existing chunk.
pub fn fill_chunk(existing: Option<Chunk>, x: i32, z: i32, wire: WireChunk, full: bool) -> Chunk {
    let mut c = match existing {
        Some(c) if !full => c,
        _ => Chunk::new(x, z),
    };
    for (i, ws) in wire.sections.into_iter().enumerate() {
        match ws {
            Some(ws) => {
                let mut s = Section::empty();
                s.blocks.copy_from_slice(&ws.blocks[..]);
                unpack(&ws.block_light, &mut s.block_light);
                if let Some(sl) = &ws.sky_light {
                    unpack(sl, &mut s.sky_light);
                }
                s.recount();
                c.sections[i] = Some(s);
            }
            None if full => c.sections[i] = None,
            None => {}
        }
    }
    if let Some(b) = wire.biomes {
        c.biomes = *b;
    }
    c.terrain_populated = true;
    c.light_populated = true;
    c.generate_height_map();
    c
}

/// `Slot` to the client's `(catalogue index, count)`.
pub fn slot_stack(cat: &crate::inventory::Catalogue, s: &Option<mc_protocol::Slot>) -> Option<mc_gui::ItemStack> {
    let s = s.as_ref()?;
    if s.id < 0 || s.count <= 0 {
        return None;
    }
    let item = cat.index_of(s.id as u16, s.damage.max(0) as u16).or_else(|| cat.index_of(s.id as u16, 0))?;
    Some(mc_gui::ItemStack { item, count: s.count as u8, damage: s.damage, tag: s.nbt.clone().map(mc_core::nbt::Interned::new), obj: mc_gui::ItemStack::fresh_obj() })
}

/// The client's stack as a wire `Slot`.
pub fn stack_slot(cat: &crate::inventory::Catalogue, s: Option<mc_gui::ItemStack>) -> Option<mc_protocol::Slot> {
    let s = s?;
    let d = &cat.items[s.item];
    Some(mc_protocol::Slot { id: d.id as i16, count: s.count as i8, damage: s.damage, nbt: s.tag.map(|t| (*t.get()).clone()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_protocol::chunk::{encode, WireSection};

    #[test]
    fn fill_chunk_reads_blocks_and_light() {
        let mut ws = WireSection::empty(true);
        ws.blocks[0] = 1 << 4; // stone at (0,0,0) of section 2
        ws.block_light[0] = 0x5f;
        ws.sky_light.as_mut().unwrap()[0] = 0x0a;
        let mut sections: [Option<WireSection>; 16] = Default::default();
        sections[2] = Some(ws);
        let (mask, data) = encode(&sections, Some(&[4u8; 256]), true);
        let wire = mc_protocol::chunk::decode(mask, &data, true, true).unwrap();
        let c = fill_chunk(None, 3, -2, wire, true);
        assert_eq!(c.get(0, 32, 0), (1, 0));
        let s = c.sections[2].as_ref().unwrap();
        assert_eq!((s.block_light[0], s.block_light[1], s.sky_light[0]), (15, 5, 10));
        assert_eq!(c.biomes[7], 4);
        assert_eq!(c.height(0, 0), 33);
    }

    /// A loopback server that completes the login and hands back the first `collect`
    /// serverbound play packets, so assertions run on the real encoded bytes.
    fn loopback(collect: usize) -> (Net, std::thread::JoinHandle<Vec<PlayServerbound>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let srv = std::thread::spawn(move || {
            use mc_protocol::frame::Decoder;
            use std::io::{Read, Write};
            let (mut s, _) = l.accept().unwrap();
            let mut d = Decoder::new();
            let mut buf = [0u8; 4096];
            let mut got = Vec::new();
            let mut stage = 0;
            loop {
                while let Ok(Some(p)) = d.next_packet() {
                    if stage == 0 {
                        stage = 1; // handshake
                    } else if stage == 1 {
                        stage = 2; // login start
                        let mut out = Vec::new();
                        mc_protocol::frame::encode(&LoginClientbound::LoginSuccess(LoginSuccess { uuid: "0-0".into(), name: "a".into() }).to_bytes(), -1, &mut out);
                        s.write_all(&out).unwrap();
                    } else {
                        got.push(PlayServerbound::read(&p).unwrap());
                        if got.len() == collect {
                            return got;
                        }
                    }
                }
                let n = s.read(&mut buf).unwrap();
                if n == 0 { return got; }
                d.feed(&buf[..n]);
            }
        });
        (Net::connect("127.0.0.1", port, "a", None, &Default::default(), &Default::default(), &Default::default()).unwrap(), srv)
    }

    /// Replays Java's actual EntityPlayerSP packet stream through the client movement path.
    #[test]
    fn walking_stream_matches_java() {
        use mc_core::{Aabb, BlockPos};
        use mc_data::geometry::{BlockGeometry, CollisionEntity};
        use mc_data::{BlockAccess, StateId};
        use mc_entity::{EntityWorld, Keys, PlayerEntity};
        use std::collections::HashMap;
        struct Scene {
            states: HashMap<BlockPos, StateId>,
            geometry: BlockGeometry,
        }
        struct Blocks<'a>(&'a HashMap<BlockPos, StateId>);
        impl BlockAccess for Blocks<'_> {
            fn state(&self, p: BlockPos) -> StateId {
                self.0.get(&p).copied().unwrap_or(StateId::AIR)
            }
        }
        impl EntityWorld for Scene {
            fn state(&self, p: BlockPos) -> StateId {
                self.states.get(&p).copied().unwrap_or(StateId::AIR)
            }
            fn is_chunk_loaded(&self, _: i32, _: i32) -> bool {
                true
            }
            fn add_collision_boxes(
                &mut self,
                s: StateId,
                p: BlockPos,
                mask: &Aabb,
                out: &mut Vec<Aabb>,
            ) {
                self.geometry.add_collision_boxes(
                    s,
                    &Blocks(&self.states),
                    p,
                    mask,
                    out,
                    CollisionEntity::Other,
                );
            }
        }
        fn num(v: &serde_json::Value) -> i64 {
            v.as_i64().unwrap_or_else(|| v.as_f64().unwrap() as i64)
        }
        let Some(path) = mc_testutil::golden("packets.json") else {
            return;
        };
        let scenes: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for sc in scenes.iter().filter(|s| {
            [
                "walk_turn_stop",
                "sprint_toggles",
                "jump_sprint_jump",
                "sneak_walk",
            ]
            .contains(&s["name"].as_str().unwrap())
        }) {
            let name = sc["name"].as_str().unwrap();
            let mut world = Scene {
                states: HashMap::new(),
                geometry: BlockGeometry::new(),
            };
            for b in sc["blocks"].as_array().unwrap() {
                let n: Vec<i64> = b.as_array().unwrap().iter().map(num).collect();
                for x in n[0]..=n[3] {
                    for y in n[1]..=n[4] {
                        for z in n[2]..=n[5] {
                            world.states.insert(
                                BlockPos::new(x as i32, y as i32, z as i32),
                                StateId(n[6] as u16),
                            );
                        }
                    }
                }
            }
            let start = &sc["start"];
            let bits = |v: &serde_json::Value| f64::from_bits(num(v) as u64);
            let mut e =
                PlayerEntity::new(Vec3::new(bits(&start[0]), bits(&start[1]), bits(&start[2])));
            e.yaw = f32::from_bits(num(&sc["yaw"]) as u32);
            e.pitch = f32::from_bits(num(&sc["pitch"]) as u32);
            let rows = sc["out"].as_array().unwrap();
            let count: usize = rows.iter().map(|r| r.as_array().unwrap().len()).sum();
            let (mut net, srv) = loopback(usize::MAX);
            net.entity_id = 1;
            for tick in sc["script"].as_array().unwrap() {
                let k = num(&tick["k"]);
                let has = |b| k & b != 0;
                if !tick["y"].is_null() {
                    e.yaw = f32::from_bits(num(&tick["y"]) as u32);
                }
                if !tick["p"].is_null() {
                    e.pitch = f32::from_bits(num(&tick["p"]) as u32);
                }
                e.tick(
                    &mut world,
                    Keys {
                        forward: has(1),
                        back: has(2),
                        left: has(4),
                        right: has(8),
                        jump: has(16),
                        sneak: has(32),
                        sprint: has(64),
                        using_item: false,
                    },
                );
                net.update_walking(
                    e.sprinting,
                    e.is_sneaking(),
                    true,
                    None,
                    e.pos.x,
                    e.bb.min_y,
                    e.pos.z,
                    e.yaw,
                    e.pitch,
                    e.on_ground,
                );
            }
            net.disconnect();
            let packets = srv.join().unwrap();
            assert_eq!(packets.len(), count, "{name}: packet count");
            let mut index = 0;
            for (tick, row) in rows.iter().enumerate() {
                for want in row.as_array().unwrap() {
                    let got: String = packets[index]
                        .to_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    assert_eq!(
                        got,
                        want.as_str().unwrap(),
                        "{name} tick {tick} packet {index}"
                    );
                    index += 1;
                }
            }
        }
    }

    #[test]
    fn mining_stream_matches_java() {
        use crate::*;
        let Some(path) = mc_testutil::golden("packets.json") else {
            return;
        };
        let Some(jar) = mc_testutil::jar() else {
            return;
        };
        let scenes: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for name in [
            "mine_to_completion",
            "mine_cancel_and_retarget",
            "mine_instant_stone_creative",
            "swing_air_left_click_counter",
            "place_blocks_faces",
            "place_side_face_vectors",
            "sword_block",
            "eat_apple",
            "bow_draw_release",
            "hotbar_swap",
        ] {
            let sc = scenes.iter().find(|s| s["name"] == name).unwrap();
            let mut assets = AssetSource::new();
            assets.push(&jar).unwrap();
            let models = BlockModels::load(&assets);
            let mut world = World::default();
            for x in -2..=1 {
                for z in -2..=1 {
                    world.insert(mc_world::chunk::Chunk::new(x, z));
                }
            }
            let num =
                |v: &serde_json::Value| v.as_i64().unwrap_or_else(|| v.as_f64().unwrap() as i64);
            for b in sc["blocks"].as_array().unwrap() {
                let n: Vec<i64> = b.as_array().unwrap().iter().map(num).collect();
                for x in n[0]..=n[3] {
                    for y in n[1]..=n[4] {
                        for z in n[2]..=n[5] {
                            world.set_block_state(
                                BlockPos::new(x as i32, y as i32, z as i32),
                                mc_data::StateId(n[6] as u16),
                            );
                        }
                    }
                }
            }
            let mut app = App::new(assets, models, world, (0, 0), Settings::default());
            app.entity.set_game_type(if sc["mode"] == "creative" {
                crate::level::GameType::Creative
            } else {
                crate::level::GameType::Survival
            });
            app.entity.food_level = num(&sc["food"]) as i32;
            app.player.food = num(&sc["food"]) as i32;
            for item in sc["inventory"].as_array().unwrap() {
                let slot = num(&item[0]) as usize;
                let index = app
                    .catalogue
                    .find(item[1].as_str().unwrap(), num(&item[3]) as u16)
                    .unwrap();
                app.ui.inventory[slot] = Some(app.catalogue.stack(index, num(&item[2]) as u8));
            }
            app.entity.set_position(mc_core::Vec3::new(0.5, 64., 0.5));
            app.entity.yaw = 0.;
            app.entity.prev_yaw = 0.;
            app.entity.pitch = 0.;
            app.entity.prev_pitch = 0.;
            app.entity.on_ground = false;
            app.entity.abilities_dirty = false;
            app.grabbed = true;
            app.in_game = true;
            let rows = sc["out"].as_array().unwrap();
            let count: usize = rows.iter().map(|r| r.as_array().unwrap().len()).sum();
            let (mut net, srv) = loopback(usize::MAX);
            net.entity_id = 1;
            app.net = Some(net);
            for tk in sc["script"].as_array().unwrap() {
                if !tk["y"].is_null() {
                    app.entity.yaw = f32::from_bits(num(&tk["y"]) as u32);
                }
                if !tk["p"].is_null() {
                    app.entity.pitch = f32::from_bits(num(&tk["p"]) as u32);
                }
                if !tk["s"].is_null() {
                    app.pending_selected_slot = Some(num(&tk["s"]) as usize);
                }
                app.keys.clear();
                let k = num(&tk["k"]);
                for (bit, key) in [
                    (1, KeyCode::KeyW),
                    (2, KeyCode::KeyS),
                    (4, KeyCode::KeyA),
                    (8, KeyCode::KeyD),
                    (16, KeyCode::Space),
                    (32, KeyCode::ShiftLeft),
                    (64, KeyCode::ControlLeft),
                ] {
                    if k & bit != 0 {
                        app.keys.insert(key);
                    }
                }
                app.buttons.clear();
                if tk["ad"].as_bool().unwrap() {
                    app.buttons.insert(MouseButton::Left);
                }
                if tk["ud"].as_bool().unwrap() {
                    app.buttons.insert(MouseButton::Right);
                }
                for _ in 0..num(&tk["ap"]) {
                    app.clicks.push(MouseButton::Left);
                }
                for _ in 0..num(&tk["up"]) {
                    app.clicks.push(MouseButton::Right);
                }
                app.run_tick();
            }
            app.net.as_mut().unwrap().disconnect();
            let packets = srv.join().unwrap();
            assert_eq!(packets.len(), count, "{name}: packet count");
            let mut index = 0;
            for (tick, row) in rows.iter().enumerate() {
                for want in row.as_array().unwrap() {
                    let got: String = packets
                        .get(index)
                        .unwrap_or_else(|| {
                            panic!(
                                "missing packet {index} at tick {tick}; got {} of {count}",
                                packets.len()
                            )
                        })
                        .to_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    assert_eq!(
                        got,
                        want.as_str().unwrap(),
                        "{name} tick {tick} packet {index}"
                    );
                    index += 1;
                }
            }
        }
    }

    #[test]
    fn walking_packets_follow_vanilla_thresholds() {
        let (mut n, srv) = loopback(4);
        n.update_walking(true, false, true, None, 0.0, 64.0, 0.0, 0.0, 0.0, true); // sprint + moved
        n.update_walking(true, false, true, None, 0.01, 64.0, 0.0, 0.0, 0.0, true); // below threshold
        n.update_walking(true, false, true, None, 0.01, 64.0, 0.0, 5.0, 0.0, false); // look only
        let got = srv.join().unwrap();
        assert!(matches!(got[0], PlayServerbound::EntityAction(EntityAction { action: EntityActionKind::StartSprinting, .. })));
        assert!(matches!(got[1], PlayServerbound::PlayerPosition(PlayerPosition { y, .. }) if y == 64.0));
        assert!(matches!(got[2], PlayServerbound::PlayerGround(PlayerGround { on_ground: true })));
        assert!(matches!(got[3], PlayServerbound::PlayerLook(PlayerLook { yaw, on_ground: false, .. }) if yaw == 5.0));
    }

    #[test]
    fn riding_tick_sends_look_then_input() {
        let (mut n, srv) = loopback(2);
        n.update_riding(30.0, 10.0, true, -0.5, 0.75, true, true);
        let got = srv.join().unwrap();
        assert!(matches!(
            got[0],
            PlayServerbound::PlayerLook(PlayerLook {
                yaw: 30.0,
                pitch: 10.0,
                on_ground: true
            })
        ));
        assert!(matches!(
            got[1],
            PlayServerbound::SteerVehicle(SteerVehicle {
                strafe: -0.5,
                forward: 0.75,
                flags: 3
            })
        ));
    }

    /// `ridingEntity != null`: `C06PacketPlayerPosLook(motionX, -999, motionZ, yaw, pitch,
    /// onGround)` every tick, and `flag2` is forced false so the reported position stays stale.
    #[test]
    fn riding_sends_motion_with_minus_999_y() {
        let (mut n, srv) = loopback(2);
        n.update_walking(false, false, true, None, 1.0, 64.0, 2.0, 0.0, 0.0, true);
        n.update_walking(false, false, true, Some((0.25, -0.5)), 9.0, 80.0, 9.0, 30.0, 10.0, false);
        let got = srv.join().unwrap();
        assert!(matches!(got[0], PlayServerbound::PlayerPosition(_)));
        match &got[1] {
            PlayServerbound::PlayerPosLook(p) => {
                assert_eq!((p.x, p.y, p.z), (0.25, -999.0, -0.5));
                assert_eq!((p.yaw, p.pitch, p.on_ground), (30.0, 10.0, false));
            }
            other => panic!("expected C06, got {other:?}"),
        }
    }

    /// `isCurrentViewEntity()`: while the `S43` camera watches someone else, only the sprint and
    /// sneak entity actions go out (they are sent before the guard).
    #[test]
    fn spectating_sends_only_entity_actions() {
        let (mut n, srv) = loopback(2);
        n.update_walking(true, true, false, None, 5.0, 70.0, 5.0, 45.0, 45.0, true);
        n.update_walking(true, true, true, None, 5.0, 70.0, 5.0, 45.0, 45.0, true);
        let got = srv.join().unwrap();
        assert!(matches!(got[0], PlayServerbound::EntityAction(EntityAction { action: EntityActionKind::StartSprinting, .. })));
        assert!(matches!(got[1], PlayServerbound::EntityAction(EntityAction { action: EntityActionKind::StartSneaking, .. })));
    }

    /// `handlePlayerPosLook` zeroes the motion of every absolute axis, answers with a C06 whose
    /// `onGround` is hard-coded false, and snaps `prevPos` on the first teleport only.
    #[test]
    fn pos_look_zeroes_absolute_axes_and_snaps_prev_pos() {
        let (mut n, srv) = loopback(2);
        let mut pos = Vec3::new(10., 64., 10.);
        let mut prev = Vec3::new(9., 63., 9.);
        let mut motion = Vec3::new(0.5, -0.25, 0.75);
        let (mut yaw, mut pitch) = (0.0f32, 0.0f32);
        // Absolute y only, relative x/z: only motionY is cleared.
        let p = PlayerPosLookS { x: 1.0, y: 70.0, z: 2.0, yaw: 90.0, pitch: 15.0, flags: 1 | 4 };
        n.handle_pos_look(&p, &mut pos, &mut motion, &mut prev, &mut yaw, &mut pitch);
        assert_eq!(pos, Vec3::new(11., 70., 12.));
        assert_eq!(motion, Vec3::new(0.5, 0., 0.75));
        assert_eq!((yaw, pitch), (90.0, 15.0));
        // First teleport snaps prevPos; the second one leaves it alone.
        assert_eq!(prev, pos);
        // All three axes absolute this time: every motion component is cleared.
        let p2 = PlayerPosLookS { x: 0.0, y: 0.0, z: 0.0, yaw: 0.0, pitch: 0.0, flags: 0 };
        n.handle_pos_look(&p2, &mut pos, &mut motion, &mut prev, &mut yaw, &mut pitch);
        assert_eq!(pos, Vec3::new(0., 0., 0.));
        assert_eq!(motion, Vec3::new(0., 0., 0.));
        assert_eq!(prev, Vec3::new(11., 70., 12.), "prevPos stays at the first teleport");
        let got = srv.join().unwrap();
        for p in &got {
            assert!(matches!(p, PlayServerbound::PlayerPosLook(PlayerPosLook { on_ground: false, .. })));
        }
        assert!(matches!(got[0], PlayServerbound::PlayerPosLook(PlayerPosLook { x: 11.0, y: 70.0, z: 12.0, .. })));
    }
}
