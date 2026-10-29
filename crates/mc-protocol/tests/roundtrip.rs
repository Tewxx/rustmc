use mc_core::nbt::{Compound, Tag};
use mc_core::BlockPos;
use mc_protocol::chunk::{self, WireSection};
use mc_protocol::frame;
use mc_protocol::packets::*;
use mc_protocol::{MetaValue, Slot, Uuid};

fn item() -> Option<Slot> {
    let mut tag = Compound::new();
    tag.insert("display".into(), Tag::Compound({
        let mut d = Compound::new();
        d.insert("Name".into(), Tag::String("Sword".into()));
        d
    }));
    Some(Slot { id: 276, count: 1, damage: 12, nbt: Some(tag) })
}

fn uuid() -> Uuid {
    Uuid::offline_player("Steve")
}

fn check_c(p: PlayClientbound) {
    let bytes = p.to_bytes();
    let back = PlayClientbound::read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.name()));
    assert_eq!(back, p, "{}", p.name());
    assert_eq!(back.to_bytes(), bytes);
}

fn check_s(p: PlayServerbound) {
    let bytes = p.to_bytes();
    let back = PlayServerbound::read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.name()));
    assert_eq!(back, p, "{}", p.name());
}

pub fn serverbound() -> Vec<PlayServerbound> {
    let pos = BlockPos::new(-123, 64, 4567);
    vec![
        KeepAliveC { key: 99 }.into(),
        ChatMessage { message: "/give @p stone".into() }.into(),
        UseEntity { entity_id: 5, action: UseEntityAction::Attack, hit: None }.into(),
        UseEntity { entity_id: 5, action: UseEntityAction::InteractAt, hit: Some((0.5, 1.25, -0.5)) }.into(),
        PlayerGround { on_ground: true }.into(),
        PlayerPosition { x: 1.5, y: 64.0, z: -3.25, on_ground: false }.into(),
        PlayerLook { yaw: 90.0, pitch: -45.0, on_ground: true }.into(),
        PlayerPosLook { x: 1.0, y: 2.0, z: 3.0, yaw: 4.0, pitch: 5.0, on_ground: true }.into(),
        PlayerDigging { status: DiggingStatus::StopDestroyBlock, pos, face: 1 }.into(),
        BlockPlacement { pos, direction: 1, item: item(), cursor: (0.5, 1.0, 0.0625) }.into(),
        HeldItemChangeC { slot: 8 }.into(),
        AnimationC {}.into(),
        EntityAction { entity_id: 1, action: EntityActionKind::StartSprinting, aux: 0 }.into(),
        SteerVehicle { strafe: 0.98, forward: -0.98, flags: 3 }.into(),
        CloseWindowC { window_id: 1 }.into(),
        ClickWindow { window_id: 0, slot: 36, button: 1, action: 7, mode: 0, item: item() }.into(),
        ConfirmTransactionC { window_id: 0, action: 7, accepted: true }.into(),
        CreativeInventoryAction { slot: 36, item: None }.into(),
        EnchantItem { window_id: 2, enchantment: 1 }.into(),
        UpdateSignC { pos, lines: ["\"a\"".into(), "\"b\"".into(), "\"\"".into(), "\"d\"".into()] }.into(),
        PlayerAbilitiesC { flags: 6, fly_speed: 0.05, walk_speed: 0.1 }.into(),
        TabCompleteC { text: "/tp ".into(), looking_at: Some(pos) }.into(),
        TabCompleteC { text: "hi".into(), looking_at: None }.into(),
        ClientSettings { locale: "en_US".into(), view_distance: 12, chat_mode: 0, chat_colors: true, skin_parts: 0x7f }.into(),
        ClientStatus { status: ClientStatusKind::PerformRespawn }.into(),
        PluginMessageC { channel: "MC|Brand".into(), data: b"\x07vanilla".to_vec() }.into(),
        Spectate { target: uuid() }.into(),
        ResourcePackStatus { hash: "abc".into(), result: ResourcePackResult::Accepted }.into(),
    ]
}

#[test]
fn every_serverbound_play_packet_round_trips() {
    let all = serverbound();
    let mut ids: Vec<i32> = all.iter().map(|p| p.id()).collect();
    ids.dedup();
    assert_eq!(ids, (0..=0x19).collect::<Vec<_>>());
    for p in all {
        check_s(p);
    }
}

