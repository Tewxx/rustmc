//! rustmc: a 1.8.9 client in Rust.
//!
//! Usage: rustmc [--jar <1.8.9.jar>] [--pack <zip|dir>]... [--world <save dir>] [--radius N] [--pos X Z]
//!               [--screenshot <out.png> [--screen title|pause|options|video|controls|hud|debug|chat]]
//!               [--bench <seconds>] [--play]
//! In game: WASD, Space jump, Shift sneak, Ctrl sprint; double-tap Space for creative flight.
//! E inventory, F5 perspective, mouse break/place/pick, T chat, / commands, Esc pause.

mod bench;
mod auth;
mod chat;
mod entities;
mod music;
mod net;
mod commands;
mod container;
mod icon;
mod inventory;
mod player;
mod server;
mod skins;
mod level;
mod survival;
mod tools;

use bench::Bench;
use glam::Vec3;
use mc_assets::AssetSource;
use mc_core::{nbt, BlockPos, Facing};
use mc_data::geometry::BlockGeometry;
use mc_gui::batch::GuiBatch;
use mc_gui::hud::{DebugInfo, Hud, PlayerHud};
use mc_gui::screens::{self, Action, Key, Screen};
use mc_gui::settings::Settings;
use mc_gui::{Scaled, Ui};
use mc_render::{lightmap, mesh, models::BlockModels, Camera, Renderer};
use mc_world::World;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

struct Args {
    play: bool,
    jar: Option<PathBuf>,
    packs: Vec<PathBuf>,
    world: Option<PathBuf>,
    radius: i32,
    pos: Option<(i32, i32)>,
    screenshot: Option<PathBuf>,
    screen: String,
    bench: Option<f64>,
    seed: Option<i64>,
    /// `host[:port]` of a server to join instead of a local world.
    connect: Option<String>,
    name: String,
    /// Sign in with a Microsoft account for online-mode servers.
    online: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        play: false,
        jar: None,
        packs: vec![],
        world: None,
        radius: 8,
        pos: None,
        screenshot: None,
        screen: "hud".into(),
        bench: None,
        seed: None,
        connect: None,
        name: "Player".into(),
        online: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--play" => a.play = true,
            "--jar" => a.jar = it.next().map(PathBuf::from),
            "--pack" => a.packs.extend(it.next().map(PathBuf::from)),
            "--world" => a.world = it.next().map(PathBuf::from),
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--screen" => a.screen = it.next().unwrap_or_default(),
            "--bench" => a.bench = Some(it.next().and_then(|s| s.parse().ok()).unwrap_or(30.0)),
            "--seed" => a.seed = it.next().map(|s| s.parse().unwrap_or_else(|_| java_string_hash(&s) as i64)),
            "--online" => a.online = true,
            "--connect" => a.connect = it.next(),
            "--name" => a.name = it.next().unwrap_or_else(|| "Player".into()),
            "--radius" => a.radius = it.next().and_then(|s| s.parse().ok()).unwrap_or(4),
            "--pos" => {
                let x = it.next().and_then(|s| s.parse().ok());
                let z = it.next().and_then(|s| s.parse().ok());
                if let (Some(x), Some(z)) = (x, z) {
                    a.pos = Some((x, z));
                }
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }
    a
}

