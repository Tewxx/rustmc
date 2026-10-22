//! `GuiScreen` and the screens the client has so far.

use crate::batch::{GuiBatch, Special};
use crate::settings::Opt;
use crate::widgets::{Button, Kind};
use crate::Ui;

pub const OPTIONS_BACKGROUND: &str = "textures/gui/options_background.png";
const TITLE: &str = "textures/gui/title/minecraft.png";

/// What a screen asks the client to do.
pub enum Action {
    None,
    /// Close this screen (back to the game, or to `parent` if there is one).
    Close,
    Open(Box<dyn Screen>),
    StartGame,
    QuitToTitle,
    Quit,
    SendChat(String),
    /// `GuiChat.sendAutocompleteRequest`: the text before the cursor.
    TabComplete(String),
    /// A line for the chat log (several completion matches).
    PrintChat(String),
    Respawn,
}

#[derive(Default)]
pub struct DeathScreen {buttons:Vec<Button>}
impl Screen for DeathScreen {
    fn init(&mut self,_:&Ui,w:i32,h:i32) {
        self.buttons=vec![Button::new(0,w/2-100,h/4+72,"Respawn"),Button::new(1,w/2-100,h/4+96,"Title screen")];
    }
    fn draw(&mut self,ui:&mut Ui,b:&mut GuiBatch,w:i32,h:i32,mx:i32,my:i32) {
        b.gradient(0,0,w,h,0x60500000,0xa0803030);
        ui.font.draw_centered(b,"You died!",w/2,30,0xffffffff);
        ui.font.draw_centered(b,"Score: \u{a7}e0",w/2,100,0xffffffff);
        draw_buttons(&mut self.buttons,ui,b,mx,my);
    }
    fn click(&mut self,ui:&mut Ui,mx:i32,my:i32)->Action {match click_buttons(&mut self.buttons,ui,mx,my) {Some(0)=>Action::Respawn,Some(1)=>Action::QuitToTitle,_=>Action::None}}
    fn key(&mut self,_:&mut Ui,_:Key)->Action {Action::None}
    fn pauses(&self)->bool {false}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Inventory,
    Number(usize),
    Escape,
    Enter,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Delete,
    Tab,
    Other,
}

pub trait Screen {
    /// `initGui`: lay out widgets for a `w`×`h` scaled screen.
    fn init(&mut self, ui: &Ui, w: i32, h: i32);
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32);
    /// Returns the clicked button's id, if any (`actionPerformed`).
    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action;
    fn right_click(&mut self, _ui:&mut Ui, _mx:i32, _my:i32)->Action {Action::None}
    fn release(&mut self, _ui: &mut Ui) {}
    fn key(&mut self, _ui: &mut Ui, key: Key) -> Action {
        if key == Key::Escape { Action::Close } else { Action::None }
    }
    fn typed(&mut self, _c: char) {}
    /// `GuiChat.onAutocompleteResponse`; returns a line to print in chat (several matches).
    fn autocomplete(&mut self, _ui: &mut Ui, _list: Vec<String>) -> Option<String> {
        None
    }
    fn scroll(&mut self, _delta: i32) {}
    /// `doesGuiPauseGame`
    fn pauses(&self) -> bool {
        true
    }
    /// Screens that draw over the HUD (chat) rather than hiding it.
    fn shows_hud(&self) -> bool {
        false
    }
    /// Settings changed; the client should re-read them.
    fn settings_dirty(&self) -> bool {
        false
    }
}

/// `drawDefaultBackground` / `drawWorldBackground(0)`.
pub fn default_background(ui: &Ui, b: &mut GuiBatch, w: i32, h: i32) {
    if ui.in_world {
        b.gradient(0, 0, w, h, 0xC010_1010, 0xD010_1010);
    } else {
        dirt(b, 0, h, w, 64, 0.0);
    }
}

/// `drawBackground` / `GuiSlot.overlayBackground`: tiled options texture, gray-tinted.
fn dirt(b: &mut GuiBatch, top: i32, bottom: i32, w: i32, gray: u8, scroll: f32) {
    let c = [gray, gray, gray, 255];
    let f = 32.0;
    b.quad(
        Some(OPTIONS_BACKGROUND),
        false,
        [[0.0, bottom as f32], [w as f32, bottom as f32], [w as f32, top as f32], [0.0, top as f32]],
        [
            [0.0, (bottom as f32 + scroll) / f],
            [w as f32 / f, (bottom as f32 + scroll) / f],
            [w as f32 / f, (top as f32 + scroll) / f],
            [0.0, (top as f32 + scroll) / f],
        ],
        [c; 4],
    );
}