pub fn clientbound() -> Vec<PlayClientbound> {
    let pos = BlockPos::new(30_000_000 - 1, 255, -30_000_000);
    let meta = vec![
        (0, MetaValue::Byte(0x02)),
        (1, MetaValue::Short(300)),
        (6, MetaValue::Float(20.0)),
        (2, MetaValue::String("Steve".into())),
        (10, MetaValue::Item(item())),
        (11, MetaValue::Pos(1, 2, 3)),
        (12, MetaValue::Rotations(0.0, 90.0, 180.0)),
        (16, MetaValue::Int(-1)),
    ];
    let mut sec = WireSection::empty(true);
    sec.blocks[0] = 7 << 4;
    sec.blocks[4095] = 2 << 4;
    chunk::set_nibble(sec.sky_light.as_mut().unwrap(), 4095, 15);
    let mut sections: [Option<WireSection>; 16] = Default::default();
    sections[0] = Some(sec.clone());
    sections[4] = Some(sec);
    let (mask, data) = chunk::encode(&sections, Some(&[1u8; 256]), true);
    vec![
        KeepAliveS { key: 1 }.into(),
        JoinGame {
            entity_id: 1,
            game_mode: 0,
            hardcore: true,
            dimension: -1,
            difficulty: 2,
            max_players: 20,
            level_type: "default".into(),
            reduced_debug_info: false,
        }
        .into(),
        ChatS { json: "\"hi\"".into(), position: 2 }.into(),
        TimeUpdate { world_age: 100, time_of_day: -6000 }.into(),
        EntityEquipment { entity_id: 3, slot: 4, item: item() }.into(),
        SpawnPosition { pos }.into(),
        UpdateHealth { health: 20.0, food: 20, saturation: 5.0 }.into(),
        Respawn { dimension: 0, difficulty: 1, game_mode: 1, level_type: "flat".into() }.into(),
        PlayerPosLookS { x: 0.5, y: 70.0, z: 0.5, yaw: 0.0, pitch: 0.0, flags: 0x18 }.into(),
        HeldItemChangeS { slot: 3 }.into(),
        UseBed { entity_id: 1, pos }.into(),
        AnimationS { entity_id: 1, animation: 0 }.into(),
        SpawnPlayer { entity_id: 2, uuid: uuid(), x: 32, y: 2048, z: -32, yaw: -128, pitch: 10, current_item: 0, metadata: meta.clone() }.into(),
        CollectItem { collected_id: 5, collector_id: 1 }.into(),
        SpawnObject { entity_id: 9, kind: 2, x: 1, y: 2, z: 3, pitch: 0, yaw: 0, data: 1, velocity: (100, -200, 300) }.into(),
        SpawnObject { entity_id: 9, kind: 2, x: 1, y: 2, z: 3, pitch: 0, yaw: 0, data: 0, velocity: (0, 0, 0) }.into(),
        SpawnMob { entity_id: 7, kind: 54, x: 1, y: 2, z: 3, yaw: 1, pitch: 2, head_pitch: 3, vx: 4, vy: 5, vz: 6, metadata: meta.clone() }.into(),
        SpawnPainting { entity_id: 8, title: "Kebab".into(), pos, direction: 2 }.into(),
        SpawnExperienceOrb { entity_id: 10, x: 1, y: 2, z: 3, count: 7 }.into(),
        EntityVelocity { entity_id: 1, vx: -8000, vy: 0, vz: 8000 }.into(),
        DestroyEntities { ids: vec![1, 2, 300000] }.into(),
        EntityIdle { entity_id: 1 }.into(),
        EntityRelMove { entity_id: 1, dx: -1, dy: 2, dz: 127, on_ground: true }.into(),
        EntityLook { entity_id: 1, yaw: 64, pitch: -10, on_ground: false }.into(),
        EntityLookMove { entity_id: 1, dx: 1, dy: 2, dz: 3, yaw: 4, pitch: 5, on_ground: true }.into(),
        EntityTeleport { entity_id: 1, x: 1, y: 2, z: 3, yaw: 4, pitch: 5, on_ground: false }.into(),
        EntityHeadLook { entity_id: 1, yaw: 90 }.into(),
        EntityStatus { entity_id: 1, status: 2 }.into(),
        AttachEntity { entity_id: 1, vehicle_id: -1, leash: 0 }.into(),
        EntityMetadata { entity_id: 1, metadata: meta }.into(),
        EntityEffect { entity_id: 1, effect_id: 1, amplifier: 2, duration: 600, hide_particles: 0 }.into(),
        RemoveEntityEffect { entity_id: 1, effect_id: 1 }.into(),
        SetExperience { bar: 0.5, level: 30, total: 1395 }.into(),
        EntityProperties {
            entity_id: 1,
            attributes: vec![AttributeSnapshot {
                key: "generic.movementSpeed".into(),
                base: 0.1,
                modifiers: vec![AttributeModifier { uuid: uuid(), amount: 0.3, operation: 2 }],
            }],
        }
        .into(),
        ChunkData { x: -5, z: 9, full: true, bitmask: mask, data: data.clone() }.into(),
        MultiBlockChange { chunk_x: 1, chunk_z: -1, records: vec![(0x1F40, 1 << 4), (0, 0)] }.into(),
        BlockChange { pos, state: 35 << 4 | 14 }.into(),
        BlockAction { pos, byte1: 1, byte2: 1, block_id: 54 }.into(),
        BlockBreakAnim { breaker_id: 1, pos, stage: 9 }.into(),
        MapChunkBulk { sky_light: true, chunks: vec![(0, 0, mask, data.clone()), (1, 0, mask, data)] }.into(),
        Explosion { x: 0.5, y: 64.0, z: 0.5, strength: 4.0, records: vec![(1, -1, 0)], player_motion: (0.1, 0.2, 0.3) }.into(),
        Effect { effect_id: 2001, pos, data: 1, global: false }.into(),
        SoundEffect { name: "dig.stone".into(), x: 8, y: 512, z: 8, volume: 1.0, pitch: 63 }.into(),
        Particles { particle_id: 37, long_distance: false, pos: (1.0, 2.0, 3.0), offset: (0.0, 0.0, 0.0), speed: 0.1, count: 10, args: vec![1] }.into(),
        ChangeGameState { reason: 3, value: 1.0 }.into(),
        SpawnGlobalEntity { entity_id: 11, kind: 1, x: 1, y: 2, z: 3 }.into(),
        OpenWindow { window_id: 1, kind: "minecraft:chest".into(), title: "\"Chest\"".into(), slots: 27, entity_id: 0 }.into(),
        OpenWindow { window_id: 2, kind: "EntityHorse".into(), title: "\"Horse\"".into(), slots: 2, entity_id: 42 }.into(),
        CloseWindowS { window_id: 1 }.into(),
        SetSlot { window_id: -1, slot: -1, item: item() }.into(),
        WindowItems { window_id: 0, items: vec![None, item(), None] }.into(),
        WindowProperty { window_id: 1, property: 0, value: 200 }.into(),
        ConfirmTransactionS { window_id: 0, action: 3, accepted: false }.into(),
        UpdateSignS { pos, lines: ["\"\"".into(), "\"x\"".into(), "\"\"".into(), "\"\"".into()] }.into(),
        Maps {
            map_id: 0,
            scale: 0,
            icons: vec![MapIcon { kind: 0, x: 10, z: -10, direction: 8 }],
            columns: 128,
            rows: 128,
            x: 0,
            z: 0,
            data: vec![4; 16384],
        }
        .into(),
        Maps { map_id: 0, scale: 1, icons: vec![], columns: 0, rows: 0, x: 0, z: 0, data: vec![] }.into(),
        UpdateTileEntity { pos, action: 1, nbt: item().unwrap().nbt }.into(),
        SignEditorOpen { pos }.into(),
        Statistics { entries: vec![("stat.jump".into(), 4)] }.into(),
        PlayerListItem {
            action: PlayerListAction::AddPlayer,
            entries: vec![PlayerListEntry {
                uuid: uuid(),
                name: "Steve".into(),
                properties: vec![ProfileProperty { name: "textures".into(), value: "e30=".into(), signature: Some("sig".into()) }],
                game_mode: 0,
                ping: 12,
                display_name: None,
            }],
        }
        .into(),
        PlayerListItem { action: PlayerListAction::UpdateLatency, entries: vec![PlayerListEntry { uuid: uuid(), ping: 50, ..Default::default() }] }.into(),
        PlayerListItem { action: PlayerListAction::RemovePlayer, entries: vec![PlayerListEntry { uuid: uuid(), ..Default::default() }] }.into(),
        PlayerAbilitiesS { flags: 13, fly_speed: 0.05, walk_speed: 0.1 }.into(),
        TabCompleteS { matches: vec!["/time".into(), "/tp".into()] }.into(),
        ScoreboardObjective { name: "kills".into(), mode: 0, display_name: "Kills".into(), render_type: "integer".into() }.into(),
        ScoreboardObjective { name: "kills".into(), mode: 1, display_name: String::new(), render_type: String::new() }.into(),
        UpdateScore { name: "Steve".into(), action: ScoreAction::Change, objective: "kills".into(), value: 3 }.into(),
        UpdateScore { name: "Steve".into(), action: ScoreAction::Remove, objective: "kills".into(), value: 0 }.into(),
        DisplayScoreboard { position: 1, name: "kills".into() }.into(),
        Teams {
            name: "red".into(),
            mode: 0,
            display_name: "Red".into(),
            prefix: "§c".into(),
            suffix: String::new(),
            friendly_flags: 3,
            name_tag_visibility: "always".into(),
            color: 12,
            players: vec!["Steve".into()],
        }
        .into(),
        Teams { name: "red".into(), mode: 1, ..Default::default() }.into(),
        PluginMessageS { channel: "MC|Brand".into(), data: b"\x07vanilla".to_vec() }.into(),
        Disconnect { reason: "\"bye\"".into() }.into(),
        ServerDifficulty { difficulty: 3 }.into(),
        CombatEvent::EntityDied { player_id: 1, entity_id: -1, message: "died".into() }.into(),
        Camera { entity_id: 1 }.into(),
        WorldBorder::Initialize {
            x: 0.0,
            z: 0.0,
            old_diameter: 6e7,
            new_diameter: 6e7,
            speed: 0,
            portal_teleport_boundary: 29999984,
            warning_distance: 5,
            warning_time: 15,
        }
        .into(),
        Title::Times { fade_in: 10, stay: 70, fade_out: 20 }.into(),
        SetCompression { threshold: 256 }.into(),
        PlayerListHeaderFooter { header: "\"h\"".into(), footer: "\"f\"".into() }.into(),
        ResourcePackSend { url: "https://example.com/p.zip".into(), hash: "0123".into() }.into(),
        UpdateEntityNbt { entity_id: 1, nbt: None }.into(),
    ]
}

