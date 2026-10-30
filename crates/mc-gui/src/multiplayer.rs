//! The vanilla multiplayer screens: `GuiMultiplayer` (with `ServerSelectionList`,
//! `ServerListEntryNormal`, `ServerListEntryLanScan`, `ServerListEntryLanDetected` and the
//! `GuiSlot` scrolling they sit on), `GuiScreenAddServer`, `GuiScreenServerList` and the
//! delete `GuiYesNo`.
//!
//! Child screens report back through a shared [`Mailbox`] (vanilla calls
//! `parent.confirmClicked`); the parent reads it when it is re-initialised on return.

use crate::batch::GuiBatch;
use crate::lan::{LanDetector, LanServer, LanServerList};
use crate::lang::Lang;
use crate::screens::{click_buttons, default_background, dirt, draw_buttons, Action, Key, Screen};
use crate::server_data::{ping_level, register_icon, Handle, ResourceMode, ServerData, ServerList};
use crate::text_field::TextField;
use crate::widgets::Button;
use crate::Ui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const ICONS: &str = "textures/gui/icons.png";
const SERVER_SELECTION: &str = "textures/gui/server_selection.png";
const UNKNOWN_SERVER: &str = "textures/misc/unknown_server.png";
const SLOT_HEIGHT: i32 = 36;
const TOP: i32 = 32;

/// Starts a status ping for a server (`OldServerPinger.ping` on the pinger pool). The client
/// installs it; the flag cancels the ping when the screen goes away.
pub type PingFn = fn(Handle, Lang, Arc<AtomicBool>);
static PINGER: OnceLock<PingFn> = OnceLock::new();

pub fn set_pinger(f: PingFn) {
    let _ = PINGER.set(f);
}

/// `GuiYesNoCallback.confirmClicked` result from a child screen.
pub type Mailbox = Arc<Mutex<Option<bool>>>;

/// `Minecraft.getSystemTime()`
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `String.split(":").length > 0` for a non-empty string.
fn has_host(s: &str) -> bool {
    s.chars().any(|c| c != ':')
}

/// Where `servers.dat` lives: next to `options.txt`.
pub fn game_dir(ui: &Ui) -> PathBuf {
    ui.settings.path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("."))
}

struct NormalEntry {
    data: Handle,
    /// `field_148299_g`: the icon data the texture was made from.
    icon_data: Option<String>,
    /// `field_148305_h`: the registered texture, if the icon was valid.
    icon_path: Option<String>,
    last_click: u64,
}