fn draw_buttons(buttons: &mut [Button], ui: &mut Ui, b: &mut GuiBatch, mx: i32, my: i32) {
    for btn in buttons {
        btn.draw(ui, b, mx, my);
    }
}

/// `GuiScreen.mouseClicked`: first button hit wins.
fn click_buttons(buttons: &mut [Button], ui: &mut Ui, mx: i32, my: i32) -> Option<i32> {
    buttons.iter_mut().find_map(|btn| btn.press(ui, mx, my).then_some(btn.id))
}

// ---------------------------------------------------------------- title screen

#[derive(Default)]
pub struct MainMenu {
    buttons: Vec<Button>,
}

impl Screen for MainMenu {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        let j = h / 4 + 48;
        self.buttons = vec![
            Button::new(1, w / 2 - 100, j, t("menu.singleplayer")),
            Button::new(2, w / 2 - 100, j + 24, t("menu.multiplayer")).disabled(),
            Button::new(14, w / 2 - 100, j + 48, t("menu.online")).disabled(),
            Button::sized(0, w / 2 - 100, j + 72 + 12, 98, 20, t("menu.options")),
            Button::sized(4, w / 2 + 2, j + 72 + 12, 98, 20, t("menu.quit")),
            Button { kind: Kind::Language, ..Button::sized(5, w / 2 - 124, j + 72 + 12, 20, 20, "") },
        ];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        b.special(Special::Panorama { ticks: ui.ticks });
        b.gradient(0, 0, w, h, 0x80FF_FFFF, 0x00FF_FFFF);
        b.gradient(0, 0, w, h, 0, 0x8000_0000);
        b.color = [1.0; 4];
        let (j, k) = (w / 2 - 274 / 2, 30);
        b.textured_rect(TITLE, j, k, 0, 0, 155, 44);
        b.textured_rect(TITLE, j + 155, k, 0, 45, 155, 44);

        // Splash: rotated -20°, pulsing; drawn into a transformed sub-batch.
        let t = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 1000) as f32;
        let mut f = 1.8 - mc_core::math::abs(mc_core::math::sin(t / 1000.0 * std::f32::consts::PI * 2.0) * 0.1);
        f = f * 100.0 / (ui.font.width(&ui.splash) + 32) as f32;
        let mut sub = GuiBatch::new();
        let splash = ui.splash.clone();
        ui.font.draw_centered(&mut sub, &splash, 0, -8, 0xFFFF_FF00);
        let (s, c) = (-20f32).to_radians().sin_cos();
        let (tx, ty) = ((w / 2 + 90) as f32, 70.0);
        for item in sub.items {
            if let crate::batch::Item::Draw(mut d) = item {
                for v in &mut d.vertices {
                    let (x, y) = (v.pos[0] * f, v.pos[1] * f);
                    v.pos = [tx + x * c - y * s, ty + x * s + y * c];
                }
                b.items.push(crate::batch::Item::Draw(d));
            }
        }

        ui.font.draw_shadow(b, "Minecraft 1.8.9", 2.0, (h - 10) as f32, 0xFFFF_FFFF);
        let s1 = "Copyright Mojang AB. Do not distribute!";
        let sw = ui.font.width(s1);
        ui.font.draw_shadow(b, s1, (w - sw - 2) as f32, (h - 10) as f32, 0xFFFF_FFFF);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(1) => Action::StartGame,
            Some(0) => Action::Open(Box::new(Options::default())),
            Some(4) => Action::Quit,
            _ => Action::None,
        }
    }

    fn key(&mut self, _ui: &mut Ui, _key: Key) -> Action {
        Action::None
    }
}

// ---------------------------------------------------------------- pause menu

#[derive(Default)]
pub struct PauseMenu {
    buttons: Vec<Button>,
}

