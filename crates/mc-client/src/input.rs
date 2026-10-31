//! Keyboard and mouse mapping and the winit event handler.

use crate::*;

fn frame_deadline(last: Instant, limit: i32, vsync: bool, bench: bool) -> Option<Instant> {
    (!vsync && !bench && limit < 260).then(|| last + Duration::from_secs_f64(1.0 / limit.max(1) as f64))
}

/// Screen keys: the inventory and hotbar keys follow their bindings (`GuiContainer.keyTyped`).
pub(crate) fn map_key(code: KeyCode, settings: &mc_gui::settings::Settings, ctrl: bool) -> Key {
    let lw = keymap::lwjgl(code);
    // GuiScreen.isKeyComboCtrlA/C/V/X: checked before every other key.
    if ctrl {
        let letter = match code {
            KeyCode::KeyA => Some('a'),
            KeyCode::KeyC => Some('c'),
            KeyCode::KeyV => Some('v'),
            KeyCode::KeyX => Some('x'),
            _ => None,
        };
        if let Some(c) = letter {
            return Key::Combo(c);
        }
    }
    if lw != 0 && lw == settings.key("key.inventory") {
        return Key::Inventory;
    }
    if lw != 0 && lw == settings.key("key.pickItem") {
        return Key::PickItem;
    }
    if lw != 0 && lw == settings.key("key.drop") {
        return Key::Drop;
    }
    if let Some(n) = (0..9).find(|&i| lw != 0 && settings.key(&format!("key.hotbar.{}", i + 1)) == lw) {
        return Key::Number(n);
    }
    match code {
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
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => Key::Other,
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
            .with_inner_size({ let (w, h) = WINDOW_SIZE.get().copied().unwrap_or((1280, 720)); winit::dpi::PhysicalSize::new(w, h) })
            .with_visible(self.screenshot.is_none());
        // The installed Slinky launcher discovers Minecraft by its native LWJGL window
        // class. This opt-in affects discovery only; rendering still belongs to wgpu.
        #[cfg(target_os = "windows")]
        let attrs = {
            use winit::platform::windows::WindowAttributesExtWindows;
            if std::env::var("RUSTMC_WINDOW_CLASS").as_deref() == Ok("LWJGL") {
                attrs.with_class_name("LWJGL")
            } else { attrs }
        };
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
            self.force_sync_mesh = true;
            self.take_screenshot(&path, &which);
            self.force_sync_mesh = false;
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
        if self.screen.is_none() {
            self.open(Box::new(screens::MainMenu::default()), false);
        }
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
                if self.forge_on && !self.forge_mod_screen { self.forge_open_native(); }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_px = (position.x, position.y);
                let (mx, my) = self.mouse_gui();
                if let Some(s) = &mut self.screen { s.mouse_move(&mut self.ui, mx, my); }
            },
            WindowEvent::MouseInput { state, button, .. } => {
                let (mx, my) = self.mouse_gui();
                #[cfg(feature = "wasm-mods")]
                self.mod_mouse_event(mx, my, match button { MouseButton::Left => 0, MouseButton::Right => 1, MouseButton::Middle => 2, _ => -1 }, state == ElementState::Pressed, 0);
                let lwjgl_button = match button { MouseButton::Left => 0, MouseButton::Right => 1, MouseButton::Middle => 2, _ => -1 };
                if self.grabbed && self.forge_mouse(lwjgl_button, state == ElementState::Pressed, 0) {
                    return;
                }
                match state {
                    ElementState::Pressed => {
                        if button == MouseButton::Left && self.screen.as_ref().is_some_and(|s| s.is_chat()) {
                            if let Some(piece) = self.chat_piece_under_mouse() {
                                if self.handle_component_click(&piece) {
                                    return;
                                }
                            }
                        }
                        if button == MouseButton::Left && self.forge_click(mx, my) {
                            return;
                        }
                        if let Some(s) = &mut self.screen {
                            let a = match button {
                                MouseButton::Left => s.click(&mut self.ui, mx, my),
                                MouseButton::Right => s.right_click(&mut self.ui, mx, my),
                                MouseButton::Middle if self.ui.settings.key("key.pickItem") == -98 => s.pick_item(&mut self.ui, mx, my),
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
                            s.release_at(&mut self.ui, mx, my, match button { MouseButton::Left => 0, MouseButton::Right => 1, MouseButton::Middle => 2, _ => 255 });
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
                #[cfg(feature = "wasm-mods")]
                { let (mx, my) = self.mouse_gui(); self.mod_mouse_event(mx, my, -1, false, d); }
                if self.screen.as_ref().is_some_and(|s| s.is_chat()) {
                    // GuiChat.handleMouseInput: one line with shift, seven without.
                    if d != 0 {
                        let n = if self.ui.shift_down { d } else { d * 7 };
                        self.hud.chat.scroll(&self.ui, n, true);
                    }
                } else if let Some(s) = &mut self.screen {
                    s.scroll(d);
                } else if self.in_game && d != 0 && !self.forge_mouse(-1, false, d * 120) {
                    // Scrolling down moves right, like vanilla.
                    self.pending_selected_slot = Some((self.pending_selected_slot.unwrap_or(self.player.selected_slot) as i32 - d).rem_euclid(9) as usize);
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
                #[cfg(feature = "wasm-mods")]
                self.mod_key_event(code, event.state == ElementState::Pressed, event.repeat, event.text.as_ref().and_then(|t| t.chars().next()));
                self.forge_key(keymap::lwjgl(code), event.state == ElementState::Pressed, event.repeat, event.text.as_ref().and_then(|t| t.chars().next()));
                if event.state == ElementState::Released {
                    self.keys.remove(&code);
                    self.ui.shift_down = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
                    self.ui.ctrl_down = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight)
                        || (cfg!(target_os = "macos") && (self.keys.contains(&KeyCode::SuperLeft) || self.keys.contains(&KeyCode::SuperRight)));
                    return;
                }
                if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    self.keys.insert(code);
                    self.ui.shift_down = true;
                }
                // Command acts as Control on macOS, as Minecraft treats it.
                if matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight) || (cfg!(target_os = "macos") && matches!(code, KeyCode::SuperLeft | KeyCode::SuperRight)) {
                    self.keys.insert(code);
                    self.ui.ctrl_down = true;
                }
                self.ui.key_code = keymap::lwjgl(code);
                if !event.repeat && keymap::lwjgl(code) != 0 && keymap::lwjgl(code) == self.ui.settings.key("key.screenshot") {
                    self.save_screenshot(None);
                }
                let mapped = map_key(code, &self.ui.settings, self.ui.ctrl_down);
                if let Some(s) = &mut self.screen {
                    let a = s.key(&mut self.ui, mapped);
                    if let Some(text) = &event.text {
                        if let Some(s) = &mut self.screen {
                            for c in text.chars() {
                                s.typed(&mut self.ui, c);
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
                // Fixed keys first (GuiScreen/Minecraft.runTick hard-codes these), then bindings.
                match code {
                    KeyCode::Escape => return self.open(Box::new(screens::PauseMenu::default()), false),
                    KeyCode::F3 => return self.ui.settings.show_debug = !self.ui.settings.show_debug,
                    KeyCode::F1 => return self.ui.settings.hide_gui = !self.ui.settings.hide_gui,
                    _ => {}
                }
                let lw = keymap::lwjgl(code);
                let is = |name: &str| lw != 0 && self.ui.settings.key(name) == lw;
                if is("key.inventory") {
                    self.pending_open_inventory = self.net.is_some();
                    if self.net.is_some() && self.riding_horse() {
                        // The server opens the horse container after C0B OPEN_INVENTORY.
                    } else if self.entity.creative {
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
                } else if is("key.drop") {
                    let ctrl = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
                    self.drop_presses.push(ctrl);
                } else if is("key.togglePerspective") {
                    self.entity.perspective = (self.entity.perspective + 1) % 3;
                    self.sync_view();
                } else if is("key.chat") {
                    self.open(Box::new(screens::Chat::default()), false)
                } else if is("key.command") {
                    self.open(Box::new(screens::Chat::with_text("/")), false)
                } else if is("key.fullscreen") {
                    self.ui.settings.fullscreen = !self.ui.settings.fullscreen;
                    self.ui.settings.save();
                    self.apply_settings();
                } else if let Some(n) = (1..=9).find(|i| is(&format!("key.hotbar.{i}"))) {
                    self.pending_selected_slot = Some(n - 1);
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - self.last_frame).as_secs_f32().min(0.5);
                self.last_frame = now;
                crate::bench::profile_frame();
                if let Some(b) = &mut self.bench {
                    let p = b.pose(now);
                    (self.camera.pos, self.camera.yaw, self.camera.pitch) = (p.pos, p.yaw, p.pitch);
                }
                self.forge_frame(|app| { app.update(dt); app.draw_frame(); });
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
                let invert = if self.ui.settings.invert_mouse { -1. } else { 1. };
                self.entity.turn(delta.0 as f32 * f1, -delta.1 as f32 * f1 * invert);
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.forge_quit { return el.exit(); }
        if let Some(w) = &self.window {
            let deadline = frame_deadline(self.last_frame, self.ui.settings.limit_framerate, self.ui.settings.vsync, self.bench.is_some());
            if let Some(deadline) = deadline.filter(|deadline| *deadline > Instant::now()) {
                el.set_control_flow(ControlFlow::WaitUntil(deadline));
            } else {
                el.set_control_flow(ControlFlow::Poll);
                w.request_redraw();
            }
        }
    }
}

#[cfg(test)]
mod frame_pacing_tests {
    use super::*;

    #[test]
    fn capped_frames_have_deadlines_and_other_modes_do_not() {
        let now = Instant::now();
        assert_eq!(frame_deadline(now, 100, false, false), Some(now + Duration::from_millis(10)));
        assert_eq!(frame_deadline(now, 0, false, false), Some(now + Duration::from_secs(1)));
        assert_eq!(frame_deadline(now, 260, false, false), None);
        assert_eq!(frame_deadline(now, 60, true, false), None);
        assert_eq!(frame_deadline(now, 60, false, true), None);
    }
}