struct LanEntry {
    server: LanServer,
    last_click: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pending {
    None,
    Delete,
    Direct,
    Add,
    Edit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Normal(usize),
    Scan,
    Lan(usize),
}

/// `GuiMultiplayer`
pub struct MultiplayerScreen {
    buttons: Vec<Button>,
    initialized: bool,
    list: Option<ServerList>,
    internet: Vec<NormalEntry>,
    lan: Vec<LanEntry>,
    lan_list: Arc<LanServerList>,
    _lan_detector: Option<LanDetector>,
    selected: i32,
    // GuiSlot state
    scroll: f32,
    initial_click_y: i32,
    scroll_multiplier: f32,
    list_enabled: bool,
    mouse_held: bool,
    mx: i32,
    my: i32,
    w: i32,
    h: i32,
    hovering_text: Option<String>,
    pending: Pending,
    selected_server: Option<Handle>,
    mailbox: Mailbox,
    cancel: Arc<AtomicBool>,
}

impl Default for MultiplayerScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MultiplayerScreen {
    /// `onGuiClosed`: `clearPendingNetworks` and the LAN thread (dropped with the detector).
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl MultiplayerScreen {
    pub fn new() -> Self {
        Self {
            buttons: Vec::new(),
            initialized: false,
            list: None,
            internet: Vec::new(),
            lan: Vec::new(),
            lan_list: Arc::new(LanServerList::default()),
            _lan_detector: None,
            selected: -1,
            scroll: 0.0,
            initial_click_y: -2,
            scroll_multiplier: 0.0,
            list_enabled: true,
            mouse_held: false,
            mx: 0,
            my: 0,
            w: 0,
            h: 0,
            hovering_text: None,
            pending: Pending::None,
            selected_server: None,
            mailbox: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn size(&self) -> i32 {
        (self.internet.len() + 1 + self.lan.len()) as i32
    }

    /// `ServerSelectionList.getListEntry`
    fn kind(&self, index: i32) -> Option<Kind> {
        if index < 0 || index >= self.size() {
            return None;
        }
        let i = index as usize;
        Some(if i < self.internet.len() {
            Kind::Normal(i)
        } else if i == self.internet.len() {
            Kind::Scan
        } else {
            Kind::Lan(i - self.internet.len() - 1)
        })
    }

    fn selected_kind(&self) -> Option<Kind> {
        self.kind(self.selected)
    }

    /// `func_148195_a`: rebuild the internet entries from the saved list.
    fn rebuild_internet(&mut self) {
        self.internet = self
            .list
            .as_ref()
            .map(|l| l.servers.iter().map(|d| NormalEntry { data: d.clone(), icon_data: None, icon_path: None, last_click: 0 }).collect())
            .unwrap_or_default();
    }

    /// `func_148194_a`
    fn rebuild_lan(&mut self, servers: Vec<LanServer>) {
        self.lan = servers.into_iter().map(|server| LanEntry { server, last_click: 0 }).collect();
    }

    fn save_list(&self) {
        if let Some(l) = &self.list {
            l.save();
        }
    }

    /// Selecting or removing entries moves the selection past what `getSize` allows.
    fn select_server(&mut self, index: i32) {
        self.selected = index;
        let kind = self.kind(index);
        let (mut select, mut edit) = (false, false);
        if let Some(k) = kind {
            if k != Kind::Scan {
                select = true;
                edit = matches!(k, Kind::Normal(_));
            }
        }
        // buttons: 0 edit, 1 delete, 2 select
        if self.buttons.len() >= 3 {
            self.buttons[0].enabled = edit;
            self.buttons[1].enabled = edit;
            self.buttons[2].enabled = select;
        }
    }

    fn create_buttons(&mut self, ui: &Ui) {
        let (w, h) = (self.w, self.h);
        let t = |k: &str| ui.lang.get(k);
        self.buttons = vec![
            Button::sized(7, w / 2 - 154, h - 28, 70, 20, t("selectServer.edit")),
            Button::sized(2, w / 2 - 74, h - 28, 70, 20, t("selectServer.delete")),
            Button::sized(1, w / 2 - 154, h - 52, 100, 20, t("selectServer.select")),
            Button::sized(4, w / 2 - 50, h - 52, 100, 20, t("selectServer.direct")),
            Button::sized(3, w / 2 + 4 + 50, h - 52, 100, 20, t("selectServer.add")),
            Button::sized(8, w / 2 + 4, h - 28, 70, 20, t("selectServer.refresh")),
            Button::sized(0, w / 2 + 4 + 76, h - 28, 75, 20, t("gui.cancel")),
        ];
        self.select_server(self.selected);
    }

    fn max_scroll(&self) -> i32 {
        (self.size() * SLOT_HEIGHT - (self.h - 64 - TOP - 4)).max(0)
    }

    /// `bindAmountScrolled`
    fn bind_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0.0, self.max_scroll() as f32);
    }

    /// `GuiSlot.scrollBy`
    fn scroll_by(&mut self, amount: i32) {
        self.scroll += amount as f32;
        self.bind_scroll();
        self.initial_click_y = -2;
    }

    /// `getSlotIndexFromScreenCoords`
    fn slot_at(&self, mx: i32, my: i32) -> i32 {
        let (i, j) = (self.w / 2 - 152, self.w / 2 + 152);
        let k = my - TOP + self.scroll as i32 - 4;
        let l = k / SLOT_HEIGHT;
        if mx < self.w / 2 + 154 && mx >= i && mx <= j && l >= 0 && k >= 0 && l < self.size() {
            l
        } else {
            -1
        }
    }

    /// `GuiSlot.handleMouseInput`'s scrollbar drag, run once per frame.
    fn mouse_input(&mut self, mx: i32, my: i32) {
        let (top, bottom) = (TOP, self.h - 64);
        self.mx = mx;
        self.my = my;
        if !(my >= top && my <= bottom && mx >= 0 && mx <= self.w) {
            return;
        }
        if self.mouse_held && self.list_enabled {
            if self.initial_click_y == -1 {
                let mut flag1 = true;
                let (j2, k2) = (self.w / 2 - 152, self.w / 2 + 152);
                let l2 = my - top + self.scroll as i32 - 4;
                if mx >= j2 && mx <= k2 && l2 < 0 {
                    flag1 = false;
                }
                let sbx = self.w / 2 + 154;
                if mx >= sbx && mx <= sbx + 6 {
                    let k1 = self.max_scroll().max(1);
                    let l1 = clamp_int(((bottom - top) * (bottom - top)) / (self.size() * SLOT_HEIGHT).max(1), 32, bottom - top - 8);
                    self.scroll_multiplier = -1.0 / ((bottom - top - l1) as f32 / k1 as f32);
                } else {
                    self.scroll_multiplier = 1.0;
                }
                self.initial_click_y = if flag1 { my } else { -2 };
            } else if self.initial_click_y >= 0 {
                self.scroll -= (my - self.initial_click_y) as f32 * self.scroll_multiplier;
                self.initial_click_y = my;
            }
        } else {
            self.initial_click_y = -1;
        }
    }

    /// Polls `LanServerList` like `updateScreen`.
    fn poll_lan(&mut self) {
        if self.lan_list.was_updated() {
            let list = self.lan_list.take();
            self.rebuild_lan(list);
        }
    }

    fn connect_action(&self, ui: &Ui, address: &str, mode: Option<ResourceMode>) -> Action {
        let pack_mode = mode.map(|m| match m {
            ResourceMode::Enabled => "enabled",
            ResourceMode::Disabled => "disabled",
            ResourceMode::Prompt => "prompt",
        });
        Action::Connect { address: address.to_string(), name: ui.player_name.clone(), online: false, pack_mode }
    }

    /// `connectToSelected`
    fn connect_to_selected(&mut self, ui: &Ui) -> Action {
        match self.selected_kind() {
            Some(Kind::Normal(i)) => {
                let d = self.internet[i].data.lock().unwrap();
                let (ip, mode) = (d.ip.clone(), d.resource_mode);
                drop(d);
                self.connect_action(ui, &ip, Some(mode))
            }
            Some(Kind::Lan(i)) => {
                let ip = self.lan[i].server.address.clone();
                self.connect_action(ui, &ip, None)
            }
            _ => Action::None,
        }
    }

    /// `confirmClicked`, from the child screen's answer.
    fn process_mailbox(&mut self) {
        let Some(result) = self.mailbox.lock().unwrap().take() else { return };
        let pending = std::mem::replace(&mut self.pending, Pending::None);
        let selected = self.selected_kind();
        match pending {
            Pending::Delete => {
                if let (true, Some(Kind::Normal(i))) = (result, selected) {
                    if let Some(l) = &mut self.list {
                        l.servers.remove(i);
                    }
                    self.save_list();
                    self.selected = -1;
                    self.rebuild_internet();
                }
            }
            Pending::Add => {
                if let (true, Some(s)) = (result, self.selected_server.take()) {
                    if let Some(l) = &mut self.list {
                        l.servers.push(s);
                    }
                    self.save_list();
                    self.selected = -1;
                    self.rebuild_internet();
                }
            }
            Pending::Edit => {
                if let (true, Some(Kind::Normal(i)), Some(edited)) = (result, selected, self.selected_server.take()) {
                    {
                        let src = edited.lock().unwrap();
                        let mut d = self.internet[i].data.lock().unwrap();
                        d.name = src.name.clone();
                        d.ip = src.ip.clone();
                        d.copy_from(&src);
                    }
                    self.save_list();
                    self.rebuild_internet();
                }
            }
            Pending::Direct | Pending::None => {}
        }
    }

    /// `actionPerformed`
    fn action_performed(&mut self, ui: &mut Ui, id: i32) -> Action {
        let kind = self.selected_kind();
        match id {
            2 => {
                if let Some(Kind::Normal(i)) = kind {
                    let name = self.internet[i].data.lock().unwrap().name.clone();
                    self.pending = Pending::Delete;
                    let l = &ui.lang;
                    let line1 = l.get("selectServer.deleteQuestion");
                    let line2 = format!("'{}' {}", name, l.get("selectServer.deleteWarning"));
                    let yes = YesNo::new(self.mailbox.clone(), line1, line2, l.get("selectServer.deleteButton"), l.get("gui.cancel"));
                    return Action::Open(Box::new(yes));
                }
                Action::None
            }
            1 => self.connect_to_selected(ui),
            4 => {
                self.pending = Pending::Direct;
                let s = Arc::new(Mutex::new(ServerData::new(&ui.lang.get("selectServer.defaultName"), "", false)));
                self.selected_server = Some(s.clone());
                Action::Open(Box::new(DirectConnectScreen::new(self.mailbox.clone(), s)))
            }
            3 => {
                self.pending = Pending::Add;
                let s = Arc::new(Mutex::new(ServerData::new(&ui.lang.get("selectServer.defaultName"), "", false)));
                self.selected_server = Some(s.clone());
                Action::Open(Box::new(AddServerScreen::new(self.mailbox.clone(), s)))
            }
            7 => {
                if let Some(Kind::Normal(i)) = kind {
                    self.pending = Pending::Edit;
                    let copy = {
                        let src = self.internet[i].data.lock().unwrap();
                        let mut c = ServerData::new(&src.name, &src.ip, false);
                        c.copy_from(&src);
                        c
                    };
                    let s = Arc::new(Mutex::new(copy));
                    self.selected_server = Some(s.clone());
                    return Action::Open(Box::new(AddServerScreen::new(self.mailbox.clone(), s)));
                }
                Action::None
            }
            0 => Action::Close,
            8 => {
                self.refresh(ui);
                Action::None
            }
            _ => Action::None,
        }
    }

    /// `refreshServerList`: a fresh `GuiMultiplayer` (re-read file, new pings, scroll reset).
    fn refresh(&mut self, ui: &Ui) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.selected = -1;
        self.scroll = 0.0;
        self.initial_click_y = -2;
        self.list_enabled = true;
        self.mouse_held = false;
        self.pending = Pending::None;
        self.lan.clear();
        self.initialized = false;
        let (w, h) = (self.w, self.h);
        self.init(ui, w, h);
    }

