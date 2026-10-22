//! rustmc: a 1.8.9 client in Rust.
//!
//! Usage: rustmc [--jar <1.8.9.jar>] [--pack <zip|dir>]... [--world <save dir>] [--radius N] [--pos X Z]
//!               [--screenshot <out.png> [--screen title|pause|options|video|controls|hud|debug|chat]]
//! In game: click to grab mouse, WASD + Space/Shift to fly, Ctrl faster, Esc pause,
//! F1 hide HUD, F3 debug, T chat, 1-9 / wheel hotbar.

use glam::Vec3;
use mc_assets::AssetSource;
use mc_core::nbt;
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
    jar: Option<PathBuf>,
    packs: Vec<PathBuf>,
    world: Option<PathBuf>,
    radius: i32,
    pos: Option<(i32, i32)>,
    screenshot: Option<PathBuf>,
    screen: String,
}

fn parse_args() -> Args {
    let mut a = Args { jar: None, packs: vec![], world: None, radius: 4, pos: None, screenshot: None, screen: "hud".into() };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--jar" => a.jar = it.next().map(PathBuf::from),
            "--pack" => a.packs.extend(it.next().map(PathBuf::from)),
            "--world" => a.world = it.next().map(PathBuf::from),
            "--screenshot" => a.screenshot = it.next().map(PathBuf::from),
            "--screen" => a.screen = it.next().unwrap_or_default(),
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
    if let Ok(appdata) = std::env::var("APPDATA") {
        candidates.push(Path::new(&appdata).join(".minecraft/versions/1.8.9/1.8.9.jar"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Spawn X/Z from `level.dat` (gzipped NBT).
fn read_spawn(world: &Path) -> Option<(i32, i32)> {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(&std::fs::read(world.join("level.dat")).ok()?[..]).read_to_end(&mut raw).ok()?;
    let (_, root) = nbt::read(&raw).ok()?;
    let data = root.as_compound()?.get("Data")?.as_compound()?;
    Some((data.get("SpawnX")?.as_i64()? as i32, data.get("SpawnZ")?.as_i64()? as i32))
}

fn load_world(args: &Args) -> (World, (i32, i32)) {
    let mut world = World::default();
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
                None => mc_world::gen::generate(cx, cz),
            };
            world.insert(chunk);
        }
    }
    let t = Instant::now();
    mc_world::light::relight(&mut world);
    println!("lit chunks in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    (world, center)
}

fn map_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        _ => Key::Other,
    }
}

struct App {
    assets: AssetSource,
    models: BlockModels,
    world: World,
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
}

