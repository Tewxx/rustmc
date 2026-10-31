//! In-process server sessions: `GuiConnecting`'s background connector, entering the world on
//! login, and the `Minecraft.loadWorld(null)` teardown when a connection ends.

use crate::*;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

static CONNECTION_ID: AtomicUsize = AtomicUsize::new(0);

/// A finished login: the connection and the name we joined as, or the failure text.
type Outcome = Result<(net::Net, String), String>;

/// `GuiConnecting.connect`: the "Server Connector #n" thread and its cancel flag.
pub(crate) struct Connector {
    cancel: Arc<AtomicBool>,
    rx: Receiver<Outcome>,
    socket: Arc<std::sync::Mutex<Option<std::net::TcpStream>>>,
}

impl Connector {
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(stream) = self.socket.lock().unwrap().take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl App {
    /// `new GuiConnecting(previous, mc, serverData)`: `loadWorld(null)`, then connect on a thread.
    /// `address` is `host[:port]`; the session is Prism's, else a Microsoft account when asked
    /// for (or cached in `rustmc_account.json`), else offline as `name`.
    pub(crate) fn start_connect(&mut self, address: &str, name: &str, online: bool, pack_mode: Option<pack::Mode>) {
        self.cancel_connect();
        self.leave_world();
        let key = normalize_server_key(address);
        self.pack_store = pack::Store::load();
        self.server_key = key.clone();
        self.packs = pack::ResourcePacks::new(pack_mode.unwrap_or_else(|| self.pack_store.mode_for(&key)));
        let online = online || Path::new("rustmc_account.json").exists();
        let addr = mc_protocol::status::parse_address(address);
        eprintln!("[connect] address={address:?} name={name:?} online={online}");
        let stage = Arc::new(AtomicU8::new(screens::STAGE_CONNECTING));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let asset = if self.asset_download.is_none() {
            self.auto_assets_dir.clone().map(|dir| {
                let (sender, receiver) = channel();
                self.asset_download = Some(receiver);
                (dir, sender)
            })
        } else { None };
        let socket = Arc::new(std::sync::Mutex::new(None));
        let t_socket = socket.clone();
        let (t_stage, t_cancel, prism, name) = (stage.clone(), cancel.clone(), self.session.clone(), name.to_string());
        let id = CONNECTION_ID.fetch_add(1, Ordering::Relaxed) + 1;
        let spawned = std::thread::Builder::new().name(format!("Server Connector #{id}")).spawn(move || {
            let outcome = connect_blocking(&addr, &name, online, prism, &t_stage, &t_socket, &t_cancel, asset);
            if t_cancel.load(Ordering::Relaxed) {
                // Aborted while connecting: close the socket, nobody is waiting for the result.
                if let Ok((mut n, _)) = outcome {
                    n.disconnect();
                }
                return;
            }
            let _ = tx.send(outcome);
        });
        if let Err(e) = spawned {
            self.connect_failed(format!("Could not start the connector thread: {e}"));
            return;
        }
        self.connecting = Some(Connector { cancel, rx, socket });
        if self.screen.is_none() && self.parents.is_empty() {
            self.open(Box::new(screens::MainMenu::default()), false);
        }
        self.open(Box::new(screens::Connecting::new(stage)), true);
    }

    /// `GuiConnecting.actionPerformed` Cancel: stops the connector; the caller returns to the
    /// previous screen.
    pub(crate) fn cancel_connect(&mut self) {
        if let Some(c) = self.connecting.take() {
            c.cancel();
        }
    }

    /// Per-frame: `GuiConnecting.updateScreen` hands a finished login to the game.
    pub(crate) fn poll_connecting(&mut self) {
        let Some(c) = &self.connecting else { return };
        let outcome = match c.rx.try_recv() {
            Ok(o) => o,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Internal Exception: the connector thread died".into()),
        };
        self.connecting = None;
        match outcome {
            Ok((n, name)) => self.enter_world(n, name),
            Err(reason) => self.connect_failed(reason),
        }
    }

    /// Blocks until the pending connection finishes (screenshot and benchmark runs).
    pub(crate) fn wait_connect(&mut self) {
        while self.connecting.is_some() {
            self.poll_connecting();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// `NetHandlerLoginClient.onDisconnect`: `GuiDisconnected("connect.failed", reason)` over the
    /// screen the connection started from.
    fn connect_failed(&mut self, reason: String) {
        eprintln!("Failed to connect to the server: {reason}");
        let text = chat::format(&reason, &self.ui.lang);
        self.open(Box::new(screens::Disconnected::new("connect.failed", text)), false);
    }

    /// Login succeeded: the connection becomes the game's world.
    pub(crate) fn enter_world(&mut self, n: net::Net, name: String) {
        self.reset_world_state();
        self.net = Some(n);
        self.name = name.clone();
        self.ui.player_name = name;
        self.ui.remote = true;
        self.in_game = true;
        self.ui.in_world = true;
        self.parents.clear();
        self.screen = None;
        self.grab(true);
    }

    /// `Minecraft.loadWorld(null)`: closes the connection and drops everything that belongs to
    /// the world. Settings, assets, client resource packs, chat history and the GUI stay.
    pub(crate) fn leave_world(&mut self) {
        // Dropping `Net` shuts the socket, which ends the reader thread.
        self.net = None;
        self.reset_world_state();
        self.ui.remote = false;
        self.in_game = false;
        self.ui.in_world = false;
        // `mcResourcePackRepository.clearResourcePack()` + refresh when the server had one.
        if self.packs.applied.take().is_some() {
            if let Err(e) = self.reload_resources(None) {
                eprintln!("resource packs could not be restored: {e}");
            }
        }
        self.packs = pack::ResourcePacks::default();
    }

    /// `GuiDisconnected` over `GuiMultiplayer(GuiMainMenu)`: the screen every ended connection
    /// returns to.
    pub(crate) fn show_disconnected(&mut self, title_key: &str, reason: String) {
        self.screen = None;
        self.parents = vec![Box::new(screens::MainMenu::default()), Box::new(screens::MultiplayerScreen::new())];
        self.open(Box::new(screens::Disconnected::new(title_key, reason)), false);
    }

    /// Pause menu Disconnect: back to the server list (`GuiMultiplayer(GuiMainMenu)`).
    pub(crate) fn disconnect_to_server_list(&mut self) {
        self.leave_world();
        self.screen = None;
        self.parents.clear();
        self.open(Box::new(screens::MainMenu::default()), false);
        self.open(Box::new(screens::MultiplayerScreen::new()), true);
    }

    /// Replaces every piece of per-world state with its fresh value.
    pub(crate) fn reset_world_state(&mut self) {
        self.forge_generation = self.forge_generation.wrapping_add(1);
        #[cfg(feature = "wasm-mods")]
        if let Some(mods) = &self.mods { mods.clear(); }
        let mut world = World::default();
        world.track_dirty = true;
        self.world = world;
        if let Some(pool) = &mut self.mesh_workers { pool.clear(); }
        if let Some(r) = &mut self.renderer {
            r.clear_sections();
        }
        let spawn = find_player_spawn(&self.world, (8, 8));
        self.spawn = spawn;
        self.entity = player::Player::new(spawn);
        self.entity.yaw = -45.;
        self.entity.pitch = 20.;
        self.entity.prev_yaw = -45.;
        self.entity.prev_pitch = 20.;
        self.target = None;
        self.clicks.clear();
        self.keys.clear();
        self.buttons.clear();
        self.right_click_delay = 0;
        self.left_click_counter = 0;
        self.hitting = None;
        self.block_damage = 0.;
        self.block_hit_delay = 0;
        self.items.clear();
        self.next_entity_id = 1;
        self.particles = particles::Particles::default();
        self.drop_presses.clear();
        self.pending_open_inventory = false;
        self.info = level::WorldInfo::new(0, BlockPos::new(spawn.x.floor() as i32, spawn.y.floor() as i32, spawn.z.floor() as i32));
        self.weather = level::Weather::default();
        // The chat history and its tick clock survive (`GuiNewChat` is only cleared on the title screen).
        let (chat, ticks) = (std::mem::take(&mut self.hud.chat), self.hud.update_counter);
        self.hud = Hud::default();
        self.hud.chat = chat;
        self.hud.update_counter = ticks;
        self.player = PlayerHud::default();
        self.has_valid_health = false;
        self.score = 0;
        self.remote = entities::Entities::default();
        self.creative_sync = None;
        self.open_window = None;
        self.player_names.clear();
        self.scoreboard = Default::default();
        self.highlight_stack = None;
        self.step_sound_counter = 0.;
        self.pointed_entity = None;
        self.pointed_hit = mc_core::Vec3::new(0., 0., 0.);
        self.item_in_use = None;
        self.item_in_use_count = 0;
        self.pending_items.clear();
        self.paintings.clear();
        self.lightning.clear();
        self.attached.clear();
        self.break_anims.clear();
        self.bed = None;
        self.border = level::WorldBorder::default();
        self.brand.clear();
        self.stats.clear();
        self.maps.clear();
        self.tiles = Default::default();
        self.signs.clear();
        self.camera_entity = None;
        self.tab_complete = None;
        self.ui.inventory = [None; 36];
        self.ui.armor = [None; 4];
        self.ui.crafting = [None; 4];
        self.ui.drops.clear();
        self.ui.effects.clear();
        self.ui.sounds.clear();
        self.ui.selected_slot = 0;
        // `mcSoundHandler.stopSounds()`
        if let Some(s) = &mut self.sound {
            s.stop_all();
        }
        self.camera = Camera { pos: Vec3::new(spawn.x as f32, spawn.y as f32 + 1.62, spawn.z as f32), yaw: -45., pitch: 20., fov_deg: self.ui.settings.fov, bob: glam::Mat4::IDENTITY };
    }
}

/// The connector thread's work: resolve the session, then connect and log in.
fn connect_blocking(addr: &mc_protocol::status::Address, name: &str, online: bool, prism: Option<auth::Session>, stage: &AtomicU8, socket: &std::sync::Mutex<Option<std::net::TcpStream>>, cancel: &AtomicBool, asset: Option<(PathBuf, std::sync::mpsc::Sender<Result<PathBuf, String>>)>) -> Outcome {
    let session = if prism.is_some() {
        prism
    } else if online {
        match auth::login() {
            Ok(s) => {
                println!("Signed in as {}", s.name);
                Some(s)
            }
            Err(e) => return Err(format!("Microsoft sign-in failed: {e}")),
        }
    } else {
        None
    };
    if session.is_some() { if let Some((dir, tx)) = asset { crate::asset_download::start(dir, tx); } }
    let name = session.as_ref().map_or(name.to_string(), |s| s.name.clone());
    let forge = match std::env::var("RUSTMC_FORGE").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => mc_protocol::status::query(addr, cancel, Duration::from_millis(1200))
            .ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|s| s.get("modinfo").and_then(|m| m.get("type")).and_then(|v| v.as_str()) == Some("FML")),
    };
    if cancel.load(Ordering::Relaxed) { return Err("Connection cancelled".into()); }
    net::Net::connect(&addr.host, addr.port, &name, session.as_ref(), stage, socket, cancel, forge).map(|n| (n, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};

    struct Mock(Child);
    impl Drop for Mock {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Builds and starts `mc-protocol`'s mock server (busy scene knobs on) on a free port.
    fn mock_server() -> (Mock, u16) {
        let exe = std::env::current_exe().unwrap();
        let profile_dir = exe.parent().and_then(|d| d.parent()).unwrap().to_path_buf();
        let mut build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        build.args(["build", "-p", "mc-protocol", "--example", "mock_server"]).env("CARGO_TARGET_DIR", profile_dir.parent().unwrap());
        if profile_dir.file_name().is_some_and(|n| n == "release") {
            build.arg("--release");
        }
        assert!(build.status().unwrap().success());
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let child = Command::new(profile_dir.join("examples/mock_server"))
            .arg(port.to_string())
            .env("MOCK_MOBS", "1")
            .env("MOCK_TILES", "1")
            .env("MOCK_SCORE", "1")
            .env("MOCK_CHEST", "1")
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        (Mock(child), port)
    }

    fn app() -> Option<App> {
        let jar = find_jar()?;
        let mut assets = AssetSource::new();
        assets.push(&jar).ok()?;
        let models = BlockModels::load(&assets);
        let mut world = World::default();
        world.track_dirty = true;
        Some(App::new(assets, models, world, (8, 8), Settings::default()))
    }

    /// Connects, runs the net loop until the join sequence has produced a world.
    fn join(app: &mut App, port: u16) {
        app.start_connect(&format!("127.0.0.1:{port}"), "Tester", false, Some(pack::Mode::Disabled));
        app.wait_connect();
        assert!(app.net.is_some(), "login failed: {}", app.screen.is_some());
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) && (app.world.chunks.len() < 16 || app.remote.map.is_empty()) {
            app.process_net();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.world.chunks.len() >= 16 && !app.remote.map.is_empty());
    }

    fn assert_empty_world(app: &App) {
        assert!(app.net.is_none() && !app.in_game && !app.ui.in_world);
        assert!(app.world.chunks.is_empty() && app.world.dirty.is_empty());
        assert!(app.remote.map.is_empty() && app.items.is_empty() && app.paintings.is_empty() && app.attached.is_empty());
        assert!(app.scoreboard.players.is_empty() && app.player_names.is_empty());
        assert!(app.signs.is_empty() && app.maps.is_empty() && app.pending_items.is_empty() && app.brand.is_empty());
        assert!(app.open_window.is_none() && app.camera_entity.is_none());
    }

    #[test]
    fn leaving_a_server_resets_the_world_and_a_rejoin_starts_clean() {
        let Some(mut app) = app() else { return };
        let (_mock, port) = mock_server();
        join(&mut app, port);
        app.hud.add_chat("kept across servers");
        app.leave_world();
        assert_empty_world(&app);
        app.hud.chat.layout(&app.ui, true);
        assert!(app.hud.chat.line_count(&app.ui, true) >= 1, "chat history survives");
        // Rejoin: the scene arrives again and nothing from the first session is left over.
        join(&mut app, port);
        let ids: Vec<i32> = app.remote.map.keys().copied().collect();
        assert!(!ids.is_empty());
        app.leave_world();
        assert_empty_world(&app);
    }

    #[test]
    fn a_failed_connection_shows_disconnected_over_the_previous_screen() {
        let Some(mut app) = app() else { return };
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        app.open(Box::new(screens::MainMenu::default()), false);
        app.start_connect(&format!("127.0.0.1:{port}"), "Tester", false, None);
        assert!(app.connecting.is_some() && app.screen.is_some());
        app.wait_connect();
        assert!(app.net.is_none() && !app.in_game);
        assert_eq!(app.parents.len(), 1, "the screen it came from is kept for the Back button");
    }

    #[test]
    fn cancel_aborts_the_connector() {
        let Some(mut app) = app() else { return };
        let (_mock, port) = mock_server();
        app.open(Box::new(screens::MainMenu::default()), false);
        app.start_connect(&format!("127.0.0.1:{port}"), "Tester", false, None);
        app.cancel_connect();
        app.close_screen();
        std::thread::sleep(Duration::from_millis(300));
        app.poll_connecting();
        assert!(app.net.is_none() && app.connecting.is_none() && !app.in_game);
    }

    #[test]
    fn cancel_closes_a_silent_login_socket() {
        use std::io::Read;
        let Some(mut app) = app() else { return };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        app.start_connect(&format!("127.0.0.1:{port}"), "Tester", false, None);
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        // Wait until the socket is registered, then cancel while login waits for a reply.
        let start = Instant::now();
        while app.connecting.as_ref().unwrap().socket.lock().unwrap().is_none() {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(1));
        }
        app.cancel_connect();
        let mut bytes = [0; 4096];
        loop {
            match stream.read(&mut bytes) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(e) => panic!("socket did not close: {e}"),
            }
        }
    }

