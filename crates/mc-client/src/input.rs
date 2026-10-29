//! Keyboard and mouse mapping and the winit event handler.

use crate::*;

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
                        if button == MouseButton::Left && self.screen.as_ref().is_some_and(|s| s.is_chat()) {
                            if let Some(piece) = self.chat_piece_under_mouse() {
                                if self.handle_component_click(&piece) {
                                    return;
                                }
                            }
                        }
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
                if self.screen.as_ref().is_some_and(|s| s.is_chat()) {
                    // GuiChat.handleMouseInput: one line with shift, seven without.
                    if d != 0 {
                        let n = if self.ui.shift_down { d } else { d * 7 };
                        self.hud.chat.scroll(&self.ui, n, true);
                    }
                } else if let Some(s) = &mut self.screen {
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
                    self.ui.ctrl_down = self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight);
                    return;
                }
                if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                    self.keys.insert(code);
                    self.ui.shift_down = true;
                }
                if matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight) {
                    self.keys.insert(code);
                    self.ui.ctrl_down = true;
                }
                self.ui.key_code = keymap::lwjgl(code);
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
                    self.player.selected_slot = n - 1;
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
                let invert = if self.ui.settings.invert_mouse { -1. } else { 1. };
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