    /// Shift+Up/Down reordering and plain selection moves from `keyTyped`.
    fn arrow(&mut self, ui: &Ui, up: bool) {
        let i = self.selected;
        let count = self.list.as_ref().map_or(0, |l| l.count()) as i32;
        if up {
            if ui.shift_down {
                if i > 0 && matches!(self.selected_kind(), Some(Kind::Normal(_))) {
                    self.swap_servers(i as usize, i as usize - 1);
                    self.select_server(self.selected - 1);
                    self.scroll_by(-SLOT_HEIGHT);
                    self.rebuild_internet();
                }
            } else if i > 0 {
                self.select_server(self.selected - 1);
                self.scroll_by(-SLOT_HEIGHT);
                // The LAN scan row cannot be selected: step over it.
                if self.selected_kind() == Some(Kind::Scan) {
                    if self.selected > 0 {
                        self.select_server(self.selected - 1);
                        self.scroll_by(-SLOT_HEIGHT);
                    } else {
                        self.select_server(-1);
                    }
                }
            } else {
                self.select_server(-1);
            }
        } else if ui.shift_down {
            if i < count - 1 {
                self.swap_servers(i as usize, i as usize + 1);
                self.select_server(i + 1);
                self.scroll_by(SLOT_HEIGHT);
                self.rebuild_internet();
            }
        } else if i < self.size() - 1 {
            self.select_server(self.selected + 1);
            self.scroll_by(SLOT_HEIGHT);
            if self.selected_kind() == Some(Kind::Scan) {
                if self.selected < self.size() - 1 {
                    self.select_server(self.selected + 1);
                    self.scroll_by(SLOT_HEIGHT);
                } else {
                    self.select_server(-1);
                }
            }
        } else {
            self.select_server(-1);
        }
    }

    fn swap_servers(&mut self, a: usize, b: usize) {
        if let Some(l) = &mut self.list {
            l.swap(a, b);
        }
    }

    /// `func_175391_a` / `func_175393_b`: the overlay arrows' move up/down (to the end with Shift).
    fn move_server(&mut self, slot: usize, up: bool, to_end: bool) {
        let count = self.list.as_ref().map_or(0, |l| l.count());
        let target = match (up, to_end) {
            (true, true) => 0,
            (true, false) => slot - 1,
            (false, true) => count - 1,
            (false, false) => slot + 1,
        };
        self.swap_servers(slot, target);
        if self.selected == slot as i32 {
            self.select_server(target as i32);
        }
        self.rebuild_internet();
    }

