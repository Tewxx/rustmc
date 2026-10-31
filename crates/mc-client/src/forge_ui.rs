//! Forge mods inside the client: Java screens become client screens, mods' buttons appear on native screens,
//! and ticks, input and window state reach the embedded JVM. Everything is a no-op unless `forge_on`.

use crate::*;
use mc_forge::{game::{self, Focus}, impls::{self, Draw}, runtime};
use mc_gui::{batch::GuiBatch, screens::{Action, Key, Screen}, Ui};

/// Runs `f` on the JVM thread; a Java failure is logged (with its stack trace) and becomes `None`.
fn java<R>(f: impl FnOnce(&mut mc_forge::jni::JNIEnv<'_>) -> mc_forge::jni::errors::Result<R>) -> Option<R> {
    match runtime::with_env(f)? {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("forge: {e}");
            runtime::with_env(|env| { let _ = env.exception_describe(); let _ = env.exception_clear(); });
            None
        }
    }
}

/// Framebuffer pixels per GUI unit, for converting `glLineWidth`.
static PIXELS_PER_UNIT: std::sync::Mutex<f32> = std::sync::Mutex::new(2.);

/// The item catalogue `Draw::Item` replays through; `replay` also runs for mod screens, which have no `App`.
static CATALOGUE: std::sync::Mutex<Option<std::sync::Arc<inventory::Catalogue>>> = std::sync::Mutex::new(None);

/// Replays what mod code drew (text, tessellator quads and lines) into the client's GUI batch.
fn replay(ui: &mut Ui, b: &mut GuiBatch) {
    let cmds = impls::take_draw();
    if cmds.is_empty() { return; }
    crate::forge_live::timed("replay", || replay_cmds(ui, b, cmds));
}