fn find_jar() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("../MCP-919/jars/versions/1.8.9/1.8.9.jar"),
        PathBuf::from("MCP-919/jars/versions/1.8.9/1.8.9.jar"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        for parent in exe.ancestors().skip(1).take(6) {
            candidates.push(parent.join("MCP-919/jars/versions/1.8.9/1.8.9.jar"));
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(Path::new(&appdata).join(".minecraft/versions/1.8.9/1.8.9.jar"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Spawn X/Z from `level.dat` (gzipped NBT).
fn read_spawn(world: &Path) -> Option<(i32, i32)> {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(&std::fs::read(world.join("level.dat")).ok()?[..])
        .read_to_end(&mut raw)
        .ok()?;
    let (_, root) = nbt::read(&raw).ok()?;
    let data = root.as_compound()?.get("Data")?.as_compound()?;
    Some((data.get("SpawnX")?.as_i64()? as i32, data.get("SpawnZ")?.as_i64()? as i32))
}

/// `String.hashCode`, used for non-numeric seeds like vanilla's world creation screen.
/// Entity flags (index 0): sneaking 0x02, sprinting 0x08.
fn apply_meta(r: &mut entities::Remote, meta: &mc_protocol::Metadata) {
    for (i, v) in meta {
        if let (0, mc_protocol::MetaValue::Byte(b)) = (i, v) {
            r.sneaking = b & 2 != 0;
            r.sprinting = b & 8 != 0;
        }
    }
}

fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

fn load_world(args: &Args) -> (World, (i32, i32), i64) {
    let mut world = World::default();
    let seed = args.seed.unwrap_or_else(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        t.as_nanos() as i64
    });
    let mut generator = mc_gen::Generator::new(seed);
    if args.world.is_none() {
        println!("seed {seed}");
    }
    let center = args.pos.or_else(|| args.world.as_deref().and_then(read_spawn)).unwrap_or((8, 8));
    let (ccx, ccz) = (center.0 >> 4, center.1 >> 4);
    for cx in ccx - args.radius..=ccx + args.radius {
        for cz in ccz - args.radius..=ccz + args.radius {
            let chunk = match &args.world {
                Some(dir) => match mc_world::region::load_chunk(dir, cx, cz) {
                    Ok(Some(c)) => c,
                    Ok(None) => continue,
                    Err(e) => {
                        eprintln!("chunk {cx},{cz}: {e}");
                        continue;
                    }
                },
                None => {
                    let mut c = generator.provide_chunk(cx, cz);
                    c.terrain_populated = true;
                    c
                }
            };
            world.insert(chunk);
        }
    }
    if args.world.is_none() {
        // Chunk.populateChunk: a chunk is decorated once it and its +x, +z neighbours exist,
        // in load order (x outer, z inner).
        let t = Instant::now();
        for cx in ccx - args.radius..ccx + args.radius {
            for cz in ccz - args.radius..ccz + args.radius {
                mc_gen::populate::populate(&mut generator, &mut world, cx, cz);
            }
        }
        println!("populated chunks in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    }
    let t = Instant::now();
    let keys: Vec<(i32, i32)> = world.chunks.keys().copied().collect();
    for (cx, cz) in keys {
        world.tick_chunk_light(cx, cz, false, false);
    }
    world.track_dirty = true;
    world.take_dirty();
    println!("lit chunks in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    (world, center, seed)
}

fn find_player_spawn(world: &World, center: (i32, i32)) -> mc_core::Vec3 {
    let mut candidates = Vec::new();
    for x in center.0 - 16..=center.0 + 16 {
        for z in center.1 - 16..=center.1 + 16 {
            candidates.push((x, z));
        }
    }
    candidates.sort_by_key(|&(x, z)| (x - center.0).pow(2) + (z - center.1).pow(2));
    let mut geometry = BlockGeometry::new();
    for (x, z) in candidates {
        if !world.chunks.contains_key(&(x >> 4, z >> 4)) {
            continue;
        }
        for y in (0..254).rev() {
            if !matches!(world.get(x, y, z).0, 1 | 2 | 3 | 12 | 13 | 24 | 80) {
                continue;
            }
            let p = player::Player::new(mc_core::Vec3::new(x as f64 + 0.5, y as f64 + 1., z as f64 + 0.5));
            if world.light(x, y + 1, z).0 == 15 && player::collisions(world, &mut geometry, &p.bounds().expand(0.7, 0., 0.7)).is_empty() {
                return p.pos;
            }
        }
    }
    let y = (0..256).rev().find(|&y| world.get(center.0, y, center.1).0 != 0).unwrap_or(64);
    mc_core::Vec3::new(center.0 as f64 + 0.5, y as f64 + 1., center.1 as f64 + 0.5)
}

/// `Math.random()`
fn rand_f64() -> f64 {
    use std::cell::Cell;
    thread_local!(static STATE: Cell<u64> = Cell::new(0x2545F4914F6CDD1D ^ std::process::id() as u64));
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

fn map_key(code: KeyCode) -> Key {
    match code {
        KeyCode::KeyE => Key::Inventory,
        KeyCode::Digit1
        | KeyCode::Digit2
        | KeyCode::Digit3
        | KeyCode::Digit4
        | KeyCode::Digit5
        | KeyCode::Digit6
        | KeyCode::Digit7
        | KeyCode::Digit8
        | KeyCode::Digit9 => Key::Number(format!("{code:?}").trim_start_matches("Digit").parse::<usize>().unwrap() - 1),
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Delete => Key::Delete,
        KeyCode::Tab => Key::Tab,
        _ => Key::Other,
    }
}

struct App {
    entity: player::Player,
    spawn: mc_core::Vec3,
    catalogue: Arc<inventory::Catalogue>,
    target: Option<player::Target>,
    buttons: HashSet<MouseButton>,
    /// Presses since the last tick, handled at the start of the next one like LWJGL's event queue.
    clicks: Vec<MouseButton>,
    right_click_delay: u8,
    left_click_counter: u8,
    // PlayerControllerMP
    hitting: Option<BlockPos>,
    block_damage: f32,
    block_hit_delay: u8,
    /// Dropped item entities and the world's entity id counter.
    items: Vec<mc_entity::item::ItemEntity>,
    next_entity_id: u32,
    rand: mc_core::random::JavaRandom,
    /// Integrated server tick state (singleplayer).
    server: server::Server,
    /// Q presses since the last tick (with Ctrl held: whole stack).
    drop_presses: Vec<bool>,
    info: level::WorldInfo,
    weather: level::Weather,
    // EntityRenderer.updateTorchFlicker
    torch_flicker_x: f32,
    torch_flicker_dx: f32,
    lightmap: [u8; 1024],
    assets: AssetSource,
    models: BlockModels,
    world: World,
    geometry: BlockGeometry,
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    camera: Camera,
    ui: Ui,
    hud: Hud,
    player: PlayerHud,
    batch: GuiBatch,
    /// Open screen and the screens it was opened from (parents).
    screen: Option<Box<dyn Screen>>,
    parents: Vec<Box<dyn Screen>>,
    in_game: bool,
    keys: HashSet<KeyCode>,
    grabbed: bool,
    mouse_px: (f64, f64),
    /// Applied settings, to notice changes made in option screens.
    applied: Settings,
    last_frame: Instant,
    tick_accum: f32,
    fps_timer: Instant,
    frames: u32,
    fps: u32,
    screenshot: Option<(PathBuf, String)>,
    bench: Option<Bench>,
    /// The server connection when playing multiplayer.
    net: Option<net::Net>,
    name: String,
    remote: entities::Entities,
    music: music::MusicTicker,
    skins: skins::Skins,
    /// Inventory as last synced, to send creative edits (C10) for what changed.
    creative_sync: Option<([Option<mc_gui::ItemStack>; 36], [Option<mc_gui::ItemStack>; 4])>,
    /// The open server window (`openContainer` other than the player's).
    open_window: Option<Arc<std::sync::Mutex<container::Window>>>,
    /// Tab list names by profile UUID (`playerInfoMap`).
    player_names: std::collections::HashMap<mc_protocol::Uuid, String>,
    sound: Option<mc_sound::SoundManager>,
    /// `PlayerControllerMP.stepSoundTickCounter`
    step_sound_counter: f32,
    /// `objectMouseOver` when it is an entity.
    pointed_entity: Option<i32>,
    /// Item entities announced by `SpawnObject`, waiting for their stack: (pos, velocity).
    pending_items: std::collections::HashMap<i32, (mc_core::Vec3, mc_core::Vec3)>,
}

impl App {
    fn new(assets: AssetSource, models: BlockModels, world: World, center: (i32, i32), settings: Settings) -> Self {
        let ui = Ui::new(&assets, settings.clone());
        let catalogue = Arc::new(inventory::Catalogue::load(&assets, &models));
        let spawn = find_player_spawn(&world, center);
        let mut entity = player::Player::new(spawn);
        entity.yaw = -45.;
        entity.pitch = 20.;
        entity.prev_yaw = -45.;
        entity.prev_pitch = 20.;
        Self {
            entity,
            spawn,
            catalogue,
            target: None,
            buttons: HashSet::new(),
            clicks: Vec::new(),
            right_click_delay: 0,
            left_click_counter: 0,
            hitting: None,
            block_damage: 0.,
            block_hit_delay: 0,
            items: Vec::new(),
            next_entity_id: 1,
            rand: mc_core::random::JavaRandom::new(0x5DEECE66D),
            server: server::Server::default(),
            drop_presses: Vec::new(),
            info: level::WorldInfo::new(0, BlockPos::new(spawn.x.floor() as i32, spawn.y.floor() as i32, spawn.z.floor() as i32)),
            weather: level::Weather::default(),
            torch_flicker_x: 0.,
            torch_flicker_dx: 0.,
            lightmap: lightmap::build(1.0, 0.0, settings.gamma),
            assets,
            models,
            world,
            geometry: BlockGeometry::new(),
            renderer: None,
            window: None,
            camera: Camera {
                pos: Vec3::new(spawn.x as f32, spawn.y as f32 + 1.62, spawn.z as f32),
                yaw: -45.,
                pitch: 20.,
                fov_deg: settings.fov,
                bob: glam::Mat4::IDENTITY,
            },
            ui,
            hud: Hud::default(),
            player: PlayerHud::default(),
            batch: GuiBatch::new(),
            screen: None,
            parents: Vec::new(),
            in_game: false,
            keys: HashSet::new(),
            grabbed: false,
            mouse_px: (0., 0.),
            applied: settings,
            last_frame: Instant::now(),
            tick_accum: 0.,
            fps_timer: Instant::now(),
            frames: 0,
            fps: 0,
            screenshot: None,
            bench: None,
            net: None,
            name: "Player".into(),
            remote: entities::Entities::default(),
            music: music::MusicTicker::default(),
            skins: skins::Skins::default(),
            creative_sync: None,
            open_window: None,
            player_names: Default::default(),
            sound: None,
            step_sound_counter: 0.,
            pointed_entity: None,
            pending_items: Default::default(),
        }
    }
    fn scaled(&self) -> Scaled {
        let (w, h) = self.renderer.as_ref().map_or((1280, 720), |r| (r.config.width, r.config.height));
        Scaled::new(w, h, self.ui.settings.gui_scale)
    }

    fn mouse_gui(&self) -> (i32, i32) {
        let f = self.scaled().factor as f64;
        ((self.mouse_px.0 / f) as i32, (self.mouse_px.1 / f) as i32)
    }

    fn open(&mut self, mut s: Box<dyn Screen>, keep_parent: bool) {
        let sr = self.scaled();
        s.init(&self.ui, sr.width, sr.height);
        if let Some(old) = self.screen.take() {
            if keep_parent {
                self.parents.push(old);
            }
        }
        self.screen = Some(s);
        self.grab(false);
        self.keys.clear();
        self.buttons.clear();
    }

    fn close_screen(&mut self) {
        self.screen = self.parents.pop();
        if let Some(s) = &mut self.screen {
            let sr = Scaled::new(
                self.renderer.as_ref().map_or(1280, |r| r.config.width),
                self.renderer.as_ref().map_or(720, |r| r.config.height),
                self.ui.settings.gui_scale,
            );
            // Re-init so labels pick up settings changed in the child screen.
            s.init(&self.ui, sr.width, sr.height);
        } else if self.in_game {
            self.grab(true);
        }
    }

    fn handle(&mut self, action: Action, el: &ActiveEventLoop) {
        match action {
            Action::None => {}
            Action::Close => self.close_screen(),
            Action::Open(s) => self.open(s, true),
            Action::StartGame => {
                self.in_game = true;
                self.ui.in_world = true;
                self.parents.clear();
                self.screen = None;
                self.grab(true);
            }
            Action::QuitToTitle => {
                self.in_game = false;
                self.ui.in_world = false;
                self.parents.clear();
                self.screen = None;
                self.open(Box::new(screens::MainMenu::default()), false);
            }
            Action::Quit => el.exit(),
            Action::TabComplete(prefix) => {
                let list = commands::complete(&prefix, &self.ui.lang, self.target.map(|t| t.pos));
                if let Some(screen) = &mut self.screen {
                    if let Some(line) = screen.autocomplete(&mut self.ui, list) {
                        self.hud.add_chat(line);
                    }
                }
            }
            Action::PrintChat(line) => self.hud.add_chat(line),
            Action::SendChat(msg) => {
                self.close_screen();
                self.send_chat(&msg);
            }
            Action::Respawn if self.net.is_some() => {
                self.send(mc_protocol::packets::PlayServerbound::ClientStatus(mc_protocol::packets::ClientStatus { status: mc_protocol::packets::ClientStatusKind::PerformRespawn }));
                self.close_screen();
            }
            Action::Respawn => {
                self.entity.dead = false;
                let g = self.entity.game_type;
                self.entity.set_game_type(g);
                let at = match self.entity.spawn_point {
                    Some(p) => mc_core::Vec3::new(p.x as f64 + 0.5, p.y as f64 + 0.1, p.z as f64 + 0.5),
                    None => self.spawn,
                };
                self.entity.teleport(at);
                self.close_screen();
                self.sync_view();
            }
        }
        self.apply_settings();
    }

    /// Pushes changed settings into the renderer/camera (what `GameSettings.setOptionValue` triggers).
    fn apply_settings(&mut self) {
        let s = self.ui.settings.clone();
        self.camera.fov_deg = s.fov;
        let Some(r) = &mut self.renderer else { return };
        r.render_distance_blocks = (s.render_distance * 16) as f32;
        r.set_vsync(s.vsync);
        if s.gamma != self.applied.gamma {
            r.set_lightmap(&self.lightmap);
        }
        let (w, h) = (r.config.width, r.config.height);
        if (s.ambient_occlusion != 0) != (self.applied.ambient_occlusion != 0) || s.fancy_graphics != self.applied.fancy_graphics {
            self.remesh(s.ambient_occlusion != 0);
        }
        if s.gui_scale != self.applied.gui_scale {
            if let Some(scr) = &mut self.screen {
                let sr = Scaled::new(w, h, s.gui_scale);
                scr.init(&self.ui, sr.width, sr.height);
            }
        }
        self.applied = s;
    }

    fn remesh(&mut self, smooth: bool) {
        let Some(r) = &mut self.renderer else { return };
        let t = Instant::now();
        let keys: Vec<_> = self.world.chunks.keys().copied().collect();
        let mut quads = 0;
        for (cx, cz) in keys {
            for sy in 0..16 {
                let m = mesh::build_section(&self.world, &self.models, cx, sy, cz, smooth, self.ui.settings.fancy_graphics, &mut self.geometry);
                quads += m.indices.len() / 6;
                r.upload_section(cx, sy, cz, &m);
            }
        }
        println!(
            "meshed {} chunks ({quads} quads) in {:.1} ms",
            self.world.chunks.len(),
            t.elapsed().as_secs_f64() * 1e3
        );
    }

    fn grab(&mut self, on: bool) {
        let Some(w) = &self.window else { return };
        if on {
            let ok = w
                .set_cursor_grab(CursorGrabMode::Confined)
                .or_else(|_| w.set_cursor_grab(CursorGrabMode::Locked));
            if ok.is_err() {
                return;
            }
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
        }
        w.set_cursor_visible(!on);
        self.grabbed = on;
    }

    fn update(&mut self, dt: f32) {
        self.process_net();
        self.flush_window();
        for (cell, rgba) in self.skins.poll() {
            if let Some(r) = &self.renderer {
                r.upload_skin(cell, &rgba);
            }
        }
        self.sync_creative();
        if !self.ui.sounds.is_empty() {
            self.flush_sounds();
        }
        let paused = self.screen.as_ref().is_some_and(|s| s.pauses());
        self.tick_accum += dt * 20.0;
        while self.tick_accum >= 1.0 {
            self.tick_accum -= 1.0;
            self.hud.tick();
            if let Some(sc) = &mut self.screen {
                sc.tick();
            }
            // Minecraft.runTick: the music ticker runs in menus and in game.
            if let Some(s) = &mut self.sound {
                let kind = if !self.in_game {
                    music::MusicType::Menu
                } else if self.entity.creative && self.entity.allow_flying {
                    music::MusicType::Creative
                } else {
                    music::MusicType::Game
                };
                for c in mc_sound::Category::ALL {
                    s.volumes.insert(c, self.ui.settings.sound_volume(c.key()));
                }
                self.music.update(s, kind);
                s.update_stream_volumes();
            }
            if let Some(r) = &self.renderer {
                let uploads = self.models.atlas.tick();
                r.upload_atlas(&uploads);
            }
            if self.in_game && self.bench.is_none() && !paused {
                self.run_tick();
            }
        }
        self.ui.ticks = self.hud.update_counter as f32 + self.tick_accum;
        if self.in_game && self.bench.is_none() {
            self.sync_view();
        }
    }

    /// The in-game part of `Minecraft.runTick`: input, then `updateEquippedItem`, then entities.
    fn run_tick(&mut self) {
        // PlayerControllerMP.updateController
        let sel = self.player.selected_slot as i32;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel);
        }
        self.right_click_delay = self.right_click_delay.saturating_sub(1);
        self.left_click_counter = self.left_click_counter.saturating_sub(1);
        self.update_target(1.0);
        if self.screen.is_none() {
            for b in std::mem::take(&mut self.clicks) {
                self.interact(b);
            }
            if self.buttons.contains(&MouseButton::Right) && self.right_click_delay == 0 {
                self.interact(MouseButton::Right);
            }
            self.send_click_block(self.buttons.contains(&MouseButton::Left) && self.grabbed);
        } else {
            self.clicks.clear();
        }
        for whole in std::mem::take(&mut self.drop_presses) {
            // EntityPlayerSP.dropOneItem
            let slot = self.player.selected_slot;
            if let Some(s) = self.ui.inventory[slot] {
                let n = if whole { s.count } else { 1 };
                self.ui.inventory[slot] = (s.count > n).then_some(mc_gui::ItemStack { count: s.count - n, ..s });
                if self.net.is_some() {
                    let status = if whole { mc_protocol::packets::DiggingStatus::DropAllItems } else { mc_protocol::packets::DiggingStatus::DropItem };
                    self.send_dig(status, BlockPos::new(0, 0, 0), Facing::Down);
                } else {
                    self.throw_stack(mc_gui::ItemStack { count: n, ..s });
                }
            }
        }
        for s in std::mem::take(&mut self.ui.drops) {
            self.throw_stack(s);
        }
        let current = self.ui.inventory[self.player.selected_slot].map(|s| (s.item, s.count));
        self.entity.update_equipped(current);
        let k = |c| self.screen.is_none() && self.keys.contains(&c);
        let keys = player::Keys {
            forward: k(KeyCode::KeyW),
            back: k(KeyCode::KeyS),
            left: k(KeyCode::KeyA),
            right: k(KeyCode::KeyD),
            jump: k(KeyCode::Space),
            sneak: k(KeyCode::ShiftLeft) || k(KeyCode::ShiftRight),
            sprint: k(KeyCode::ControlLeft) || k(KeyCode::ControlRight),
            using_item: false,
        };
        // EntityPlayerSP.onUpdate only runs over a loaded chunk.
        let (px, pz) = (self.entity.pos.x.floor() as i32, self.entity.pos.z.floor() as i32);
        if self.net.is_none() || self.world.is_chunk_loaded(px >> 4, pz >> 4) {
            self.entity.tick(&self.world, &mut self.geometry, keys);
            if let Some(n) = &mut self.net {
                let e = &self.entity;
                n.update_walking(e.sprinting, e.is_sneaking(), e.pos.x, e.bb.min_y, e.pos.z, e.yaw, e.pitch, e.on_ground);
            }
        }
        self.tick_items();
        self.remote.tick();
        self.flush_sounds();
        if self.net.is_none() && self.entity.pos.y < -64. {
            self.entity.teleport(self.spawn);
        }
        // EntityRenderer.updateTorchFlicker
        let (a, b, c, d) = (rand_f64(), rand_f64(), rand_f64(), rand_f64());
        self.torch_flicker_dx = (self.torch_flicker_dx as f64 + (a - b) * c * d) as f32;
        self.torch_flicker_dx = (self.torch_flicker_dx as f64 * 0.9) as f32;
        self.torch_flicker_x += self.torch_flicker_dx - self.torch_flicker_x;
        // WorldServer.tick: weather, then the clocks.
        if self.net.is_some() {
            // WorldClient: only the strengths follow the server's raining flags.
            self.weather.tick_remote(&self.info);
        } else {
            self.weather.tick(&mut self.info, &mut self.rand);
        }
        if self.net.is_none() {
            // WorldServer.tick: tickUpdates, then updateBlocks' random ticks (chunks within 7).
            let (cx, cz) = (px >> 4, pz >> 4);
            let chunks: Vec<(i32, i32)> = (-7..=7).flat_map(|x| (-7..=7).map(move |z| (cx + x, cz + z))).filter(|&(x, z)| self.world.is_chunk_loaded(x, z)).collect();
            let speed = self.info.rules.get_int("randomTickSpeed");
            self.with_level(|lv| {
                lv.tick_updates();
                lv.random_ticks(&chunks, speed);
            });
        }
        self.info.total_time += 1;
        if self.info.rules.get_bool("doDaylightCycle") {
            self.info.time += 1;
        }
    }

    /// `NetHandlerPlayClient`: applies everything the server sent since the last frame.
    fn process_net(&mut self) {
        use mc_protocol::packets::{PlayClientbound as C, PlayServerbound as S, *};
        let Some(n) = &mut self.net else { return };
        let incoming = n.poll();
        let mut edits = Vec::new();
        for inc in incoming {
            let p = match inc {
                net::Incoming::Packet(p) => *p,
                net::Incoming::Closed(reason) => {
                    if let Some(n) = &mut self.net {
                        if n.closed.is_none() {
                            let text = chat::format(&reason, &self.ui.lang);
                            self.hud.add_chat(format!("{}: {text}", self.ui.lang.get("disconnect.lost")));
                            n.closed = Some(reason);
                        }
                    }
                    continue;
                }
            };
            let Some(n) = &mut self.net else { return };
            match p {
                C::KeepAliveS(k) => n.send(S::KeepAliveC(KeepAliveC { key: k.key })),
                C::JoinGame(j) => {
                    n.entity_id = j.entity_id;
                    let view = self.ui.settings.render_distance as i8;
                    n.send_join_replies("en_US", view, true, 0x7f);
                    self.entity.set_game_type(level::GameType::by_id(j.game_mode as i32));
                    self.info.difficulty = j.difficulty;
                    self.player.max_health = 20.;
                    self.player.experience = 0.;
                    self.player.level = 0;
                }
                C::PlayerPosLookS(pl) => {
                    let (mut pos, mut yaw, mut pitch) = (self.entity.pos, self.entity.yaw, self.entity.pitch);
                    n.handle_pos_look(&pl, &mut pos, &mut yaw, &mut pitch);
                    self.entity.set_position(pos);
                    if pl.flags & 7 == 0 {
                        self.entity.motion = mc_core::Vec3::new(0., 0., 0.);
                    }
                    self.entity.yaw = yaw;
                    self.entity.pitch = pitch;
                    self.entity.prev_yaw = yaw;
                    self.entity.prev_pitch = pitch;
                }
                C::ChunkData(c) => {
                    if c.full && c.bitmask == 0 {
                        self.unload_chunk(c.x, c.z);
                    } else if let Ok(w) = mc_protocol::chunk::decode(c.bitmask, &c.data, self.world.has_sky, c.full) {
                        let old = self.world.remove(c.x, c.z);
                        self.world.insert(net::fill_chunk(old, c.x, c.z, w, c.full));
                    }
                }
                C::MapChunkBulk(b) => {
                    for (x, z, mask, data) in b.chunks {
                        if let Ok(w) = mc_protocol::chunk::decode(mask, &data, b.sky_light, true) {
                            self.world.insert(net::fill_chunk(None, x, z, w, true));
                        }
                    }
                }
                C::BlockChange(b) => {
                    let st = mc_data::StateId::try_from_network(b.state).unwrap_or(mc_data::StateId::AIR);
                    self.world.set_block_state(b.pos, st);
                    edits.push(b.pos);
                }
                C::MultiBlockChange(m) => {
                    for (p, state) in m.records {
                        let pos = BlockPos::new(m.chunk_x * 16 + (p >> 12) as i32, (p & 255) as i32, m.chunk_z * 16 + ((p >> 8) & 15) as i32);
                        let st = mc_data::StateId::try_from_network(state).unwrap_or(mc_data::StateId::AIR);
                        self.world.set_block_state(pos, st);
                        edits.push(pos);
                    }
                }
                C::TimeUpdate(t) => {
                    // WorldClient.setWorldTime: a negative time freezes the daylight cycle.
                    self.info.total_time = t.world_age;
                    let neg = t.time_of_day < 0;
                    self.info.time = if neg { -t.time_of_day } else { t.time_of_day };
                    self.info.rules.set("doDaylightCycle", if neg { "false" } else { "true" });
                }
                C::ChatS(c) => {
                    let text = chat::format(&c.json, &self.ui.lang);
                    if c.position == 2 {
                        self.hud.set_action_bar(text);
                    } else {
                        self.hud.add_chat(text);
                    }
                }
                C::SpawnPosition(sp) => self.info.spawn = sp.pos,
                C::OpenWindow(o) => {
                    let title = chat::format(&o.title, &self.ui.lang);
                    let title = title.trim_end_matches("\u{a7}r").to_string();
                    let w = Arc::new(std::sync::Mutex::new(container::Window::new(o.window_id, &o.kind, title, o.slots)));
                    self.open_window = Some(w.clone());
                    self.parents.clear();
                    self.screen = None;
                    self.open(Box::new(container::ContainerScreen::new(w, self.catalogue.clone())), false);
                }
                C::CloseWindowS(_) => {
                    // handleCloseWindow: closeScreenAndDropStack without a packet.
                    self.open_window = None;
                    if self.screen.is_some() {
                        self.close_screen();
                    }
                }
                C::WindowProperty(p) => {
                    if let Some(w) = &self.open_window {
                        let mut w = w.lock().unwrap();
                        if w.id == p.window_id && (0..4).contains(&p.property) {
                            w.props[p.property as usize] = p.value;
                        }
                    }
                }
                C::ConfirmTransactionS(t) => {
                    if !t.accepted {
                        n.send(S::ConfirmTransactionC(ConfirmTransactionC { window_id: t.window_id, action: t.action, accepted: true }));
                    }
                }
                C::PlayerListItem(pl) => {
                    for e in pl.entries {
                        match pl.action {
                            PlayerListAction::AddPlayer => {
                                if let Some(t) = e.properties.iter().find(|p| p.name == "textures") {
                                    self.skins.request(e.uuid, &t.value);
                                }
                                self.player_names.insert(e.uuid, e.name);
                            }
                            PlayerListAction::RemovePlayer => {
                                self.player_names.remove(&e.uuid);
                            }
                            _ => {}
                        }
                    }
                }
                C::SoundEffect(se) => {
                    let p = mc_core::Vec3::new(se.x as f64 / 8., se.y as f64 / 8., se.z as f64 / 8.);
                    self.play_sound(&se.name, Some(p), se.volume, se.pitch as f32 / 63.);
                }
                C::Effect(ef) => {
                    let p = ef.pos;
                    let c = mc_core::Vec3::new(p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5);
                    match ef.effect_id {
                        // RenderGlobal.playAuxSFX
                        1000 => self.play_sound("random.click", Some(c), 1.0, 1.0),
                        1001 => self.play_sound("random.click", Some(c), 1.0, 1.2),
                        1002 => self.play_sound("random.bow", Some(c), 1.0, 1.2),
                        1003 => self.play_sound("random.door_open", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1006 => self.play_sound("random.door_close", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1007 | 1008 | 1009 => self.play_sound("mob.ghast.fireball", Some(c), 10.0, (rand_f64() as f32 - rand_f64() as f32) * 0.2 + 1.0),
                        1020 => self.play_sound("random.anvil_break", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1021 => self.play_sound("random.anvil_use", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1022 => self.play_sound("random.anvil_land", Some(c), 0.3, rand_f64() as f32 * 0.1 + 0.9),
                        2001 => {
                            let st = mc_data::StateId::try_from_network(ef.data).unwrap_or(mc_data::StateId::AIR);
                            if st.block().id != 0 {
                                self.block_break_sound(st, p);
                            }
                        }
                        2002 => self.play_sound("game.potion.smash", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        _ => {}
                    }
                }
                C::SpawnPlayer(sp) => {
                    let mut r = entities::Remote::new(sp.entity_id, entities::Kind::Player, sp.x, sp.y, sp.z, sp.yaw, sp.pitch);
                    r.name = self.player_names.get(&sp.uuid).cloned().unwrap_or_default();
                    r.uuid = Some(sp.uuid);
                    r.held = (sp.current_item > 0).then(|| self.catalogue.index_of(sp.current_item as u16, 0)).flatten().map(|i| (i, 1));
                    apply_meta(&mut r, &sp.metadata);
                    self.remote.map.insert(sp.entity_id, r);
                }
                C::SpawnMob(m) => {
                    let mut r = entities::Remote::new(m.entity_id, entities::Kind::Mob(m.kind), m.x, m.y, m.z, m.yaw, m.pitch);
                    r.head_yaw = entities::angle(m.head_pitch);
                    self.remote.map.insert(m.entity_id, r);
                }
                C::SpawnObject(o) => {
                    let r = entities::Remote::new(o.entity_id, entities::Kind::Object(o.kind), o.x, o.y, o.z, o.yaw, o.pitch);
                    if o.kind == 2 {
                        let v = mc_core::Vec3::new(o.velocity.0 as f64 / 8000., o.velocity.1 as f64 / 8000., o.velocity.2 as f64 / 8000.);
                        self.pending_items.insert(o.entity_id, (r.pos, v));
                    }
                    self.remote.map.insert(o.entity_id, r);
                }
                C::EntityMetadata(m) => {
                    if let Some((pos, v)) = self.pending_items.remove(&m.entity_id) {
                        let stack = m.metadata.iter().find_map(|(i, v)| match (i, v) {
                            (10, mc_protocol::MetaValue::Item(s)) => net::slot_stack(&self.catalogue, s),
                            _ => None,
                        });
                        if let Some(s) = stack {
                            let mut it = mc_entity::item::ItemEntity::new(m.entity_id as u32, pos, (s.item, s.count), &mut self.rand);
                            it.motion = v;
                            it.pickup_delay = 32767;
                            self.items.push(it);
                        } else {
                            self.pending_items.insert(m.entity_id, (pos, v));
                        }
                    }
                    if let Some(r) = self.remote.map.get_mut(&m.entity_id) {
                        apply_meta(r, &m.metadata);
                    }
                }
                C::EntityEquipment(e) => {
                    if e.slot == 0 {
                        let held = net::slot_stack(&self.catalogue, &e.item).map(|s| (s.item, s.count));
                        if let Some(r) = self.remote.map.get_mut(&e.entity_id) {
                            r.held = held;
                        }
                    }
                }
                C::EntityVelocity(v) => {
                    let m = mc_core::Vec3::new(v.vx as f64 / 8000., v.vy as f64 / 8000., v.vz as f64 / 8000.);
                    if v.entity_id == n.entity_id {
                        // Knockback: Entity.setVelocity.
                        self.entity.motion = m;
                    } else if let Some(it) = self.items.iter_mut().find(|i| i.id == v.entity_id as u32) {
                        it.motion = m;
                    }
                }
                C::Explosion(x) => {
                    let (a, b, c) = x.player_motion;
                    self.entity.motion.x += a as f64;
                    self.entity.motion.y += b as f64;
                    self.entity.motion.z += c as f64;
                    let base = BlockPos::new(x.x as i32, x.y as i32, x.z as i32);
                    for (dx, dy, dz) in x.records {
                        let p = BlockPos::new(base.x + dx as i32, base.y + dy as i32, base.z + dz as i32);
                        self.world.set_block_state(p, mc_data::StateId::AIR);
                        edits.push(p);
                    }
                }
                C::DestroyEntities(d) => {
                    for id in d.ids {
                        self.remote.map.remove(&id);
                        self.pending_items.remove(&id);
                        self.items.retain(|i| i.id != id as u32);
                    }
                }
                C::CollectItem(c) => {
                    // handleCollectItem
                    if let Some(p) = self.items.iter().find(|i| i.id == c.collected_id as u32).map(|i| i.pos) {
                        let pitch = ((rand_f64() as f32 - rand_f64() as f32) * 0.7 + 1.0) * 2.0;
                        self.play_sound("random.pop", Some(p), 0.2, pitch);
                    }
                    self.items.retain(|i| i.id != c.collected_id as u32);
                    self.remote.map.remove(&c.collected_id);
                }
                C::EntityRelMove(m) => self.remote_move(m.entity_id, (m.dx as i32, m.dy as i32, m.dz as i32), None, Some(m.on_ground), true),
                C::EntityLookMove(m) => self.remote_move(m.entity_id, (m.dx as i32, m.dy as i32, m.dz as i32), Some((m.yaw, m.pitch)), Some(m.on_ground), true),
                C::EntityLook(m) => self.remote_move(m.entity_id, (0, 0, 0), Some((m.yaw, m.pitch)), Some(m.on_ground), true),
                C::EntityTeleport(t) => self.remote_move(t.entity_id, (t.x, t.y, t.z), Some((t.yaw, t.pitch)), Some(t.on_ground), false),
                C::EntityHeadLook(h) => {
                    if let Some(r) = self.remote.map.get_mut(&h.entity_id) {
                        r.head_yaw = entities::angle(h.yaw);
                    }
                }
                C::AnimationS(a) => {
                    if let Some(r) = self.remote.map.get_mut(&a.entity_id) {
                        match a.animation {
                            0 => r.swing_arm(),
                            1 => r.hurt_time = 10,
                            _ => {}
                        }
                    }
                }
                C::EntityStatus(s) => {
                    if s.status == 2 {
                        if let Some(r) = self.remote.map.get_mut(&s.entity_id) {
                            r.hurt_time = 10;
                        }
                    }
                }
                C::SetExperience(x) => {
                    self.player.experience = x.bar;
                    self.player.level = x.level;
                }
                C::UpdateHealth(h) => {
                    self.player.health = h.health;
                    self.player.food = h.food;
                    self.entity.food_level = h.food;
                    if h.health <= 0. && !self.entity.dead {
                        self.entity.dead = true;
                        self.open(Box::new(screens::DeathScreen::new(false, self.net.is_some(), 0)), false);
                    }
                }
                C::Respawn(r) => {
                    self.world.has_sky = r.dimension == 0;
                    let keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
                    for (x, z) in keys {
                        self.unload_chunk(x, z);
                    }
                    self.entity.dead = false;
                    self.entity.set_game_type(level::GameType::by_id(r.game_mode as i32));
                    self.info.difficulty = r.difficulty;
                }
                C::ChangeGameState(g) => {
                    let j = (g.value + 0.5).floor() as i32;
                    match g.reason {
                        0 => self.hud.add_chat(self.ui.lang.get("tile.bed.notValid")),
                        1 => {
                            self.info.raining = true;
                            self.weather.set_rain(0.);
                        }
                        2 => {
                            self.info.raining = false;
                            self.weather.set_rain(1.);
                        }
                        3 => {
                            self.hud.add_chat(self.ui.lang.get("gameMode.changed"));
                            self.entity.set_game_type(level::GameType::by_id(j));
                        }
                        7 => self.weather.set_rain(g.value),
                        8 => self.weather.set_thunder(g.value),
                        _ => {}
                    }
                }
                C::PlayerAbilitiesS(a) => {
                    // handlePlayerAbilities
                    self.entity.flying = a.flags & 2 != 0;
                    self.entity.creative = a.flags & 8 != 0;
                    self.entity.allow_flying = a.flags & 4 != 0;
                    self.entity.fly_speed = a.fly_speed;
                    self.entity.walk_speed = a.walk_speed;
                }
                C::HeldItemChangeS(h) => {
                    if (0..9).contains(&h.slot) {
                        self.player.selected_slot = h.slot as usize;
                    }
                }
                C::SetSlot(s) => {
                    let stack = net::slot_stack(&self.catalogue, &s.item);
                    let open = self.open_window.clone().filter(|w| w.lock().unwrap().id as i8 == s.window_id);
                    if s.window_id == -1 && s.slot == -1 {
                        if let Some(w) = &self.open_window {
                            w.lock().unwrap().cursor = stack;
                        }
                    } else if let Some(w) = open {
                        w.lock().unwrap().set(&mut self.ui, s.slot as usize, stack);
                    } else if s.window_id == 0 {
                        self.set_container_slot(s.slot as usize, stack);
                    } else if s.window_id == -2 && (0..36).contains(&s.slot) {
                        self.ui.inventory[s.slot as usize] = stack;
                    }
                }
                C::WindowItems(w) => {
                    if let Some(win) = self.open_window.clone().filter(|x| x.lock().unwrap().id == w.window_id && w.window_id != 0) {
                        let mut win = win.lock().unwrap();
                        for (i, it) in w.items.iter().enumerate() {
                            let stack = net::slot_stack(&self.catalogue, it);
                            win.set(&mut self.ui, i, stack);
                        }
                    } else if w.window_id == 0 {
                        for (i, it) in w.items.iter().enumerate() {
                            let stack = net::slot_stack(&self.catalogue, it);
                            self.set_container_slot(i, stack);
                        }
                    }
                }
                C::Disconnect(d) => {
                    let text = chat::format(&d.reason, &self.ui.lang);
                    self.hud.add_chat(format!("{}: {text}", self.ui.lang.get("disconnect.lost")));
                    n.closed = Some(d.reason);
                    n.disconnect();
                }
                _ => {}
            }
        }
        self.refresh_edits(&edits);
        // Sections of newly arrived or unloaded chunks.
        self.refresh_edits(&[]);
    }

    /// `handleEntityMovement` / `handleEntityTeleport`: relative (×32 fixed point) or absolute.
    fn remote_move(&mut self, id: i32, d: (i32, i32, i32), look: Option<(i8, i8)>, on_ground: Option<bool>, relative: bool) {
        if let Some(it) = self.items.iter_mut().find(|i| i.id == id as u32) {
            // Entity.setPositionAndRotation2 for items: snap to the server position.
            let base = self.remote.map.get(&id).map(|r| r.server).unwrap_or((0, 0, 0));
            let s = if relative { (base.0 + d.0, base.1 + d.1, base.2 + d.2) } else { d };
            it.pos = mc_core::Vec3::new(s.0 as f64 / 32., s.1 as f64 / 32., s.2 as f64 / 32.);
            it.bb = mc_core::Aabb::new(it.pos.x - 0.125, it.pos.y, it.pos.z - 0.125, it.pos.x + 0.125, it.pos.y + 0.25, it.pos.z + 0.125);
        }
        let Some(r) = self.remote.map.get_mut(&id) else { return };
        r.server = if relative { (r.server.0 + d.0, r.server.1 + d.1, r.server.2 + d.2) } else { d };
        let (yaw, pitch) = look.map_or((r.yaw, r.pitch), |(y, p)| (entities::angle(y), entities::angle(p)));
        r.move_to(yaw, pitch);
        if let Some(g) = on_ground {
            r.on_ground = g;
        }
    }

    /// `PlayerControllerMP.sendSlotPacket`: in creative the client owns the inventory and
    /// reports each changed slot (container numbering) with C10; thrown stacks use slot -1.
    fn sync_creative(&mut self) {
        if self.net.is_none() || !self.entity.creative {
            self.creative_sync = None;
            return;
        }
        let now = (self.ui.inventory, self.ui.armor);
        let Some((inv, armor)) = self.creative_sync.replace(now) else { return };
        let mut packets = Vec::new();
        for i in 0..36 {
            if inv[i] != now.0[i] {
                let slot = if i < 9 { 36 + i } else { i };
                packets.push((slot as i16, now.0[i]));
            }
        }
        for i in 0..4 {
            if armor[i] != now.1[i] {
                packets.push((5 + i as i16, now.1[i]));
            }
        }
        for (slot, st) in packets {
            let item = net::stack_slot(&self.catalogue, st);
            self.send(mc_protocol::packets::PlayServerbound::CreativeInventoryAction(mc_protocol::packets::CreativeInventoryAction { slot, item }));
        }
    }

    /// Sends the open window's queued clicks (`PlayerControllerMP.windowClick`) and its close.
    fn flush_window(&mut self) {
        let Some(w) = self.open_window.clone() else { return };
        let (clicks, closed, id, mut tx) = {
            let mut w = w.lock().unwrap();
            (std::mem::take(&mut w.clicks), w.closed, w.id, w.transaction)
        };
        for c in clicks {
            tx += 1;
            let item = net::stack_slot(&self.catalogue, c.item);
            self.send(mc_protocol::packets::PlayServerbound::ClickWindow(mc_protocol::packets::ClickWindow { window_id: id, slot: c.slot, button: c.button, action: tx, mode: c.mode, item }));
        }
        w.lock().unwrap().transaction = tx;
        if closed {
            self.send(mc_protocol::packets::PlayServerbound::CloseWindowC(mc_protocol::packets::CloseWindowC { window_id: id }));
            self.open_window = None;
        }
    }

    /// `ContainerPlayer` slot numbers to the client inventory.
    fn set_container_slot(&mut self, slot: usize, stack: Option<mc_gui::ItemStack>) {
        match slot {
            5..=8 => self.ui.armor[slot - 5] = stack,
            9..=35 => self.ui.inventory[slot] = stack,
            36..=44 => self.ui.inventory[slot - 36] = stack,
            _ => {}
        }
    }

    /// `WorldClient.doPreChunk(x, z, false)`
    fn unload_chunk(&mut self, x: i32, z: i32) {
        if self.world.remove(x, z).is_some() {
            if let Some(r) = &mut self.renderer {
                let empty = mc_render::mesh::ChunkMesh::default();
                for sy in 0..16 {
                    r.upload_section(x, sy, z, &empty);
                }
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                for sy in 0..16 {
                    self.world.dirty.insert((x + dx, sy, z + dz));
                }
            }
        }
    }

    /// The `Item` in the selected hotbar slot.
    fn held_registry_item(&self) -> Option<&'static mc_data::item::Item> {
        let s = self.ui.inventory[self.player.selected_slot]?;
        mc_data::item::Item::by_id(self.catalogue.items[s.item].id)
    }

    /// `Block.getPlayerRelativeBlockHardness`
    fn relative_hardness(&self, pos: BlockPos) -> f32 {
        let block = self.world.state_at(pos.x, pos.y, pos.z).block();
        if block.hardness < 0. {
            return 0.;
        }
        let held = self.held_registry_item();
        let mut speed = tools::dig_speed(held, block);
        if self.entity.in_water {
            speed /= 5.;
        }
        if !self.entity.on_ground {
            speed /= 5.;
        }
        let harvest = tools::can_harvest(held, block);
        speed / block.hardness / if harvest { 30. } else { 100. }
    }

    /// Runs world logic through the integrated server and applies what it produced.
    fn with_level<R>(&mut self, f: impl FnOnce(&mut server::Lv) -> R) -> R {
        let mut fx = server::Effects::default();
        let r = f(&mut server::Lv {
            world: &mut self.world,
            server: &mut self.server,
            fx: &mut fx,
            total_time: self.info.total_time,
            rules_daylight: self.info.rules.get_bool("doDaylightCycle"),
        });
        for (pos, state, chance, _fortune) in fx.drops {
            // Block.dropBlockAsItemWithChance
            for (id, meta, count) in tools::drops(state, &mut self.rand) {
                if self.rand.next_float() <= chance {
                    if let Some(i) = self.catalogue.index_of(id, meta) {
                        let id = self.alloc_entity_id();
                        let e = mc_entity::item::ItemEntity::from_block(id, pos, (i, count), &mut self.rand);
                        self.items.push(e);
                    }
                }
            }
        }
        for (p, name, vol, pitch) in fx.sounds {
            self.play_sound(&name, Some(mc_core::Vec3::new(p[0], p[1], p[2])), vol, pitch);
        }
        self.refresh_edits(&fx.edits);
        r
    }

    fn destroy_block(&mut self, pos: BlockPos) {
        let state = self.world.state_at(pos.x, pos.y, pos.z);
        let removed = if self.net.is_none() {
            self.with_level(|lv| mc_logic::Level::set_state(lv, pos, mc_data::StateId::AIR, mc_logic::flags::DEFAULT))
        } else {
            self.world.set_block(pos, 0, 0)
        };
        if removed {
            self.block_break_sound(state, pos);
            if self.net.is_none() && !self.entity.creative && tools::can_harvest(self.held_registry_item(), state.block()) {
                for (id, meta, count) in tools::drops(state, &mut self.rand) {
                    if let Some(i) = self.catalogue.index_of(id, meta) {
                        let id = self.alloc_entity_id();
                        let e = mc_entity::item::ItemEntity::from_block(id, pos, (i, count), &mut self.rand);
                        self.items.push(e);
                    }
                }
            }
            self.refresh_edits(&[pos]);
        }
    }

    /// `PlayerControllerMP.attackEntity` and the client half of
    /// `EntityPlayer.attackTargetEntityWithCurrentItem`.
    fn attack_entity(&mut self, id: i32) {
        let sel = self.player.selected_slot as i32;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel);
        }
        self.send(mc_protocol::packets::PlayServerbound::UseEntity(mc_protocol::packets::UseEntity { entity_id: id, action: mc_protocol::packets::UseEntityAction::Attack, hit: None }));
        if self.entity.game_type == level::GameType::Spectator {
            return;
        }
        // EntityOtherPlayerMP.attackEntityFrom returns true, so a sprint hit's knockback slows us
        // and ends the sprint; other living entities return false on the client.
        if self.remote.map.get(&id).is_some_and(|e| e.kind == entities::Kind::Player) && self.entity.sprinting {
            self.entity.motion.x *= 0.6;
            self.entity.motion.z *= 0.6;
            self.entity.set_sprinting(false);
        }
    }

    /// `World.playSound` at a position (`None`: a GUI sound).
    fn play_sound(&mut self, event: &str, pos: Option<mc_core::Vec3>, volume: f32, pitch: f32) {
        if let Some(s) = &mut self.sound {
            s.play(event, pos.map(|p| [p.x, p.y, p.z]), volume, pitch);
        }
    }

    /// `playAuxSFX(2001)`: a block's break sound at its center.
    fn block_break_sound(&mut self, state: mc_data::StateId, pos: BlockPos) {
        let st = state.block().sound;
        let c = mc_core::Vec3::new(pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5);
        self.play_sound(st.break_sound, Some(c), (st.volume + 1.0) / 2.0, st.frequency * 0.8);
    }

    /// Drains sounds queued by the player entity and GUI widgets; updates the listener.
    fn flush_sounds(&mut self) {
        let pos = self.entity.pos;
        for (ev, v, p) in std::mem::take(&mut self.entity.e.sounds) {
            self.play_sound(ev, Some(pos), v, p);
        }
        for ev in std::mem::take(&mut self.ui.sounds) {
            // PositionedSoundRecord.create(location, 1.0F)
            self.play_sound(ev, None, 0.25, 1.0);
        }
        let eye = self.entity.pos;
        let yaw = self.entity.yaw;
        let h = self.entity.eye_height() as f64;
        if let Some(s) = &mut self.sound {
            s.set_listener([eye.x, eye.y + h, eye.z], yaw);
            for c in mc_sound::Category::ALL {
                s.volumes.insert(c, self.ui.settings.sound_volume(c.key()));
            }
        }
    }

    /// `EntityPlayerSP.swingItem`: every call also sends C0A.
    fn swing(&mut self) {
        self.entity.swing_item();
        self.send(mc_protocol::packets::PlayServerbound::AnimationC(mc_protocol::packets::AnimationC {}));
    }

    fn send(&mut self, p: mc_protocol::packets::PlayServerbound) {
        if let Some(n) = &mut self.net {
            n.send(p);
        }
    }

    fn send_dig(&mut self, status: mc_protocol::packets::DiggingStatus, pos: BlockPos, face: Facing) {
        self.send(mc_protocol::packets::PlayServerbound::PlayerDigging(mc_protocol::packets::PlayerDigging { status, pos, face: face.index() as u8 }));
    }

    fn alloc_entity_id(&mut self) -> u32 {
        self.next_entity_id += 1;
        self.next_entity_id
    }

    /// `EntityPlayer.dropItem(stack, false, true)`
    fn throw_stack(&mut self, stack: mc_gui::ItemStack) {
        if self.net.is_some() {
            // The server spawns the item: creative throws use sendPacketDropItem (C10 slot -1);
            // survival throws already went out as window clicks.
            if self.entity.creative {
                let item = net::stack_slot(&self.catalogue, Some(stack));
                self.send(mc_protocol::packets::PlayServerbound::CreativeInventoryAction(mc_protocol::packets::CreativeInventoryAction { slot: -1, item }));
            }
            return;
        }
        let id = self.alloc_entity_id();
        let e = &self.entity;
        let item = mc_entity::item::ItemEntity::thrown(id, e.pos, e.eye_height(), e.yaw, e.pitch, (stack.item, stack.count), &mut self.rand);
        self.items.push(item);
    }

    /// `InventoryPlayer.addItemStackToInventory`; returns what didn't fit.
    fn add_to_inventory(&mut self, item: usize, mut count: u8) -> u8 {
        let max = self.catalogue.items[item].max.max(1);
        for slot in 0..36 {
            if let Some(s) = &mut self.ui.inventory[slot] {
                if s.item == item && s.count < max {
                    let n = count.min(max - s.count);
                    s.count += n;
                    count -= n;
                    if count == 0 {
                        return 0;
                    }
                }
            }
        }
        while count > 0 {
            let Some(slot) = self.ui.inventory.iter().position(Option::is_none) else { break };
            let n = count.min(max);
            self.ui.inventory[slot] = Some(mc_gui::ItemStack { item, count: n });
            count -= n;
        }
        count
    }

    /// Item entity ticks and `EntityItem.onCollideWithPlayer`.
    fn tick_items(&mut self) {
        let mut items = std::mem::take(&mut self.items);
        for it in &mut items {
            it.tick(&mut player::PhysWorld { world: &self.world, geometry: &mut self.geometry }, &mut self.rand);
        }
        let reach = self.entity.bb.expand(1.0, 0.5, 1.0);
        for it in &mut items {
            if self.net.is_none() && !it.dead && !self.entity.dead && it.pickup_delay == 0 && reach.intersects_with(&it.bb) {
                let pitch = ((rand_f64() as f32 - rand_f64() as f32) * 0.7 + 1.0) * 2.0;
                let p = it.pos;
                if let Some(s) = &mut self.sound {
                    s.play("random.pop", Some([p.x, p.y, p.z]), 0.2, pitch);
                }
                let left = self.add_to_inventory(it.stack.0, it.stack.1);
                if left == 0 {
                    it.dead = true;
                } else {
                    it.stack.1 = left;
                }
            }
        }
        items.retain(|i| !i.dead);
        self.items = items;
    }

    /// `PlayerControllerMP.clickBlock`
    fn click_block(&mut self, pos: BlockPos) {
        // PlayerControllerMP.clickBlock: adventure/spectator players can't break blocks.
        if !self.entity.allow_edit() {
            return;
        }
        let face = self.target.map_or(Facing::Down, |t| t.face);
        if self.entity.creative {
            self.send_dig(mc_protocol::packets::DiggingStatus::StartDestroyBlock, pos, face);
            self.destroy_block(pos);
            self.block_hit_delay = 5;
            return;
        }
        if self.hitting == Some(pos) {
            return;
        }
        if let Some(old) = self.hitting {
            self.send_dig(mc_protocol::packets::DiggingStatus::AbortDestroyBlock, old, face);
        }
        self.send_dig(mc_protocol::packets::DiggingStatus::StartDestroyBlock, pos, face);
        let block = self.world.state_at(pos.x, pos.y, pos.z).block();
        if block.material != mc_data::Material::Air && self.relative_hardness(pos) >= 1. {
            self.destroy_block(pos);
        } else {
            self.hitting = Some(pos);
            self.block_damage = 0.;
        }
    }

    /// `Minecraft.sendClickBlockToController` / `PlayerControllerMP.onPlayerDamageBlock`
    fn send_click_block(&mut self, held: bool) {
        if self.left_click_counter > 0 && !self.entity.creative {
            return;
        }
        let target = self.target.filter(|t| self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().id != 0);
        let (true, Some(t), true) = (held, target, self.entity.allow_edit()) else {
            // resetBlockRemoving
            if let Some(old) = self.hitting {
                self.send_dig(mc_protocol::packets::DiggingStatus::AbortDestroyBlock, old, Facing::Down);
            }
            self.hitting = None;
            self.block_damage = 0.;
            return;
        };
        if self.block_hit_delay > 0 {
            self.block_hit_delay -= 1;
        } else if self.entity.creative {
            self.block_hit_delay = 5;
            self.send_dig(mc_protocol::packets::DiggingStatus::StartDestroyBlock, t.pos, t.face);
            self.destroy_block(t.pos);
        } else if self.hitting == Some(t.pos) {
            self.block_damage += self.relative_hardness(t.pos);
            if self.step_sound_counter % 4.0 == 0.0 {
                let st = self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().sound;
                let c = mc_core::Vec3::new(t.pos.x as f64 + 0.5, t.pos.y as f64 + 0.5, t.pos.z as f64 + 0.5);
                self.play_sound(st.step_sound, Some(c), (st.volume + 1.0) / 8.0, st.frequency * 0.5);
            }
            self.step_sound_counter += 1.0;
            if self.block_damage >= 1. {
                self.hitting = None;
                self.step_sound_counter = 0.;
                self.send_dig(mc_protocol::packets::DiggingStatus::StopDestroyBlock, t.pos, t.face);
                self.destroy_block(t.pos);
                self.block_damage = 0.;
                self.block_hit_delay = 5;
            }
        } else {
            self.click_block(t.pos);
        }
        self.swing();
    }

    /// `BlockRendererDispatcher.renderBlockDamage`: the block's model with every quad's UVs moved
    /// from its own sprite onto `destroy_stage_N`.
    fn damage_quads(&self) -> Vec<mc_render::avatar::DamageQuad> {
        let Some((pos, stage)) = self.break_progress() else { return Vec::new() };
        let atlas = &self.models.atlas;
        let crack = atlas.sprite(&format!("blocks/destroy_stage_{}", stage.min(9)));
        let stored = self.world.state_at(pos.x, pos.y, pos.z);
        let actual = stored.actual_state(&self.world, pos);
        let Some(entry) = self.models.get_state(actual) else { return Vec::new() };
        if entry.models.is_empty() {
            return Vec::new();
        }
        let model = &entry.models[mc_assets::model::pick_weighted(&entry.weights, pos.x, pos.y, pos.z)].1;
        let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
        model
            .quads
            .iter()
            .filter_map(|q| {
                let c = [0.25f32, 0.75].map(|t| q.uv[0][0] * t + q.uv[2][0] * (1. - t));
                let own = atlas.sprite_at((c[0] + c[1]) / 2., (q.uv[0][1] + q.uv[2][1]) / 2.)?;
                let uv = q.uv.map(|[u, v]| {
                    let fu = (u - own.min_u) / (own.max_u - own.min_u);
                    let fv = (v - own.min_v) / (own.max_v - own.min_v);
                    [crack.min_u + fu * (crack.max_u - crack.min_u), crack.min_v + fv * (crack.max_v - crack.min_v)]
                });
                Some(mc_render::avatar::DamageQuad { pos: q.pos.map(|p| origin + Vec3::from(p)), uv })
            })
            .collect()
    }

    /// Destroy stage shown on the block being hit (`sendBlockBreakProgress`).
    pub fn break_progress(&self) -> Option<(BlockPos, i32)> {
        self.hitting.map(|p| (p, (self.block_damage * 10.) as i32 - 1)).filter(|(_, s)| *s >= 0)
    }

    /// `NetHandlerPlayServer.processChatMessage` on the integrated server.
    fn send_chat(&mut self, msg: &str) {
        if let Some(n) = &mut self.net {
            // GuiScreen.sendChatMessage: C01 carries at most 100 characters.
            let m: String = msg.chars().take(100).collect();
            n.send(mc_protocol::packets::PlayServerbound::ChatMessage(mc_protocol::packets::ChatMessage { message: m }));
            return;
        }
        let msg = commands::normalize_space(msg);
        if !msg.starts_with('/') {
            let line = self.ui.lang.format("chat.type.text", &["Player", &msg]);
            self.hud.add_chat(line);
            return;
        }
        let was_dead = self.entity.dead;
        let out = {
            let mut c = commands::Ctx {
                player: &mut self.entity,
                world: &mut self.world,
                ui: &mut self.ui,
                items: &self.catalogue,
                info: &mut self.info,
                name: "Player",
            };
            commands::execute(&msg, &mut c)
        };
        for m in out.messages {
            self.hud.add_chat(m);
        }
        for (stack, fake) in out.drops {
            // dropPlayerItemWithRandomChoice(stack, false), then setNoPickupDelay, or the
            // one-tick fake item of a fully given stack.
            let id = self.alloc_entity_id();
            let e = &self.entity;
            let mut it = mc_entity::item::ItemEntity::thrown(id, e.pos, e.eye_height(), e.yaw, e.pitch, (stack.item, stack.count), &mut self.rand);
            if fake {
                it.pickup_delay = 32767;
                it.age = 5999;
            } else {
                it.pickup_delay = 0;
            }
            self.items.push(it);
        }
        self.refresh_edits(&out.edits);
        self.sync_view();
        if self.entity.dead && !was_dead {
            self.open(Box::new(screens::DeathScreen::new(false, self.net.is_some(), 0)), false);
        }
    }
    fn refresh_edits(&mut self, edits: &[BlockPos]) {
        if edits.is_empty() && self.world.dirty.is_empty() {
            return;
        }
        let t0 = Instant::now();
        // Light is updated incrementally by set_block_state; settle sky-light gaps nearby.
        for p in edits {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    self.world.tick_chunk_light((p.x >> 4) + dx, (p.z >> 4) + dz, false, false);
                }
            }
        }
        // Remesh only the columns whose blocks or light actually changed.
        let dirty = self.world.take_dirty();
        let t1 = Instant::now();
        let n = dirty.len();
        let _ = &n;
        let Some(r) = &mut self.renderer else {
            return;
        };
        for (x, sy, z) in dirty {
            if self.world.chunks.contains_key(&(x, z)) && (0..16).contains(&sy) {
                let m = mesh::build_section(
                    &self.world,
                    &self.models,
                    x,
                    sy,
                    z,
                    self.ui.settings.ambient_occlusion != 0,
                    self.ui.settings.fancy_graphics,
                    &mut self.geometry,
                );
                r.upload_section(x, sy, z, &m);
            }
        }
        if std::env::var_os("RUSTMC_TIMING").is_some() {
            eprintln!("edit: light {:?}, mesh {n} sections {:?}", t1 - t0, t1.elapsed());
        }
    }
    fn eye(&self, pt: f32) -> Vec3 {
        self.entity.interpolated(pt) + Vec3::Y * self.entity.eye_height()
    }

    /// `EntityRenderer.getMouseOver`
    fn update_target(&mut self, pt: f32) {
        let eye = self.eye(pt);
        let yaw = self.entity.prev_yaw + (self.entity.yaw - self.entity.prev_yaw) * pt;
        let pitch = self.entity.prev_pitch + (self.entity.pitch - self.entity.prev_pitch) * pt;
        let dir = Camera { pos: eye, yaw, pitch, fov_deg: 70., bob: glam::Mat4::IDENTITY }.look_dir();
        self.target = player::target(
            &self.world,
            &mut self.geometry,
            mc_core::Vec3::new(eye.x as f64, eye.y as f64, eye.z as f64),
            dir,
            if self.entity.creative { 5. } else { 4.5 },
        );
        // getMouseOver: entities within 3 blocks (6 in creative) closer than the block hit.
        self.pointed_entity = None;
        if self.net.is_some() {
            let e = mc_core::Vec3::new(eye.x as f64, eye.y as f64, eye.z as f64);
            let reach = if self.entity.creative { 6.0 } else { 3.0 };
            let block_d = self.target.map_or(reach, |t| ((t.hit.x - e.x).powi(2) + (t.hit.y - e.y).powi(2) + (t.hit.z - e.z).powi(2)).sqrt());
            let look = mc_core::Vec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
            if let Some((id, d)) = self.remote.ray(e, look, reach) {
                if d < block_d || self.target.is_none() {
                    self.pointed_entity = Some(id);
                    self.target = None;
                }
            }
        }
    }

    /// Lightmap color at the player's eyes (`getBrightnessForRender`).
    fn light_at_eyes(&self) -> [f32; 3] {
        let e = &self.entity;
        let (x, y, z) = (e.pos.x.floor() as i32, (e.pos.y + e.eye_height() as f64).floor() as i32, e.pos.z.floor() as i32);
        let (sky, block) = self.world.light(x, y, z);
        let block = block.max(self.world.state_at(x, y, z).block().light_value);
        self.lightmap_color(sky, block)
    }

    fn lightmap_color(&self, sky: u8, block: u8) -> [f32; 3] {
        let table = &self.lightmap;
        let i = (sky as usize * 16 + block as usize) * 4;
        [table[i] as f32 / 255., table[i + 1] as f32 / 255., table[i + 2] as f32 / 255.]
    }

    fn held_item(&self, index: usize) -> Option<mc_render::avatar::HeldItem> {
        let def = &self.catalogue.items[index];
        let model = self.models.items.get(def.id, def.meta)?.clone();
        let tints = std::array::from_fn(|i| mc_render::items::item_color(def.id, def.meta, i as i32, &self.models.colors));
        Some(mc_render::avatar::HeldItem { model, tints, entity_block: matches!(def.block, 54 | 130 | 146) })
    }

    /// `EntityRenderer.setupViewBobbing`
    fn view_bobbing(&self, pt: f32) -> glam::Mat4 {
        let e = &self.entity;
        if !self.ui.settings.view_bobbing {
            return glam::Mat4::IDENTITY;
        }
        let f = e.distance_walked - e.prev_distance_walked;
        let f1 = -(e.distance_walked + f * pt);
        let f2 = e.prev_camera_yaw + (e.camera_yaw - e.prev_camera_yaw) * pt;
        let f3 = e.prev_camera_pitch + (e.camera_pitch - e.prev_camera_pitch) * pt;
        let pi = std::f32::consts::PI;
        glam::Mat4::from_translation(Vec3::new(
            mc_core::math::sin(f1 * pi) * f2 * 0.5,
            -(mc_core::math::cos(f1 * pi) * f2).abs(),
            0.,
        )) * glam::Mat4::from_rotation_z((mc_core::math::sin(f1 * pi) * f2 * 3.).to_radians())
            * glam::Mat4::from_rotation_x(((mc_core::math::cos(f1 * pi - 0.2) * f2).abs() * 5.).to_radians())
            * glam::Mat4::from_rotation_x(f3.to_radians())
    }

    fn sync_view(&mut self) {
        let pt = self.tick_accum;
        let pos = self.entity.interpolated(pt);
        let eye = self.eye(pt);
        let e = &self.entity;
        let yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * pt;
        let pitch = e.prev_pitch + (e.pitch - e.prev_pitch) * pt;
        let bob = self.view_bobbing(pt);
        let in_water = {
            let (x, y, z) = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
            self.world.state_at(x, y, z).block().material == mc_data::Material::Water
        };
        let mut fov = self.ui.settings.fov * (e.prev_fov_modifier_hand + (e.fov_modifier_hand - e.prev_fov_modifier_hand) * pt);
        if in_water {
            fov = fov * 60. / 70.;
        }
        self.camera.fov_deg = fov;
        self.camera.yaw = yaw;
        self.camera.pitch = pitch;
        self.camera.pos = eye;
        self.camera.bob = bob * glam::Mat4::from_translation(Vec3::new(0., 0., -0.1));
        self.update_target(pt);
        if self.entity.perspective != 0 {
            if self.entity.perspective == 2 {
                self.camera.yaw += 180.;
                self.camera.pitch = -self.camera.pitch;
            }
            self.camera.bob = bob;
            let back = -self.camera.look_dir();
            let hit = player::target(
                &self.world,
                &mut self.geometry,
                mc_core::Vec3::new(eye.x as f64, eye.y as f64, eye.z as f64),
                back,
                4.,
            );
            let distance = hit.map_or(4., |h| {
                ((Vec3::new(h.hit.x as f32, h.hit.y as f32, h.hit.z as f32) - eye).length() - 0.15).max(0.)
            });
            self.camera.pos = eye + back * distance;
        }
        self.player.survival = !self.entity.creative;
        self.ui.selected_slot = self.player.selected_slot;
        let held = self.ui.inventory[self.player.selected_slot].and_then(|s| self.held_item(s.item));
        let hand_item = self.entity.item_to_render.and_then(|(i, _)| self.held_item(i));
        let outline = if self.ui.settings.hide_gui {
            None
        } else {
            self.target.map(|t| {
                self.geometry
                    .selected_box(self.world.state_at(t.pos.x, t.pos.y, t.pos.z), &self.world, t.pos)
            })
        };
        let block_pos = BlockPos::new(
            self.entity.pos.x.floor() as i32,
            self.entity.pos.y.floor() as i32,
            self.entity.pos.z.floor() as i32,
        );
        let biome = mc_data::biome::Biome::by_id(self.world.biome_id(block_pos)).unwrap_or(mc_data::biome::Biome::by_id(1).unwrap());
        let mut sky = mc_render::sky::Sky::overworld(self.info.time, pt, biome.temperature_at(block_pos), self.ui.settings.render_distance);
        let (rain, thunder) = (self.weather.rain_strength(pt), self.weather.thunder_strength(pt));
        sky.apply_weather(rain, thunder);
        // EntityRenderer.updateLightmap (getSunBrightness(1.0)).
        let sun_sky = mc_render::sky::Sky::overworld(self.info.time, 1.0, 0.0, 2);
        let sun = lightmap::sun_brightness_weather(sun_sky.angle, self.weather.rain_strength(1.0), self.weather.thunder_strength(1.0));
        self.lightmap = lightmap::build(sun, self.torch_flicker_x, self.ui.settings.gamma);
        if let Some(r) = &self.renderer {
            r.set_lightmap(&self.lightmap);
        }
        sky.ticks = self.ui.ticks;
        sky.fancy_clouds = self.ui.settings.fancy_graphics;
        let light = self.light_at_eyes();
        let dropped = self
            .items
            .iter()
            .filter_map(|it| {
                let p = it.interpolated(pt);
                let (sky, block) = self.world.light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
                Some(mc_render::avatar::DroppedItem {
                    pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
                    age: it.age as f32 + pt,
                    hover_start: it.hover_start,
                    count: it.stack.1,
                    item: self.held_item(it.stack.0)?,
                    light: self.lightmap_color(sky, block),
                })
            })
            .collect();
        let e = &self.entity;
        let frame = mc_render::avatar::AvatarFrame {
            pos,
            body_yaw: e.prev_render_yaw_offset + (e.render_yaw_offset - e.prev_render_yaw_offset) * pt,
            head_yaw: e.prev_rotation_yaw_head + (e.rotation_yaw_head - e.prev_rotation_yaw_head) * pt,
            pitch,
            limb_swing: e.limb_swing - e.limb_swing_amount * (1. - pt),
            limb_amount: (e.prev_limb_swing_amount + (e.limb_swing_amount - e.prev_limb_swing_amount) * pt).min(1.),
            age: e.ticks_existed as f32 + pt,
            swing: e.swing_progress(pt),
            sneaking: e.is_sneaking(),
            perspective: self.entity.perspective,
            hide_hand: self.ui.settings.hide_gui || self.entity.dead,
            outline,
            held,
            light,
            equip: 1. - (self.entity.prev_equipped_progress + (self.entity.equipped_progress - self.entity.prev_equipped_progress) * pt),
            yaw,
            arm_pitch: e.prev_render_arm_pitch + (e.render_arm_pitch - e.prev_render_arm_pitch) * pt,
            arm_yaw: e.prev_render_arm_yaw + (e.render_arm_yaw - e.prev_render_arm_yaw) * pt,
            bob,
            hand_light: light,
            hand_item,
            slim: self.net.as_ref().is_some_and(|n| self.skins.loaded.get(&n.uuid).map_or(skins::default_slim(n.uuid), |s| s.1)),
            skin: self.net.as_ref().and_then(|n| self.skins.loaded.get(&n.uuid)).map(|s| s.0),
            dropped,
            damage: self.damage_quads(),
            others: self.other_players(pt),
        };
        if let Some(r) = &mut self.renderer {
            r.sky = sky;
            r.avatar_frame = Some(frame);
        }
    }
    fn other_players(&self, pt: f32) -> Vec<mc_render::avatar::AvatarFrame> {
        let cam = self.entity.interpolated(pt);
        self.remote
            .map
            .values()
            .filter(|r| r.kind == entities::Kind::Player)
            .filter_map(|r| {
                let p = r.interpolated(pt);
                let d2 = (p.x - cam.x as f64).powi(2) + (p.z - cam.z as f64).powi(2);
                if d2 > 64.0 * 64.0 * 4.0 {
                    return None;
                }
                let (sky, block) = self.world.light(p.x.floor() as i32, (p.y + 1.62).floor() as i32, p.z.floor() as i32);
                let lerp = |a: f32, b: f32| a + (b - a) * pt;
                let swing = {
                    let mut f = r.swing - r.prev_swing;
                    if f < 0. {
                        f += 1.;
                    }
                    r.prev_swing + f * pt
                };
                Some(mc_render::avatar::AvatarFrame {
                    pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
                    body_yaw: lerp(r.prev_body_yaw, r.body_yaw),
                    head_yaw: lerp(r.prev_head_yaw, r.head_yaw),
                    pitch: lerp(r.prev_pitch, r.pitch),
                    limb_swing: r.limb_swing - r.limb_amount * (1. - pt),
                    limb_amount: lerp(r.prev_limb_amount, r.limb_amount).min(1.),
                    age: r.ticks_existed as f32 + pt,
                    swing,
                    sneaking: r.sneaking,
                    perspective: 1,
                    held: r.held.and_then(|(i, _)| self.held_item(i)),
                    light: self.lightmap_color(sky, block),
                    skin: r.uuid.and_then(|u| self.skins.loaded.get(&u)).map(|s| s.0),
                    slim: r.uuid.is_some_and(|u| self.skins.loaded.get(&u).map_or(skins::default_slim(u), |s| s.1)),
                    ..Default::default()
                })
            })
            .collect()
    }

    fn interact(&mut self, button: MouseButton) {
        if self.entity.dead {
            return;
        }
        if button == MouseButton::Left {
            // clickMouse
            if self.left_click_counter > 0 {
                return;
            }
            self.swing();
            if let Some(id) = self.pointed_entity {
                self.attack_entity(id);
                return;
            }
            match self.target {
                None => {
                    if !self.entity.creative {
                        self.left_click_counter = 10;
                    }
                }
                Some(t) => {
                    if self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().id != 0 {
                        self.click_block(t.pos);
                    }
                }
            }
            return;
        }
        if button == MouseButton::Right {
            self.right_click_delay = 4;
        }
        let Some(t) = self.target else { return };
        if button == MouseButton::Middle {
            let (id, meta) = self.world.get(t.pos.x, t.pos.y, t.pos.z);
            if let Some(item) = self.catalogue.items.iter().position(|i| i.block == id && i.meta == meta as u16) {
                if let Some(slot) = self.ui.inventory[..9].iter().position(|s| s.is_some_and(|s| s.item == item)) {
                    self.player.selected_slot = slot;
                } else if self.entity.creative {
                    self.ui.inventory[self.player.selected_slot] = Some(mc_gui::ItemStack { item, count: 1 });
                }
            }
            return;
        }
        if button == MouseButton::Right && self.net.is_some() && self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().id != 0 {
            let stack = self.ui.inventory[self.player.selected_slot];
            let item = net::stack_slot(&self.catalogue, stack);
            // ItemBlock.canPlaceBlockOnSide failing sends nothing.
            let blocked = stack.is_some_and(|s| {
                let b = self.catalogue.items[s.item].block;
                let replaceable = |p: BlockPos| self.world.state_at(p.x, p.y, p.z).block().material.props().replaceable;
                b != 0 && !replaceable(t.pos) && !replaceable(t.pos.offset_facing(t.face))
            });
            if !blocked {
                let c = ((t.hit.x - t.pos.x as f64) as f32, (t.hit.y - t.pos.y as f64) as f32, (t.hit.z - t.pos.z as f64) as f32);
                self.send(mc_protocol::packets::PlayServerbound::BlockPlacement(mc_protocol::packets::BlockPlacement { pos: t.pos, direction: t.face.index() as u8, item, cursor: c }));
            }
        }
        if button == MouseButton::Right {
            let Some(stack) = self.ui.inventory[self.player.selected_slot] else {
                return;
            };
            // EntityPlayer.canPlayerEdit
            if !self.entity.allow_edit() {
                return;
            }
            let item = &self.catalogue.items[stack.item];
            if item.block == 0 {
                return;
            }
            let old = self.world.state_at(t.pos.x, t.pos.y, t.pos.z);
            let p = if old.block().material.props().replaceable {
                t.pos
            } else {
                t.pos.offset_facing(t.face)
            };
            if !self.world.state_at(p.x, p.y, p.z).block().material.props().replaceable {
                return;
            }
            let block = mc_data::Block::by_id(item.block).unwrap();
            let mut meta = (item.meta & 15) as u8;
            let facing = Facing::from_angle(self.entity.yaw as f64);
            if block.class == "BlockStairs" {
                meta = match facing {
                    Facing::East => 0,
                    Facing::West => 1,
                    Facing::South => 2,
                    _ => 3,
                };
                if t.face == Facing::Down || t.face != Facing::Up && t.hit.y - t.pos.y as f64 > 0.5 {
                    meta |= 4;
                }
            } else if matches!(block.id, 17 | 162) {
                meta = (meta & 3)
                    | match t.face {
                        Facing::East | Facing::West => 4,
                        Facing::North | Facing::South => 8,
                        _ => 0,
                    };
            } else if matches!(block.id, 54 | 61 | 62 | 65 | 130 | 146) {
                meta = facing.opposite().index() as u8;
            } else if matches!(block.id, 44 | 126 | 182) {
                if t.face == Facing::Down || t.face != Facing::Up && t.hit.y - t.pos.y as f64 > 0.5 {
                    meta |= 8;
                }
            } else if matches!(block.id, 50 | 75 | 76) {
                meta = match t.face {
                    Facing::East => 1,
                    Facing::West => 2,
                    Facing::South => 3,
                    Facing::North => 4,
                    _ => 5,
                };
            }
            let Some(state) = block.state_from_meta(meta) else { return };
            let mut boxes = Vec::new();
            self.geometry.add_collision_boxes(
                state,
                &self.world,
                p,
                &self.entity.bounds(),
                &mut boxes,
                mc_data::geometry::CollisionEntity::Other,
            );
            if !boxes.is_empty() {
                return;
            }
            let placed = if self.net.is_none() {
                self.with_level(|lv| mc_logic::Level::set_state(lv, p, state, mc_logic::flags::DEFAULT))
            } else {
                self.world.set_block(p, block.id, meta)
            };
            if placed {
                if !self.entity.creative {
                    self.ui.inventory[self.player.selected_slot] = if stack.count > 1 {
                        Some(mc_gui::ItemStack {
                            count: stack.count - 1,
                            ..stack
                        })
                    } else {
                        None
                    };
                }
                let st = block.sound;
                let c = mc_core::Vec3::new(p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5);
                self.play_sound(st.place_sound, Some(c), (st.volume + 1.0) / 2.0, st.frequency * 0.8);
                self.swing();
                self.entity.reset_equipped_progress();
                self.refresh_edits(&[p]);
            }
        }
        self.sync_view();
    }

    fn debug_info(&self) -> DebugInfo {
        let r = self.renderer.as_ref();
        let p = self.entity.pos;
        let (bx, by, bz) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        DebugInfo {
            fps: self.fps,
            chunk_updates: 0,
            chunks_drawn: r.map_or(0, |r| r.drawn_chunks),
            chunks_total: self.world.chunks.len() * 16,
            render_distance: self.ui.settings.render_distance,
            pos: [p.x, p.y, p.z],
            yaw: self.entity.yaw,
            pitch: self.entity.pitch,
            light: self.world.light(bx, by, bz),
            display: r.map_or((0, 0), |r| (r.config.width, r.config.height)),
            gpu: r.map_or(String::new(), |r| r.adapter_info.name.clone()),
            backend: r.map_or(String::new(), |r| format!("{:?}", r.adapter_info.backend)),
            vsync: self.ui.settings.vsync,
            limit_framerate: self.ui.settings.limit_framerate,
        }
    }

    /// Builds this frame's GUI batch.
    fn build_gui(&mut self) -> Scaled {
        let sr = self.scaled();
        let (mx, my) = self.mouse_gui();
        self.batch.clear();
        let hud_visible = self.in_game && self.screen.as_ref().is_none_or(|s| s.shows_hud());
        if hud_visible {
            let chat_open = self.screen.is_some();
            let debug = self.debug_info();
            self.hud.render(&mut self.ui, &mut self.batch, sr, &self.player, &debug, chat_open);
            if !self.ui.settings.hide_gui {
                for slot in 0..9 {
                    if let Some(stack) = self.ui.inventory[slot] {
                        self.catalogue.draw(
                            &mut self.ui,
                            &mut self.batch,
                            stack,
                            sr.width / 2 - 90 + slot as i32 * 20 + 2,
                            sr.height - 19,
                        );
                    }
                }
            }
        }
        if hud_visible && !self.ui.settings.hide_gui {
            self.name_tags(sr);
        }
        if let Some(s) = &mut self.screen {
            s.draw(&mut self.ui, &mut self.batch, sr.width, sr.height, mx, my);
        }
        sr
    }

    /// `RendererLivingEntity.renderName` / `renderLivingLabel`, projected into the GUI pass:
    /// a 0.25-alpha backing and the name at 1/60 × 1.6 blocks per text pixel, 0.5 above the
    /// head. Normal tags show through walls (vanilla's see-through pass); sneaking ones are faint.
    fn name_tags(&mut self, sr: Scaled) {
        let pt = self.tick_accum.clamp(0., 1.);
        let Some(win) = &self.window else { return };
        let size = win.inner_size();
        let aspect = size.width as f32 / size.height.max(1) as f32;
        let vp = self.camera.view_proj(aspect, self.ui.settings.render_distance as f32 * 16.);
        let tan = (self.camera.fov_deg.to_radians() / 2.).tan();
        let mut tags: Vec<(f32, f32, f32, String, bool)> = Vec::new();
        for r in self.remote.map.values() {
            if r.kind != entities::Kind::Player || r.name.is_empty() {
                continue;
            }
            let p = r.interpolated(pt);
            let rel = Vec3::new(p.x as f32, p.y as f32 + 1.8 + 0.5, p.z as f32) - self.camera.pos;
            let max = if r.sneaking { 32. } else { 64. };
            if rel.length() >= max {
                continue;
            }
            let clip = vp * rel.extend(1.);
            if clip.w <= 0.05 {
                continue;
            }
            let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
            let x = (nx * 0.5 + 0.5) * sr.width_f as f32;
            let y = (0.5 - ny * 0.5) * sr.height_f as f32;
            tags.push((clip.w, x, y, r.name.clone(), r.sneaking));
        }
        // Far tags first so near ones draw on top.
        tags.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (w, x, y, name, sneaking) in tags {
            let scale = 0.026666668 * (sr.height_f as f32 / 2.) / (tan * w);
            self.batch.transform = Some((scale, [x, y]));
            let half = self.ui.font.width(&name) / 2;
            self.batch.rect(-half - 1, -1, half + 1, 8, 0x40000000);
            let color = if sneaking { 0x20FFFFFF } else { 0xFFFFFFFF };
            self.ui.font.draw(&mut self.batch, &name, -half as f32, 0., color);
            self.batch.transform = None;
        }
    }

    fn draw_frame(&mut self) {
        let sr = self.build_gui();
        let cam = self.in_game.then_some(&self.camera);
        let Some(r) = &mut self.renderer else { return };
        match r.render(cam, Some((&self.batch, sr)), &self.assets) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let (w, h) = (r.config.width, r.config.height);
                r.resize(w, h);
            }
            Err(e) => eprintln!("render error: {e}"),
        }
    }

    /// `--screenshot`: set up the requested screen, render once off-screen, exit.
    fn take_screenshot(&mut self, path: &Path, which: &str) {
        let in_game = which != "title" && which != "title-options";
        self.in_game = in_game;
        self.ui.in_world = in_game;
        if self.net.is_some() {
            // Wait for the join sequence, then run real ticks so movement packets go out.
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(5) && self.world.chunks.len() < 64 {
                self.process_net();
                std::thread::sleep(Duration::from_millis(20));
            }
            for _ in 0..40 {
                self.process_net();
                self.run_tick();
                self.hud.tick();
            }
            std::thread::sleep(Duration::from_millis(500));
            self.process_net();
            for (cell, rgba) in self.skins.poll() {
                if let Some(r) = &self.renderer {
                    r.upload_skin(cell, &rgba);
                }
            }
        }
        for _ in 0..30 {
            self.entity.tick(&self.world, &mut self.geometry, player::Keys::default());
            let current = self.ui.inventory[self.player.selected_slot].map(|s| (s.item, s.count));
            self.entity.update_equipped(current);
        }
        self.tick_accum = 1.;
        self.sync_view();
        match which {
            "glass" => {
                self.screen = None;
                self.entity.yaw = 0.;
                self.entity.pitch = 0.;
                let p = BlockPos::new(
                    self.entity.pos.x.floor() as i32,
                    self.entity.pos.y.floor() as i32,
                    self.entity.pos.z.floor() as i32,
                );
                let mut edits = Vec::new();
                // Keep this render fixture independent of the demo's trees and terrain.
                for x in -4..=4 {
                    for z in -2..=8 {
                        for y in -1..=6 {
                            let q = BlockPos::new(p.x + x, p.y + y, p.z + z);
                            self.world.set_block(q, if y == -1 { 1 } else { 0 }, 0);
                            edits.push(q);
                        }
                    }
                }
                for x in -2..=2 {
                    for y in 0..=3 {
                        let q = BlockPos::new(p.x + x, p.y + y, p.z + 6);
                        self.world.set_block(q, 5, 0);
                        edits.push(q);
                    }
                }
                for x in -1..=1 {
                    for y in 0..=2 {
                        let q = BlockPos::new(p.x + x, p.y + y, p.z + 3);
                        self.world.set_block(
                            q,
                            95,
                            if x < 0 {
                                4
                            } else if x > 0 {
                                3
                            } else {
                                14
                            },
                        );
                        edits.push(q);
                    }
                }
                self.refresh_edits(&edits);
                self.sync_view();
            }
            "death" => self.send_chat("/kill"),
            "breaking" => {
                self.screen = None;
                self.send_chat("/gamemode survival");
                self.entity.pitch = 60.;
                self.entity.prev_pitch = 60.;
                self.grabbed = true;
                self.buttons.insert(MouseButton::Left);
                self.clicks.push(MouseButton::Left);
                for _ in 0..13 {
                    self.run_tick();
                }
                self.sync_view();
            }
            "drops" => {
                self.screen = None;
                self.send_chat("/give Player stone 64");
                self.send_chat("/give Player diamond_sword 1");
                self.send_chat("/give Player leaves 3");
                self.entity.pitch = 30.;
                for slot in 0..3 {
                    self.player.selected_slot = slot;
                    self.drop_presses.push(slot == 0);
                    self.run_tick();
                    for _ in 0..6 {
                        self.run_tick();
                    }
                }
                for _ in 0..10 {
                    self.run_tick();
                }
                self.entity.pitch = 45.;
                self.sync_view();
            }
            "creative" => self.open(Box::new(inventory::Creative::new(self.catalogue.clone())), false),
            "night" | "sunset" | "rain" | "cmds" => {
                match which {
                    "night" => self.send_chat("/time set 18000"),
                    "sunset" => self.send_chat("/time set 12500"),
                    "rain" => {
                        self.send_chat("/weather thunder");
                        for _ in 0..100 {
                            self.run_tick();
                        }
                    }
                    _ => {
                        for c in ["/help", "/gamemode 1", "/tp ~ ~ ~", "/give Player golden_apple 1 1", "/time query daytime", "/fill 0 0 0 1 1", "/foo"] {
                            self.send_chat(c);
                        }
                        self.open(Box::new(screens::Chat::with_text("/gamer")), false);
                    }
                }
                self.entity.pitch = -10.;
                self.sync_view();
            }
            "survival" => {
                self.send_chat("/give Player stone 64");
                self.send_chat("/give Player iron_helmet 1");
                self.open(Box::new(survival::SurvivalInventory::new(self.catalogue.clone())), false)
            }
            "body" | "body-back" => {
                self.entity.perspective = if which == "body" { 2 } else { 1 };
                self.entity.pitch = 10.;
                self.screen = None;
                self.sync_view();
            }
            w if w == "held" || w.starts_with("held-") => {
                let name = w.strip_prefix("held-").unwrap_or("planks");
                if let Some(item) = self.catalogue.find(name, 0) {
                    self.ui.inventory[0] = Some(mc_gui::ItemStack { item, count: 1 });
                }
                for _ in 0..5 {
                    self.entity.update_equipped(self.ui.inventory[0].map(|s| (s.item, s.count)));
                }
                self.screen = None;
                self.sync_view();
            }
            "commands" => {
                self.send_chat("/give Player stone 64");
                self.send_chat("/gamemode creative");
                self.open(Box::new(screens::Chat::with_text("/help")), false);
            }
            "title" => self.open(Box::new(screens::MainMenu::default()), false),
            "title-options" => self.open(Box::new(screens::Options::default()), false),
            "pause" => self.open(Box::new(screens::PauseMenu::default()), false),
            "options" => self.open(Box::new(screens::Options::default()), false),
            "video" => self.open(Box::new(screens::VideoSettings::default()), false),
            "sounds" => self.open(Box::new(screens::SoundSettings::default()), false),
            "controls" => self.open(Box::new(screens::Controls::default()), false),
            "chat" => {
                self.hud.add_chat("<Player> hello from rustmc");
                self.hud
                    .add_chat("\u{a7}eWelcome! \u{a7}aColors \u{a7}lbold\u{a7}r and \u{a7}nunderline");
                self.open(Box::new(screens::Chat::with_text("typing a message")), false);
            }
            "debug" => {
                self.ui.settings.show_debug = true;
                self.screen = None;
            }
            "net" => {}
            _ => self.screen = None,
        }
        // Give screens a fake mouse over the first button for hover states.
        let sr = self.scaled();
        self.mouse_px = (
            (sr.width / 2) as f64 * sr.factor as f64,
            (sr.height / 4 + 48 + 5) as f64 * sr.factor as f64,
        );
        self.player.level = 7;
        self.player.experience = 0.4;
        self.player.selected_slot = 0;
        self.fps = 0;
        let sr = self.build_gui();
        let cam = self.in_game.then_some(&self.camera);
        let r = self.renderer.as_mut().unwrap();
        let (w, h) = (r.config.width, r.config.height);
        let pixels = r.capture(cam, Some((&self.batch, sr)), &self.assets, w, h);
        match image::save_buffer(path, &pixels, w, h, image::ColorType::Rgba8) {
            Ok(()) => println!("saved {} ({which}, gui scale {})", path.display(), sr.factor),
            Err(e) => eprintln!("failed to save {}: {e}", path.display()),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Minecraft 1.8.9")
            .with_window_icon(icon::window_icon())
            .with_inner_size(winit::dpi::PhysicalSize::new(1280, 720))
            .with_visible(self.screenshot.is_none());
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        let renderer = pollster::block_on(Renderer::new(window.clone(), &self.models, &self.assets));
        self.window = Some(window);
        self.renderer = Some(renderer);
        // Force every setting to be (re)applied once.
        self.applied = self.ui.settings.clone();
        self.applied.gamma = -1.0;
        self.applied.gui_scale = -1;
        // Flip the "AO on" state so the first apply meshes everything.
        self.applied.ambient_occlusion = if self.ui.settings.ambient_occlusion != 0 { 0 } else { 2 };
        self.apply_settings();

        if let Some((path, which)) = self.screenshot.take() {
            self.take_screenshot(&path, &which);
            el.exit();
            return;
        }
        if self.bench.is_some() {
            // Uncapped for the run only; options.txt is untouched (only screens save settings).
            self.ui.settings.vsync = false;
            self.ui.settings.limit_framerate = 260;
            self.apply_settings();
            self.in_game = true;
            self.ui.in_world = true;
            return;
        }
        if self.in_game {
            self.ui.in_world = true;
            self.sync_view();
            self.grab(true);
            return;
        }
        self.open(Box::new(screens::MainMenu::default()), false);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(n) = &mut self.net {
                    n.disconnect();
                }
                el.exit()
            }
            WindowEvent::Resized(s) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(s.width, s.height);
                }
                let sr = self.scaled();
                if let Some(scr) = &mut self.screen {
                    scr.init(&self.ui, sr.width, sr.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.mouse_px = (position.x, position.y),
            WindowEvent::MouseInput { state, button, .. } => {
                let (mx, my) = self.mouse_gui();
                match state {
                    ElementState::Pressed => {
                        if let Some(s) = &mut self.screen {
                            let a = match button {
                                MouseButton::Left => s.click(&mut self.ui, mx, my),
                                MouseButton::Right => s.right_click(&mut self.ui, mx, my),
                                _ => Action::None,
                            };
                            self.handle(a, el);
                        } else if self.in_game && !self.grabbed {
                            self.grab(true);
                        } else if self.in_game {
                            self.buttons.insert(button);
                            self.clicks.push(button);
                        }
                    }
                    ElementState::Released => {
                        self.buttons.remove(&button);
                        if let Some(s) = &mut self.screen {
                            s.release(&mut self.ui);
                            self.apply_settings();
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.signum() as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y.signum() as i32,
                };
                if let Some(s) = &mut self.screen {
                    s.scroll(d);
                } else if self.in_game && d != 0 {
                    // Scrolling down moves right, like vanilla.
                    self.player.selected_slot = (self.player.selected_slot as i32 - d).rem_euclid(9) as usize;
                }
            }
            WindowEvent::Focused(false) => {
                if self.in_game && self.screen.is_none() && self.bench.is_none() {
                    self.open(Box::new(screens::PauseMenu::default()), false);
                }
                self.keys.clear();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                if event.state == ElementState::Released {
                    self.keys.remove(&code);
                    self.ui.shift_down = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
                    return;
                }
                if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    self.keys.insert(code);
                    self.ui.shift_down = true;
                }
                if let Some(s) = &mut self.screen {
                    let a = s.key(&mut self.ui, map_key(code));
                    if let Some(text) = &event.text {
                        if let Some(s) = &mut self.screen {
                            for c in text.chars() {
                                s.typed(c);
                            }
                        }
                    }
                    self.handle(a, el);
                    return;
                }
                if !self.in_game {
                    return;
                }
                self.keys.insert(code);
                if event.repeat {
                    return;
                }
                match code {
                    KeyCode::KeyE => {
                        if self.entity.creative {
                            self.open(Box::new(inventory::Creative::new(self.catalogue.clone())), false)
                        } else if self.net.is_some() {
                            // ContainerPlayer is window 0; clicks go to the server like any window.
                            let title = self.ui.lang.get("container.crafting");
                            let w = Arc::new(std::sync::Mutex::new(container::Window::new(0, "player", title, 5)));
                            self.open_window = Some(w.clone());
                            self.open(Box::new(container::ContainerScreen::new(w, self.catalogue.clone())), false)
                        } else {
                            self.open(Box::new(survival::SurvivalInventory::new(self.catalogue.clone())), false)
                        }
                    }
                    KeyCode::KeyQ => {
                        let ctrl = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
                        self.drop_presses.push(ctrl);
                    }
                    KeyCode::F5 => {
                        self.entity.perspective = (self.entity.perspective + 1) % 3;
                        self.sync_view();
                    }
                    KeyCode::Escape => self.open(Box::new(screens::PauseMenu::default()), false),
                    KeyCode::F3 => self.ui.settings.show_debug = !self.ui.settings.show_debug,
                    KeyCode::F1 => self.ui.settings.hide_gui = !self.ui.settings.hide_gui,
                    KeyCode::KeyT => self.open(Box::new(screens::Chat::default()), false),
                    KeyCode::Slash => self.open(Box::new(screens::Chat::with_text("/")), false),
                    KeyCode::Digit1
                    | KeyCode::Digit2
                    | KeyCode::Digit3
                    | KeyCode::Digit4
                    | KeyCode::Digit5
                    | KeyCode::Digit6
                    | KeyCode::Digit7
                    | KeyCode::Digit8
                    | KeyCode::Digit9 => {
                        let n = format!("{code:?}").trim_start_matches("Digit").parse::<usize>().unwrap_or(1);
                        self.player.selected_slot = n - 1;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32().min(0.5);
                self.last_frame = now;
                if let Some(b) = &mut self.bench {
                    let p = b.pose(now);
                    (self.camera.pos, self.camera.yaw, self.camera.pitch) = (p.pos, p.yaw, p.pitch);
                }
                self.update(dt);
                self.draw_frame();
                self.frames += 1;
                if let Some(b) = &mut self.bench {
                    if b.frame(now, now.elapsed().as_secs_f64() * 1e3) {
                        let gpu = self.renderer.as_ref().map_or(String::new(), |r| r.adapter_info.name.clone());
                        b.finish(Path::new("bench.csv"), &gpu, self.world.chunks.len());
                        el.exit();
                    }
                    return;
                }
                if self.fps_timer.elapsed().as_secs_f32() >= 1.0 {
                    self.fps = self.frames;
                    self.frames = 0;
                    self.fps_timer = Instant::now();
                }
                // Max Framerate (260 = unlimited; vsync caps on its own).
                let limit = self.ui.settings.limit_framerate;
                if limit < 260 && !self.ui.settings.vsync {
                    let target = Duration::from_secs_f64(1.0 / limit as f64);
                    let spent = now.elapsed();
                    if spent < target {
                        std::thread::sleep(target - spent);
                    }
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.grabbed && self.screen.is_none() {
                // EntityRenderer: f = sens*0.6+0.2; delta * f^3 * 8 * 0.15 degrees.
                let f = self.ui.settings.mouse_sensitivity * 0.6 + 0.2;
                let f1 = f * f * f * 8.0;
                let invert = 1.;
                self.entity.turn(delta.0 as f32 * f1, -delta.1 as f32 * f1 * invert);
            }
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn main() {
    let args = parse_args();
    let Some(jar) = args.jar.clone().or_else(find_jar) else {
        eprintln!("Could not find 1.8.9.jar. Pass --jar <path to 1.8.9.jar>.");
        std::process::exit(1);
    };
    let mut assets = AssetSource::new();
    if let Err(e) = assets.push(&jar) {
        eprintln!("failed to open {}: {e}", jar.display());
        std::process::exit(1);
    }
    for p in &args.packs {
        if let Err(e) = assets.push(p) {
            eprintln!("skipping pack {}: {e}", p.display());
        }
    }

    let t = Instant::now();
    let models = BlockModels::load(&assets);
    println!(
        "baked block models + {}x{} atlas in {:.1} ms",
        models.atlas.width,
        models.atlas.height,
        t.elapsed().as_secs_f64() * 1e3
    );

    let connection = args.connect.as_deref().map(|addr| {
        let (host, port) = match addr.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.parse().unwrap()),
            _ => (addr.to_string(), 25565),
        };
        let session = args.online.then(|| match auth::login() {
            Ok(s) => {
                println!("Signed in as {}", s.name);
                s
            }
            Err(e) => {
                eprintln!("Microsoft sign-in failed: {e}");
                std::process::exit(1);
            }
        });
        let name = session.as_ref().map_or(args.name.clone(), |s| s.name.clone());
        match net::Net::connect(&host, port, &name, session.as_ref()).map(|n| (n, name)) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("Failed to connect to the server: {}", chat::format(&e, &mc_gui::lang::Lang::default()));
                std::process::exit(1);
            }
        }
    });
    let (world, center, seed) = if connection.is_some() {
        let mut w = World::default();
        w.track_dirty = true;
        (w, (8, 8), 0)
    } else {
        load_world(&args)
    };
    if world.chunks.is_empty() && connection.is_none() {
        eprintln!("no chunks loaded around {center:?}");
        std::process::exit(1);
    }
    let surface_y = (0..256).rev().find(|&y| world.get(center.0, y, center.1).0 != 0).unwrap_or(64);

    // Our own options.txt next to the binary's working dir; the real .minecraft one is left alone.
    let settings = Settings::load(Path::new("options.txt"));
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(assets, models, world, center, settings);
    app.info.seed = seed;
    app.in_game = args.play || connection.is_some();
    if args.screenshot.is_none() && std::env::var_os("RUSTMC_NO_SOUND").is_none() {
        let dir = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(".minecraft").join("assets"));
        if let Some(read) = dir.and_then(|d| mc_sound::manager::index_reader(&d, "1.8")) {
            let s = mc_sound::SoundManager::new(std::sync::Arc::new(read));
            println!("sound: {} events, output {}", s.event_count(), if s.mixer.available { "ok" } else { "unavailable" });
            app.sound = Some(s);
        } else {
            eprintln!("sound: .minecraft/assets with the 1.8 index not found; running silent");
        }
    }
    app.name = connection.as_ref().map_or(args.name.clone(), |c| c.1.clone());
    app.net = connection.map(|c| c.0);
    app.screenshot = args.screenshot.clone().map(|p| (p, args.screen.clone()));
    app.bench = args
        .bench
        .map(|s| Bench::new(s, Vec3::new(center.0 as f32 + 0.5, surface_y as f32 + 6., center.1 as f32 + 0.5)));
    event_loop.run_app(&mut app).expect("run app");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gameplay_break_place_pick_and_commands_share_the_world() {
        let Some(jar) = find_jar() else {
            return;
        };
        let mut assets = AssetSource::new();
        assets.push(&jar).unwrap();
        let models = BlockModels::load(&assets);
        let mut world = World::default();
        let mut c = mc_world::chunk::Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                c.set(x, 0, z, 1, 0);
            }
        }
        for y in 1..4 {
            c.set(8, y, 10, 1, 0);
        }
        world.insert(c);
        let mut app = App::new(assets, models, world, (8, 8), Settings::default());
        app.in_game = true;
        app.tick_accum = 1.;
        app.entity.yaw = 0.;
        app.entity.pitch = 20.;
        app.sync_view();
        app.send_chat("/give Player stone 64");
        assert_eq!(app.ui.inventory[0].unwrap().count, 64);
        assert_eq!(app.target.unwrap().pos, BlockPos::new(8, 2, 10));
        app.interact(MouseButton::Left);
        assert_eq!(app.world.get(8, 2, 10), (0, 0));
        app.world.set_block(BlockPos::new(8, 2, 10), 1, 0);
        app.sync_view();
        app.interact(MouseButton::Right);
        assert_eq!(app.world.get(8, 2, 9), (1, 0));
        app.sync_view();
        app.interact(MouseButton::Right);
        assert_eq!(app.world.get(8, 2, 8), (0, 0), "placement must not intersect the player");
        let p = BlockPos::new(7, 1, 9);
        app.world.set_block(p, 5, 2);
        app.target = Some(player::Target {
            pos: p,
            face: Facing::North,
            hit: mc_core::Vec3::new(7.5, 1.5, 9.),
        });
        app.player.selected_slot = 3;
        app.interact(MouseButton::Middle);
        let held = &app.catalogue.items[app.ui.inventory[3].unwrap().item];
        assert_eq!((held.block, held.meta), (5, 2));
        app.send_chat("/tp ~1 ~ ~");
        assert_eq!(app.entity.pos.x, app.spawn.x + 1.);
    }
}