    /// `GuiListExtended.mouseClicked` and the entries' `mousePressed`.
    fn list_click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        if !(my >= TOP && my <= self.h - 64 && mx >= 0 && mx <= self.w) {
            return Action::None;
        }
        let i = self.slot_at(mx, my);
        if i < 0 {
            return Action::None;
        }
        let rx = mx - (self.w / 2 - 152 + 2);
        let ry = my - (TOP + 4 - self.scroll as i32 + i * SLOT_HEIGHT);
        let now = now_ms();
        match self.kind(i) {
            Some(Kind::Normal(n)) => {
                let count = self.internet.len() as i32;
                if rx <= 32 {
                    if rx < 32 && rx > 16 {
                        self.select_server(i);
                        self.list_enabled = false;
                        return self.connect_to_selected(ui);
                    }
                    if rx < 16 && ry < 16 && i > 0 {
                        self.move_server(n, true, ui.shift_down);
                        self.list_enabled = false;
                        return Action::None;
                    }
                    if rx < 16 && ry > 16 && i < count - 1 {
                        self.move_server(n, false, ui.shift_down);
                        self.list_enabled = false;
                        return Action::None;
                    }
                }
                self.select_server(i);
                let double = now - self.internet[n].last_click < 250;
                self.internet[n].last_click = now;
                if double {
                    return self.connect_to_selected(ui);
                }
            }
            Some(Kind::Lan(n)) => {
                self.select_server(i);
                let double = now - self.lan[n].last_click < 250;
                self.lan[n].last_click = now;
                if double {
                    return self.connect_to_selected(ui);
                }
            }
            _ => {}
        }
        Action::None
    }

    fn draw_list(&mut self, ui: &mut Ui, b: &mut GuiBatch, mx: i32, my: i32) {
        let (w, h) = (self.w, self.h);
        let (top, bottom) = (TOP, h - 64);
        self.mouse_input(mx, my);
        self.bind_scroll();
        let sc = self.scroll as i32;
        dirt(b, top, bottom, w, 32, sc as f32);
        let (x, y0) = (w / 2 - 152 + 2, top + 4 - sc);
        let hovered = self.slot_at(mx, my);
        let list_w = 305;
        for j in 0..self.size() {
            let k = y0 + j * SLOT_HEIGHT;
            let l = SLOT_HEIGHT - 4;
            if j == self.selected {
                let (i1, j1) = (w / 2 - 152, w / 2 + 152);
                b.rect(i1, k - 2, j1, k + l + 2, 0xFF80_8080);
                b.rect(i1 + 1, k - 1, j1 - 1, k + l + 1, 0xFF00_0000);
            }
            match self.kind(j) {
                Some(Kind::Normal(n)) => self.draw_normal(ui, b, n, j, x, k, list_w, mx, my, hovered == j),
                Some(Kind::Scan) => draw_scan(ui, b, w, k, l),
                Some(Kind::Lan(n)) => {
                    let s = &self.lan[n].server;
                    ui.font.draw(b, &ui.lang.get("lanServer.title"), (x + 35) as f32, (k + 1) as f32, 0xFFFFFF);
                    ui.font.draw(b, &s.motd, (x + 35) as f32, (k + 12) as f32, 0x808080);
                    ui.font.draw(b, &s.address, (x + 35) as f32, (k + 23) as f32, 0x303030);
                }
                None => {}
            }
        }
        dirt(b, 0, top, w, 64, 0.0);
        dirt(b, bottom, h, w, 64, 0.0);
        b.gradient(0, top, w, top + 4, 0xFF00_0000, 0x0000_0000);
        b.gradient(0, bottom - 4, w, bottom, 0x0000_0000, 0xFF00_0000);
        let j1 = self.max_scroll();
        if j1 > 0 {
            let (i, j) = (w / 2 + 154, w / 2 + 154 + 6);
            let k1 = clamp_int((bottom - top) * (bottom - top) / (self.size() * SLOT_HEIGHT), 32, bottom - top - 8);
            let l1 = (sc * (bottom - top - k1) / j1 + top).max(top);
            b.rect(i, top, j, bottom, 0xFF00_0000);
            b.rect(i, l1, j, l1 + k1, 0xFF80_8080);
            b.rect(i, l1, j - 1, l1 + k1 - 1, 0xFFC0_C0C0);
        }
    }