#[test]
fn every_clientbound_play_packet_round_trips() {
    let all = clientbound();
    let mut ids: Vec<i32> = all.iter().map(|p| p.id()).collect();
    ids.dedup();
    assert_eq!(ids, (0..=0x49).collect::<Vec<_>>());
    for p in all {
        check_c(p);
    }
}

#[test]
fn login_and_status_round_trip() {
    let h = HandshakeServerbound::from(Handshake { protocol_version: 47, address: "localhost".into(), port: 25565, next_state: 2 });
    assert_eq!(HandshakeServerbound::read(&h.to_bytes()).unwrap(), h);
    for p in [
        LoginClientbound::from(LoginSuccess { uuid: uuid().to_string(), name: "Steve".into() }),
        EnableCompression { threshold: 256 }.into(),
        EncryptionRequest { server_id: String::new(), public_key: vec![1, 2, 3], verify_token: vec![4; 4] }.into(),
        LoginDisconnect { reason: "\"no\"".into() }.into(),
    ] {
        assert_eq!(LoginClientbound::read(&p.to_bytes()).unwrap(), p);
    }
    let s = StatusClientbound::from(StatusResponse { json: "{}".into() });
    assert_eq!(StatusClientbound::read(&s.to_bytes()).unwrap(), s);
    assert_eq!(StatusServerbound::read(&[0x00]).unwrap(), StatusServerbound::StatusRequest(StatusRequest {}));
}