fn replay_cmds(ui: &mut Ui, b: &mut GuiBatch, cmds: Vec<impls::Draw>) {
    let ppu = *PIXELS_PER_UNIT.lock().unwrap();
    // Editable fields may contain credentials. Never copy rendered text into logs.
    for cmd in cmds {
        match cmd {
            Draw::Text { text, x, y, scale, color, shadow } => {
                // FontRenderer.renderString: a colour with no alpha bits is opaque.
                let c = if color as u32 & 0xfc00_0000 == 0 { color as u32 | 0xff00_0000 } else { color as u32 };
                if scale < 1e-3 { continue; }
                let scaled = (scale - 1.).abs() > 1e-3;
                let (x, y) = if scaled { b.transform = Some((scale, [x, y])); (0., 0.) } else { (x, y) };
                if shadow { ui.font.draw_shadow(b, &text, x, y, c); } else { ui.font.draw(b, &text, x, y, c); }
                if scaled { b.transform = None; }
            }
            Draw::Lines { width, verts } => {
                let half = (width / ppu).max(0.5) / 2.;
                let rgba = |v: &impls::Vertex| v.rgba.map(|c| (c.clamp(0., 1.) * 255.).round() as u8);
                for s in verts.chunks_exact(2) {
                    let (a, z) = ([s[0].pos[0] as f32, s[0].pos[1] as f32], [s[1].pos[0] as f32, s[1].pos[1] as f32]);
                    let (dx, dy) = (z[0] - a[0], z[1] - a[1]);
                    let len = dx.hypot(dy);
                    if len < 1e-4 { continue; }
                    let (nx, ny) = (-dy / len * half, dx / len * half);
                    let (ca, cz) = (rgba(&s[0]), rgba(&s[1]));
                    b.quad(None, false, [[a[0] + nx, a[1] + ny], [a[0] - nx, a[1] - ny], [z[0] - nx, z[1] - ny], [z[0] + nx, z[1] + ny]],
                        [[0.; 2]; 4], [ca, ca, cz, cz]);
                }
            }
            Draw::Vertices { mode: 7, texture, verts, blend } => {
                if std::env::var_os("RUSTMC_FORGE_TIMING").is_some() {
                    static SEEN: std::sync::Mutex<Vec<(Option<String>, usize)>> = std::sync::Mutex::new(Vec::new());
                    let mut seen = SEEN.lock().unwrap();
                    let key = (texture.clone(), verts.len());
                    if !seen.contains(&key) { eprintln!("[draw] quads texture={:?} verts={} first=({:.0},{:.0}) blend={:?}", texture, verts.len(), verts[0].pos[0], verts[0].pos[1], blend); seen.push(key); }
                }
                // The GUI batch blends with alpha by default; (ONE_MINUS_DST_COLOR, ONE_MINUS_SRC_COLOR) is its Invert.
                let saved = b.blend;
                if blend == Some((0x0307, 0x0301)) { b.blend = mc_gui::batch::Blend::Invert; }
                for q in verts.chunks_exact(4) {
                    let rgba = |v: &impls::Vertex| v.rgba.map(|c| (c.clamp(0., 1.) * 255.).round() as u8);
                    b.quad(texture.as_deref(), texture.is_some(),
                        std::array::from_fn(|i| [q[i].pos[0] as f32, q[i].pos[1] as f32]),
                        std::array::from_fn(|i| [q[i].uv[0] as f32, q[i].uv[1] as f32]),
                        std::array::from_fn(|i| rgba(&q[i])));
                }
                b.blend = saved;
            }
            Draw::Vertices { .. } => {} // only quads (GL_QUADS) are replayed so far
            // renderSkybox's copied, blurred panorama: the client's own (one per frame is enough).
            Draw::Backdrop => if !b.items.iter().any(|i| matches!(i, mc_gui::batch::Item::Special(mc_gui::batch::Special::Panorama { .. }))) {
                b.special(mc_gui::batch::Special::Panorama { ticks: ui.ticks });
            },
            Draw::Item { id, meta, x, y, scale, glint, .. } => {
                // RenderItem: id/meta become atlas quads through the client's catalogue; nothing else has the UVs.
                if scale < 1e-3 { continue; }
                let cat = CATALOGUE.lock().unwrap().clone();
                let Some(cat) = cat else { continue };
                let Some(index) = cat.index_of(id, meta) else { continue };
                let stack = mc_gui::ItemStack::new(index, 1, meta as i16);
                let item = cat.render_def(&stack);
                // Item.getColorFromItemStack: potion damage values tint the icon that carries tint index 0.
                let tint = (item.id == 373 && stack.damage as u16 != item.meta).then(|| mc_render::items::potion_color(meta));
                let tinted = |q: &inventory::IconQuad| match (tint, q.tint0) {
                    (Some(c), Some(shade)) => {
                        let ch = |v: u32| ((v & 255) as f32 * shade) as u8;
                        [[ch(c >> 16), ch(c >> 8), ch(c), 255]; 4]
                    }
                    _ => q.color,
                };
                for q in &item.icon {
                    b.quad(Some(&q.texture), true, q.pos.map(|p| [x + p[0] * scale, y + p[1] * scale]), q.uv, tinted(q));
                }
                // RenderItem.renderEffect: the glint pass runs over the icon's texels in color -8372020.
                if glint {
                    let saved = b.blend;
                    b.blend = mc_gui::batch::Blend::Glint;
                    for q in &item.icon {
                        b.quad(Some(&q.texture), true, q.pos.map(|p| [x + p[0] * scale, y + p[1] * scale]), q.uv, [[0x80, 0x40, 0xcc, 255]; 4]);
                    }
                    b.blend = saved;
                }
            }
        }
    }
}

fn draw_java(ui: &mut Ui, b: &mut GuiBatch, mx: i32, my: i32) {
    crate::forge_live::timed("screen_java", || java(|env| game::draw_screen(env, mx, my, 0.0)));
    replay(ui, b);
}

/// A screen a mod opened with `displayGuiScreen`: the Java object draws and handles input; this just forwards.
/// Remembers the last pointer position, since `scroll` carries none. Pointer moves are queued and delivered once per
/// frame, just before drawing: the window reports them at the mouse's polling rate, and each one costs a full Java
/// `handleMouseInput` (display sync, then the mod's drag handler), which made dragging mod windows stutter.
pub(crate) struct ModScreen(pub i32, pub i32, pub bool, pub i32);