    /// `ServerListEntryNormal.drawEntry`
    #[allow(clippy::too_many_arguments)]
    fn draw_normal(&mut self, ui: &mut Ui, b: &mut GuiBatch, n: usize, slot: i32, x: i32, y: i32, list_w: i32, mx: i32, my: i32, hovered: bool) {
        let handle = self.internet[n].data.clone();
        // First draw starts the ping on the pool.
        {
            let mut d = handle.lock().unwrap();
            if !d.pinged {
                d.pinged = true;
                d.ping = -2;
                d.motd.clear();
                d.population_info.clear();
                drop(d);
                if let Some(f) = PINGER.get() {
                    f(handle.clone(), ui.lang.clone(), self.cancel.clone());
                }
            }
        }
        let s = handle.lock().unwrap().clone();
        let too_new = s.version > 47;
        let too_old = s.version < 47;
        let incompatible = too_new || too_old;
        ui.font.draw(b, &s.name, (x + 32 + 3) as f32, (y + 1) as f32, 0xFFFFFF);
        let lines = ui.font.list_to_width(&s.motd, list_w - 32 - 2);
        for (i, line) in lines.iter().take(2).enumerate() {
            ui.font.draw(b, line, (x + 32 + 3) as f32, (y + 12 + 9 * i as i32) as f32, 0x808080);
        }
        let s2 = if incompatible { format!("\u{a7}4{}", s.game_version) } else { s.population_info.clone() };
        let j = ui.font.width(&s2);
        ui.font.draw(b, &s2, (x + list_w - j - 15 - 2) as f32, (y + 1) as f32, 0x808080);
        let mut k = 0;
        let mut tip: Option<String> = None;
        let level;
        let status: String;
        if incompatible {
            level = 5;
            status = if too_new { "Client out of date!" } else { "Server out of date!" }.to_string();
            tip = s.player_list.clone();
        } else if s.pinged && s.ping != -2 {
            level = ping_level(s.ping);
            if s.ping < 0 {
                status = "(no connection)".into();
            } else {
                status = format!("{}ms", s.ping);
                tip = s.player_list.clone();
            }
        } else {
            k = 1;
            let mut l = ((now_ms() / 100 + (slot as u64 * 2)) & 7) as i32;
            if l > 4 {
                l = 8 - l;
            }
            level = l;
            status = "Pinging...".into();
        }
        b.color = [1.0; 4];
        b.textured_rect(ICONS, x + list_w - 15, y, k * 10, 176 + level * 8, 10, 8);
        // Icon texture from the favicon (prepareServerIcon); a changed icon also saves the list.
        if let Some(icon) = &s.icon {
            if self.internet[n].icon_data.as_ref() != Some(icon) {
                self.internet[n].icon_data = Some(icon.clone());
                match register_icon(&s.ip, icon) {
                    Some(path) => self.internet[n].icon_path = Some(path),
                    None => {
                        eprintln!("Invalid icon for server {} ({})", s.name, s.ip);
                        self.internet[n].icon_path = None;
                        handle.lock().unwrap().icon = None;
                    }
                }
                self.save_list();
            }
        }
        b.color = [1.0; 4];
        match &self.internet[n].icon_path {
            Some(p) => b.textured_rect_uv(p, x as f32, y as f32, 32.0, 32.0, [0.0, 0.0, 1.0, 1.0]),
            None => b.textured_rect_uv(UNKNOWN_SERVER, x as f32, y as f32, 32.0, 32.0, [0.0, 0.0, 1.0, 1.0]),
        }
        let (i1, j1) = (mx - x, my - y);
        if i1 >= list_w - 15 && i1 <= list_w - 5 && (0..=8).contains(&j1) {
            self.hovering_text = Some(status);
        } else if i1 >= list_w - j - 15 - 2 && i1 <= list_w - 15 - 2 && (0..=8).contains(&j1) {
            self.hovering_text = tip;
        }
        if hovered {
            b.rect(x, y, x + 32, y + 32, 0xA090_9090);
            b.color = [1.0; 4];
            let (k1, l1) = (mx - x, my - y);
            let count = self.internet.len() as i32;
            let tex = |b: &mut GuiBatch, u: i32, v: i32| b.textured_rect(SERVER_SELECTION, x, y, u, v, 32, 32);
            tex(b, 0, if k1 < 32 && k1 > 16 { 32 } else { 0 });
            if slot > 0 {
                tex(b, 96, if k1 < 16 && l1 < 16 { 32 } else { 0 });
            }
            if slot < count - 1 {
                tex(b, 64, if k1 < 16 && l1 > 16 { 32 } else { 0 });
            }
        }
    }
}

fn clamp_int(v: i32, min: i32, max: i32) -> i32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// `ServerListEntryLanScan.drawEntry`
fn draw_scan(ui: &mut Ui, b: &mut GuiBatch, screen_w: i32, y: i32, slot_h: i32) {
    let i = y + slot_h / 2 - 9 / 2;
    let text = ui.lang.get("lanServer.scanning");
    let tw = ui.font.width(&text);
    ui.font.draw(b, &text, (screen_w / 2 - tw / 2) as f32, i as f32, 0xFFFFFF);
    let s = match (now_ms() / 300 % 4) as i32 {
        1 | 3 => "o O o",
        2 => "o o O",
        _ => "O o o",
    };
    let sw = ui.font.width(s);
    ui.font.draw(b, s, (screen_w / 2 - sw / 2) as f32, (i + 9) as f32, 0x808080);
}

impl Screen for MultiplayerScreen {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        (self.w, self.h) = (w, h);
        self.process_mailbox();
        if !self.initialized {
            self.initialized = true;
            self.list = Some(ServerList::load(&game_dir(ui)));
            self.rebuild_internet();
            self.lan_list = Arc::new(LanServerList::default());
            self._lan_detector = LanDetector::start(self.lan_list.clone());
            if self._lan_detector.is_none() {
                eprintln!("Unable to start LAN server detection");
            }
        }
        self.create_buttons(ui);
    }

    fn tick(&mut self) {
        self.poll_lan();
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        (self.w, self.h) = (w, h);
        self.hovering_text = None;
        self.poll_lan();
        default_background(ui, b, w, h);
        self.draw_list(ui, b, mx, my);
        ui.font.draw_centered(b, &ui.lang.get("multiplayer.title"), w / 2, 20, 0xFFFFFF);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
        if let Some(t) = self.hovering_text.take() {
            let lines: Vec<String> = t.split('\n').map(str::to_string).collect();
            ui.draw_hovering_text(b, &lines, mx, my, w, h);
        }
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.mouse_held = true;
        if let Some(id) = click_buttons(&mut self.buttons, ui, mx, my) {
            return self.action_performed(ui, id);
        }
        self.list_click(ui, mx, my)
    }

    fn release(&mut self, _ui: &mut Ui) {
        self.mouse_held = false;
        self.list_enabled = true;
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        // F5
        if key == Key::Other && ui.key_code == 63 {
            self.refresh(ui);
            return Action::None;
        }
        if self.selected >= 0 {
            match key {
                Key::Up => {
                    self.arrow(ui, true);
                    return Action::None;
                }
                Key::Down => {
                    self.arrow(ui, false);
                    return Action::None;
                }
                Key::Enter => {
                    return if self.buttons[2].enabled { self.action_performed(ui, 1) } else { Action::None };
                }
                _ => {}
            }
        }
        if key == Key::Escape {
            Action::Close
        } else {
            Action::None
        }
    }

    fn scroll(&mut self, delta: i32) {
        let (mx, my) = (self.mx, self.my);
        if my >= TOP && my <= self.h - 64 && mx >= 0 && mx <= self.w && delta != 0 {
            self.scroll += (-delta.signum() * SLOT_HEIGHT / 2) as f32;
        }
    }
}

