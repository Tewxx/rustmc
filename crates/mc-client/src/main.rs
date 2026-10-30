//! rustmc: a 1.8.9 client in Rust.
//!
//! Usage: rustmc [--connect host[:port]] [--jar <1.8.9.jar>] [--pack <zip|dir>]...
//!               [--screenshot <out.png> [--screen title|pause|options|video|controls|hud|debug|chat]]
//!               [--bench <seconds>]
//! With `--features singleplayer` (off by default): [--play] [--world <save dir>] [--radius N] [--pos X Z] [--seed S].
//! In game: WASD, Space jump, Shift sneak, Ctrl sprint; double-tap Space for creative flight.
//! E inventory, F5 perspective, mouse break/place/pick, T chat, / commands, Esc pause.

mod bench;
mod client_packs;
mod auth;
mod chat;
mod entities;
mod music;
mod net;
mod pack;
mod particles;
#[cfg(feature = "singleplayer")]
mod commands;
mod container;
mod icon;
mod launch;
mod inventory;
mod keymap;
mod player;
#[cfg(feature = "singleplayer")]
mod server;
mod skins;
mod level;
mod survival;
mod tools;
mod tooltip;
mod item_use;
mod scoreboard;
mod tiles;

mod app;
mod args;
mod helpers;
mod input;
mod interact;
mod interact_items;
mod net_handler;
mod render_entities;
mod render_frame;
mod resource_pack;
mod screens_glue;
mod session;
mod screenshot;
mod sound_glue;
mod tick;
mod world_load;
#[cfg(test)]
mod tests;

pub(crate) use app::App;
pub(crate) use args::*;
pub(crate) use helpers::*;
pub(crate) use world_load::*;

use bench::Bench;
use glam::Vec3;
use mc_assets::AssetSource;
#[cfg(feature = "singleplayer")]
use mc_core::nbt;
use mc_core::{BlockPos, Facing};
use mc_data::geometry::BlockGeometry;
use mc_gui::batch::GuiBatch;
use mc_gui::hud::{DebugInfo, Hud, PlayerHud};
use mc_gui::screens::{self, Action, Key, Screen};
use mc_gui::settings::Settings;
use mc_gui::{Scaled, Ui};
use mc_render::{lightmap, mesh, models::BlockModels, Camera, Renderer};
use mc_world::World;
use std::collections::HashSet;
#[cfg(feature = "singleplayer")]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// Panics normally vanish with the window when the client is started from Explorer. Write the
/// panic and a backtrace to `rustmc-crash.log` (and stderr) so a crash can be reported.
fn install_crash_log() {
    let args: Vec<String> = std::env::args().collect();
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!(
            "rustmc panic\nargs: {args:?}\ncwd: {:?}\n{info}\n{bt}\n",
            std::env::current_dir().ok()
        );
        let _ = std::fs::write("rustmc-crash.log", &msg);
        eprintln!("{msg}");
    }));
}

fn main() {
    install_crash_log();
    let (args, prism_session) = match launcher_args() {
        Some(r) => r,
        None => (parse_args(), None),
    };
    bench::profile_init();
    if let Some(w) = args.window {
        let _ = WINDOW_SIZE.set(w);
    }
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

    let connecting = args.connect.is_some();
    #[cfg(feature = "singleplayer")]
    let (world, center, seed) = if connecting {
        let mut w = World::default();
        w.track_dirty = true;
        (w, (8, 8), 0)
    } else {
        load_world(&args)
    };
    #[cfg(feature = "singleplayer")]
    if world.chunks.is_empty() && !connecting {
        eprintln!("no chunks loaded around {center:?}");
        std::process::exit(1);
    }
    // Multiplayer-only: the world is whatever the server sends. In-game screens and the
    // benchmark need a connection (mock server); menu screens need nothing.
    #[cfg(not(feature = "singleplayer"))]
    let (world, center, seed) = {
        if args.bench.is_some() && !connecting {
            eprintln!("--bench needs a world: pass --connect <host:port> (BENCH.md has the mock server commands)");
            std::process::exit(1);
        }
        let mut w = World::default();
        w.track_dirty = true;
        (w, (8, 8), 0i64)
    };
    let surface_y = (0..256).rev().find(|&y| world.get(center.0, y, center.1).0 != 0).unwrap_or(64);

    // Our own options.txt next to the binary's working dir; the real .minecraft one is left alone.
    let settings = Settings::load(Path::new("options.txt"));
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(assets, models, world, center, settings);
    app.tiles.seed_from_world(&app.world);
    app.info.seed = seed;
    mc_gui::screens::SINGLEPLAYER_AVAILABLE.store(cfg!(feature = "singleplayer"), std::sync::atomic::Ordering::Relaxed);
    #[cfg(feature = "singleplayer")]
    {
        app.in_game |= args.play;
    }
    if args.screenshot.is_none() && std::env::var_os("RUSTMC_NO_SOUND").is_none() {
        let index = if args.asset_index.is_empty() { "1.8" } else { args.asset_index.as_str() };
        let dirs = args.assets_dir.clone().into_iter()
            .chain(minecraft_dir().map(|d| d.join("assets")))
            .chain([PathBuf::from("MCP-919/.minecraft/assets")]);
        if let Some(read) = dirs.into_iter().find_map(|d| mc_sound::manager::index_reader(&d, index)) {
            let s = mc_sound::SoundManager::new(std::sync::Arc::new(read));
            println!("sound: {} events, output {}", s.event_count(), if s.mixer.available { "ok" } else { "unavailable" });
            app.sound = Some(s);
        } else {
            eprintln!("sound: .minecraft/assets with the 1.8 index not found; running silent");
        }
    }
    app.name = args.name.clone();
    app.ui.player_name = app.name.clone();
    app.jar = jar.clone();
    app.base_packs = args.packs.clone();
    app.screenshot = args.screenshot.clone().map(|p| (p, args.screen.clone()));
    app.bench = args
        .bench
        .map(|s| Bench::new(s, Vec3::new(center.0 as f32 + 0.5, surface_y as f32 + 6., center.1 as f32 + 0.5)));
    // `--connect` and Prism's quick-join go through the same in-process path as the server list.
    app.session = prism_session;
    if let Some(addr) = args.connect.as_deref() {
        app.start_connect(addr, &args.name, args.online, args.pack_mode);
        if app.screenshot.is_some() || app.bench.is_some() {
            app.wait_connect();
        }
    }
    event_loop.run_app(&mut app).expect("run app");
}