impl ModScreen {
    /// Sends what is queued to the Java screen: the latest pointer position, and a key that carried no character
    /// (F-keys, modifiers), which `keyTyped` still needs for key-bind capture.
    fn flush_move(&mut self) {
        let key = std::mem::take(&mut self.3);
        if key != 0 { java(|env| game::key_typed(env, '\0', key)); }
        if !std::mem::take(&mut self.2) { return; }
        let (mx, my) = (self.0, self.1);
        java(|env| game::mouse_event(env, mx, my, -1, false, 0));
    }
}

impl Screen for ModScreen {
    fn init(&mut self, _ui: &Ui, w: i32, h: i32) { java(|env| game::resize(env, w, h)); }
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, _w: i32, _h: i32, mx: i32, my: i32) {
        self.flush_move();
        draw_java(ui, b, mx, my);
    }
    fn click(&mut self, _ui: &mut Ui, mx: i32, my: i32) -> Action { self.flush_move(); java(|env| game::mouse_event(env, mx, my, 0, true, 0)); Action::None }
    fn right_click(&mut self, _ui: &mut Ui, mx: i32, my: i32) -> Action { self.flush_move(); java(|env| game::mouse_event(env, mx, my, 1, true, 0)); Action::None }
    fn release_at(&mut self, _ui: &mut Ui, mx: i32, my: i32, button: u8) { self.flush_move(); java(|env| game::mouse_event(env, mx, my, button as i32, false, 0)); }
    fn mouse_move(&mut self, _ui: &mut Ui, mx: i32, my: i32) { self.0 = mx; self.1 = my; self.2 = true; }
    fn scroll(&mut self, delta: i32) {
        // `delta` is one notch (+1 up); LWJGL reports 120 per notch.
        self.flush_move();
        let (mx, my) = (self.0, self.1);
        java(|env| game::mouse_event(env, mx, my, -1, false, delta * 120));
    }
    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        impls::set_modifiers(ui.ctrl_down, ui.shift_down, false);
        // Printable text arrives through `typed`; this carries the keys that have no character.
        let (ch, code) = match key {
            Key::Escape => ('\0', 1), Key::Enter => ('\r', 28), Key::Backspace => ('\u{8}', 14), Key::Tab => ('\t', 15),
            Key::Left => ('\0', 203), Key::Right => ('\0', 205), Key::Up => ('\0', 200), Key::Down => ('\0', 208),
            Key::Home => ('\0', 199), Key::End => ('\0', 207), Key::Delete => ('\0', 211),
            Key::PageUp => ('\0', 201), Key::PageDown => ('\0', 209),
            Key::Combo(c) => {
                // ctrl+letter: the control character and the letter's LWJGL code.
                const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
                let code = ROWS.iter().zip([16, 30, 44]).find_map(|(row, first)| row.find(c).map(|i| first + i as i32)).unwrap_or(0);
                (((c as u8).wrapping_sub(b'a') + 1) as char, code)
            }
            // No character of its own (or one `typed` is about to deliver): `typed` cancels this, else the next frame sends it.
            _ => { self.3 = ui.key_code; return Action::None }
        };
        java(|env| game::key_typed(env, ch, code));
        Action::None
    }
    fn typed(&mut self, ui: &mut Ui, c: char) {
        impls::set_modifiers(ui.ctrl_down, ui.shift_down, false);
        // A printable key arrives right after `key`: it is this character, with its LWJGL code (bind capture reads it).
        self.3 = 0;
        if !c.is_control() { let code = ui.key_code; java(|env| game::key_typed(env, c, code)); }
    }
}

