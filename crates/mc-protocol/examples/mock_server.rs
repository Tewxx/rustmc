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