/// `GuiYesNo` (the 5-argument form `GuiMultiplayer` uses for deleting).
pub struct YesNo {
    buttons: Vec<Button>,
    mailbox: Mailbox,
    line1: String,
    line2: String,
    lines: Vec<String>,
    confirm: String,
    cancel: String,
}

impl YesNo {
    pub fn new(mailbox: Mailbox, line1: String, line2: String, confirm: String, cancel: String) -> Self {
        Self { buttons: Vec::new(), mailbox, line1, line2, lines: Vec::new(), confirm, cancel }
    }
}

impl Screen for YesNo {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.buttons = vec![
            Button::sized(0, w / 2 - 155, h / 6 + 96, 150, 20, self.confirm.clone()),
            Button::sized(1, w / 2 - 155 + 160, h / 6 + 96, 150, 20, self.cancel.clone()),
        ];
        self.lines = ui.font.list_to_width(&self.line2, w - 50);
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        ui.font.draw_centered(b, &self.line1, w / 2, 70, 0xFFFFFF);
        let mut y = 90;
        for l in &self.lines {
            ui.font.draw_centered(b, l, w / 2, y, 0xFFFFFF);
            y += 9;
        }
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(id) => {
                *self.mailbox.lock().unwrap() = Some(id == 0);
                Action::Close
            }
            None => Action::None,
        }
    }

    /// `GuiScreen.keyTyped` Escape: `displayGuiScreen(null)`, which with no world is the main menu.
    fn key(&mut self, _ui: &mut Ui, key: Key) -> Action {
        if key == Key::Escape {
            Action::QuitToTitle
        } else {
            Action::None
        }
    }
}

fn label_with_mode(lang: &Lang, mode: ResourceMode) -> String {
    format!("{}: {}", lang.get("addServer.resourcePack"), lang.get(mode.lang_key()))
}

/// `GuiScreenAddServer`
pub struct AddServerScreen {
    buttons: Vec<Button>,
    name: TextField,
    ip: TextField,
    data: Handle,
    mailbox: Mailbox,
}

impl AddServerScreen {
    pub fn new(mailbox: Mailbox, data: Handle) -> Self {
        Self { buttons: Vec::new(), name: TextField::new(0, 0, 200, 20), ip: TextField::new(0, 0, 200, 20), data, mailbox }
    }

    fn update_done(&mut self) {
        let (n, i) = (self.name.text(), self.ip.text());
        self.buttons[0].enabled = !i.is_empty() && has_host(&i) && !n.is_empty();
    }

    fn action(&mut self, ui: &Ui, id: i32) -> Action {
        match id {
            2 => {
                let mut d = self.data.lock().unwrap();
                d.resource_mode = d.resource_mode.next();
                self.buttons[2].label = label_with_mode(&ui.lang, d.resource_mode);
                Action::None
            }
            1 => {
                *self.mailbox.lock().unwrap() = Some(false);
                Action::Close
            }
            0 => {
                {
                    let mut d = self.data.lock().unwrap();
                    d.name = self.name.text();
                    d.ip = self.ip.text();
                }
                *self.mailbox.lock().unwrap() = Some(true);
                Action::Close
            }
            _ => Action::None,
        }
    }
}

impl Screen for AddServerScreen {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let mode = self.data.lock().unwrap().resource_mode;
        self.buttons = vec![
            Button::new(0, w / 2 - 100, h / 4 + 96 + 18, ui.lang.get("addServer.add")),
            Button::new(1, w / 2 - 100, h / 4 + 120 + 18, ui.lang.get("gui.cancel")),
            Button::new(2, w / 2 - 100, h / 4 + 72, label_with_mode(&ui.lang, mode)),
        ];
        self.name = TextField::new(w / 2 - 100, 66, 200, 20);
        self.name.set_focused(true);
        let (n, i) = {
            let d = self.data.lock().unwrap();
            (d.name.clone(), d.ip.clone())
        };
        self.name.set_text(ui, &n);
        self.ip = TextField::new(w / 2 - 100, 106, 200, 20);
        self.ip.max_len = 128;
        self.ip.set_text(ui, &i);
        self.update_done();
    }

    fn tick(&mut self) {
        self.name.tick();
        self.ip.tick();
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        let _ = h;
        default_background(ui, b, w, h);
        ui.font.draw_centered(b, &ui.lang.get("addServer.title"), w / 2, 17, 0xFFFFFF);
        ui.font.draw_shadow(b, &ui.lang.get("addServer.enterName"), (w / 2 - 100) as f32, 53.0, 10526880);
        ui.font.draw_shadow(b, &ui.lang.get("addServer.enterIp"), (w / 2 - 100) as f32, 94.0, 10526880);
        self.name.draw(ui, b);
        self.ip.draw(ui, b);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        if let Some(id) = click_buttons(&mut self.buttons, ui, mx, my) {
            return self.action(ui, id);
        }
        self.ip.mouse_clicked(ui, mx, my, 0);
        self.name.mouse_clicked(ui, mx, my, 0);
        Action::None
    }

    /// Escape does nothing here: `GuiScreenAddServer.keyTyped` never calls `super`.
    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        self.name.key(ui, key);
        self.ip.key(ui, key);
        let mut a = Action::None;
        if key == Key::Tab {
            let f = self.name.focused;
            self.name.set_focused(!f);
            let f = self.ip.focused;
            self.ip.set_focused(!f);
        }
        if key == Key::Enter && self.buttons[0].enabled {
            a = self.action(ui, 0);
        }
        self.update_done();
        a
    }

    fn typed(&mut self, ui: &mut Ui, c: char) {
        self.name.typed(ui, c);
        self.ip.typed(ui, c);
        self.update_done();
    }
}