/// The client's own screen for a [`game::vanilla_screen`] kind.
fn native_screen(kind: &str) -> Option<Box<dyn Screen>> {
    use mc_gui::screens;
    Some(match kind {
        "multiplayer" => Box::new(screens::MultiplayerScreen::new()),
        "main_menu" => Box::new(screens::MainMenu::default()),
        "ingame_menu" => Box::new(screens::PauseMenu::default()),
        "options" => Box::new(screens::Options::default()),
        "video" => Box::new(screens::VideoSettings::default()),
        "controls" => Box::new(screens::Controls::default()),
        "resource_packs" => Box::new(mc_gui::resource_packs::ResourcePacks::default()),
        _ => return None,
    })
}

impl App {
    /// Sizes, pointer and modifier keys as LWJGL reports them; call before any Java call that may read them.
    fn forge_prepare(&self) {
        let sr = self.scaled();        let fb = self.renderer.as_ref().map_or((1280, 720), |r| (r.config.width as i32, r.config.height as i32));
        impls::set_display((sr.width, sr.height), fb);
        *CATALOGUE.lock().unwrap() = Some(self.catalogue.clone());
        impls::set_mouse(self.mouse_px.0 as i32, self.mouse_px.1 as i32);
        impls::set_modifiers(self.ui.ctrl_down, self.ui.shift_down, false);
    }

    /// A native screen just opened: give mods its Java twin so they can add buttons to it.
    pub(crate) fn forge_open_native(&mut self) {
        if !self.forge_on || self.forge_mod_screen { return; }
        self.forge_prepare();
        let kind = self.screen.as_ref().and_then(|s| s.java_screen()).unwrap_or("");
        java(|env| game::open_shadow(env, kind));
        self.forge_sync();
    }

    /// Keeps the client's screen stack in step with the Java current screen: a mod's screen becomes a client
    /// screen over the native one; when Java returns to the twin or closes, it is popped again.
    pub(crate) fn forge_sync(&mut self) {
        if !self.forge_on { return; }
        let Some(focus) = java(game::focus) else { return };
        match (focus == Focus::Mod, self.forge_mod_screen) {
            (true, false) => {
                // A vanilla screen class the client draws itself: open the native one (its stub draws nothing).
                if self.forge_open_vanilla(false) { return; }
                self.forge_mod_screen = true;
                self.open(Box::new(ModScreen(0, 0, false, 0)), true);
            }
            // A mod screen opened a vanilla one (Lunar's home -> GuiMultiplayer).
            (true, true) => { self.forge_open_vanilla(true); }
            (false, true) => {
                self.forge_mod_screen = false;
                self.close_screen(); // rebuilds the native screen's twin
            }
            _ => {}
        }
    }

    /// Shows the native screen for a vanilla Java screen. Over a mod screen, that screen is replaced and closing the
    /// native one returns to the parent the mod gave it (`forge_return`), as the vanilla screen's Done would.
    fn forge_open_vanilla(&mut self, over_mod: bool) -> bool {
        let Some(native) = java(game::vanilla_screen).flatten().and_then(native_screen) else { return false };
        let parent = java(game::vanilla_parent).flatten();
        if over_mod {
            self.forge_mod_screen = false;
            self.open(native, false);
            self.forge_return = parent.map(|p| (p, self.parents.len()));
        } else {
            self.open(native, true);
        }
        true
    }

    /// Closing the native twin of a mod-opened vanilla screen: Java shows the mod's parent screen again.
    pub(crate) fn forge_close_vanilla(&mut self) -> bool {
        match self.forge_return.take() {
            Some((parent, depth)) if depth == self.parents.len() => {
                if let Some(s) = &mut self.screen { let _ = s.close_action(); }
                self.screen = None;
                java(|env| game::show_screen(env, parent.as_obj()));
                self.forge_sync();
                true
            }
            other => { self.forge_return = other; false }
        }
    }

    /// A left click on a native screen goes to its Java twin first; true if that opened a mod screen.
    pub(crate) fn forge_click(&mut self, mx: i32, my: i32) -> bool {
        if !self.forge_on || self.forge_mod_screen { return false; }
        self.forge_prepare();
        if java(game::focus) != Some(Focus::Shadow) { return false; }
        java(|env| game::mouse_click(env, mx, my, 0));
        self.forge_sync();
        self.forge_mod_screen
    }

