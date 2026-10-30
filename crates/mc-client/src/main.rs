//! rustmc: a 1.8.9 client in Rust.
//!
//! Usage: rustmc [--jar <1.8.9.jar>] [--pack <zip|dir>]... [--world <save dir>] [--radius N] [--pos X Z]
//!               [--screenshot <out.png> [--screen title|pause|options|video|controls|hud|debug|chat]]
//!               [--bench <seconds>] [--play]
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
mod commands;
mod container;
mod icon;
mod inventory;
mod keymap;
mod player;
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
    // `ServerData.ServerResourceMode` is remembered per server; an accepted pack from an
    // earlier join is pushed on top of the stack before the atlases are baked.
    let server_key = args.connect.as_deref().map(normalize_server_key).unwrap_or_default();
    let pack_store = pack::Store::load();
    let pack_mode = args.pack_mode.unwrap_or_else(|| pack_store.mode_for(&server_key));
    let mut cached_applied = None;
    if let Some(cached) = pack_store.cached_for(&server_key) {
        if let Err(e) = assets.push(&cached) {
            eprintln!("skipping cached server pack {}: {e}", cached.display());
        } else {
            println!("using cached server resource pack {}", cached.display());
            let hash = pack_store.servers.get(&server_key).map(|s| s.hash.clone()).unwrap_or_default();
            cached_applied = Some((hash, cached));
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

    let mut connection = None;
    let mut connect_error: Option<String> = None;
    if let Some(addr) = args.connect.as_deref() {
        let (host, port) = match addr.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.parse().unwrap()),
            _ => (addr.to_string(), 25565),
        };
        let session = if args.online {
            match auth::login() {
                Ok(s) => {
                    println!("Signed in as {}", s.name);
                    Some(s)
                }
                Err(e) => {
                    connect_error = Some(format!("Microsoft sign-in failed: {e}"));
                    None
                }
            }
        } else {
            None
        };
        if connect_error.is_none() {
            let name = session.as_ref().map_or(args.name.clone(), |s| s.name.clone());
            match net::Net::connect(&host, port, &name, session.as_ref()) {
                Ok(n) => connection = Some((n, name)),
                Err(e) => {
                    let reason = chat::format(&e, &mc_gui::lang::Lang::default());
                    eprintln!("Failed to connect to the server: {reason}");
                    connect_error = Some(reason);
                }
            }
        }
    }
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
    app.tiles.seed_from_world(&app.world);
    app.info.seed = seed;
    app.in_game = args.play || connection.is_some();
    if args.screenshot.is_none() && std::env::var_os("RUSTMC_NO_SOUND").is_none() {
        let dirs = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(".minecraft").join("assets")).into_iter()
            .chain([PathBuf::from("MCP-919/.minecraft/assets")]);
        if let Some(read) = dirs.into_iter().find_map(|d| mc_sound::manager::index_reader(&d, "1.8")) {
            let s = mc_sound::SoundManager::new(std::sync::Arc::new(read));
            println!("sound: {} events, output {}", s.event_count(), if s.mixer.available { "ok" } else { "unavailable" });
            app.sound = Some(s);
        } else {
            eprintln!("sound: .minecraft/assets with the 1.8 index not found; running silent");
        }
    }
    app.name = connection.as_ref().map_or(args.name.clone(), |c| c.1.clone());
    app.ui.player_name = app.name.clone();
    app.net = connection.map(|c| c.0);
    app.packs = pack::ResourcePacks::new(pack_mode);
    app.packs.applied = cached_applied;
    app.pack_store = pack_store;
    app.server_key = server_key;
    app.jar = jar.clone();
    app.base_packs = args.packs.clone();
    app.screenshot = args.screenshot.clone().map(|p| (p, args.screen.clone()));
    app.bench = args
        .bench
        .map(|s| Bench::new(s, Vec3::new(center.0 as f32 + 0.5, surface_y as f32 + 6., center.1 as f32 + 0.5)));
    if let Some(reason) = connect_error {
        app.in_game = false;
        app.ui.in_world = false;
        app.open(Box::new(screens::Disconnected::new(reason)), false);
    }
    event_loop.run_app(&mut app).expect("run app");
}
