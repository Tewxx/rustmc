//! `GuiScreen` and the screens the client has so far.

use crate::batch::{GuiBatch, Special};
use crate::settings::Opt;
use crate::text_field::TextField;
use crate::widgets::{Button, Kind};
use crate::Ui;

pub const OPTIONS_BACKGROUND: &str = "textures/gui/options_background.png";
const TITLE: &str = "textures/gui/title/minecraft.png";

pub use crate::multiplayer::MultiplayerScreen;

/// What a screen asks the client to do.
pub enum Action {
    None,
    /// Close this screen (back to the game, or to `parent` if there is one).
    Close,
    Open(Box<dyn Screen>),
    StartGame,
    QuitToTitle,
    /// Pause menu "Disconnect": a server world returns to the server list, a local one to the title.
    Disconnect,
    /// `GuiConnecting`'s Cancel button.
    CancelConnect,
    Quit,
    SendChat(String),
    /// `GuiChat.sendAutocompleteRequest`: the text before the cursor.
    TabComplete(String),
    /// A line for the chat log (several completion matches).
    PrintChat(String),
    Respawn,
    /// `GuiYesNo` answer for the pending `S48PacketResourcePackSend`.
    ResourcePackDecision(bool),
    /// Join a server from the multiplayer screens (`GuiConnecting`).
    /// `pack_mode` is the saved server's `acceptTextures` (`enabled`/`disabled`/`prompt`).
    Connect { address: String, name: String, online: bool, pack_mode: Option<&'static str> },
    /// `displayGuiScreen(null)`: close this screen and every parent.
    CloseAll,
    /// `GuiChat` Page Up/Down: scroll the chat by `lineCount - 1` in that direction.
    ScrollChatPage(i32),
    /// `GuiConfirmOpenLink` accepted: open the link, then return to the chat.
    OpenLink(String),
}

/// `GuiGameOver`
#[derive(Default)]
pub struct DeathScreen {
    buttons: Vec<Button>,
    /// `enableButtonsTimer`: buttons enable on the 20th tick.
    timer: i32,
    pub hardcore: bool,
    pub multiplayer: bool,
    /// `EntityPlayer.getScore`
    pub score: i32,
}

impl DeathScreen {
    pub fn new(hardcore: bool, multiplayer: bool, score: i32) -> Self {
        Self { hardcore, multiplayer, score, ..Default::default() }
    }
}

impl Screen for DeathScreen {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.buttons = if self.hardcore {
            let key = if self.multiplayer { "deathScreen.leaveServer" } else { "deathScreen.deleteWorld" };
            vec![Button::new(1, w / 2 - 100, h / 4 + 96, ui.lang.get(key))]
        } else {
            vec![
                Button::new(0, w / 2 - 100, h / 4 + 72, ui.lang.get("deathScreen.respawn")),
                Button::new(1, w / 2 - 100, h / 4 + 96, ui.lang.get("deathScreen.titleScreen")),
            ]
        };
        let enabled = self.timer >= 20;
        for b in &mut self.buttons {
            b.enabled = enabled;
        }
    }
    fn tick(&mut self) {
        self.timer += 1;
        if self.timer == 20 {
            for b in &mut self.buttons {
                b.enabled = true;
            }
        }
    }
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        b.gradient(0, 0, w, h, 0x60500000, 0xa0803030);
        let title = ui.lang.get(if self.hardcore { "deathScreen.title.hardcore" } else { "deathScreen.title" });
        b.transform = Some((2.0, [0.0, 0.0]));
        ui.font.draw_centered(b, &title, w / 2 / 2, 30, 0xffffffff);
        b.transform = None;
        if self.hardcore {
            let info = ui.lang.get("deathScreen.hardcoreInfo");
            ui.font.draw_centered(b, &info, w / 2, 144, 0xffffffff);
        }
        let score = format!("{}: \u{a7}e{}", ui.lang.get("deathScreen.score"), self.score);
        ui.font.draw_centered(b, &score, w / 2, 100, 0xffffffff);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }
    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(0) => Action::Respawn,
            Some(1) => Action::QuitToTitle,
            _ => Action::None,
        }
    }
    /// `GuiGameOver.keyTyped` swallows Escape.
    fn key(&mut self, _: &mut Ui, _: Key) -> Action {
        Action::None
    }
    fn pauses(&self) -> bool {
        false
    }
}