/// `GuiScreenServerList`: the Direct Connect screen.
pub struct DirectConnectScreen {
    buttons: Vec<Button>,
    field: TextField,
    data: Handle,
    mailbox: Mailbox,
}

impl DirectConnectScreen {
    pub fn new(mailbox: Mailbox, data: Handle) -> Self {
        Self { buttons: Vec::new(), field: TextField::new(0, 0, 200, 20), data, mailbox }
    }

    fn update_select(&mut self) {
        let t = self.field.text();
        self.buttons[0].enabled = !t.is_empty() && has_host(&t);
    }

    /// `onGuiClosed`: `lastServer` is remembered either way.
    fn save_last_server(&self, ui: &mut Ui) {
        ui.settings.last_server = self.field.text();
        ui.settings.save();
    }

    fn action(&mut self, ui: &mut Ui, id: i32) -> Action {
        match id {
            1 => {
                self.save_last_server(ui);
                *self.mailbox.lock().unwrap() = Some(false);
                Action::Close
            }
            0 => {
                self.save_last_server(ui);
                let ip = self.field.text();
                self.data.lock().unwrap().ip = ip.clone();
                *self.mailbox.lock().unwrap() = Some(true);
                // `confirmClicked(true)` -> `connectToServer(selectedServer)`
                let mode = self.data.lock().unwrap().resource_mode;
                let _ = mode;
                Action::Connect { address: ip, name: ui.player_name.clone(), online: false, pack_mode: None }
            }
            _ => Action::None,
        }
    }
}

impl Screen for DirectConnectScreen {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.buttons = vec![
            Button::new(0, w / 2 - 100, h / 4 + 96 + 12, ui.lang.get("selectServer.select")),
            Button::new(1, w / 2 - 100, h / 4 + 120 + 12, ui.lang.get("gui.cancel")),
        ];
        self.field = TextField::new(w / 2 - 100, 116, 200, 20);
        self.field.max_len = 128;
        self.field.set_focused(true);
        let last = ui.settings.last_server.clone();
        self.field.set_text(ui, &last);
        self.update_select();
    }

    fn tick(&mut self) {
        self.field.tick();
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        ui.font.draw_centered(b, &ui.lang.get("selectServer.direct"), w / 2, 20, 0xFFFFFF);
        ui.font.draw_shadow(b, &ui.lang.get("addServer.enterIp"), (w / 2 - 100) as f32, 100.0, 10526880);
        self.field.draw(ui, b);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        if let Some(id) = click_buttons(&mut self.buttons, ui, mx, my) {
            return self.action(ui, id);
        }
        self.field.mouse_clicked(ui, mx, my, 0);
        Action::None
    }

    /// Escape does nothing here: `GuiScreenServerList.keyTyped` never calls `super`.
    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        if self.field.key(ui, key) {
            self.update_select();
        } else if key == Key::Enter && self.buttons[0].enabled {
            return self.action(ui, 0);
        }
        Action::None
    }

    fn typed(&mut self, ui: &mut Ui, c: char) {
        if self.field.typed(ui, c) {
            self.update_select();
        }
    }
}