    /// A mod (account switcher) replaced `Minecraft.session`: make the client that player. A mod-supplied real token
    /// signs in online; the placeholder token ("0", cracked/offline accounts) just changes the name.
    fn forge_sync_session(&mut self) {
        let Some(now) = java(game::session) else { return };
        let Some(last) = self.forge_session.replace(now.clone()) else { return }; // first read is the baseline
        if last == now || now.0.is_empty() { return; }
        let (name, uuid, token) = now;
        eprintln!("forge: session changed to {name}");
        self.name = name.clone();
        self.ui.player_name = name.clone();
        self.session = (token != "0" && !token.is_empty()).then(|| crate::auth::Session { name, uuid: uuid.replace('-', ""), access_token: token });
    }

    /// START precedes Rust simulation; END follows it. Java hooks enclose both.
    pub(crate) fn forge_tick_begin(&mut self) {
        if !self.forge_on { return; }
        self.forge_prepare();
        crate::forge_live::timed("tick_start",||java(|env| game::client_tick(env, true)));
        self.forge_apply();
    }

    pub(crate) fn forge_tick_end(&mut self) {
        if !self.forge_on { return; }
        self.forge_publish();
        crate::forge_live::timed("tick_end",||java(|env| game::client_tick(env, false)));
        self.forge_apply();
        self.forge_sync();
        self.forge_sync_session();
    }

    /// `RenderWorldLastEvent` at the end of the world pass. The camera is vanilla's: a model-view of
    /// `orientCamera` with the world placed relative to the interpolated feet (`RenderManager.renderPos`), so
    /// mods' `pos - renderPos` geometry lands where it would in Java. Draws are projected and replayed now,
    /// before the HUD, without depth testing.
    pub(crate) fn forge_world_last(&mut self, sr: Scaled) {
        if !self.forge_on || !self.in_game { return; }
        self.forge_prepare();
        let fb = self.renderer.as_ref().map_or((1280., 720.), |r| (r.config.width as f32, r.config.height as f32));
        *PIXELS_PER_UNIT.lock().unwrap() = fb.0 / sr.width.max(1) as f32;
        let pt = self.tick_accum;
        let e = &self.entity;
        let lerp = |a: f64, b: f64| a + (b - a) * pt as f64;
        let feet = glam::Vec3::new(lerp(e.last_tick_pos.x, e.pos.x) as f32, lerp(e.last_tick_pos.y, e.pos.y) as f32, lerp(e.last_tick_pos.z, e.pos.z) as f32);
        let far = self.ui.settings.render_distance as f32 * 16. * std::f32::consts::SQRT_2;
        let projection = self.camera.proj(fb.0 / fb.1.max(1.), far.max(32.));
        let modelview = self.camera.view() * glam::Mat4::from_translation(feet - self.camera.pos);
        let f64s = |m: glam::Mat4| m.to_cols_array().map(|v| v as f64);
        let (p, m, gui) = (f64s(projection), f64s(modelview), (sr.width_f, sr.height_f));
        crate::forge_live::timed("world_last",||java(|env| game::world_last(env, pt, p, m, gui)));
        replay(&mut self.ui, &mut self.batch);
        self.forge_apply();
    }

    /// `GuiIngameForge.renderGameOverlay` begins: `RenderGameOverlayEvent.Pre(ALL)`; true if a mod hid the HUD.
    pub(crate) fn forge_hud_pre(&mut self) -> bool {
        if !self.forge_on { return false; }
        self.forge_prepare();
        let pt = self.tick_accum;
        let cancelled = crate::forge_live::timed("hud_pre",||java(|env| game::overlay(env, pt, true, "ALL"))).unwrap_or(false);
        replay(&mut self.ui, &mut self.batch);
        cancelled
    }