/// `GuiYesNo` shown by `NetHandlerPlayClient.handleResourcePack` when the server's pack is
/// first offered (`ServerData.ServerResourceMode.PROMPT`).
#[derive(Default)]
pub struct ResourcePackPrompt {
    buttons: Vec<Button>,
}

impl ResourcePackPrompt {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Screen for ResourcePackPrompt {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        // GuiYesNo: 150x20 buttons at w/2 - 155 and w/2 - 155 + 160, h/6 + 96.
        self.buttons = vec![
            Button::sized(0, w / 2 - 155, h / 6 + 96, 150, 20, ui.lang.get("gui.yes")),
            Button::sized(1, w / 2 - 155 + 160, h / 6 + 96, 150, 20, ui.lang.get("gui.no")),
        ];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        // `GuiScreen.drawDefaultBackground` in a loaded world: the dark gradient.
        b.gradient(0, 0, w, h, 0xC0101010, 0xD0101010);
        ui.font.draw_centered(b, &ui.lang.get("multiplayer.texturePrompt.line1"), w / 2, 70, 0xffffffff);
        ui.font.draw_centered(b, &ui.lang.get("multiplayer.texturePrompt.line2"), w / 2, 90, 0xffffffff);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(0) => Action::ResourcePackDecision(true),
            Some(1) => Action::ResourcePackDecision(false),
            _ => Action::None,
        }
    }

    /// `GuiScreen.keyTyped`'s Escape closes the screen without answering, like vanilla.
    fn key(&mut self, _: &mut Ui, key: Key) -> Action {
        if key == Key::Escape {
            Action::Close
        } else {
            Action::None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Inventory,
    Drop,
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
    PageUp,
    PageDown,
    /// Ctrl held with a letter key (`isKeyComboCtrlA` and friends), lower-case.
    Combo(char),
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
    fn mouse_move(&mut self, _ui: &mut Ui, _mx: i32, _my: i32) {}
    fn release_at(&mut self, ui: &mut Ui, _mx: i32, _my: i32, _right: bool) { self.release(ui); }
    fn key(&mut self, _ui: &mut Ui, key: Key) -> Action {
        if key == Key::Escape { Action::Close } else { Action::None }
    }
    fn typed(&mut self, _ui: &mut Ui, _c: char) {}
    /// `GuiChat.setText` for chat-component clicks.
    fn set_chat_text(&mut self, _ui: &Ui, _text: &str, _overwrite: bool) {}
    /// `instanceof GuiChat`: the chat lines take clicks, hovers and the mouse wheel.
    fn is_chat(&self) -> bool {
        false
    }
    /// `GuiChat.onAutocompleteResponse`; returns a line to print in chat (several matches).
    fn autocomplete(&mut self, _ui: &mut Ui, _list: Vec<String>) -> Option<String> {
        None
    }
    fn scroll(&mut self, _delta: i32) {}
    /// `updateScreen`, once per client tick.
    fn tick(&mut self) {}
    /// `GuiInventory.drawEntityOnScreen(posX, posY, scale, ...)` for the local player.
    fn entity_preview(&self) -> Option<(i32, i32, i32)> {
        None
    }
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
pub(crate) fn dirt(b: &mut GuiBatch, top: i32, bottom: i32, w: i32, gray: u8, scroll: f32) {
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

pub(crate) fn draw_buttons(buttons: &mut [Button], ui: &mut Ui, b: &mut GuiBatch, mx: i32, my: i32) {
    for btn in buttons {
        btn.draw(ui, b, mx, my);
    }
}

/// `GuiScreen.mouseClicked`: first button hit wins.
pub(crate) fn click_buttons(buttons: &mut [Button], ui: &mut Ui, mx: i32, my: i32) -> Option<i32> {
    buttons.iter_mut().find_map(|btn| btn.press(ui, mx, my).then_some(btn.id))
}

// ---------------------------------------------------------------- title screen

/// Whether the host client can start a local world. When false the title screen shows the
/// Singleplayer button disabled, like vanilla greys out unavailable buttons.
pub static SINGLEPLAYER_AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct MainMenu {
    buttons: Vec<Button>,
}

impl Screen for MainMenu {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        let j = h / 4 + 48;
        let mut single = Button::new(1, w / 2 - 100, j, t("menu.singleplayer"));
        if !SINGLEPLAYER_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
            single = single.disabled();
        }
        self.buttons = vec![
            single,
            Button::new(2, w / 2 - 100, j + 24, t("menu.multiplayer")),
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
            Some(2) => Action::Open(Box::new(MultiplayerScreen::new())),
            Some(0) => Action::Open(Box::new(Options::default())),
            Some(4) => Action::Quit,
            _ => Action::None,
        }
    }

    fn key(&mut self, _ui: &mut Ui, _key: Key) -> Action {
        Action::None
    }
}

// ---------------------------------------------------------------- connecting / disconnected

/// What `GuiConnecting.drawScreen` shows: `networkManager == null` is Connecting, else Logging in.
pub const STAGE_CONNECTING: u8 = 0;
pub const STAGE_LOGGING_IN: u8 = 1;

/// `GuiConnecting`: the connection runs on a background thread owned by the client; the stage
/// is shared with it.
pub struct Connecting {
    buttons: Vec<Button>,
    stage: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl Connecting {
    pub fn new(stage: std::sync::Arc<std::sync::atomic::AtomicU8>) -> Self {
        Self { buttons: Vec::new(), stage }
    }
}

impl Screen for Connecting {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.buttons = vec![Button::new(0, w / 2 - 100, h / 4 + 120 + 12, ui.lang.get("gui.cancel"))];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let key = if self.stage.load(std::sync::atomic::Ordering::Relaxed) == STAGE_CONNECTING { "connect.connecting" } else { "connect.authorizing" };
        ui.font.draw_centered(b, &ui.lang.get(key), w / 2, h / 2 - 50, 0xFFFF_FFFF);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(0) => Action::CancelConnect,
            _ => Action::None,
        }
    }

    /// `GuiConnecting.keyTyped` is empty: Escape does nothing.
    fn key(&mut self, _ui: &mut Ui, _key: Key) -> Action {
        Action::None
    }
}