#[test]
fn trailing_bytes_and_bad_ids_are_errors() {
    let mut b = PlayServerbound::from(KeepAliveC { key: 1 }).to_bytes();
    b.push(0);
    assert!(matches!(PlayServerbound::read(&b), Err(mc_protocol::Error::Trailing(1))));
    assert!(matches!(PlayServerbound::read(&[0x1a]), Err(mc_protocol::Error::BadPacketId(0x1a))));
    // Chat messages are limited to 100 characters.
    let long = PlayServerbound::from(ChatMessage { message: "a".repeat(101) }).to_bytes();
    assert!(PlayServerbound::read(&long).is_err());
}

#[test]
fn framing_with_and_without_compression() {
    let small = PlayClientbound::from(KeepAliveS { key: 7 }).to_bytes();
    let big = PlayClientbound::from(ChatS { json: "x".repeat(5000), position: 0 }).to_bytes();
    for threshold in [-1, 0, 256] {
        let mut stream = Vec::new();
        frame::encode(&small, threshold, &mut stream);
        frame::encode(&big, threshold, &mut stream);
        let mut d = frame::Decoder::new();
        d.threshold = threshold;
        // Feed one byte at a time to exercise partial frames.
        let mut got = Vec::new();
        for b in &stream {
            d.feed(std::slice::from_ref(b));
            while let Some(p) = d.next_packet().unwrap() {
                got.push(p);
            }
        }
        assert_eq!(got, vec![small.clone(), big.clone()], "threshold {threshold}");
        if threshold == 256 {
            assert!(stream.len() < big.len(), "large packet should be compressed");
            // Uncompressed packets below the threshold carry a 0 data length.
            assert_eq!(&stream[..3], &[3, 0, 0x00]);
        }
    }
}

