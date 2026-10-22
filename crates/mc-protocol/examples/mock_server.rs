//! A minimal offline-mode 1.8.9 server for testing the client: flat stone/grass chunks, a
//! position, keep-alives, chat echo, and a log of every serverbound play packet.
//!
//! `cargo run --release --offline -p mc-protocol --example mock_server -- [port]`
use mc_core::BlockPos;
use mc_protocol::chunk::{encode, WireSection};
use mc_protocol::frame::{self, Decoder};
use mc_protocol::packets::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn send(s: &Mutex<TcpStream>, threshold: i32, body: &[u8]) {
    let mut out = Vec::new();
    frame::encode(body, threshold, &mut out);
    let _ = s.lock().unwrap().write_all(&out);
}

fn flat_chunk() -> (u16, Vec<u8>) {
    let mut sections: [Option<WireSection>; 16] = Default::default();
    let mut s = WireSection::empty(true);
    for i in 0..4096 {
        let y = i >> 8;
        s.blocks[i] = match y {
            0 => 7 << 4,
            1..=2 => 1 << 4,
            3 => 2 << 4,
            _ => 0,
        };
    }
    // Full sky light above the grass.
    for (i, b) in s.sky_light.as_mut().unwrap().iter_mut().enumerate() {
        *b = if (i * 2) >> 8 >= 4 { 0xff } else { 0 };
    }
    sections[0] = Some(s);
    encode(&sections, Some(&[1u8; 256]), true)
}