/// `GuiDisconnected`: a title (`reasonLocalizationKey`) over the wrapped reason and a single
/// "Back to server list" button that returns to the screen the connection started from.
pub struct Disconnected {
    buttons: Vec<Button>,
    title_key: String,
    pub reason: String,
    lines: Vec<String>,
}

impl Disconnected {
    pub fn new(title_key: impl Into<String>, reason: impl Into<String>) -> Self {
        Self { buttons: Vec::new(), title_key: title_key.into(), reason: reason.into(), lines: Vec::new() }
    }
}

impl Screen for Disconnected {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        // `fontRendererObj.listFormattedStringToWidth(message, width - 50)`
        self.lines.clear();
        let mut line = String::new();
        for word in self.reason.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && ui.font.width(&candidate) > w - 50 {
                self.lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            self.lines.push(line);
        }
        let total = self.lines.len() as i32 * 9;
        self.buttons = vec![Button::new(0, w / 2 - 100, h / 2 + total / 2 + 9, ui.lang.get("gui.toMenu"))];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let total = self.lines.len() as i32 * 9;
        ui.font.draw_centered(b, &ui.lang.get(&self.title_key), w / 2, h / 2 - total / 2 - 9 * 2, 0xFFAA_AAAA);
        let mut y = h / 2 - total / 2;
        for l in &self.lines {
            ui.font.draw_centered(b, l, w / 2, y, 0xFFFF_FFFF);
            y += 9;
        }
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(0) => Action::Close,
            _ => Action::None,
        }
    }

    /// `GuiDisconnected.keyTyped` is empty.
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
            Button::new(1, w / 2 - 100, h / 4 + 120 + i, t(if ui.remote { "menu.disconnect" } else { "menu.returnToMenu" })),
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
            Some(1) => Action::Disconnect,
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
            Button::sized(110, w / 2 - 155, h / 6 + 48 - 6, 150, 20, t("options.skinCustomisation")),
            Button::sized(8675309, w / 2 + 5, h / 6 + 48 - 6, 150, 20, "Super Secret Settings...").disabled(),
            Button::sized(106, w / 2 - 155, h / 6 + 72 - 6, 150, 20, t("options.sounds")),
            Button::sized(107, w / 2 + 5, h / 6 + 72 - 6, 150, 20, t("options.stream")).disabled(),
            Button::sized(101, w / 2 - 155, h / 6 + 96 - 6, 150, 20, t("options.video")),
            Button::sized(100, w / 2 + 5, h / 6 + 96 - 6, 150, 20, t("options.controls")),
            Button::sized(102, w / 2 - 155, h / 6 + 120 - 6, 150, 20, t("options.language")).disabled(),
            Button::sized(103, w / 2 + 5, h / 6 + 120 - 6, 150, 20, t("options.chat.title")),
            Button::sized(105, w / 2 - 155, h / 6 + 144 - 6, 150, 20, t("options.resourcepack")),
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
            Some(106) => Action::Open(Box::new(SoundSettings::default())),
            Some(103) => Action::Open(Box::new(OptionList::chat())),
            Some(110) => Action::Open(Box::new(OptionList::skin())),
            Some(100) => Action::Open(Box::new(Controls::default())),
            Some(105) => Action::Open(Box::new(crate::resource_packs::ResourcePacks::default())),
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
            b.release(ui);
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

