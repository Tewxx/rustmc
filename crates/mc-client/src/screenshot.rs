//! `--screenshot` mode.

use crate::*;

impl App {
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
            for _ in 0..40 {
                self.process_net();
                self.run_tick();
                self.hud_tick();
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