    /// The end of the HUD: `RenderGameOverlayEvent.Text` (lines mods add are drawn like `renderHUDText`), then
    /// `Post(ALL)`; mod draws replay here, under any open screen.
    pub(crate) fn forge_hud_post(&mut self, sr: Scaled) {
        if !self.forge_on { return; }
        let pt = self.tick_accum;
        if let Some((left, right)) = crate::forge_live::timed("hud_text",||java(|env| game::overlay_text(env, pt, &[], &[]))).flatten() {
            for (i, line) in left.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
                let (w, top) = (self.ui.font.width(line), 2 + i as i32 * 9);
                self.batch.rect(1, top - 1, 2 + w + 1, top + 8, 0x9050_5050);
                self.ui.font.draw(&mut self.batch, line, 2., top as f32, 0xffe0_e0e0);
            }
            for (i, line) in right.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
                let (w, top) = (self.ui.font.width(line), 2 + i as i32 * 9);
                let left = sr.width - 2 - w;
                self.batch.rect(left - 1, top - 1, left + w + 1, top + 8, 0x9050_5050);
                self.ui.font.draw(&mut self.batch, line, left as f32, top as f32, 0xffe0_e0e0);
            }
        }
        crate::forge_live::timed("hud_post",||java(|env| game::overlay(env, pt, false, "ALL")));
        replay(&mut self.ui, &mut self.batch);
        self.forge_apply();
    }

    /// `ClientChatReceivedEvent`: `None` if a mod cancelled the message, else its (possibly replaced) JSON.
    pub(crate) fn forge_chat(&mut self, position: i8, json: &str) -> Option<String> {
        if !self.forge_on { return Some(json.to_string()); }
        match java(|env| game::chat_received(env, position, json)) { Some(v) => v, None => Some(json.to_string()) }
    }

    /// A keyboard event: `Keyboard` state always; while playing with no screen open, key bindings and
    /// `KeyInputEvent` as `Minecraft.runTick` does.
    pub(crate) fn forge_key(&mut self, code: i32, down: bool, repeat: bool, ch: Option<char>) {
        if !self.forge_on { return; }
        impls::set_key(code, down, ch, repeat);
        if !self.in_game || self.screen.is_some() { return; }
        self.forge_prepare();
        java(|env| { if !repeat { impls::key_bindings(env, code, down)?; } game::key_input(env) });
        self.forge_apply();
    }

    /// An in-game mouse button or wheel event: Forge's `MouseEvent`, then key bindings on `button - 100`. True if a mod
    /// cancelled it, so the client must not act on it.
    pub(crate) fn forge_mouse(&mut self, button: i32, down: bool, wheel: i32) -> bool {
        if !self.forge_on || !self.in_game || self.screen.is_some() { return false; }
        self.forge_prepare();
        impls::set_mouse_button(button, down, wheel);
        let cancelled = java(game::mouse_input).unwrap_or(false);
        if !cancelled && button >= 0 { java(|env| impls::key_bindings(env, button - 100, down)); }
        self.forge_apply();
        cancelled
    }

    /// `KeyBinding.unPressAllKeys` when a screen opens.
    pub(crate) fn forge_unpress(&mut self) {
        if self.forge_on { java(impls::unpress_key_bindings); }
    }

    /// Just before the screen draws: sync and `RenderTickEvent.START`.
    pub(crate) fn forge_frame_begin(&mut self, partial_ticks: f32) {
        if !self.forge_on { return; }
        self.forge_prepare();
        self.forge_sync();
        crate::forge_live::timed("render_start",||java(|env| game::render_tick(env, true, partial_ticks)));
        self.forge_apply();
    }

    /// Just after the screen draws: the twin's mod buttons over a native screen, then `RenderTickEvent.END`.
    pub(crate) fn forge_frame_end(&mut self, mx: i32, my: i32, partial_ticks: f32) {
        if !self.forge_on { return; }
        if !self.forge_mod_screen && java(game::focus) == Some(Focus::Shadow) { draw_java(&mut self.ui, &mut self.batch, mx, my); }
        crate::forge_live::timed("render_end",||java(|env| game::render_tick(env, false, partial_ticks)));
        self.forge_apply();
        replay(&mut self.ui, &mut self.batch); // anything mods drew in RenderTickEvent
    }
}