// ---------------------------------------------------------------- chat / skin options

/// `ScreenChatOptions` and `GuiCustomizeSkin`: two columns of options, then Done.
pub struct OptionList {
    title: &'static str,
    opts: Vec<Opt>,
    buttons: Vec<Button>,
}

impl OptionList {
    pub fn chat() -> Self {
        use Opt::*;
        let opts = vec![ChatVisibility, ChatColors, ChatLinks, ChatOpacity, ChatLinksPrompt, ChatScale, ChatWidth, ChatHeightFocused, ChatHeightUnfocused];
        OptionList { title: "options.chat.title", opts, buttons: Vec::new() }
    }
    pub fn skin() -> Self {
        OptionList { title: "options.skinCustomisation.title", opts: (0..7).map(Opt::ModelPart).collect(), buttons: Vec::new() }
    }
}

impl Screen for OptionList {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.buttons.clear();
        let mut i = 0;
        for &o in &self.opts {
            self.buttons.push(Button::option(i, w / 2 - 155 + i % 2 * 160, h / 6 + 24 * (i >> 1), o, ui));
            i += 1;
        }
        let done_y = if self.title == "options.chat.title" {
            h / 6 + 120
        } else {
            if i % 2 == 1 {
                i += 1;
            }
            h / 6 + 24 * (i >> 1)
        };
        self.buttons.push(Button::new(200, w / 2 - 100, done_y, ui.lang.get("gui.done")));
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let title = ui.lang.get(self.title);
        ui.font.draw_centered(b, &title, w / 2, 20, 16777215);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        if click_buttons(&mut self.buttons, ui, mx, my) == Some(200) {
            ui.settings.save();
            return Action::Close;
        }
        Action::None
    }

    fn release(&mut self, ui: &mut Ui) {
        for b in &mut self.buttons {
            b.release(ui);
        }
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        if key == Key::Escape {
            ui.settings.save();
            return Action::Close;
        }
        Action::None
    }
}

// ---------------------------------------------------------------- sound settings

/// `GuiScreenOptionsSounds`: master (310 wide) then the other categories two per row.
#[derive(Default)]
pub struct SoundSettings {
    buttons: Vec<Button>,
}

