//! rustmc: a 1.8.9 client in Rust.
//!
//! Usage: rustmc [--jar <1.8.9.jar>] [--pack <zip|dir>]... [--world <save dir>] [--radius N] [--pos X Z]
//!               [--screenshot <out.png> [--screen title|pause|options|video|controls|hud|debug|chat]]
//!               [--bench <seconds>] [--play]
//! In game: WASD, Space jump, Shift sneak, Ctrl sprint; double-tap Space for creative flight.
//! E inventory, F5 perspective, mouse break/place/pick, T chat, / commands, Esc pause.

mod bench;
mod commands;
mod icon;
mod inventory;
mod player;

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
fn java_string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

fn load_world(args: &Args) -> (World, (i32, i32)) {
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
                    // Not decorated yet; mark populated so the light pass runs.
                    c.terrain_populated = true;
                    c
                }
            };
            world.insert(chunk);
        }
    }
    let t = Instant::now();
    let keys: Vec<(i32, i32)> = world.chunks.keys().copied().collect();
    for (cx, cz) in keys {
        world.tick_chunk_light(cx, cz, false, false);
    }
    world.track_dirty = true;
    world.take_dirty();
    println!("lit chunks in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    (world, center)
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
            Action::SendChat(msg) => {
                self.close_screen();
                self.send_chat(&msg);
            }
            Action::Respawn => {
                self.entity.dead = false;
                self.entity.flying = false;
                self.entity.teleport(self.spawn);
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
            r.set_lightmap(&lightmap::build(lightmap::sun_brightness(0.0), 0.0, s.gamma));
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
            let m = mesh::build_with_geometry(
                &self.world,
                &self.models,
                cx,
                cz,
                smooth,
                self.ui.settings.fancy_graphics,
                &mut self.geometry,
            );
            quads += m.indices.len() / 6;
            r.upload_chunk(cx, cz, &m);
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
        let paused = self.screen.as_ref().is_some_and(|s| s.pauses());
        self.tick_accum += dt * 20.0;
        while self.tick_accum >= 1.0 {
            self.tick_accum -= 1.0;
            self.hud.update_counter += 1;
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
        self.entity.tick(&self.world, &mut self.geometry, keys);
        if self.entity.pos.y < -64. {
            self.entity.teleport(self.spawn);
        }
    }

    /// `getPlayerRelativeBlockHardness` with an empty hand.
    fn relative_hardness(&self, pos: BlockPos) -> f32 {
        let block = self.world.state_at(pos.x, pos.y, pos.z).block();
        if block.hardness < 0. {
            return 0.;
        }
        let mut speed = 1.0f32;
        if self.entity.in_water {
            speed /= 5.;
        }
        if !self.entity.on_ground {
            speed /= 5.;
        }
        let harvest = block.material.props().tool_not_required;
        speed / block.hardness / if harvest { 30. } else { 100. }
    }

    fn destroy_block(&mut self, pos: BlockPos) {
        if self.world.set_block(pos, 0, 0) {
            self.refresh_edits(&[pos]);
        }
    }

    /// `PlayerControllerMP.clickBlock`
    fn click_block(&mut self, pos: BlockPos) {
        if self.entity.creative {
            self.destroy_block(pos);
            self.block_hit_delay = 5;
            return;
        }
        if self.hitting == Some(pos) {
            return;
        }
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
        let (true, Some(t)) = (held, target) else {
            self.hitting = None;
            self.block_damage = 0.;
            return;
        };
        if self.block_hit_delay > 0 {
            self.block_hit_delay -= 1;
        } else if self.entity.creative {
            self.block_hit_delay = 5;
            self.destroy_block(t.pos);
        } else if self.hitting == Some(t.pos) {
            self.block_damage += self.relative_hardness(t.pos);
            if self.block_damage >= 1. {
                self.hitting = None;
                self.destroy_block(t.pos);
                self.block_damage = 0.;
                self.block_hit_delay = 5;
            }
        } else {
            self.click_block(t.pos);
        }
        self.entity.swing_item();
    }

    /// Destroy stage shown on the block being hit (`sendBlockBreakProgress`).
    pub fn break_progress(&self) -> Option<(BlockPos, i32)> {
        self.hitting.map(|p| (p, (self.block_damage * 10.) as i32 - 1)).filter(|(_, s)| *s >= 0)
    }

    fn send_chat(&mut self, msg: &str) {
        if !msg.starts_with('/') {
            self.hud.add_chat(format!("<Player> {msg}"));
            return;
        }
        match commands::execute(msg, &mut self.entity, &mut self.world, &mut self.ui, &self.catalogue, self.spawn) {
            Ok(result) => {
                self.hud.add_chat(result.text);
                self.refresh_edits(&result.edits);
                self.sync_view();
            }
            Err(error) => self.hud.add_chat(format!("\u{a7}c{error}")),
        }
        if self.entity.dead {
            self.open(Box::new(screens::DeathScreen::default()), false);
        }
    }
    fn refresh_edits(&mut self, edits: &[BlockPos]) {
        if edits.is_empty() {
            return;
        }
        // Light is updated incrementally by set_block_state; settle sky-light gaps nearby.
        for p in edits {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    self.world.tick_chunk_light((p.x >> 4) + dx, (p.z >> 4) + dz, false, false);
                }
            }
        }
        // Remesh only the columns whose blocks or light actually changed.
        let mut dirty = HashSet::new();
        for (x, _, z) in self.world.take_dirty() {
            dirty.insert((x, z));
        }
        let Some(r) = &mut self.renderer else {
            return;
        };
        for (x, z) in dirty {
            if self.world.chunks.contains_key(&(x, z)) {
                let m = mesh::build_with_geometry(
                    &self.world,
                    &self.models,
                    x,
                    z,
                    self.ui.settings.ambient_occlusion != 0,
                    self.ui.settings.fancy_graphics,
                    &mut self.geometry,
                );
                r.upload_chunk(x, z, &m);
            }
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
    }

    /// Lightmap color at the player's eyes (`getBrightnessForRender`).
    fn light_at_eyes(&self) -> [f32; 3] {
        let e = &self.entity;
        let (x, y, z) = (e.pos.x.floor() as i32, (e.pos.y + e.eye_height() as f64).floor() as i32, e.pos.z.floor() as i32);
        let (sky, block) = self.world.light(x, y, z);
        let block = block.max(self.world.state_at(x, y, z).block().light_value);
        let table = lightmap::build(lightmap::sun_brightness(0.0), 0.0, self.ui.settings.gamma);
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
        let mut sky = mc_render::sky::Sky::overworld(6000, 0., biome.temperature_at(block_pos), self.ui.settings.render_distance);
        sky.ticks = self.ui.ticks;
        sky.fancy_clouds = self.ui.settings.fancy_graphics;
        let light = self.light_at_eyes();
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
            slim: false,
        };
        if let Some(r) = &mut self.renderer {
            r.sky = sky;
            r.avatar_frame = Some(frame);
        }
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
            self.entity.swing_item();
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
        if button == MouseButton::Right {
            let Some(stack) = self.ui.inventory[self.player.selected_slot] else {
                return;
            };
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
            if self.world.set_block(p, block.id, meta) {
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
                self.entity.swing_item();
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
            chunks_drawn: r.map_or(0, |r| r.drawn_chunks) * 16,
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
        if let Some(s) = &mut self.screen {
            s.draw(&mut self.ui, &mut self.batch, sr.width, sr.height, mx, my);
        }
        sr
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
            "creative" => self.open(Box::new(inventory::Creative::new(self.catalogue.clone())), false),
            "body" | "body-back" => {
                self.entity.perspective = if which == "body" { 2 } else { 1 };
                self.entity.pitch = 10.;
                self.screen = None;
                self.sync_view();
            }
            "held" => {
                if let Some(item) = self.catalogue.find("planks", 0) {
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
            WindowEvent::CloseRequested => el.exit(),
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
                    KeyCode::KeyE => self.open(Box::new(inventory::Creative::new(self.catalogue.clone())), false),
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

    let (world, center) = load_world(&args);
    if world.chunks.is_empty() {
        eprintln!("no chunks loaded around {center:?}");
        std::process::exit(1);
    }
    let surface_y = (0..256).rev().find(|&y| world.get(center.0, y, center.1).0 != 0).unwrap_or(64);

    // Our own options.txt next to the binary's working dir; the real .minecraft one is left alone.
    let settings = Settings::load(Path::new("options.txt"));
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(assets, models, world, center, settings);
    app.in_game = args.play;
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
