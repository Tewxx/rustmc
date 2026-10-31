//! `--screenshot` mode.

use crate::*;

impl App {
    /// Same-camera comparison with the vanilla harness (`tools/visual`): `RUSTMC_VISUAL=1` pins the
    /// settings the harness uses, `RUSTMC_CAM="x y z yaw pitch"` places the eye, `RUSTMC_TIME` sets
    /// the world time. Unset, the screenshot path is unchanged.
    fn apply_visual_fixture(&mut self, which: &str) {
        if std::env::var_os("RUSTMC_VISUAL").is_none() {
            return;
        }
        let st = &mut self.ui.settings;
        st.render_distance = 16;
        st.gui_scale = 2;
        st.fov = 70.;
        st.view_bobbing = false;
        st.clouds = 0;
        st.fancy_graphics = true;
        st.ambient_occlusion = 2;
        st.gamma = 0.;
        st.hide_gui = which == "hud" || which == "scene";
        self.ui.splash = "Hello world!".into();
        self.apply_settings();
    }

    /// Over a connection, put the camera where the vanilla harness had it (`/tp`, `/time set`; the
    /// player needs op) and wait for the chunks around it. `RUSTMC_CAM` is the EYE position.
    fn visual_teleport(&mut self) {
        let Ok(c) = std::env::var("RUSTMC_CAM") else { return };
        let v: Vec<f64> = c.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() != 5 {
            return;
        }
        self.entity.flying = true;
        let name = self.name.clone();
        self.send_chat(&format!("/tp {name} {} {} {} {} {}", v[0], v[1] - 1.62, v[2], v[3], v[4]));
        if let Ok(t) = std::env::var("RUSTMC_TIME") {
            self.send_chat(&format!("/time set {t}"));
        }
        let (mut last, mut stable) = (self.world.chunks.len(), Instant::now());
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(30) && stable.elapsed() < Duration::from_secs(3) {
            self.process_net();
            std::thread::sleep(Duration::from_millis(50));
            if self.world.chunks.len() != last {
                last = self.world.chunks.len();
                stable = Instant::now();
            }
        }
    }

