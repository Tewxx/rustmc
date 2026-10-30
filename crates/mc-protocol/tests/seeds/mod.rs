#![allow(dead_code, unused_imports)]
use mc_core::nbt::{Compound, Tag};
use mc_core::BlockPos;
use mc_protocol::chunk::{self, WireSection};
use mc_protocol::packets::*;
use mc_protocol::{MetaValue, Slot, Uuid};


pub fn item() -> Option<Slot> {
    let mut tag = Compound::new();
    tag.insert("display".into(), Tag::Compound({
        let mut d = Compound::new();
        d.insert("Name".into(), Tag::String("Sword".into()));
        d
    }));
    Some(Slot { id: 276, count: 1, damage: 12, nbt: Some(tag) })
}

pub fn uuid() -> Uuid {
    Uuid::offline_player("Steve")
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

