//! The multiplayer side of the client: `NetHandlerPlayClient` packet plumbing, the
//! `Chunk.fillChunk` conversion, and `EntityPlayerSP.onUpdateWalkingPlayer`'s movement packets.
use mc_core::Vec3;
use mc_protocol::chunk::WireChunk;
use mc_protocol::net::{login, Writer};
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
}

impl Net {
    /// `GuiConnecting`: blocks until login succeeds, then reads packets on a thread.
    pub fn connect(host: &str, port: u16, name: &str, session: Option<&crate::auth::Session>) -> Result<Net, String> {
        let mut join = |hash: &str| match session {
            Some(s) => crate::auth::join_server(s, hash),
            None => Err("Not authenticated with Minecraft.net (use --online)".into()),
        };
        let (mut reader, writer, ok) = login(host, port, name, Some(&mut join)).map_err(|e| match e {
            mc_protocol::net::LoginError::Kicked(json) => json,
            mc_protocol::net::LoginError::Io(e) => format!("Connection refused: {e}"),
            mc_protocol::net::LoginError::Auth(s) | mc_protocol::net::LoginError::Protocol(s) => s,
        })?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("Netty Client IO".into())
            .spawn(move || loop {
                match reader.recv_play() {
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
    pub fn handle_pos_look(&mut self, p: &PlayerPosLookS, pos: &mut Vec3, yaw: &mut f32, pitch: &mut f32) {
        let x = if p.flags & 1 != 0 { pos.x + p.x } else { p.x };
        let y = if p.flags & 2 != 0 { pos.y + p.y } else { p.y };
        let z = if p.flags & 4 != 0 { pos.z + p.z } else { p.z };
        let pt = if p.flags & 16 != 0 { *pitch + p.pitch } else { p.pitch };
        let yw = if p.flags & 8 != 0 { *yaw + p.yaw } else { p.yaw };
        *pos = Vec3::new(x, y, z);
        *yaw = yw;
        *pitch = pt;
        // The reply uses getEntityBoundingBox().minY, which equals posY right after setPosition.
        self.send(PlayServerbound::PlayerPosLook(PlayerPosLook { x, y, z, yaw: yw, pitch: pt, on_ground: false }));
    }

    /// `EntityPlayerSP.onUpdateWalkingPlayer`
    #[allow(clippy::too_many_arguments)]
    pub fn update_walking(&mut self, sprinting: bool, sneaking: bool, x: f64, min_y: f64, z: f64, yaw: f32, pitch: f32, on_ground: bool) {
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
        let (d0, d1, d2) = (x - self.last_x, min_y - self.last_y, z - self.last_z);
        let d3 = (yaw - self.last_yaw) as f64;
        let d4 = (pitch - self.last_pitch) as f64;
        let moved = d0 * d0 + d1 * d1 + d2 * d2 > 9.0E-4 || self.position_update_ticks >= 20;
        let rotated = d3 != 0.0 || d4 != 0.0;
        let p = match (moved, rotated) {
            (true, true) => PlayServerbound::PlayerPosLook(PlayerPosLook { x, y: min_y, z, yaw, pitch, on_ground }),
            (true, false) => PlayServerbound::PlayerPosition(PlayerPosition { x, y: min_y, z, on_ground }),
            (false, true) => PlayServerbound::PlayerLook(PlayerLook { yaw, pitch, on_ground }),
            (false, false) => PlayServerbound::PlayerGround(PlayerGround { on_ground }),
        };
        self.send(p);
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

    #[test]
    fn walking_packets_follow_vanilla_thresholds() {
        // Exercised through a loopback socket so the bytes go through the real writer.
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
                        if got.len() == 4 {
                            return got;
                        }
                    }
                }
                let n = s.read(&mut buf).unwrap();
                d.feed(&buf[..n]);
            }
        });
        let mut n = Net::connect("127.0.0.1", port, "a", None).unwrap();
        n.update_walking(true, false, 0.0, 64.0, 0.0, 0.0, 0.0, true); // sprint + moved
        n.update_walking(true, false, 0.01, 64.0, 0.0, 0.0, 0.0, true); // below threshold
        n.update_walking(true, false, 0.01, 64.0, 0.0, 5.0, 0.0, false); // look only
        let got = srv.join().unwrap();
        assert!(matches!(got[0], PlayServerbound::EntityAction(EntityAction { action: EntityActionKind::StartSprinting, .. })));
        assert!(matches!(got[1], PlayServerbound::PlayerPosition(PlayerPosition { y, .. }) if y == 64.0));
        assert!(matches!(got[2], PlayServerbound::PlayerGround(PlayerGround { on_ground: true })));
        assert!(matches!(got[3], PlayServerbound::PlayerLook(PlayerLook { yaw, on_ground: false, .. }) if yaw == 5.0));
    }
}