impl App {
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
                // No server yet: echo like a singleplayer chat message.
                self.hud.add_chat(format!("<Player> {msg}"));
                self.close_screen();
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
        if (s.ambient_occlusion != 0) != (self.applied.ambient_occlusion != 0) {
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
            let m = mesh::build(&self.world, &self.models, cx, cz, smooth);
            quads += m.indices.len() / 6;
            r.upload_chunk(cx, cz, &m);
        }
        println!("meshed {} chunks ({quads} quads) in {:.1} ms", self.world.chunks.len(), t.elapsed().as_secs_f64() * 1e3);
    }

    fn grab(&mut self, on: bool) {
        let Some(w) = &self.window else { return };
        if on {
            let ok = w.set_cursor_grab(CursorGrabMode::Confined).or_else(|_| w.set_cursor_grab(CursorGrabMode::Locked));
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
        // 20 TPS client ticks drive HUD/GUI animations.
        self.tick_accum += dt * 20.0;
        while self.tick_accum >= 1.0 {
            self.tick_accum -= 1.0;
            self.hud.update_counter += 1;
        }
        self.ui.ticks = self.hud.update_counter as f32 + self.tick_accum;
        if !self.in_game || self.screen.is_some() {
            return;
        }
        let mut speed = 10.9; // vanilla creative flying blocks/s
        if self.keys.contains(&KeyCode::ControlLeft) {
            speed *= 2.0;
        }
        let yaw = self.camera.yaw.to_radians();
        let forward = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
        let right = Vec3::new(-yaw.cos(), 0.0, -yaw.sin());
        let mut v = Vec3::ZERO;
        let k = |c| self.keys.contains(&c);
        if k(KeyCode::KeyW) {
            v += forward;
        }
        if k(KeyCode::KeyS) {
            v -= forward;
        }
        if k(KeyCode::KeyD) {
            v += right;
        }
        if k(KeyCode::KeyA) {
            v -= right;
        }
        if k(KeyCode::Space) {
            v += Vec3::Y;
        }
        if k(KeyCode::ShiftLeft) {
            v -= Vec3::Y;
        }
        self.camera.pos += v.normalize_or_zero() * speed * dt;
    }

    fn debug_info(&self) -> DebugInfo {
        let r = self.renderer.as_ref();
        let p = self.camera.pos;
        let (bx, by, bz) = (p.x.floor() as i32, (p.y - 1.62).floor() as i32, p.z.floor() as i32);
        DebugInfo {
            fps: self.fps,
            chunk_updates: 0,
            chunks_drawn: r.map_or(0, |r| r.drawn_chunks) * 16,
            chunks_total: self.world.chunks.len() * 16,
            render_distance: self.ui.settings.render_distance,
            pos: [p.x as f64, (p.y - 1.62) as f64, p.z as f64],
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
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
        match which {
            "title" => self.open(Box::new(screens::MainMenu::default()), false),
            "title-options" => self.open(Box::new(screens::Options::default()), false),
            "pause" => self.open(Box::new(screens::PauseMenu::default()), false),
            "options" => self.open(Box::new(screens::Options::default()), false),
            "video" => self.open(Box::new(screens::VideoSettings::default()), false),
            "controls" => self.open(Box::new(screens::Controls::default()), false),
            "chat" => {
                self.hud.add_chat("<Player> hello from rustmc");
                self.hud.add_chat("\u{a7}eWelcome! \u{a7}aColors \u{a7}lbold\u{a7}r and \u{a7}nunderline");
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
        self.mouse_px = ((sr.width / 2) as f64 * sr.factor as f64, (sr.height / 4 + 48 + 5) as f64 * sr.factor as f64);
        self.player.level = 7;
        self.player.experience = 0.4;
        self.player.selected_slot = 2;
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
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let (mx, my) = self.mouse_gui();
                match state {
                    ElementState::Pressed => {
                        if let Some(s) = &mut self.screen {
                            let a = s.click(&mut self.ui, mx, my);
                            self.handle(a, el);
                        } else if self.in_game && !self.grabbed {
                            self.grab(true);
                        }
                    }
                    ElementState::Released => {
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
                if self.in_game && self.screen.is_none() {
                    self.open(Box::new(screens::PauseMenu::default()), false);
                }
                self.keys.clear();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else { return };
                if event.state == ElementState::Released {
                    self.keys.remove(&code);
                    return;
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
                match code {
                    KeyCode::Escape => self.open(Box::new(screens::PauseMenu::default()), false),
                    KeyCode::F3 => self.ui.settings.show_debug = !self.ui.settings.show_debug,
                    KeyCode::F1 => self.ui.settings.hide_gui = !self.ui.settings.hide_gui,
                    KeyCode::KeyT => self.open(Box::new(screens::Chat::default()), false),
                    KeyCode::Slash => self.open(Box::new(screens::Chat::with_text("/")), false),
                    KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3 | KeyCode::Digit4 | KeyCode::Digit5 | KeyCode::Digit6
                    | KeyCode::Digit7 | KeyCode::Digit8 | KeyCode::Digit9 => {
                        let n = format!("{code:?}").trim_start_matches("Digit").parse::<usize>().unwrap_or(1);
                        self.player.selected_slot = n - 1;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32().min(0.1);
                self.last_frame = now;
                self.update(dt);
                self.draw_frame();
                self.frames += 1;
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
                let scale = f * f * f * 8.0 * 0.15;
                self.camera.yaw += delta.0 as f32 * scale;
                self.camera.pitch = (self.camera.pitch + delta.1 as f32 * scale).clamp(-90.0, 90.0);
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
    println!("baked block models + {}x{} atlas in {:.1} ms", models.atlas.width, models.atlas.height, t.elapsed().as_secs_f64() * 1e3);

    let (world, center) = load_world(&args);
    if world.chunks.is_empty() {
        eprintln!("no chunks loaded around {center:?}");
        std::process::exit(1);
    }
    let surface_y = (0..256).rev().find(|&y| world.get(center.0, y, center.1).0 != 0).unwrap_or(64);

    // Our own options.txt next to the binary's working dir; the real .minecraft one is left alone.
    let settings = Settings::load(Path::new("options.txt"));
    let ui = Ui::new(&assets, settings.clone());

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        assets,
        models,
        world,
        renderer: None,
        window: None,
        camera: Camera {
            pos: Vec3::new(center.0 as f32 + 0.5, surface_y as f32 + 6.0, center.1 as f32 + 0.5),
            yaw: -45.0,
            pitch: 20.0,
            fov_deg: settings.fov,
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
        mouse_px: (0.0, 0.0),
        applied: settings,
        last_frame: Instant::now(),
        tick_accum: 0.0,
        fps_timer: Instant::now(),
        frames: 0,
        fps: 0,
        screenshot: args.screenshot.clone().map(|p| (p, args.screen.clone())),
    };
    event_loop.run_app(&mut app).expect("run app");
}