#[test]
fn chunk_payload_round_trips() {
    let mut s = WireSection::empty(true);
    for i in 0..4096 {
        s.blocks[i] = ((i % 200) as u16) << 4 | (i % 16) as u16;
        chunk::set_nibble(&mut s.block_light, i, (i % 16) as u8);
        chunk::set_nibble(s.sky_light.as_mut().unwrap(), i, 15 - (i % 16) as u8);
    }
    let mut sections: [Option<WireSection>; 16] = Default::default();
    sections[3] = Some(s.clone());
    sections[15] = Some(s);
    let biomes = [21u8; 256];
    let (mask, data) = chunk::encode(&sections, Some(&biomes), true);
    assert_eq!(mask, 1 << 3 | 1 << 15);
    assert_eq!(data.len(), chunk::data_size(2, true, true));
    let back = chunk::decode(mask, &data, true, true).unwrap();
    assert_eq!(back.sections, sections);
    assert_eq!(back.biomes.as_deref(), Some(&biomes));
    // Little-endian block chars, low nibble first.
    assert_eq!(&data[2..4], &[(1u16 << 4 | 1).to_le_bytes()[0], 0]);
}

/// Vanilla reads and re-writes every sample packet (`oracle/PacketOracle.java`); the bytes must
/// match ours exactly. Skipped when Java 8 or the MCP classes are not available.
#[test]
fn vanilla_reencodes_every_packet_identically() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cp_file = root.join("../../tools/oracle/cp.txt");
    let java = std::path::PathBuf::from(r"C:\Program Files\Eclipse Adoptium\jdk-8.0.472.8-hotspot\bin");
    let Ok(cp) = std::fs::read_to_string(&cp_file) else {
        eprintln!("skipping: no {}", cp_file.display());
        return;
    };
    if !java.join("java.exe").exists() {
        eprintln!("skipping: no Java 8");
        return;
    }
    let out_dir = root.join("../../target/protocol-oracle");
    std::fs::create_dir_all(&out_dir).unwrap();
    let status = std::process::Command::new(java.join("javac.exe"))
        .args(["-nowarn", "-cp", cp.trim(), "-d"])
        .arg(&out_dir)
        .arg(root.join("oracle/PacketOracle.java"))
        .status()
        .unwrap();
    assert!(status.success(), "javac failed");

    let mut input = Vec::new();
    let mut expected = Vec::new();
    for p in clientbound() {
        expected.push((0u8, p.name(), p.to_bytes()));
    }
    for p in serverbound() {
        expected.push((1u8, p.name(), p.to_bytes()));
    }
    for (dir, _, b) in &expected {
        input.push(*dir);
        input.extend_from_slice(&(b.len() as u32).to_be_bytes());
        input.extend_from_slice(b);
    }
    let (inp, outp) = (out_dir.join("in.bin"), out_dir.join("out.bin"));
    std::fs::write(&inp, &input).unwrap();
    let status = std::process::Command::new(java.join("java.exe"))
        .arg("-cp")
        .arg(format!("{};{}", out_dir.display(), cp.trim()))
        .arg("PacketOracle")
        .arg(&inp)
        .arg(&outp)
        .status()
        .unwrap();
    assert!(status.success(), "oracle failed");
    let got = std::fs::read(&outp).unwrap();
    let mut p = 0;
    let mut bad = Vec::new();
    for (dir, name, want) in &expected {
        let d = got[p];
        p += 1;
        if d == 255 {
            let n = u16::from_be_bytes([got[p], got[p + 1]]) as usize;
            bad.push(format!("{name}: vanilla error {}", String::from_utf8_lossy(&got[p + 2..p + 2 + n])));
            p += 2 + n;
            continue;
        }
        let n = u32::from_be_bytes(got[p..p + 4].try_into().unwrap()) as usize;
        let b = &got[p + 4..p + 4 + n];
        p += 4 + n;
        assert_eq!(d, *dir);
        if b != &want[..] {
            let at = b.iter().zip(want.iter()).position(|(x, y)| x != y).unwrap_or(b.len().min(want.len()));
            bad.push(format!("{name}: differs at byte {at} (vanilla {} bytes, ours {})", b.len(), want.len()));
        }
    }
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
    eprintln!("{} packets identical to vanilla", expected.len());
}