    fn threads_and_rss() -> Option<(usize, usize)> {
        let pid = std::process::id().to_string();
        let ps = |args: &[&str]| Command::new("ps").args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
        let rss = ps(&["-o", "rss=", "-p", &pid])?.trim().parse().ok()?;
        let threads = if cfg!(target_os = "macos") { ps(&["-M", "-p", &pid])?.lines().count().saturating_sub(1) } else { ps(&["-T", "-p", &pid])?.lines().count().saturating_sub(1) };
        Some((threads, rss))
    }

    /// 20 connect/disconnect cycles against the mock server: threads and memory stay flat.
    #[test]
    fn twenty_cycles_leak_no_threads() {
        let Some(mut app) = app() else { return };
        let (_mock, port) = mock_server();
        join(&mut app, port);
        app.leave_world();
        std::thread::sleep(Duration::from_millis(300));
        let before = threads_and_rss();
        for _ in 0..20 {
            join(&mut app, port);
            app.leave_world();
            assert_empty_world(&app);
        }
        std::thread::sleep(Duration::from_millis(500));
        let after = threads_and_rss();
        eprintln!("cycles: before {before:?} after {after:?} (threads, rss KiB)");
        if let (Some((t0, m0)), Some((t1, m1))) = (before, after) {
            assert!(t1 <= t0 + 1, "threads grew {t0} -> {t1}");
            assert!(m1 < m0 + 150 * 1024, "rss grew {m0} -> {m1} KiB");
        }
    }
    #[test]
    #[ignore = "requires a local Forge 1.8.9 server (RUSTMC_FORGE_TEST_PORT)"]
    fn real_forge_server_auto_detect() {
        let port: u16 = std::env::var("RUSTMC_FORGE_TEST_PORT").unwrap().parse().unwrap();
        let addr = mc_protocol::status::Address { host: "127.0.0.1".into(), port };
        let cancel = AtomicBool::new(false);
        let socket = std::sync::Mutex::new(None);
        let (mut n, _) = connect_blocking(&addr, "ProbeAuto", false, None, &AtomicU8::new(0), &socket, &cancel, None).unwrap();
        let start = Instant::now();
        let mut joined = false;
        while start.elapsed() < Duration::from_secs(20) && !joined {
            for item in n.poll() {
                match item {
                    net::Incoming::Packet(p) => match *p {
                        mc_protocol::packets::PlayClientbound::PluginMessageS(p) if p.channel == "FML|HS" => n.forge_message(&p.data),
                        mc_protocol::packets::PlayClientbound::PluginMessageS(p) if p.channel == "FORGE" => n.forge_runtime(&p.data),
                        mc_protocol::packets::PlayClientbound::JoinGame(_) => joined = true,
                        mc_protocol::packets::PlayClientbound::Disconnect(p) => panic!("server disconnected: {}", p.reason),
                        _ => {}
                    },
                    net::Incoming::Closed(e) => panic!("connection closed: {e}"),
                    net::Incoming::Chunks(_) => {},
                }
                assert!(n.closed.is_none(), "{:?}", n.closed);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(joined, "Forge auto-detection did not reach JoinGame");
    }

    #[test]
    #[ignore = "requires a local Forge server and RUSTMC_FORGE_TEST_JAR"]
    fn real_forge_server_app_join() {
        let port: u16 = std::env::var("RUSTMC_FORGE_TEST_PORT").unwrap().parse().unwrap();
        let jar = std::env::var("RUSTMC_FORGE_TEST_JAR").unwrap();
        let mut assets = AssetSource::new();
        assets.push(Path::new(&jar)).unwrap();
        let models = BlockModels::load(&assets);
        let mut world = World::default();
        world.track_dirty = true;
        let mut app = App::new(assets, models, world, (8,8), Settings::default());
        app.start_connect(&format!("127.0.0.1:{port}"), "ProbeApp", false, Some(pack::Mode::Disabled));
        app.wait_connect();
        assert!(app.net.is_some(), "login failed");
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) && app.world.chunks.is_empty() {
            app.process_net();
            assert!(app.net.as_ref().is_some_and(|n| n.closed.is_none()), "Forge connection closed");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!app.world.chunks.is_empty(), "Forge world chunks did not arrive");
        app.leave_world();
        assert!(app.world.chunks.is_empty());
    }

}