impl Screen for SoundSettings {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let pos = |i: i32| (w / 2 - 155 + i % 2 * 160, h / 6 - 12 + 24 * (i >> 1));
        let (x, y) = pos(0);
        let mut master = Button::option(0, x, y, Opt::SoundVolume(0), ui);
        master.w = 310;
        self.buttons = vec![master];
        for c in 1..9u8 {
            let (x, y) = pos(c as i32 + 1);
            self.buttons.push(Button::option(c as i32, x, y, Opt::SoundVolume(c), ui));
        }
        self.buttons.push(Button::new(200, w / 2 - 100, h / 6 + 168, ui.lang.get("gui.done")));
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let title = ui.lang.get("options.sounds.title");
        ui.font.draw_centered(b, &title, w / 2, 15, 16777215);
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
            b.release(ui);
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
        self.height = h;
        // GuiVideoSettings order.
        self.buttons = vec![
            Button::option(0, 0, 0, Opt::Graphics, ui),
            Button::option(1, 0, 0, Opt::RenderDistance, ui),
            Button::option(2, 0, 0, Opt::AmbientOcclusion, ui),
            Button::option(3, 0, 0, Opt::FramerateLimit, ui),
            Button::option(4, 0, 0, Opt::Anaglyph, ui),
            Button::option(5, 0, 0, Opt::ViewBobbing, ui),
            Button::option(6, 0, 0, Opt::GuiScale, ui),
            Button::option(7, 0, 0, Opt::Gamma, ui),
            Button::option(8, 0, 0, Opt::RenderClouds, ui),
            Button::option(9, 0, 0, Opt::Particles, ui),
            Button::option(10, 0, 0, Opt::Fullscreen, ui),
            Button::option(11, 0, 0, Opt::Vsync, ui),
            Button::option(12, 0, 0, Opt::MipmapLevels, ui),
            Button::option(13, 0, 0, Opt::BlockAlternatives, ui),
            Button::option(14, 0, 0, Opt::Vbo, ui),
            Button::option(15, 0, 0, Opt::EntityShadows, ui),
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
            b.release(ui);
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

pub struct Controls {
    buttons: Vec<Button>,
    /// `GuiKeyBindingList` entries: `(description, category)` in display order.
    entries: Vec<(&'static str, &'static str)>,
    /// `buttonId`: the binding waiting for a key.
    selected: Option<&'static str>,
    scroll: i32,
    height: i32,
    width: i32,
}

impl Controls {
    const SLOT: i32 = 20;
    fn top(&self) -> i32 {
        63
    }
    fn bottom(&self) -> i32 {
        self.height - 32
    }
    /// Rows: a header before each new category, then the bindings.
    fn rows(&self) -> Vec<(Option<&'static str>, &'static str)> {
        let mut out = Vec::new();
        let mut last = "";
        for &(name, cat) in &self.entries {
            if cat != last {
                out.push((None, cat));
                last = cat;
            }
            out.push((Some(name), cat));
        }
        out
    }
    fn max_scroll(&self) -> i32 {
        (self.rows().len() as i32 * Self::SLOT + 4 - (self.bottom() - self.top() - 4)).max(0)
    }
    fn set_key(&mut self, ui: &mut Ui, code: i32) {
        if let Some(name) = self.selected.take() {
            ui.settings.key_codes.insert(name.to_string(), code);
            ui.settings.save();
        }
    }
}

impl Default for Controls {
    fn default() -> Self {
        Controls { buttons: Vec::new(), entries: Vec::new(), selected: None, scroll: 0, height: 0, width: 0 }
    }
}

impl Screen for Controls {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        self.height = h;
        self.width = w;
        self.entries = crate::keys::sorted(|k| ui.lang.get(k)).into_iter().map(|b| (b.0, b.1)).collect();
        self.buttons = vec![
            Button::option(1, w / 2 - 155, 18, Opt::Sensitivity, ui),
            Button::option(2, w / 2 - 155 + 160, 18, Opt::InvertMouse, ui),
            Button::sized(201, w / 2 - 155, h - 29, 150, 20, t("controls.resetAll")),
            Button::sized(200, w / 2 - 155 + 160, h - 29, 150, 20, t("gui.done")),
        ];
        self.scroll = self.scroll.clamp(0, self.max_scroll());
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let (top, bottom) = (self.top(), self.bottom());
        dirt(b, top, bottom, w, 32, self.scroll as f32);
        let label_w = self.entries.iter().map(|e| ui.font.width(&ui.lang.get(e.0))).max().unwrap_or(0);
        // GuiKeyBindingList.getListWidth = 220 + 32; entries start at left + 2.
        let left = w / 2 - 252 / 2 + 2;
        let mut any_reset = false;
        for (i, (name, cat)) in self.rows().into_iter().enumerate() {
            let y = top + 4 + i as i32 * Self::SLOT - self.scroll;
            if y + Self::SLOT < top || y > bottom {
                continue;
            }
            let Some(name) = name else {
                let text = ui.lang.get(cat);
                let tw = ui.font.width(&text);
                ui.font.draw(b, &text, (w / 2 - tw / 2) as f32, (y + Self::SLOT - 9 - 1) as f32, 16777215);
                continue;
            };
            let label = ui.lang.get(name);
            ui.font.draw(b, &label, (left + 90 - label_w) as f32, (y + Self::SLOT / 2 - 9 / 2) as f32, 16777215);
            let code = ui.settings.key(name);
            let default = crate::keys::BINDINGS.iter().find(|k| k.0 == name).map_or(0, |k| k.2);
            let conflict = code != 0 && self.entries.iter().any(|e| e.0 != name && ui.settings.key(e.0) == code);
            let mut text = crate::keys::key_name(code);
            if self.selected == Some(name) {
                text = format!("\u{a7}f> \u{a7}e{text}\u{a7}f <");
            } else if conflict {
                text = format!("\u{a7}c{text}");
            }
            let in_list = my >= top && my <= bottom;
            let (hx, hy) = if in_list { (mx, my) } else { (-1, -1) };
            let mut kb = Button::sized(0, left + 105, y, 75, 20, text);
            kb.draw(ui, b, hx, hy);
            let mut rb = Button::sized(1, left + 190, y, 50, 20, ui.lang.get("controls.reset"));
            rb.enabled = code != default;
            any_reset |= code != default;
            rb.draw(ui, b, hx, hy);
        }
        dirt(b, 0, top, w, 64, 0.0);
        dirt(b, bottom, h, w, 64, 0.0);
        b.gradient(0, top, w, top + 4, 0xFF00_0000, 0x0000_0000);
        b.gradient(0, bottom - 4, w, bottom, 0x0000_0000, 0xFF00_0000);
        if let Some(r) = self.buttons.iter_mut().find(|b| b.id == 201) {
            r.enabled = any_reset;
        }
        let title = ui.lang.get("controls.title");
        ui.font.draw_centered(b, &title, w / 2, 8, 16777215);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        // GuiControls.mouseClicked: a pending binding takes the mouse button.
        if self.selected.is_some() {
            self.set_key(ui, -100);
            return Action::None;
        }
        if my >= self.top() && my <= self.bottom() {
            let left = self.width / 2 - 252 / 2 + 2;
            let row = (my - self.top() - 4 + self.scroll).div_euclid(Self::SLOT);
            if let Some((Some(name), _)) = self.rows().get(row.max(0) as usize).copied() {
                let y = self.top() + 4 + row * Self::SLOT - self.scroll;
                if my >= y && my < y + 20 {
                    if mx >= left + 105 && mx < left + 180 {
                        ui.sounds.push("gui.button.press");
                        self.selected = Some(name);
                        return Action::None;
                    }
                    let default = crate::keys::BINDINGS.iter().find(|k| k.0 == name).map_or(0, |k| k.2);
                    if mx >= left + 190 && mx < left + 240 && ui.settings.key(name) != default {
                        ui.sounds.push("gui.button.press");
                        ui.settings.key_codes.remove(name);
                        ui.settings.save();
                        return Action::None;
                    }
                }
            }
        }
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(200) => {
                ui.settings.save();
                Action::Close
            }
            Some(201) => {
                ui.settings.key_codes.clear();
                ui.settings.save();
                Action::None
            }
            _ => Action::None,
        }
    }