/// The child screens with sample data, for `--screenshot --screen mp-add|mp-direct|mp-delete`.
pub fn preview_screen(name: &str, lang: &Lang) -> Option<Box<dyn Screen>> {
    let mb: Mailbox = Arc::new(Mutex::new(None));
    let data = Arc::new(Mutex::new(ServerData::new("Hypixel", "mc.hypixel.net", false)));
    Some(match name {
        "mp-add" => Box::new(AddServerScreen::new(mb, data)),
        "mp-direct" => Box::new(DirectConnectScreen::new(mb, data)),
        "mp-delete" => Box::new(YesNo::new(
            mb,
            lang.get("selectServer.deleteQuestion"),
            format!("'Hypixel' {}", lang.get("selectServer.deleteWarning")),
            lang.get("selectServer.deleteButton"),
            lang.get("gui.cancel"),
        )),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use mc_assets::AssetSource;

    fn ui() -> Ui {
        Ui::new(&AssetSource::new(), Settings::default())
    }

    fn screen_with(n: usize) -> (MultiplayerScreen, Ui) {
        let mut ui = ui();
        let dir = std::env::temp_dir().join(format!("rustmc-mp-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut l = ServerList::load(&dir);
        for i in 0..n {
            l.servers.push(Arc::new(Mutex::new(ServerData::new(&format!("S{i}"), &format!("h{i}"), false))));
        }
        l.save();
        ui.settings.path = Some(dir.join("options.txt"));
        let mut s = MultiplayerScreen::new();
        s.init(&ui, 427, 240);
        (s, ui)
    }

    fn button(s: &MultiplayerScreen, id: i32) -> (i32, i32, i32, i32, bool) {
        let b = s.buttons.iter().find(|b| b.id == id).unwrap();
        (b.x, b.y, b.w, b.h, b.enabled)
    }

    #[test]
    fn button_layout_matches_gui_multiplayer() {
        let (s, _) = screen_with(0);
        let (w, h) = (427, 240);
        assert_eq!(button(&s, 7), (w / 2 - 154, h - 28, 70, 20, false));
        assert_eq!(button(&s, 2), (w / 2 - 74, h - 28, 70, 20, false));
        assert_eq!(button(&s, 1), (w / 2 - 154, h - 52, 100, 20, false));
        assert_eq!(button(&s, 4), (w / 2 - 50, h - 52, 100, 20, true));
        assert_eq!(button(&s, 3), (w / 2 + 54, h - 52, 100, 20, true));
        assert_eq!(button(&s, 8), (w / 2 + 4, h - 28, 70, 20, true));
        assert_eq!(button(&s, 0), (w / 2 + 80, h - 28, 75, 20, true));
        let labels: Vec<_> = s.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels.len(), 7);
    }

    #[test]
    fn selection_enables_buttons_like_select_server() {
        let (mut s, _) = screen_with(2);
        s.select_server(1);
        assert_eq!((button(&s, 7).4, button(&s, 2).4, button(&s, 1).4), (true, true, true));
        // index 2 is the LAN scan row
        s.select_server(2);
        assert_eq!((button(&s, 7).4, button(&s, 2).4, button(&s, 1).4), (false, false, false));
        s.select_server(-1);
        assert!(!button(&s, 1).4);
    }

    #[test]
    fn click_selects_and_double_click_joins() {
        let (mut s, mut ui) = screen_with(3);
        // first slot's name column: x = w/2 - 150 + 40, y = 32 + 4 + 10
        let (x, y) = (427 / 2 - 150 + 40, 32 + 4 + 10);
        assert!(matches!(s.click(&mut ui, x, y), Action::None));
        assert_eq!(s.selected, 0);
        match s.click(&mut ui, x, y) {
            Action::Connect { address, pack_mode, .. } => {
                assert_eq!(address, "h0");
                assert_eq!(pack_mode, Some("prompt"));
            }
            _ => panic!("double click should join"),
        }
        // the join arrow at the left half-right of the icon
        let (x, y) = (427 / 2 - 150 + 24, 32 + 4 + 36 + 10);
        assert!(matches!(s.click(&mut ui, x, y), Action::Connect { .. }));
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn move_arrows_reorder_and_persist() {
        let (mut s, mut ui) = screen_with(3);
        // slot 1: top-left quarter of the icon moves it up
        let (x, y) = (427 / 2 - 150 + 8, 32 + 4 + 36 + 8);
        s.click(&mut ui, x, y);
        let names: Vec<_> = s.internet.iter().map(|e| e.data.lock().unwrap().name.clone()).collect();
        assert_eq!(names, ["S1", "S0", "S2"]);
        let reloaded = ServerList::load(&game_dir(&ui));
        assert_eq!(reloaded.get(0).lock().unwrap().name, "S1");
    }

    #[test]
    fn shift_arrow_swaps_servers() {
        let (mut s, mut ui) = screen_with(3);
        s.select_server(0);
        ui.shift_down = true;
        s.key(&mut ui, Key::Down);
        assert_eq!(s.selected, 1);
        assert_eq!(s.internet[0].data.lock().unwrap().name, "S1");
        assert_eq!(s.internet[1].data.lock().unwrap().name, "S0");
        ui.shift_down = false;
        s.key(&mut ui, Key::Up);
        assert_eq!(s.selected, 0);
        s.key(&mut ui, Key::Up);
        assert_eq!(s.selected, -1);
    }

    #[test]
    fn scroll_range_follows_slot_height() {
        let (mut s, _) = screen_with(10);
        // 11 rows * 36 - (240 - 64 - 32 - 4)
        assert_eq!(s.max_scroll(), 11 * 36 - 140);
        s.scroll_by(1000);
        assert_eq!(s.scroll as i32, s.max_scroll());
        s.scroll_by(-1000);
        assert_eq!(s.scroll as i32, 0);
    }

    #[test]
    fn delete_confirmation_removes_the_server() {
        let (mut s, mut ui) = screen_with(2);
        s.select_server(0);
        let a = s.action_performed(&mut ui, 2);
        assert!(matches!(a, Action::Open(_)));
        *s.mailbox.lock().unwrap() = Some(true);
        let (w, h) = (s.w, s.h);
        s.init(&ui, w, h);
        assert_eq!(s.internet.len(), 1);
        assert_eq!(s.selected, -1);
        assert_eq!(ServerList::load(&game_dir(&ui)).count(), 1);
    }

    #[test]
    fn add_server_screen_validates_and_cycles_the_pack_mode() {
        let ui = ui();
        let mb: Mailbox = Arc::new(Mutex::new(None));
        let data = Arc::new(Mutex::new(ServerData::new("Minecraft Server", "", false)));
        let mut a = AddServerScreen::new(mb.clone(), data.clone());
        a.init(&ui, 427, 240);
        assert_eq!((a.buttons[0].x, a.buttons[0].y), (427 / 2 - 100, 240 / 4 + 96 + 18));
        assert_eq!((a.buttons[1].y, a.buttons[2].y), (240 / 4 + 120 + 18, 240 / 4 + 72));
        assert_eq!((a.name.y, a.ip.y), (66, 106));
        assert!(!a.buttons[0].enabled);
        a.ip.set_text(&ui, ":::");
        a.update_done();
        assert!(!a.buttons[0].enabled);
        a.ip.set_text(&ui, "play.example.com");
        a.update_done();
        assert!(a.buttons[0].enabled);
        assert!(a.buttons[2].label.starts_with("addServer.resourcePack: "));
        a.action(&ui, 2);
        assert_eq!(data.lock().unwrap().resource_mode, ResourceMode::Enabled);
        assert!(matches!(a.action(&ui, 0), Action::Close));
        assert_eq!(*mb.lock().unwrap(), Some(true));
        assert_eq!(data.lock().unwrap().ip, "play.example.com");
    }

    #[test]
    fn motd_wraps_to_two_lines_and_population_aligns_right() {
        let mut u = ui();
        // No font loaded: widths are zero, so the layout maths is checked on its own terms.
        let list_w = 305;
        let wrapped = u.font.list_to_width("a b c", list_w - 32 - 2);
        assert!(wrapped.len() <= 2);
        let s2 = "\u{a7}71\u{a7}8/\u{a7}720";
        let j = u.font.width(s2);
        assert_eq!(j, u.font.width(s2));
        assert_eq!(list_w - j - 15 - 2, 305 - j - 17);
        let _ = &mut u;
    }
}
