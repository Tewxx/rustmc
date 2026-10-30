//! Opening and closing screens, screen actions and settings.

use crate::*;

// Declared here so `main.rs` (owned elsewhere) needs no `mod` line.
#[path = "server_list.rs"]
pub(crate) mod server_list;

impl App {
    pub(crate) fn open(&mut self, mut s: Box<dyn Screen>, keep_parent: bool) {
        server_list::install();
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

    pub(crate) fn close_screen(&mut self) {
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

    /// `displayGuiScreen(null)`: drops the screen and its parents; leaving `GuiChat` resets
    /// the chat scroll (`onGuiClosed`).
    pub(crate) fn close_all(&mut self) {
        if self.screen.as_ref().is_some_and(|s| s.is_chat()) || self.parents.iter().any(|s| s.is_chat()) {
            self.hud.chat.reset_scroll();
        }
        self.parents.clear();
        self.screen = None;
        if self.in_game {
            self.grab(true);
        }
    }

    /// `GuiChat.getChatComponent(Mouse.getX(), Mouse.getY())`
    pub(crate) fn chat_piece_under_mouse(&self) -> Option<mc_gui::component::Piece> {
        let h = self.renderer.as_ref().map_or(720, |r| r.config.height) as i32;
        let (x, y) = (self.mouse_px.0 as i32, h - self.mouse_px.1 as i32 - 1);
        self.hud.chat.component_at(&self.ui, self.scaled().factor, x, y).cloned()
    }

    /// `GuiScreen.handleComponentClick`; true when the click was used.
    pub(crate) fn handle_component_click(&mut self, piece: &mc_gui::component::Piece) -> bool {
        use mc_gui::component::ClickAction;
        if self.ui.shift_down {
            if let Some(ins) = &piece.insertion {
                if let Some(s) = &mut self.screen {
                    s.set_chat_text(&self.ui, ins, false);
                }
            }
            return false;
        }
        let Some(click) = &piece.click else { return false };
        match click.action {
            ClickAction::OpenUrl => {
                if !self.ui.settings.chat_links {
                    return false;
                }
                // Only http(s) URIs with a scheme are accepted (`PROTOCOLS`).
                let scheme = click.value.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
                match scheme.as_deref() {
                    Some("http") | Some("https") => {
                        if self.ui.settings.chat_links_prompt {
                            self.open(Box::new(screens::ConfirmOpenLink::new(click.value.clone(), false)), true);
                        } else {
                            open_web_link(&click.value);
                        }
                    }
                    _ => eprintln!("Can't open url for {:?}", click.value),
                }
            }
            ClickAction::OpenFile => open_web_link(&click.value),
            ClickAction::SuggestCommand => {
                if let Some(s) = &mut self.screen {
                    s.set_chat_text(&self.ui, &click.value, true);
                }
            }
            ClickAction::RunCommand => {
                // sendChatMessage(value, false): sent without entering the history.
                let v = click.value.clone();
                self.send_chat(&v);
            }
            ClickAction::TwitchUserInfo | ClickAction::ChangePage => eprintln!("Don't know how to handle {:?}", click.action),
        }
        true
    }

    pub(crate) fn handle(&mut self, action: Action, el: &ActiveEventLoop) {
        match action {
            Action::None => {}
            Action::Enchant { window_id, choice } => {
                self.flush_window();
                self.send(mc_protocol::packets::PlayServerbound::EnchantItem(mc_protocol::packets::EnchantItem { window_id, enchantment: choice }));
            }
            Action::Close => self.close_screen(),
            Action::Open(s) => {
                // `servers.dat` picks up a pack choice the last joined client stored.
                server_list::sync_from_store(&mc_gui::multiplayer::game_dir(&self.ui));
                self.open(s, true)
            }
            Action::StartGame => {
                self.in_game = true;
                self.ui.in_world = true;
                self.parents.clear();
                self.screen = None;
                self.grab(true);
            }
            Action::QuitToTitle => {
                self.cancel_connect();
                self.leave_world();
                self.parents.clear();
                self.screen = None;
                self.open(Box::new(screens::MainMenu::default()), false);
            }
            Action::Disconnect if self.net.is_some() => self.disconnect_to_server_list(),
            Action::Disconnect => self.handle(Action::QuitToTitle, el),
            Action::CancelConnect => {
                self.cancel_connect();
                self.close_screen();
            }
            Action::Connect { address, name, online, pack_mode } => {
                // `new GuiConnecting(...)`: connects in-process; the pack choice is the saved server's.
                if let Some(mode) = pack_mode {
                    server_list::apply_connect_mode(&address, mode);
                }
                self.start_connect(&address, &name, online, pack_mode.map(pack::Mode::parse));
            }
            Action::Quit => el.exit(),
            Action::CloseAll => self.close_all(),
            Action::ScrollChatPage(dir) => {
                let n = self.hud.chat.line_count(&self.ui, true) - 1;
                self.hud.chat.scroll(&self.ui, dir * n, true);
            }
            Action::OpenLink(url) => {
                // GuiChat.confirmClicked: open it, then back to the chat.
                open_web_link(&url);
                self.close_screen();
            }
            Action::ResourcePackDecision(accept) => self.resource_pack_decision(accept),
            Action::TabComplete(prefix) => {
                if self.net.is_some() {
                    // `GuiChat.sendAutocompleteRequest`: the server owns command completions.
                    self.tab_complete = Some((prefix.clone(), Vec::new()));
                    self.send(mc_protocol::packets::PlayServerbound::TabCompleteC(mc_protocol::packets::TabCompleteC { text: prefix, looking_at: self.target.map(|t| t.pos) }));
                } else {
                    #[cfg(feature = "singleplayer")]
                    self.local_tab_complete(&prefix);
                }
            }
            Action::PrintChat(line) => self.hud.add_chat(line),
            Action::SendChat(msg) => {
                self.close_all();
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
    pub(crate) fn apply_settings(&mut self) {
        let s = self.ui.settings.clone();
        self.camera.fov_deg = s.fov;
        let Some(r) = &mut self.renderer else { return };
        r.render_distance_blocks = (s.render_distance * 16) as f32;
        r.set_vsync(s.vsync);
        // GameSettings.sendSettingsToServer when a synced option changes.
        let a = &self.applied;
        if (s.chat_visibility, s.chat_colors, s.model_parts, s.render_distance) != (a.chat_visibility, a.chat_colors, a.model_parts, a.render_distance) {
            if let Some(n) = &mut self.net {
                n.send_settings("en_US", s.render_distance as i8, s.chat_visibility as i8, s.chat_colors, s.model_parts);
            }
        }
        if s.fullscreen != self.applied.fullscreen {
            if let Some(w) = &self.window {
                w.set_fullscreen(s.fullscreen.then(|| winit::window::Fullscreen::Borderless(None)));
            }
        }
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

    pub(crate) fn remesh(&mut self, smooth: bool) {
        let _z = crate::bench::zone(crate::bench::Phase::Mesh);
        let Some(r) = &mut self.renderer else { return };
        let t = Instant::now();
        let jobs: Vec<_> = self.world.chunks.keys().flat_map(|&(cx, cz)| (0..16).map(move |sy| (cx, sy, cz))).collect();
        let mut quads = 0;
        for ((cx, sy, cz), m) in mesh_parallel(&self.world, &self.models, &jobs, smooth, self.ui.settings.fancy_graphics) {
            quads += m.indices.len() / 6;
            r.upload_section(cx, sy, cz, &m);
        }
        println!(
            "meshed {} chunks ({quads} quads) in {:.1} ms",
            self.world.chunks.len(),
            t.elapsed().as_secs_f64() * 1e3
        );
    }

    pub(crate) fn grab(&mut self, on: bool) {
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

}