    fn release(&mut self, ui: &mut Ui) {
        for b in &mut self.buttons {
            b.release(ui);
        }
        ui.settings.save();
    }

    /// `GuiControls.keyTyped`: Escape clears the binding being set, any other key takes it.
    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        if self.selected.is_some() {
            let code = if key == Key::Escape { 0 } else { ui.key_code };
            self.set_key(ui, code);
            return Action::None;
        }
        if key == Key::Escape {
            ui.settings.save();
            return Action::Close;
        }
        Action::None
    }

    fn scroll(&mut self, delta: i32) {
        self.scroll = (self.scroll - delta * Self::SLOT / 2).clamp(0, self.max_scroll());
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

/// `GuiChat`
pub struct Chat {
    input: TextField,
    default_text: String,
    /// `sentHistoryCursor` and the text being typed before browsing history.
    history_cursor: usize,
    history_buffer: String,
    // Tab completion state
    names_found: bool,
    waiting: bool,
    index: usize,
    found: Vec<String>,
    initialised: bool,
}

impl Default for Chat {
    fn default() -> Self {
        Self::with_text("")
    }
}

impl Chat {
    pub fn with_text(text: &str) -> Self {
        Self {
            input: TextField::new(4, 0, 0, 12),
            default_text: text.into(),
            history_cursor: usize::MAX,
            history_buffer: String::new(),
            names_found: false,
            waiting: false,
            index: 0,
            found: Vec::new(),
            initialised: false,
        }
    }

    pub fn text(&self) -> String {
        self.input.text()
    }

    /// `func_146197_a(-1, cursor, false)`: start of the word under the cursor.
    fn word_start(&self) -> usize {
        self.input.nth_word_from(-1, self.input.cursor(), false)
    }

    /// `deleteFromCursor(start - cursor)` then `writeText(s)`.
    fn replace_word(&mut self, ui: &Ui, start: usize, s: &str) {
        let cur = self.input.cursor();
        self.input.delete_from_cursor(ui, start as isize - cur as isize);
        self.input.write_text(ui, s);
    }

    /// `GuiChat.autocompletePlayerNames`
    fn cycle(&mut self, ui: &Ui) -> Option<String> {
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
        self.replace_word(ui, start, &s);
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
            let t = self.history_buffer.clone();
            self.input.set_text(ui, &t);
        } else {
            if self.history_cursor == sent.len() {
                self.history_buffer = self.input.text();
            }
            let t = sent[i].clone();
            self.input.set_text(ui, &t);
            self.history_cursor = i;
        }
    }
}