/// Deterministic xorshift so failures reproduce.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    /// Bytes biased towards values that stress lengths and varints.
    fn byte(&mut self) -> u8 {
        match self.below(6) {
            0 => 0,
            1 => 0xff,
            2 => 0x7f,
            3 => 0x80,
            _ => self.next() as u8,
        }
    }
}

/// Random and mutated packet streams must produce errors, never panics.
#[test]
fn fuzz_decoders_never_panic() {
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let seeds: Vec<Vec<u8>> = clientbound().iter().map(|p| p.to_bytes()).collect();
    let sb: Vec<Vec<u8>> = serverbound().iter().map(|p| p.to_bytes()).collect();
    let mut ok = 0u32;
    for i in 0..300_000u32 {
        let mut b: Vec<u8> = match i % 3 {
            0 => {
                let n = rng.below(64);
                let mut v = vec![rng.below(0x49) as u8];
                v.extend((0..n).map(|_| rng.byte()));
                v
            }
            1 => seeds[rng.below(seeds.len())].clone(),
            _ => sb[rng.below(sb.len())].clone(),
        };
        if i % 3 != 0 {
            for _ in 0..1 + rng.below(4) {
                match rng.below(4) {
                    0 if !b.is_empty() => {
                        let k = rng.below(b.len());
                        b[k] = rng.byte();
                    }
                    1 if !b.is_empty() => {
                        let k = rng.below(b.len());
                        b.truncate(k);
                    }
                    2 => {
                        let k = rng.below(b.len() + 1);
                        b.insert(k, rng.byte());
                    }
                    _ if !b.is_empty() => {
                        let k = rng.below(b.len());
                        b.remove(k);
                    }
                    _ => {}
                }
            }
        }
        if PlayClientbound::read(&b).is_ok() {
            ok += 1;
        }
        let _ = PlayServerbound::read(&b);
        let _ = LoginClientbound::read(&b);
        let _ = StatusClientbound::read(&b);
    }
    assert!(ok > 0);
}

/// Hostile frame streams: wild lengths, bogus compression headers, garbage zlib.
#[test]
fn fuzz_frame_decoder_never_panics() {
    let mut rng = Rng(0xD1B54A32D192ED03);
    for i in 0..100_000u32 {
        let mut d = frame::Decoder::new();
        d.threshold = if i % 2 == 0 { -1 } else { rng.below(300) as i32 - 1 };
        let n = rng.below(48);
        let data: Vec<u8> = (0..n).map(|_| rng.byte()).collect();
        let mut from = 0;
        while from < data.len() {
            let to = (from + 1 + rng.below(8)).min(data.len());
            d.feed(&data[from..to]);
            from = to;
            while let Ok(Some(_)) = d.next_packet() {}
        }
    }
    // Valid zlib header, truncated / lying about size.
    let mut d = frame::Decoder::new();
    d.threshold = 10;
    d.feed(&[6, 0xff, 0xff, 0x7f, 0x78, 0x9c, 0x00]);
    assert!(d.next_packet().is_err());
}

#[test]
fn varints_past_five_bytes_error() {
    assert!(mc_protocol::Reader::new(&[0x80; 8]).varint().is_err());
    assert!(mc_protocol::Reader::new(&[0x80; 12]).varlong().is_err());
}
