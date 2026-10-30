//! Forge mods inside the client: Java screens become client screens, mods' buttons appear on native screens,
//! and ticks, input and window state reach the embedded JVM. Everything is a no-op unless `forge_on`.

use crate::*;
use mc_forge::{game::{self, Focus}, impls::{self, Draw}, runtime};
use mc_gui::{batch::GuiBatch, screens::{Action, Key, Screen}, Ui};

/// Runs `f` on the JVM thread; a Java failure is logged (with its stack trace) and becomes `None`.
fn java<R>(f: impl FnOnce(&mut mc_forge::jni::JNIEnv<'_>) -> mc_forge::jni::errors::Result<R>) -> Option<R> {
    match runtime::with_env(f)? {
        Ok(v) => Some(v),
        Err(e) => { eprintln!("forge: {e}"); None }
    }
}

/// Replays what mod code drew (text and tessellator quads) into the client's GUI batch.
fn replay(ui: &mut Ui, b: &mut GuiBatch) {
    let cmds = impls::take_draw();
    // A mod's status text ("Login failed! Reason: ...") is the only trace of what its code did: log it when it changes.
    static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    let texts: Vec<&str> = cmds.iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.as_str()) } else { None }).collect();
    if !texts.is_empty() {
        // Text fields can hold secrets (pasted tokens, passwords): mask anything that looks like one before logging.
        let line = texts.join(" | ").split(' ').map(|w| if w.len() >= 20 && w.chars().all(|c| c.is_ascii_alphanumeric() || "._-!:$".contains(c)) { "<masked>" } else { w })
            .collect::<Vec<_>>().join(" ");
        let mut last = LAST.lock().unwrap();
        if *last != line { eprintln!("[forge] screen: {line}"); *last = line; }
    }
    for cmd in cmds {
        match cmd {
            Draw::Text { text, x, y, color, shadow } => {
                // FontRenderer.renderString: a colour with no alpha bits is opaque.
                let c = if color as u32 & 0xfc00_0000 == 0 { color as u32 | 0xff00_0000 } else { color as u32 };
                if shadow { ui.font.draw_shadow(b, &text, x, y, c); } else { ui.font.draw(b, &text, x, y, c); }
            }
            Draw::Vertices { mode: 7, texture, verts } => {
                for q in verts.chunks_exact(4) {
                    let rgba = |v: &impls::Vertex| v.rgba.map(|c| (c.clamp(0., 1.) * 255.).round() as u8);
                    b.quad(texture.as_deref(), texture.is_some(),
                        std::array::from_fn(|i| [q[i].pos[0] as f32, q[i].pos[1] as f32]),
                        std::array::from_fn(|i| [q[i].uv[0] as f32, q[i].uv[1] as f32]),
                        std::array::from_fn(|i| rgba(&q[i])));
                }
            }
            Draw::Vertices { .. } => {} // only quads (GL_QUADS) are replayed so far
        }
    }
}

fn draw_java(ui: &mut Ui, b: &mut GuiBatch, mx: i32, my: i32) {
    java(|env| game::draw_screen(env, mx, my, 0.0));
    replay(ui, b);
}

/// A screen a mod opened with `displayGuiScreen`: the Java object draws and handles input; this just forwards.
/// Remembers the last pointer position, since `scroll` carries none.
pub(crate) struct ModScreen(pub i32, pub i32);

impl Screen for ModScreen {
    fn init(&mut self, _ui: &Ui, w: i32, h: i32) { java(|env| game::resize(env, w, h)); }
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, _w: i32, _h: i32, mx: i32, my: i32) { draw_java(ui, b, mx, my); }
    fn click(&mut self, _ui: &mut Ui, mx: i32, my: i32) -> Action { java(|env| game::mouse_event(env, mx, my, 0, true, 0)); Action::None }
    fn right_click(&mut self, _ui: &mut Ui, mx: i32, my: i32) -> Action { java(|env| game::mouse_event(env, mx, my, 1, true, 0)); Action::None }
    fn release_at(&mut self, _ui: &mut Ui, mx: i32, my: i32, button: u8) { java(|env| game::mouse_event(env, mx, my, button as i32, false, 0)); }
    fn mouse_move(&mut self, _ui: &mut Ui, mx: i32, my: i32) { self.0 = mx; self.1 = my; java(|env| game::mouse_event(env, mx, my, -1, false, 0)); }
    fn scroll(&mut self, delta: i32) {
        // `delta` is one notch (+1 up); LWJGL reports 120 per notch.
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
            _ => return Action::None,
        };
        java(|env| game::key_typed(env, ch, code));
        Action::None
    }
    fn typed(&mut self, ui: &mut Ui, c: char) {
        impls::set_modifiers(ui.ctrl_down, ui.shift_down, false);
        if !c.is_control() { java(|env| game::key_typed(env, c, 0)); }
    }
}

impl App {
    /// Sizes, pointer and modifier keys as LWJGL reports them; call before any Java call that may read them.
    fn forge_prepare(&self) {
        let sr = self.scaled();
        let fb = self.renderer.as_ref().map_or((1280, 720), |r| (r.config.width as i32, r.config.height as i32));
        impls::set_display((sr.width, sr.height), fb);
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
            (true, false) => { self.forge_mod_screen = true; self.open(Box::new(ModScreen(0, 0)), true); }
            (false, true) => {
                self.forge_mod_screen = false;
                self.close_screen(); // rebuilds the native screen's twin
            }
            _ => {}
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

    /// Once per client tick: `ClientTickEvent` START and END around the screen's `updateScreen`.
    pub(crate) fn forge_tick(&mut self) {
        if !self.forge_on { return; }
        self.forge_prepare();
        java(|env| game::client_tick(env, true));
        java(|env| game::client_tick(env, false));
        self.forge_sync();
        self.forge_sync_session();
    }

    /// Just before the screen draws: sync and `RenderTickEvent.START`.
    pub(crate) fn forge_frame_begin(&mut self, partial_ticks: f32) {
        if !self.forge_on { return; }
        self.forge_prepare();
        self.forge_sync();
        java(|env| game::render_tick(env, true, partial_ticks));
    }

    /// Just after the screen draws: the twin's mod buttons over a native screen, then `RenderTickEvent.END`.
    pub(crate) fn forge_frame_end(&mut self, mx: i32, my: i32, partial_ticks: f32) {
        if !self.forge_on { return; }
        if !self.forge_mod_screen && java(game::focus) == Some(Focus::Shadow) { draw_java(&mut self.ui, &mut self.batch, mx, my); }
        java(|env| game::render_tick(env, false, partial_ticks));
        replay(&mut self.ui, &mut self.batch); // anything mods drew in RenderTickEvent
    }
}