impl Screen for PauseMenu {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        let i = -16;
        self.buttons = vec![
            Button::new(1, w / 2 - 100, h / 4 + 120 + i, t("menu.returnToMenu")),
            Button::new(4, w / 2 - 100, h / 4 + 24 + i, t("menu.returnToGame")),
            Button::sized(0, w / 2 - 100, h / 4 + 96 + i, 98, 20, t("menu.options")),
            Button::sized(7, w / 2 + 2, h / 4 + 96 + i, 98, 20, t("menu.shareToLan")).disabled(),
            Button::sized(5, w / 2 - 100, h / 4 + 48 + i, 98, 20, t("gui.achievements")).disabled(),
            Button::sized(6, w / 2 + 2, h / 4 + 48 + i, 98, 20, t("gui.stats")).disabled(),
        ];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let title = ui.lang.get("menu.game");
        ui.font.draw_centered(b, &title, w / 2, 40, 16777215);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(1) => Action::QuitToTitle,
            Some(4) => Action::Close,
            Some(0) => Action::Open(Box::new(Options::default())),
            _ => Action::None,
        }
    }
}

// ---------------------------------------------------------------- options

#[derive(Default)]
pub struct Options {
    buttons: Vec<Button>,
    dirty: bool,
}

impl Screen for Options {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        let row = |i: i32| (w / 2 - 155 + i % 2 * 160, h / 6 - 12 + 24 * (i >> 1));
        let (x0, y0) = row(0);
        let (x1, y1) = row(1);
        let second = if ui.in_world {
            // Difficulty lives on the server; shown but locked, like multiplayer.
            Button::sized(108, x1, y1, 150, 20, format!("{}: {}", t("options.difficulty"), t("options.difficulty.normal"))).disabled()
        } else {
            Button::sized(1001, x1, y1, 150, 20, format!("{}: {}", t("options.realmsNotifications"), t("options.on"))).disabled()
        };
        self.buttons = vec![
            Button::option(0, x0, y0, Opt::Fov, ui),
            second,
            Button::sized(110, w / 2 - 155, h / 6 + 48 - 6, 150, 20, t("options.skinCustomisation")).disabled(),
            Button::sized(8675309, w / 2 + 5, h / 6 + 48 - 6, 150, 20, "Super Secret Settings...").disabled(),
            Button::sized(106, w / 2 - 155, h / 6 + 72 - 6, 150, 20, t("options.sounds")).disabled(),
            Button::sized(107, w / 2 + 5, h / 6 + 72 - 6, 150, 20, t("options.stream")).disabled(),
            Button::sized(101, w / 2 - 155, h / 6 + 96 - 6, 150, 20, t("options.video")),
            Button::sized(100, w / 2 + 5, h / 6 + 96 - 6, 150, 20, t("options.controls")),
            Button::sized(102, w / 2 - 155, h / 6 + 120 - 6, 150, 20, t("options.language")).disabled(),
            Button::sized(103, w / 2 + 5, h / 6 + 120 - 6, 150, 20, t("options.chat.title")).disabled(),
            Button::sized(105, w / 2 - 155, h / 6 + 144 - 6, 150, 20, t("options.resourcepack")).disabled(),
            Button::sized(104, w / 2 + 5, h / 6 + 144 - 6, 150, 20, t("options.snooper.view")).disabled(),
            Button::new(200, w / 2 - 100, h / 6 + 168, t("gui.done")),
        ];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let title = ui.lang.get("options.title");
        ui.font.draw_centered(b, &title, w / 2, 15, 16777215);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(200) => {
                ui.settings.save();
                Action::Close
            }
            Some(101) => Action::Open(Box::new(VideoSettings::default())),
            Some(100) => Action::Open(Box::new(Controls::default())),
            Some(_) => {
                self.dirty = true;
                Action::None
            }
            None => Action::None,
        }
    }

    fn release(&mut self, ui: &mut Ui) {
        for b in &mut self.buttons {
            if matches!(b.kind, Kind::Slider { dragging: true, .. }) {
                self.dirty = true;
            }
            b.release();
        }
        ui.settings.save();
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        if key == Key::Escape {
            ui.settings.save();
            return Action::Close;
        }
        Action::None
    }

    fn settings_dirty(&self) -> bool {
        self.dirty
    }
}

// ---------------------------------------------------------------- video settings (GuiOptionsRowList)

#[derive(Default)]
pub struct VideoSettings {
    buttons: Vec<Button>,
    done: Option<Button>,
    scroll: i32,
    height: i32,
}

const ROW_H: i32 = 25;

impl VideoSettings {
    fn top(&self) -> i32 {
        32
    }

    fn bottom(&self) -> i32 {
        self.height - 32
    }

    fn content_height(&self) -> i32 {
        (self.buttons.len() as i32 + 1) / 2 * ROW_H
    }

    fn max_scroll(&self) -> i32 {
        (self.content_height() - (self.bottom() - self.top() - 4)).max(0)
    }