fn handle(stream: TcpStream) {
    stream.set_nodelay(true).unwrap();
    let w = Arc::new(Mutex::new(stream.try_clone().unwrap()));
    let mut r = stream;
    let mut d = Decoder::new();
    let mut buf = [0u8; 8192];
    let mut stage = 0;
    let threshold = 256;
    let start = Instant::now();
    loop {
        while let Ok(Some(p)) = d.next_packet() {
            match stage {
                0 => stage = 1,
                1 => {
                    let LoginServerbound::LoginStart(ls) = LoginServerbound::read(&p).unwrap() else { return };
                    println!("login: {}", ls.name);
                    send(&w, -1, &LoginClientbound::EnableCompression(EnableCompression { threshold }).to_bytes());
                    d.threshold = threshold;
                    send(&w, threshold, &LoginClientbound::LoginSuccess(LoginSuccess { uuid: "00000000-0000-3000-8000-000000000000".into(), name: ls.name }).to_bytes());
                    let play = |p: PlayClientbound| send(&w, threshold, &p.to_bytes());
                    play(PlayClientbound::JoinGame(JoinGame { entity_id: 42, game_mode: 0, hardcore: false, dimension: 0, difficulty: 2, max_players: 20, level_type: "flat".into(), reduced_debug_info: false }));
                    play(PlayClientbound::SpawnPosition(SpawnPosition { pos: BlockPos::new(8, 4, 8) }));
                    play(PlayClientbound::PlayerAbilitiesS(PlayerAbilitiesS { flags: 0, fly_speed: 0.05, walk_speed: 0.1 }));
                    let (mask, data) = flat_chunk();
                    let mut chunks = Vec::new();
                    for x in -4..4 {
                        for z in -4..4 {
                            chunks.push((x, z, mask, data.clone()));
                        }
                    }
                    play(PlayClientbound::MapChunkBulk(MapChunkBulk { sky_light: true, chunks }));
                    play(PlayClientbound::TimeUpdate(TimeUpdate { world_age: 100, time_of_day: 6000 }));
                    play(PlayClientbound::PlayerPosLookS(PlayerPosLookS { x: 8.5, y: 4.0, z: 8.5, yaw: 0.0, pitch: 0.0, flags: 0 }));
                    if std::env::var_os("MOCK_WATER").is_some() {
                        // Sunken stone basin: water replaces the grass layer (top at 3.889,
                        // just below the grass rim at 4.0), a lava pool to one side, and a
                        // waterfall column at the far edge (double-sided sides).
                        for x in 4..12 {
                            for z in 7..15 {
                                play(PlayClientbound::BlockChange(BlockChange { pos: BlockPos::new(x, 2, z), state: 1 }));
                                play(PlayClientbound::BlockChange(BlockChange { pos: BlockPos::new(x, 3, z), state: 9 }));
                            }
                        }
                        for y in 4..7 {
                            play(PlayClientbound::BlockChange(BlockChange { pos: BlockPos::new(8, y, 14), state: 9 }));
                        }
                        for x in 1..4 {
                            for z in 8..12 {
                                play(PlayClientbound::BlockChange(BlockChange { pos: BlockPos::new(x, 2, z), state: 1 }));
                                play(PlayClientbound::BlockChange(BlockChange { pos: BlockPos::new(x, 3, z), state: 11 }));
                            }
                        }
                        play(PlayClientbound::PlayerPosLookS(PlayerPosLookS { x: 8.5, y: 5.0, z: 5.0, yaw: 0.0, pitch: 25.0, flags: 0 }));
                    }
                    if std::env::var_os("MOCK_TILES").is_some() {
                        // Chests on the grass at y = 4, facing south at a camera standing south
                        // of them: a single chest, a double chest along +X, a trapped chest whose
                        // lid is opened with block event 1, and an ender chest.
                        let mut tile = |pos: BlockPos, id: i32, meta: i32| {
                            play(PlayClientbound::BlockChange(BlockChange { pos, state: id + (meta << 12) }));
                            play(PlayClientbound::UpdateTileEntity(UpdateTileEntity { pos, action: 0, nbt: Some(mc_core::nbt::Compound::new()) }));
                        };
                        tile(BlockPos::new(4, 4, 8), 54, 3);
                        tile(BlockPos::new(6, 4, 8), 54, 3);
                        tile(BlockPos::new(7, 4, 8), 54, 3);
                        tile(BlockPos::new(9, 4, 8), 146, 3);
                        tile(BlockPos::new(11, 4, 8), 130, 3);
                        // `TileEntityChest.receiveClientEvent(1, 1)`: one player has it open.
                        play(PlayClientbound::BlockAction(BlockAction { pos: BlockPos::new(9, 4, 8), byte1: 1, byte2: 1, block_id: 146 }));
                        play(PlayClientbound::PlayerPosLookS(PlayerPosLookS { x: 8.5, y: 5.2, z: 12.5, yaw: 180.0, pitch: 12.0, flags: 0 }));
                    }
                    play(PlayClientbound::ChatS(ChatS { json: r#"{"text":"Welcome to the ","color":"yellow","extra":[{"text":"mock server","bold":true}]}"#.into(), position: 0 }));
                    play(PlayClientbound::ChatS(ChatS { json: r#"{"text":"action bar"}"#.into(), position: 2 }));
                    play(PlayClientbound::BlockChange(BlockChange { pos: BlockPos::new(10, 4, 8), state: 57 }));
                    // A second player 3 blocks ahead, holding a sword, walking and swinging.
                    let uuid = mc_protocol::Uuid(7, 7);
                    play(PlayClientbound::PlayerListItem(PlayerListItem {
                        action: PlayerListAction::AddPlayer,
                        entries: vec![PlayerListEntry {
                            uuid,
                            name: "Notch".into(),
                            properties: skin_property(),
                            ..Default::default()
                        }],
                    }));
                    play(PlayClientbound::SpawnPlayer(SpawnPlayer {
                        entity_id: 7,
                        uuid,
                        x: (8.5 * 32.0) as i32,
                        y: 4 * 32,
                        z: (11.5 * 32.0) as i32,
                        yaw: -128,
                        pitch: 0,
                        current_item: 276,
                        metadata: vec![(0, mc_protocol::MetaValue::Byte(0))],
                    }));
                    play(PlayClientbound::SpawnObject(SpawnObject { entity_id: 8, kind: 2, x: (7.5 * 32.0) as i32, y: 4 * 32, z: (10.5 * 32.0) as i32, pitch: 0, yaw: 0, data: 1, velocity: (0, 0, 0) }));
                    play(PlayClientbound::EntityMetadata(EntityMetadata {
                        entity_id: 8,
                        metadata: vec![(10, mc_protocol::MetaValue::Item(Some(mc_protocol::Slot { id: 264, count: 3, damage: 0, nbt: None })))],
                    }));
                    if std::env::var_os("MOCK_FX").is_some() {
                        // Speed II, Regeneration, Poison, Jump Boost; 20 -> 13 damage; 4 absorption; diamond chestplate.
                        for (id, amp, dur) in [(1i8, 1i8, 2400), (10, 0, 900), (19, 0, 32767), (8, 2, 61)] {
                            play(PlayClientbound::EntityEffect(EntityEffect { entity_id: 42, effect_id: id, amplifier: amp, duration: dur, hide_particles: 1 }));
                        }
                        play(PlayClientbound::UpdateHealth(UpdateHealth { health: 20.0, food: 17, saturation: 0.0 }));
                        play(PlayClientbound::SetSlot(SetSlot { window_id: 0, slot: 6, item: Some(mc_protocol::Slot { id: 311, count: 1, damage: 0, nbt: None }) }));
                        {
                            use mc_core::nbt::{Compound, Tag};
                            let ench = |id: i16, lvl: i16| Tag::Compound(Compound::from([("id".to_string(), Tag::Short(id)), ("lvl".to_string(), Tag::Short(lvl))]));
                            let mut display = Compound::new();
                            display.insert("Name".into(), Tag::String("\u{a7}6Excalibur".into()));
                            display.insert("Lore".into(), Tag::List(8, vec![Tag::String("Legendary blade".into())]));
                            let mut nbt = Compound::new();
                            nbt.insert("ench".into(), Tag::List(10, vec![ench(16, 5), ench(20, 2)]));
                            nbt.insert("display".into(), Tag::Compound(display));
                            play(PlayClientbound::SetSlot(SetSlot { window_id: 0, slot: 36, item: Some(mc_protocol::Slot { id: 276, count: 1, damage: 900, nbt: Some(nbt) }) }));
                        }
                        play(PlayClientbound::EntityMetadata(EntityMetadata { entity_id: 42, metadata: vec![(17, mc_protocol::MetaValue::Float(4.0))] }));
                        play(PlayClientbound::UpdateHealth(UpdateHealth { health: 13.0, food: 17, saturation: 0.0 }));
                    }
                    if std::env::var_os("MOCK_MOBS").is_some() {
                        // Three rows of mobs facing the player, with variant metadata.
                        use mc_protocol::MetaValue::{Byte, Float, Int, Short};
                        let rows: [&[(u8, Vec<(u8, mc_protocol::MetaValue)>)]; 3] = [
                            &[
                                (54, vec![]),
                                (54, vec![(13, Byte(1))]),
                                (54, vec![(12, Byte(1))]),
                                (57, vec![]),
                                (51, vec![]),
                                (51, vec![(13, Byte(1))]),
                                (50, vec![(16, Byte(1)), (18, Byte(1))]),
                                (58, vec![(16, Short(2)), (18, Byte(1))]),
                                (66, vec![]),
                                (120, vec![(16, Int(2))]),
                                (97, vec![]),
                            ],
                            &[
                                (52, vec![]),
                                (59, vec![]),
                                (90, vec![(16, Byte(1))]),
                                (90, vec![(12, Byte(-1))]),
                                (92, vec![]),
                                (96, vec![]),
                                (91, vec![(16, Byte(14))]),
                                (93, vec![]),
                                (95, vec![(16, Byte(5)), (18, Float(20.)), (20, Byte(4))]),
                                (98, vec![(18, Byte(1))]),
                                (101, vec![(18, Byte(3))]),
                            ],
                            &[
                                (55, vec![(16, Byte(2))]),
                                (62, vec![(16, Byte(2))]),
                                (99, vec![]),
                                (94, vec![]),
                                (65, vec![]),
                                (60, vec![]),
                                (67, vec![]),
                                (61, vec![]),
                                (56, vec![]),
                            ],
                        ];
                        // Hover above and behind the lineup, looking down at it.
                        play(PlayClientbound::PlayerAbilitiesS(PlayerAbilitiesS { flags: 6, fly_speed: 0.05, walk_speed: 0.1 }));
                        play(PlayClientbound::PlayerPosLookS(PlayerPosLookS { x: 8.5, y: 7.0, z: 10.0, yaw: 0.0, pitch: 28.0, flags: 0 }));
                        let mut id = 100;
                        for (row, mobs) in rows.iter().enumerate() {
                            let z = [14.5, 18.5, 24.][row];
                            let gap = [1.2, 1.4, 2.0][row];
                            for (i, (kind, metadata)) in mobs.iter().enumerate() {
                                let x = 8.5 + (i as f64 - (mobs.len() - 1) as f64 / 2.) * gap;
                                let y = if *kind == 56 { 8. } else if *kind == 65 { 6. } else { 4. };
                                play(PlayClientbound::SpawnMob(SpawnMob {
                                    entity_id: id,
                                    kind: *kind,
                                    x: (x * 32.) as i32,
                                    y: (y * 32.) as i32,
                                    z: (z * 32.) as i32,
                                    yaw: -128,
                                    pitch: 0,
                                    head_pitch: -128,
                                    vx: 0,
                                    vy: 0,
                                    vz: 0,
                                    metadata: metadata.clone(),
                                }));
                                if *kind == 54 && metadata.is_empty() {
                                    // Iron sword, enchanted diamond helmet, gold leggings.
                                    play(PlayClientbound::EntityEquipment(EntityEquipment { entity_id: id, slot: 0, item: Some(mc_protocol::Slot { id: 267, count: 1, damage: 0, nbt: None }) }));
                                    play(PlayClientbound::EntityEquipment(EntityEquipment { entity_id: id, slot: 2, item: Some(mc_protocol::Slot { id: 316, count: 1, damage: 0, nbt: None }) }));
                                    play(PlayClientbound::EntityEquipment(EntityEquipment { entity_id: id, slot: 4, item: Some(mc_protocol::Slot { id: 310, count: 1, damage: 0, nbt: None }) }));
                                }
                                if *kind == 51 && metadata.is_empty() {
                                    play(PlayClientbound::EntityEquipment(EntityEquipment { entity_id: id, slot: 0, item: Some(mc_protocol::Slot { id: 261, count: 1, damage: 0, nbt: None }) }));
                                }
                                if *kind == 92 {
                                    play(PlayClientbound::EntityStatus(EntityStatus { entity_id: id, status: 2 }));
                                }
                                id += 1;
                            }
                        }
                    }
                    if std::env::var_os("MOCK_SCORE").is_some() {
                        // Title, tab header/footer, a team, sidebar / list / below-name objectives.
                        play(PlayClientbound::PlayerListItem(PlayerListItem {
                            action: PlayerListAction::AddPlayer,
                            entries: vec![PlayerListEntry { uuid: mc_protocol::Uuid(0x3000, 0x8000_0000_0000_0000), name: "Player".into(), ping: 420, ..Default::default() }],
                        }));
                        play(PlayClientbound::Title(Title::Times { fade_in: 0, stay: 200, fade_out: 20 }));
                        play(PlayClientbound::Title(Title::Subtitle(r#"{"text":"to the mock server","color":"gray"}"#.into())));
                        play(PlayClientbound::Title(Title::Title(r#"{"text":"Welcome","color":"gold","bold":true}"#.into())));
                        play(PlayClientbound::PlayerListHeaderFooter(PlayerListHeaderFooter {
                            header: r#"{"text":"Mock Network","color":"aqua","bold":true}"#.into(),
                            footer: r#"{"text":"play.example.net - a long footer line that has to wrap across the list width","color":"gray"}"#.into(),
                        }));
                        play(PlayClientbound::Teams(Teams {
                            name: "red".into(),
                            mode: 0,
                            display_name: "Red".into(),
                            prefix: "\u{a7}c[R] ".into(),
                            suffix: "\u{a7}7*".into(),
                            friendly_flags: 3,
                            name_tag_visibility: "always".into(),
                            color: 12,
                            players: vec!["Notch".into()],
                        }));
                        for (name, title, kind, slot) in [("kills", "\u{a7}e\u{a7}lKILLS", "integer", 1i8), ("ping", "Ping", "integer", 0), ("hp", "\u{a7}chp", "integer", 2)] {
                            play(PlayClientbound::ScoreboardObjective(ScoreboardObjective { name: name.into(), mode: 0, display_name: title.into(), render_type: kind.into() }));
                            play(PlayClientbound::DisplayScoreboard(DisplayScoreboard { position: slot, name: name.into() }));
                        }
                        for (who, obj, v) in [("Notch", "kills", 5), ("Player", "kills", 3), ("\u{a7}aOnline: 2", "kills", 1), ("#hidden", "kills", 99), ("Notch", "ping", 35), ("Player", "ping", 420), ("Notch", "hp", 20)] {
                            play(PlayClientbound::UpdateScore(UpdateScore { name: who.into(), action: ScoreAction::Change, objective: obj.into(), value: v }));
                        }
                    }
                    if std::env::var_os("MOCK_OBJECTS").is_some() {
                        // Non-living entities: arrows, TNT, fireballs, an item frame, a
                        // painting, an invisible armour-stand hologram and an XP orb.
                        use mc_protocol::MetaValue::{Byte, Item, Rotations, String as MString};
                        play(PlayClientbound::PlayerAbilitiesS(PlayerAbilitiesS { flags: 6, fly_speed: 0.05, walk_speed: 0.1 }));
                        play(PlayClientbound::PlayerPosLookS(PlayerPosLookS { x: 8.5, y: 5.0, z: 10.5, yaw: 0.0, pitch: 8.0, flags: 0 }));
                        let obj = |id: i32, kind: i8, x: f64, y: f64, z: f64, yaw: i8, pitch: i8, data: i32| PlayClientbound::SpawnObject(SpawnObject {
                            entity_id: id,
                            kind,
                            x: (x * 32.) as i32,
                            y: (y * 32.) as i32,
                            z: (z * 32.) as i32,
                            yaw,
                            pitch,
                            data,
                            velocity: (0, 0, 0),
                        });
                        for (i, x) in [6.5f64, 7.5, 8.5].iter().enumerate() {
                            play(obj(200 + i as i32, 60, *x, 5.2, 14.5, (i as i8) * 32, 24, 0));
                        }
                        play(obj(210, 50, 8.5, 4.0, 13.5, 0, 0, 0));
                        play(obj(211, 63, 10.5, 5.5, 14.5, 0, 0, 0));
                        play(obj(212, 64, 10.5, 4.6, 14.5, 0, 0, 0));
                        // Item frame on the west face of the block at (8,5,12): facing east
                        // (horizontal index 3), so its yaw is 270.
                        play(obj(220, 71, 11.0, 5.0, 13.0, (90. * 256. / 360.) as i8, 0, 1));
                        play(PlayClientbound::EntityMetadata(EntityMetadata {
                            entity_id: 220,
                            metadata: vec![
                                (8, Item(Some(mc_protocol::Slot { id: 276, count: 1, damage: 0, nbt: None }))),
                                (9, Byte(2)),
                            ],
                        }));
                        play(PlayClientbound::SpawnPainting(SpawnPainting { entity_id: 230, title: "Aztec".into(), pos: BlockPos::new(5, 5, 13), direction: 2 }));
                        // Invisible armour stand with a two-part hologram would be two stands;
                        // one line is enough to check the name-tag path.
                        play(obj(240, 78, 8.5, 4.0, 16.5, 0, 0, 0));
                        play(PlayClientbound::EntityMetadata(EntityMetadata {
                            entity_id: 240,
                            metadata: vec![
                                (0, Byte(0x20)),
                                (2, MString("\u{a7}b\u{a7}lMOCK HOLOGRAM".into())),
                                (3, Byte(1)),
                                (10, Byte(4)),
                                (11, Rotations(0., 0., 0.)),
                            ],
                        }));
                        play(PlayClientbound::SpawnExperienceOrb(SpawnExperienceOrb { entity_id: 250, x: (6.5 * 32.) as i32, y: (4.5 * 32.) as i32, z: (13.5 * 32.) as i32, count: 7 }));
                    }
                    if std::env::var_os("MOCK_BREAK").is_some() {
                        // Another player's destroy-stage overlay on the grass block ahead.
                        play(PlayClientbound::BlockBreakAnim(BlockBreakAnim { breaker_id: 7, pos: BlockPos::new(8, 3, 12), stage: 6 }));
                    }
                    if std::env::var_os("MOCK_PARTICLES").is_some() {
                        play(PlayClientbound::PlayerPosLookS(PlayerPosLookS { x: 8.5, y: 5.2, z: 11.5, yaw: 0.0, pitch: 2.0, flags: 0 }));
                        // Keep a representative particle of every ported type alive for the
                        // screenshot: the client's 40-tick joining loop would outlive the short
                        // ones, so a background thread keeps re-emitting the batch.
                        let w3 = w.clone();
                        std::thread::spawn(move || loop {
                            let fx: [(i32, f32, f32, f32, Vec<i32>); 35] = [
                                (0, 4.5, 6.0, 13.5, vec![]),
                                (1, 6.0, 6.0, 13.5, vec![]),
                                (2, 7.5, 6.0, 13.5, vec![]),
                                (9, 9.0, 6.0, 13.5, vec![]),
                                (10, 10.5, 6.0, 13.5, vec![]),
                                (11, 12.0, 6.0, 13.5, vec![]),
                                (12, 4.5, 7.5, 13.5, vec![]),
                                (13, 6.0, 7.5, 13.5, vec![]),
                                (14, 7.5, 7.5, 13.5, vec![]),
                                (15, 9.0, 7.5, 13.5, vec![1, 0, 0]),
                                (17, 10.5, 7.5, 13.5, vec![]),
                                (26, 12.0, 7.5, 13.5, vec![]),
                                (27, 4.5, 5.0, 15.0, vec![]),
                                (30, 6.0, 5.0, 15.0, vec![]),
                                (34, 7.5, 5.0, 15.0, vec![]),
                                (20, 9.0, 5.0, 15.0, vec![]),
                                (21, 10.5, 5.0, 15.0, vec![]),
                                (22, 12.0, 5.0, 15.0, vec![]),
                                (23, 4.5, 6.5, 15.0, vec![]),
                                (24, 6.0, 6.5, 15.0, vec![]),
                                (25, 7.5, 6.5, 15.0, vec![]),
                                (3, 9.0, 6.5, 15.0, vec![]),
                                (4, 10.5, 6.5, 15.0, vec![]),
                                (5, 12.0, 6.5, 15.0, vec![]),
                                (29, 6.0, 7.5, 16.5, vec![]),
                                (39, 7.5, 7.5, 16.5, vec![]),
                                (6, 4.5, 8.0, 14.5, vec![]),
                                (7, 6.0, 8.0, 14.5, vec![]),
                                (8, 7.5, 8.0, 14.5, vec![]),
                                (18, 9.0, 8.0, 14.5, vec![]),
                                (19, 10.5, 8.0, 14.5, vec![]),
                                (31, 12.0, 8.0, 14.5, vec![]),
                                (32, 4.5, 8.0, 16.0, vec![]),
                                (33, 6.0, 8.0, 16.0, vec![]),
                                (35, 7.5, 8.0, 16.0, vec![]),
                            ];
                            for (id, x, y, z, args) in fx {
                                let p = PlayClientbound::Particles(Particles {
                                    particle_id: id,
                                    long_distance: true,
                                    pos: (x, y, z),
                                    offset: (0.2, 0.2, 0.2),
                                    speed: 0.02,
                                    count: 6,
                                    args,
                                });
                                send(&w3, threshold, &p.to_bytes());
                            }
                            // Large explosion's scale argument arrives through `speed * offset.x`.
                            let p = PlayClientbound::Particles(Particles { particle_id: 1, long_distance: true, pos: (10.5, 7.0, 15.0), offset: (1.0, 0.0, 0.0), speed: 0.1, count: 0, args: vec![] });
                            send(&w3, threshold, &p.to_bytes());
                            // S28 2001: the server's block-break effect must produce the 4x4x4
                            // `blockcrack` burst around this floating stone block.
                            let e = PlayClientbound::Effect(Effect { effect_id: 2001, pos: BlockPos::new(7, 5, 14), data: 1, global: false });
                            send(&w3, threshold, &e.to_bytes());
                            std::thread::sleep(Duration::from_millis(250));
                        });
                    }
                    if let Ok(url) = std::env::var("MOCK_RPACK") {
                        let hash = std::env::var("MOCK_RPACK_HASH").unwrap_or_default();
                        play(PlayClientbound::ResourcePackSend(ResourcePackSend { url, hash }));
                    }
                    if std::env::var_os("MOCK_CHEST").is_some() {
                        play(PlayClientbound::OpenWindow(OpenWindow { window_id: 3, kind: "minecraft:chest".into(), title: r#"{"text":"Kit Selector","color":"dark_red"}"#.into(), slots: 27, entity_id: 0 }));
                        let mut items: Vec<Option<mc_protocol::Slot>> = vec![None; 63];
                        for (i, id) in [(10usize, 276i16), (12, 322), (14, 261), (16, 310), (30, 1), (60, 364)] {
                            items[i] = Some(mc_protocol::Slot { id, count: if id == 1 { 64 } else { 1 }, damage: 0, nbt: None });
                        }
                        play(PlayClientbound::WindowItems(WindowItems { window_id: 3, items }));
                    }
                    let w2 = w.clone();
                    std::thread::spawn(move || {
                        let mut k = 0;
                        loop {
                            for t in 0..20 {
                                std::thread::sleep(Duration::from_millis(50));
                                let dx = if (k / 2) % 2 == 0 { 2 } else { -2 };
                                let _ = t;
                                send(&w2, threshold, &PlayClientbound::EntityRelMove(EntityRelMove { entity_id: 7, dx, dy: 0, dz: 0, on_ground: true }).to_bytes());
                                send(&w2, threshold, &PlayClientbound::EntityHeadLook(EntityHeadLook { entity_id: 7, yaw: -128 + 20 }).to_bytes());
                                if t % 6 == 0 {
                                    send(&w2, threshold, &PlayClientbound::AnimationS(AnimationS { entity_id: 7, animation: 0 }).to_bytes());
                                }
                            }
                            k += 1;
                            send(&w2, threshold, &PlayClientbound::KeepAliveS(KeepAliveS { key: k }).to_bytes());
                        }
                    });
                    stage = 2;
                }
                _ => match PlayServerbound::read(&p) {
                    Ok(PlayServerbound::ChatMessage(c)) => {
                        println!("[{:>6} ms] chat: {}", start.elapsed().as_millis(), c.message);
                        let json = format!(r#"{{"translate":"chat.type.text","with":["Player",{}]}}"#, serde_json_escape(&c.message));
                        send(&w, threshold, &PlayClientbound::ChatS(ChatS { json, position: 0 }).to_bytes());
                    }
                    Ok(PlayServerbound::PlayerGround(_)) | Ok(PlayServerbound::KeepAliveC(_)) => {}
                    Ok(p) => println!("[{:>6} ms] {p:?}", start.elapsed().as_millis()),
                    Err(e) => println!("bad packet: {e:?}"),
                },
            }
        }
        match r.read(&mut buf) {
            Ok(0) | Err(_) => {
                println!("client disconnected");
                return;
            }
            Ok(n) => d.feed(&buf[..n]),
        }
    }
}

fn serde_json_escape(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `MOCK_SKIN=<png>`: a textures property pointing at a tiny HTTP server on port 25600.
fn skin_property() -> Vec<mc_protocol::packets::ProfileProperty> {
    let Some(path) = std::env::var_os("MOCK_SKIN") else { return vec![] };
    let png = std::fs::read(path).unwrap();
    std::thread::spawn(move || {
        let l = TcpListener::bind("127.0.0.1:25600").unwrap();
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let head = format!("HTTP/1.1 200 OK
Content-Type: image/png
Content-Length: {}
Connection: close

", png.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&png);
        }
    });
    let json = r#"{"textures":{"SKIN":{"url":"http://127.0.0.1:25600/skin.png"}}}"#;
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut v = String::new();
    for c in json.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            v.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    vec![mc_protocol::packets::ProfileProperty { name: "textures".into(), value: v, signature: None }]
}

fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(25565);
    let l = TcpListener::bind(("127.0.0.1", port)).unwrap();
    println!("mock server on 127.0.0.1:{port}");
    for s in l.incoming().flatten() {
        std::thread::spawn(move || handle(s));
    }
}