impl Screen for Chat {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.history_cursor = ui.sent_messages.len();
        self.input.relayout(4, h - 12, w - 4, 12);
        if !self.initialised {
            self.initialised = true;
            self.input.max_len = 100;
            self.input.background = false;
            self.input.set_focused(true);
            self.input.can_lose_focus = false;
            let t = self.default_text.clone();
            self.input.set_text(ui, &t);
        }
    }

    fn tick(&mut self) {
        self.input.tick();
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, _mx: i32, _my: i32) {
        b.rect(2, h - 14, w - 2, h - 2, 0x8000_0000);
        self.input.draw(ui, b);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.input.mouse_clicked(ui, mx, my, 0);
        Action::None
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        // GuiChat.keyTyped: any key but Tab resets the completion cycle.
        self.waiting = false;
        if key == Key::Tab {
            if self.names_found {
                let line = self.cycle(ui);
                return line.map_or(Action::None, Action::PrintChat);
            }
            self.waiting = true;
            let prefix: String = self.input.text().chars().take(self.input.cursor()).collect();
            return Action::TabComplete(prefix);
        }
        self.names_found = false;
        match key {
            Key::Escape => Action::CloseAll,
            Key::Enter => {
                let msg = self.input.text().trim().to_string();
                if msg.is_empty() {
                    Action::CloseAll
                } else {
                    // GuiNewChat.addToSentMessages
                    if ui.sent_messages.last() != Some(&msg) {
                        ui.sent_messages.push(msg.clone());
                    }
                    Action::SendChat(msg)
                }
            }
            Key::Up => {
                self.history(ui, -1);
                Action::None
            }
            Key::Down => {
                self.history(ui, 1);
                Action::None
            }
            Key::PageUp => Action::ScrollChatPage(1),
            Key::PageDown => Action::ScrollChatPage(-1),
            k => {
                self.input.key(ui, k);
                Action::None
            }
        }
    }

    fn autocomplete(&mut self, ui: &mut Ui, list: Vec<String>) -> Option<String> {
        if !self.waiting {
            return None;
        }
        self.waiting = false;
        self.names_found = false;
        self.found = list.iter().filter(|s| !s.is_empty()).cloned().collect();
        self.index = 0;
        let start = self.word_start();
        let current: String = self.input.text().chars().skip(start).take(self.input.cursor() - start).collect();
        let common = common_prefix(&list);
        if !common.is_empty() && !current.eq_ignore_ascii_case(&common) {
            self.replace_word(ui, start, &common);
            None
        } else if !self.found.is_empty() {
            self.names_found = true;
            self.cycle(ui)
        } else {
            None
        }
    }

    fn typed(&mut self, ui: &mut Ui, c: char) {
        if self.input.typed(ui, c) {
            self.names_found = false;
        }
    }

    /// `GuiChat.setText`: suggestions overwrite, shift-click insertions are typed in.
    fn set_chat_text(&mut self, ui: &Ui, text: &str, overwrite: bool) {
        if overwrite {
            self.input.set_text(ui, text);
        } else {
            self.input.write_text(ui, text);
        }
    }

    fn is_chat(&self) -> bool {
        true
    }

    fn pauses(&self) -> bool {
        false
    }

    fn shows_hud(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------- link prompt

/// `GuiConfirmOpenLink` over `GuiChat` (`chatLinksPrompt`).
pub struct ConfirmOpenLink {
    buttons: Vec<Button>,
    pub link: String,
    trusted: bool,
}

impl ConfirmOpenLink {
    pub fn new(link: impl Into<String>, trusted: bool) -> Self {
        Self { buttons: Vec::new(), link: link.into(), trusted }
    }
}

impl Screen for ConfirmOpenLink {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let t = |k: &str| ui.lang.get(k);
        let (yes, no) = if self.trusted { (t("chat.link.open"), t("gui.cancel")) } else { (t("gui.yes"), t("gui.no")) };
        self.buttons = vec![
            Button::sized(0, w / 2 - 50 - 105, h / 6 + 96, 100, 20, yes),
            Button::sized(2, w / 2 - 50, h / 6 + 96, 100, 20, t("chat.copy")),
            Button::sized(1, w / 2 - 50 + 105, h / 6 + 96, 100, 20, no),
        ];
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        // GuiYesNo.drawScreen, then the warning.
        default_background(ui, b, w, h);
        let title = ui.lang.get(if self.trusted { "chat.link.confirmTrusted" } else { "chat.link.confirm" });
        ui.font.draw_centered(b, &title, w / 2, 70, 0xFFFF_FFFF);
        let mut y = 90;
        for line in ui.font.list_to_width(&self.link, w - 50) {
            ui.font.draw_centered(b, &line, w / 2, y, 0xFFFF_FFFF);
            y += 9;
        }
        draw_buttons(&mut self.buttons, ui, b, mx, my);
        let warning = ui.lang.get("chat.link.warning");
        ui.font.draw_centered(b, &warning, w / 2, 110, 0xFF00_0000 | 16764108);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(0) => Action::OpenLink(self.link.clone()),
            Some(2) => {
                crate::clipboard::set(&self.link);
                Action::Close
            }
            Some(1) => Action::Close,
            _ => Action::None,
        }
    }

    /// `GuiScreen.keyTyped`: Escape is `displayGuiScreen(null)`, past the chat as well.
    fn key(&mut self, _: &mut Ui, key: Key) -> Action {
        if key == Key::Escape {
            Action::CloseAll
        } else {
            Action::None
        }
    }
}