    /// Positions rows for the current scroll (GuiSlot.drawSelectionBox).
    fn layout(&mut self, w: i32) {
        let k = self.top() + 4 - self.scroll;
        let (top, bottom) = (self.top(), self.bottom());
        for (i, b) in self.buttons.iter_mut().enumerate() {
            let row = i as i32 / 2;
            b.x = w / 2 - 155 + (i as i32 % 2) * 160;
            b.y = k + row * ROW_H;
            b.visible = b.y + b.h > top && b.y < bottom;
        }
    }
}

impl Screen for VideoSettings {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        let off = |k: &str| format!("{}: {}", t(k), t("options.off"));
        let on = |k: &str| format!("{}: {}", t(k), t("options.on"));
        self.height = h;
        // Vanilla order. Settings this client doesn't have yet are shown locked.
        self.buttons = vec![
            Button::option(0, 0, 0, Opt::Graphics, ui),
            Button::option(1, 0, 0, Opt::RenderDistance, ui),
            Button::option(2, 0, 0, Opt::AmbientOcclusion, ui),
            Button::option(3, 0, 0, Opt::FramerateLimit, ui),
            Button::sized(4, 0, 0, 150, 20, off("options.anaglyph")).disabled(),
            Button::option(5, 0, 0, Opt::ViewBobbing, ui),
            Button::option(6, 0, 0, Opt::GuiScale, ui),
            Button::option(7, 0, 0, Opt::Gamma, ui),
            Button::sized(8, 0, 0, 150, 20, format!("{}: {}", t("options.renderClouds"), t("options.graphics.fancy"))).disabled(),
            Button::sized(9, 0, 0, 150, 20, format!("{}: {}", t("options.particles"), t("options.particles.all"))).disabled(),
            Button::sized(10, 0, 0, 150, 20, off("options.fullscreen")).disabled(),
            Button::option(11, 0, 0, Opt::Vsync, ui),
            Button::sized(12, 0, 0, 150, 20, format!("{}: 4", t("options.mipmapLevels"))).disabled(),
            Button::sized(13, 0, 0, 150, 20, on("options.blockAlternatives")).disabled(),
            Button::sized(14, 0, 0, 150, 20, on("options.vbo")).disabled(),
            Button::sized(15, 0, 0, 150, 20, on("options.entityShadows")).disabled(),
        ];
        self.done = Some(Button::new(200, w / 2 - 100, h - 27, t("gui.done")));
        self.scroll = self.scroll.clamp(0, self.max_scroll());
        self.layout(w);
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        self.layout(w);
        let (top, bottom) = (self.top(), self.bottom());
        default_background(ui, b, w, h);
        // GuiSlot: darker list body, scrolling with the content.
        dirt(b, top, bottom, w, 32, self.scroll as f32);
        let mouse_in_list = my >= top && my <= bottom;
        for btn in &mut self.buttons {
            let (hx, hy) = if mouse_in_list { (mx, my) } else { (-1, -1) };
            btn.draw(ui, b, hx, hy);
        }
        // overlayBackground above and below, then the 4px edge shadows.
        dirt(b, 0, top, w, 64, 0.0);
        dirt(b, bottom, h, w, 64, 0.0);
        b.gradient(0, top, w, top + 4, 0xFF00_0000, 0x0000_0000);
        b.gradient(0, bottom - 4, w, bottom, 0x0000_0000, 0xFF00_0000);
        if self.max_scroll() > 0 {
            // GuiOptionsRowList.getScrollBarX: GuiSlot's width/2 + 124, plus 32.
            let sx = w / 2 + 124 + 32;
            let view = bottom - top;
            let content = self.content_height() + 4;
            let bar = (view * view / content).clamp(32, view - 8);
            let by = self.scroll * (view - bar) / self.max_scroll() + top;
            b.rect(sx, top, sx + 6, bottom, 0xFF00_0000);
            b.rect(sx, by, sx + 6, by + bar, 0xFF80_8080);
            b.rect(sx, by, sx + 5, by + bar - 1, 0xFFC0_C0C0);
        }
        let title = ui.lang.get("options.videoTitle");
        ui.font.draw_centered(b, &title, w / 2, 5, 16777215);
        if let Some(d) = &mut self.done {
            d.draw(ui, b, mx, my);
        }
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        if self.done.as_ref().is_some_and(|d| d.hit(mx, my)) {
            ui.sounds.push("gui.button.press");
            ui.settings.save();
            return Action::Close;
        }
        if my >= self.top() && my <= self.bottom() {
            click_buttons(&mut self.buttons, ui, mx, my);
        }
        Action::None
    }

    fn release(&mut self, ui: &mut Ui) {
        for b in &mut self.buttons {
            b.release();
        }
        ui.settings.save();
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        if key == Key::Escape {
            ui.settings.save();
            return Action::Close;
        }
        Action::None
    }

    fn scroll(&mut self, delta: i32) {
        self.scroll = (self.scroll - delta * ROW_H / 2).clamp(0, self.max_scroll());
    }

    fn settings_dirty(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------- controls (sensitivity only for now)

#[derive(Default)]
pub struct Controls {
    buttons: Vec<Button>,
}

impl Screen for Controls {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        self.buttons = vec![
            Button::option(1, w / 2 - 155, 18, Opt::Sensitivity, ui),
            Button::sized(2, w / 2 - 155 + 160, 18, 150, 20, format!("{}: {}", t("options.invertMouse"), t("options.off"))).disabled(),
            Button::sized(201, w / 2 - 155, h - 29, 150, 20, t("controls.resetAll")).disabled(),
            Button::sized(200, w / 2 - 155 + 160, h - 29, 150, 20, t("gui.done")),
        ];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let title = ui.lang.get("controls.title");
        ui.font.draw_centered(b, &title, w / 2, 8, 16777215);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(200) => {
                ui.settings.save();
                Action::Close
            }
            _ => Action::None,
        }
    }

    fn release(&mut self, ui: &mut Ui) {
        for b in &mut self.buttons {
            b.release();
        }
        ui.settings.save();
    }

    fn settings_dirty(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------- chat (GuiChat)

/// `StringUtils.getCommonPrefix`
fn common_prefix(list: &[String]) -> String {
    let Some(first) = list.first() else { return String::new() };
    let mut n = first.chars().count();
    for s in &list[1..] {
        n = n.min(first.chars().zip(s.chars()).take_while(|(a, b)| a == b).count());
    }
    first.chars().take(n).collect()
}

#[derive(Default)]
pub struct Chat {
    pub text: String,
    cursor: usize,
    opened_at: f32,
    /// `sentHistoryCursor` and the text being typed before browsing history.
    history_cursor: usize,
    history_buffer: String,
    // Tab completion state
    names_found: bool,
    waiting: bool,
    index: usize,
    found: Vec<String>,
}

impl Chat {
    pub fn with_text(text: &str) -> Self {
        Self {
            text: text.into(),
            cursor: text.chars().count(),
            opened_at: 0.0,
            history_cursor: usize::MAX,
            history_buffer: String::new(),
            names_found: false,
            waiting: false,
            index: 0,
            found: Vec::new(),
        }
    }

    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    /// `func_146197_a(-1, cursor, false)`: start of the word under the cursor.
    fn word_start(&self) -> usize {
        let c = self.chars();
        let mut p = self.cursor;
        while p > 0 && c[p - 1] != ' ' {
            p -= 1;
        }
        p
    }

    /// `deleteFromCursor(start - cursor)` then `writeText(s)`.
    fn replace_word(&mut self, start: usize, s: &str) {
        let mut c = self.chars();
        c.drain(start..self.cursor);
        let ins: Vec<char> = s.chars().take(100usize.saturating_sub(c.len())).collect();
        let n = ins.len();
        c.splice(start..start, ins);
        self.text = c.into_iter().collect();
        self.cursor = start + n;
    }

    /// `GuiChat.autocompletePlayerNames`
    fn cycle(&mut self) -> Option<String> {
        if self.found.is_empty() {
            return None;
        }
        let start = self.word_start();
        if self.index >= self.found.len() {
            self.index = 0;
        }
        let line = (self.found.len() > 1).then(|| self.found.join(", "));
        let s = self.found[self.index].clone();
        self.index += 1;
        self.replace_word(start, &s);
        line
    }

    /// `GuiChat.getSentHistory`
    fn history(&mut self, ui: &Ui, delta: i32) {
        let sent = &ui.sent_messages;
        if self.history_cursor == usize::MAX {
            self.history_cursor = sent.len();
        }
        let i = (self.history_cursor as i64 + delta as i64).clamp(0, sent.len() as i64) as usize;
        if i == self.history_cursor {
            return;
        }
        if i == sent.len() {
            self.history_cursor = sent.len();
            self.text = self.history_buffer.clone();
        } else {
            if self.history_cursor == sent.len() {
                self.history_buffer = self.text.clone();
            }
            self.text = sent[i].clone();
            self.history_cursor = i;
        }
        self.cursor = self.text.chars().count();
    }
}

impl Screen for Chat {
    fn init(&mut self, ui: &Ui, _w: i32, _h: i32) {
        self.opened_at = ui.ticks;
        self.history_cursor = ui.sent_messages.len();
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, _mx: i32, _my: i32) {
        b.rect(2, h - 14, w - 2, h - 2, 0x8000_0000);
        // GuiTextField: text with a blinking cursor (6 ticks on, 6 off).
        let (x, y) = (4.0, (h - 12) as f32);
        let before: String = self.text.chars().take(self.cursor).collect();
        let end = ui.font.draw_shadow(b, &self.text, x, y, 14737632);
        let blink = ((ui.ticks - self.opened_at) as i32 / 6) % 2 == 0;
        if blink {
            if self.cursor >= self.text.chars().count() {
                ui.font.draw_shadow(b, "_", end as f32, y, 14737632);
            } else {
                let cx = x as i32 + ui.font.width(&before);
                b.rect(cx, y as i32 - 1, cx + 1, y as i32 + 1 + 9, 0xFFD0_D0D0);
            }
        }
    }

    fn click(&mut self, _ui: &mut Ui, _mx: i32, _my: i32) -> Action {
        Action::None
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        // GuiChat.keyTyped: any key but Tab resets the completion cycle.
        self.waiting = false;
        if key == Key::Tab {
            if self.names_found {
                let line = self.cycle();
                return line.map_or(Action::None, Action::PrintChat);
            }
            self.waiting = true;
            let prefix: String = self.text.chars().take(self.cursor).collect();
            return Action::TabComplete(prefix);
        }
        self.names_found = false;
        match key {
            Key::Escape => Action::Close,
            Key::Enter => {
                let msg = self.text.trim().to_string();
                if msg.is_empty() {
                    Action::Close
                } else {
                    // GuiNewChat.addToSentMessages
                    if ui.sent_messages.last() != Some(&msg) {
                        ui.sent_messages.push(msg.clone());
                    }
                    Action::SendChat(msg)
                }
            }
            Key::Backspace => {
                if self.cursor > 0 {
                    let mut chars = self.chars();
                    chars.remove(self.cursor - 1);
                    self.text = chars.into_iter().collect();
                    self.cursor -= 1;
                }
                Action::None
            }
            Key::Delete => {
                let mut chars = self.chars();
                if self.cursor < chars.len() {
                    chars.remove(self.cursor);
                    self.text = chars.into_iter().collect();
                }
                Action::None
            }
            Key::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Action::None
            }
            Key::Right => {
                self.cursor = (self.cursor + 1).min(self.text.chars().count());
                Action::None
            }
            Key::Home => {
                self.cursor = 0;
                Action::None
            }
            Key::End => {
                self.cursor = self.text.chars().count();
                Action::None
            }
            Key::Up => {
                self.history(ui, -1);
                Action::None
            }
            Key::Down => {
                self.history(ui, 1);
                Action::None
            }
            Key::Tab | Key::Other | Key::Inventory | Key::Number(_) => Action::None,
        }
    }

    fn autocomplete(&mut self, _ui: &mut Ui, list: Vec<String>) -> Option<String> {
        if !self.waiting {
            return None;
        }
        self.waiting = false;
        self.names_found = false;
        self.found = list.iter().filter(|s| !s.is_empty()).cloned().collect();
        self.index = 0;
        let start = self.word_start();
        let current: String = self.chars()[start..self.cursor].iter().collect();
        let common = common_prefix(&list);
        if !common.is_empty() && !current.eq_ignore_ascii_case(&common) {
            self.replace_word(start, &common);
            None
        } else if !self.found.is_empty() {
            self.names_found = true;
            self.cycle()
        } else {
            None
        }
    }

    /// `ChatAllowedCharacters.isAllowedCharacter` and the 100-char limit.
    fn typed(&mut self, c: char) {
        if c == '\u{a7}' || (c as u32) < 32 || c == '\u{7f}' || self.text.chars().count() >= 100 {
            return;
        }
        self.names_found = false;
        let mut chars = self.chars();
        chars.insert(self.cursor, c);
        self.text = chars.into_iter().collect();
        self.cursor += 1;
    }

    fn pauses(&self) -> bool {
        false
    }

    fn shows_hud(&self) -> bool {
        true
    }
}