    /// `--screenshot`: set up the requested screen, render once off-screen, exit.
    pub(crate) fn take_screenshot(&mut self, path: &Path, which: &str) {
        let in_game = !matches!(which, "title" | "title-options" | "connecting" | "disconnected" | "connect-failed") && !which.starts_with("multiplayer") && !which.starts_with("mp-");
        self.in_game = in_game;
        self.ui.in_world = in_game;
        if self.net.is_some() {
            // Wait for the join sequence, then run real ticks so movement packets go out.
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(5) && self.world.chunks.len() < 64 {
                self.process_net();
                std::thread::sleep(Duration::from_millis(20));
            }
            self.visual_teleport();
            // The full client tick (hud, screen, world), inside Minecraft.runTick when Forge mods run.
            for _ in 0..40 {
                self.process_net();
                self.forge_run_client_tick(false);
            }
            std::thread::sleep(Duration::from_millis(500));
            self.process_net();
            for (uuid, cell, rgba) in self.skins.poll() {
                if let Some(r) = &mut self.renderer {
                    r.upload_skin(cell, &rgba);
                    r.register_gui_texture(&skins::gui_path(uuid), 64, 64, &rgba);
                    skins::mark_gui_skin(uuid);
                }
            }
        }
        for _ in 0..30 {
            self.entity.tick(&self.world, &mut self.geometry, player::Keys::default());
            let current = self.ui.inventory[self.player.selected_slot].map(|s| (s.item, s.count));
            self.entity.update_equipped(current);
        }
        self.tick_accum = 1.;
        self.apply_visual_fixture(which);
        if std::env::var_os("RUSTMC_VISUAL").is_some() && in_game {
            assert!(self.net.is_some(), "visual scene requires a server connection");
            let cam = std::env::var("RUSTMC_CAM").expect("visual scene needs RUSTMC_CAM");
            let v: Vec<f64> = cam.split_whitespace().map(|x| x.parse().expect("invalid camera coordinate")).collect();
            assert!(v.len() == 5 && v.iter().all(|x| x.is_finite()), "expected five finite camera values");
            let eye_height = self.entity.eye_height() as f64;
            self.entity.pos = mc_core::Vec3::new(v[0], v[1] - eye_height, v[2]);
            self.entity.prev_pos = self.entity.pos;
            self.entity.yaw = v[3] as f32;
            self.entity.prev_yaw = self.entity.yaw;
            self.entity.pitch = v[4] as f32;
            self.entity.prev_pitch = self.entity.pitch;
        }
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
            "multiplayer" | "multiplayer-tip" => {
                self.open(Box::new(screens::MultiplayerScreen::new()), false);
                // Pings start on the first draw; let them land before the captured frame.
                self.build_gui();
                std::thread::sleep(Duration::from_millis(1500));
            }
            "mp-add" | "mp-direct" | "mp-delete" => {
                if let Some(s) = mc_gui::multiplayer::preview_screen(which, &self.ui.lang) {
                    self.open(s, false);
                }
            }
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
            "armor" | "armor-back" | "armor-inv" | "armor-tip" => {
                use mc_core::nbt::{Compound, Interned, Tag};
                // Enchanted diamond helmet, iron chestplate, red leather leggings, gold boots.
                let mut ench = Compound::new();
                let mut e = Compound::new();
                e.insert("id".into(), Tag::Short(0));
                e.insert("lvl".into(), Tag::Short(4));
                ench.insert("ench".into(), Tag::List(10, vec![Tag::Compound(e)]));
                let mut dyed = Compound::new();
                let mut display = Compound::new();
                display.insert("color".into(), Tag::Int(0xB02E26));
                dyed.insert("display".into(), Tag::Compound(display));
                for (slot, name, tag) in [(0, "diamond_helmet", Some(ench)), (1, "iron_chestplate", None), (2, "leather_leggings", Some(dyed)), (3, "golden_boots", None)] {
                    if let Some(item) = self.catalogue.find(name, 0) {
                        let mut st = self.catalogue.stack(item, 1);
                        st.tag = tag.map(Interned::new);
                        self.ui.armor[slot] = Some(st);
                    }
                }
                // Potions by damage value (Swiftness II, splash Poison, Awkward), the enchanted apple.
                for (slot, name, damage) in [(0, "potion", 8226), (1, "potion", 16388), (2, "potion", 16), (3, "golden_apple", 1), (4, "potion", 8197)] {
                    if let Some(item) = self.catalogue.find(name, 0) {
                        self.ui.inventory[slot] = Some(mc_gui::ItemStack { damage, ..self.catalogue.stack(item, 1) });
                    }
                }
                if which == "armor-inv" || which == "armor-tip" {
                    self.open(Box::new(survival::SurvivalInventory::new(self.catalogue.clone())), false);
                } else {
                    self.entity.perspective = if which == "armor" { 2 } else { 1 };
                    self.entity.pitch = 10.;
                    self.screen = None;
                }
                self.sync_view();
            }
            "underwater" => {
                // A water pool around the camera: GL_EXP water fog and the narrowed FOV.
                self.send_chat("/fill ~-3 ~-1 ~-3 ~3 ~3 ~3 water");
                for _ in 0..5 {
                    self.run_tick();
                }
                self.entity.pitch = 0.;
                self.screen = None;
                self.sync_view();
            }
            "skulls" | "skulls-body" => {
                // Every skull type in the hotbar; a creeper head worn.
                for k in 0..5 {
                    if let Some(item) = self.catalogue.find("skull", k) {
                        self.ui.inventory[k as usize] = Some(mc_gui::ItemStack { damage: k as i16, ..self.catalogue.stack(item, 1) });
                    }
                }
                if let Some(item) = self.catalogue.find("skull", 4) {
                    self.ui.armor[0] = Some(mc_gui::ItemStack { damage: 4, ..self.catalogue.stack(item, 1) });
                }
                if which == "skulls" {
                    self.open(Box::new(survival::SurvivalInventory::new(self.catalogue.clone())), false);
                } else {
                    self.entity.perspective = 2;
                    self.entity.pitch = 10.;
                    self.screen = None;
                }
                self.sync_view();
            }
            w if w.starts_with("use-") => {
                // use-<item>[-<ticks used>][-body]: an item in use, first person or F5.
                let rest = w.strip_prefix("use-").unwrap();
                let body = rest.ends_with("-body");
                let rest = rest.trim_end_matches("-body");
                let (name, used) = match rest.rsplit_once('-') {
                    Some((n, t)) if t.parse::<i32>().is_ok() => (n, t.parse().unwrap()),
                    _ => (rest, 5),
                };
                if let Some(item) = self.catalogue.find(name, 0) {
                    let s = self.catalogue.stack(item, 1);
                    self.ui.inventory[0] = Some(s);
                    self.player.selected_slot = 0;
                    for _ in 0..5 {
                        self.entity.update_equipped(Some((s.item, s.count)));
                    }
                    self.item_in_use = Some(s);
                    self.item_in_use_count = item_use::action(self.catalogue.items[item].id).1 - used;
                }
                self.entity.perspective = if body { 2 } else { 0 };
                self.screen = None;
                self.sync_view();
            }
            w if w == "held" || w.starts_with("held-") => {
                let name = w.strip_prefix("held-").unwrap_or("planks");
                if let Some(item) = self.catalogue.find(name, 0) {
                    self.ui.inventory[0] = Some(self.catalogue.stack(item, 1));
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
            "connecting" => self.open(Box::new(screens::Connecting::new(Default::default())), false),
            "disconnected" => self.open(Box::new(screens::Disconnected::new("disconnect.lost", "Internal Exception: io.netty.handler.timeout.ReadTimeoutException")), false),
            "connect-failed" => self.open(Box::new(screens::Disconnected::new("connect.failed", "Unknown host")), false),
            "title" => self.open(Box::new(screens::MainMenu::default()), false),
            "title-options" => self.open(Box::new(screens::Options::default()), false),
            "pause" => self.open(Box::new(screens::PauseMenu::default()), false),
            "options" => self.open(Box::new(screens::Options::default()), false),
            "video" => self.open(Box::new(screens::VideoSettings::default()), false),
            "chatopts" => self.open(Box::new(screens::OptionList::chat()), false),
            "skinopts" => self.open(Box::new(screens::OptionList::skin()), false),
            "sounds" => self.open(Box::new(screens::SoundSettings::default()), false),
            "controls" => self.open(Box::new(screens::Controls::default()), false),
            "resourcepacks" | "resourcepacks-hover" => self.open(Box::new(mc_gui::resource_packs::ResourcePacks::default()), false),
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
            "net-tab" => self.force_tab = true,
            "net-inv" | "net-inv-tip" => {
                let title = self.ui.lang.get("container.crafting");
                let w = Arc::new(std::sync::Mutex::new(container::Window::new(0, "player", title, 5)));
                self.open_window = Some(w.clone());
                self.open(Box::new(container::ContainerScreen::new(w, self.catalogue.clone())), false)
            }
            _ => self.screen = None,
        }
        // RUSTMC_SHOT_KEYS="24 54": LWJGL key codes tapped in-game before the capture, e.g. a Forge mod's GUI key.
        if let Ok(keys) = std::env::var("RUSTMC_SHOT_KEYS") {
            for code in keys.split_whitespace().filter_map(|k| k.parse::<i32>().ok()) {
                self.forge_key(code, true, false, None);
                // Held across one frame (runGameLoop: render-tick handlers) and one client tick.
                self.forge_frame(|_| {});
                self.forge_run_client_tick(false);
                self.forge_key(code, false, false, None);
                self.forge_run_client_tick(false);
            }
        }
        // RUSTMC_SHOT_CLICKS="390,32,0 645,32,1": framebuffer-pixel clicks (x,y,button) on the open mod screen.
        if let Ok(clicks) = std::env::var("RUSTMC_SHOT_CLICKS") {
            let f = self.scaled().factor.max(1) as i32;
            // RUSTMC_SHOT_WAIT_MS: let opening animations finish (frames keep drawing) before clicking.
            let wait = std::env::var("RUSTMC_SHOT_WAIT_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            let until = std::time::Instant::now() + std::time::Duration::from_millis(wait);
            let (mut n, mut spent, mut worst) = (0u32, std::time::Duration::ZERO, std::time::Duration::ZERO);
            while std::time::Instant::now() < until {
                let t = std::time::Instant::now();
                self.build_gui();
                spent += t.elapsed();
                worst = worst.max(t.elapsed());
                n += 1;
            }
            if n > 0 { eprintln!("shot: {n} gui frames, avg {:.2} ms, worst {:.2} ms", spent.as_secs_f64() * 1000.0 / n as f64, worst.as_secs_f64() * 1000.0); }
            for click in clicks.split_whitespace() {
                let click_start = std::time::Instant::now();
                let v: Vec<i32> = click.split(',').filter_map(|n| n.parse().ok()).collect();
                let [x, y, button] = v[..] else { continue };
                // The pointer is there first, as a real click is (screens hit-test with Mouse.getX/getY).
                self.mouse_px = (x as f64, y as f64);
                mc_forge::impls::set_mouse(x, y);
                // A drawn frame at that position: screens track hover in drawScreen.
                self.build_gui();
                for down in [true, false] {
                    mc_forge::runtime::with_env(|env| mc_forge::game::mouse_event(env, x / f, y / f, button, down, 0));
                    self.forge_frame(|_| {});
                }
                self.forge_run_client_tick(false);
                eprintln!("shot: click {click} took {:.1} ms", click_start.elapsed().as_secs_f64() * 1000.0);
            }
            // A click may have opened another screen (a frame's forge_sync).
            let sync_start = std::time::Instant::now();
            self.forge_sync();
            eprintln!("shot: forge_sync after clicks took {:.1} ms", sync_start.elapsed().as_secs_f64() * 1000.0);
            if std::env::var_os("RUSTMC_SHOT_DEBUG").is_some() {
                let f = mc_forge::runtime::with_env(|env| (mc_forge::game::focus(env).ok(), mc_forge::game::vanilla_screen(env).ok()));
                let css = mc_forge::runtime::with_env(|env| -> Option<(f32, f32, i32)> {
                    let mc = mc_forge::impls::minecraft(env).ok()?;
                    let sc = env.get_field(&mc, "field_71462_r", "Lnet/minecraft/client/gui/GuiScreen;").ok()?.l().ok()?;
                    let x = env.get_field(&sc, "mouseCssX", "F").ok()?.f().ok()?;
                    let y = env.get_field(&sc, "mouseCssY", "F").ok()?.f().ok()?;
                    let hits = env.get_field(&sc, "hits", "Ljava/util/List;").ok()?.l().ok()?;
                    let n = env.call_method(&hits, "size", "()I", &[]).ok()?.i().ok()?;
                    Some((x, y, n))
                });
                let _ = mc_forge::runtime::with_env(|env| env.exception_clear());
                eprintln!("shot: lunar css/hits {css:?}");
                eprintln!("shot: after clicks focus={f:?} mod_screen={} screen_kind={:?}", self.forge_mod_screen, self.screen.as_ref().and_then(|s| s.java_screen()));
            }
        }
        // RUSTMC_SHOT_MOD_MULTIPLAYER: the current (mod) screen opens vanilla GuiMultiplayer, as mod menus' buttons do.
        if std::env::var_os("RUSTMC_SHOT_MOD_MULTIPLAYER").is_some() {
            mc_forge::runtime::with_env(|env| { let _ = mc_forge::game::open_multiplayer_from_current(env); });
            self.forge_sync();
            // "back": then its Cancel (closing it returns to the mod's screen).
            if std::env::var("RUSTMC_SHOT_MOD_MULTIPLAYER").as_deref() == Ok("back") { self.close_screen(); self.forge_sync(); }
        }
        // Give screens a fake mouse over the first button for hover states.
        let sr = self.scaled();
        self.mouse_px = (
            (sr.width / 2) as f64 * sr.factor as f64,
            (sr.height / 4 + 48 + 5) as f64 * sr.factor as f64,
        );
        if which == "resourcepacks-hover" {
            // Second selected entry, upper right quarter: the "move up" arrow.
            self.mouse_px = (((sr.width / 2 + 4 + 24) * sr.factor) as f64, (85 + 8) as f64 * sr.factor as f64);
        }
        if which == "multiplayer-tip" {
            // The first server's ping bars (list x + 305 - 15, y + 32 + 4).
            self.mouse_px = (((sr.width / 2 + 145) * sr.factor) as f64, (40 * sr.factor) as f64);
        }
        if which == "net-inv-tip" || which == "armor-tip" {
            // Hover the first hotbar slot of GuiInventory.
            let (left, top) = (self.ui.effect_gui_left(sr.width, 176), (sr.height - 166) / 2);
            self.mouse_px = (((left + 16) * sr.factor) as f64, ((top + 150) * sr.factor) as f64);
        }
        self.player.level = 7;
        self.player.experience = 0.4;
        self.player.selected_slot = 0;
        self.fps = 0;
        self.sync_view();
        let sr = self.build_gui();
        // As a live frame does: mod texture uploads made while building the GUI reach the renderer first.
        self.sync_pack_ui();
        let cam = self.in_game.then_some(&self.camera);
        let r = self.renderer.as_mut().unwrap();
        let (w, h) = (r.config.width, r.config.height);
        let pixels = r.capture(cam, Some((&self.batch, sr)), &self.assets, w, h);
        match image::save_buffer(path, &pixels, w, h, image::ColorType::Rgba8) {
            Ok(()) => println!("saved {} ({which}, gui scale {})", path.display(), sr.factor),
            Err(e) => eprintln!("failed to save {}: {e}", path.display()),
        }
    }

    /// `ScreenShotHelper.saveScreenshot` (F2, or a mod): `path`, or `screenshots/yyyy-MM-dd_HH.mm.ss[_n].png` in local
    /// time, then the `screenshot.success` chat line with the file name underlined and clickable. The PNG encode runs
    /// off the render thread; vanilla's synchronous write would stall the frame.
    pub(crate) fn save_screenshot(&mut self, path: Option<PathBuf>) {
        let Some(r) = self.renderer.as_mut() else { return };
        let path = path.unwrap_or_else(|| {
            let dir = self.ui.settings.game_dir().join("screenshots");
            let stamp = local_stamp();
            (1..).map(|n| dir.join(if n == 1 { format!("{stamp}.png") } else { format!("{stamp}_{n}.png") }))
                .find(|p| !p.exists()).unwrap()
        });
        let sr = self.build_gui();
        let cam = self.in_game.then_some(&self.camera);
        let r = self.renderer.as_mut().unwrap();
        let (w, h) = (r.config.width, r.config.height);
        let pixels = r.capture(cam, Some((&self.batch, sr)), &self.assets, w, h);
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let full = std::fs::canonicalize(path.parent().unwrap_or(Path::new("."))).map(|d| d.join(&name)).unwrap_or_else(|_| path.clone());
        let json = serde_json::json!({"translate":"screenshot.success","with":[{"text":name,"underlined":true,
            "clickEvent":{"action":"open_file","value":full.to_string_lossy()}}]}).to_string();
        self.hud.add_chat(crate::chat::parse(&json, &self.ui.lang));
        std::thread::spawn(move || {
            if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
            if let Err(e) = image::save_buffer(&path, &pixels, w, h, image::ColorType::Rgba8) {
                eprintln!("Couldn't save screenshot {}: {e}", path.display());
            }
        });
    }
}

/// `new SimpleDateFormat("yyyy-MM-dd_HH.mm.ss").format(new Date())`, in the local time zone.
fn local_stamp() -> String {
    let (y, mo, d, h, mi, s) = local_now();
    format!("{y:04}-{mo:02}-{d:02}_{h:02}.{mi:02}.{s:02}")
}

#[cfg(windows)]
fn local_now() -> (i32, u32, u32, u32, u32, u32) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime { year: u16, month: u16, weekday: u16, day: u16, hour: u16, minute: u16, second: u16, millis: u16 }
    extern "system" { fn GetLocalTime(t: *mut SystemTime); }
    let mut t = SystemTime::default();
    unsafe { GetLocalTime(&mut t) };
    (t.year as i32, t.month as u32, t.day as u32, t.hour as u32, t.minute as u32, t.second as u32)
}

#[cfg(unix)]
fn local_now() -> (i32, u32, u32, u32, u32, u32) {
    #[repr(C)]
    struct Tm { sec: i32, min: i32, hour: i32, mday: i32, mon: i32, year: i32, wday: i32, yday: i32, isdst: i32, gmtoff: i64, zone: *const u8 }
    extern "C" { fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm; }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let mut tm: Tm = unsafe { std::mem::zeroed() };
    unsafe { localtime_r(&now, &mut tm) };
    (tm.year + 1900, (tm.mon + 1) as u32, tm.mday as u32, tm.hour as u32, tm.min as u32, tm.sec as u32)
}
