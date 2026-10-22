//! `GuiIngame`: crosshair, hotbar, player stats, XP bar, chat and the F3 overlay.

use crate::batch::{Blend, GuiBatch};
use crate::font::FONT_HEIGHT;
use crate::widgets::WIDGETS;
use crate::{Scaled, Ui};

pub const ICONS: &str = "textures/gui/icons.png";

/// Player values the HUD shows. Without a server these are survival defaults.
#[derive(Clone, Debug)]
pub struct PlayerHud {
    pub health: f32,
    pub max_health: f32,
    pub food: i32,
    pub armor: i32,
    pub experience: f32,
    pub level: i32,
    pub selected_slot: usize,
    pub survival: bool,
    /// `getAbsorptionAmount` (data watcher 17).
    pub absorption: f32,
    pub hurt_resistant_time: i32,
    pub regeneration: bool,
    pub poison: bool,
    pub wither: bool,
    pub hunger: bool,
    pub hardcore: bool,
    /// `FoodStats.getSaturationLevel`
    pub saturation: f32,
    /// `getAir` (data watcher 1).
    pub air: i32,
    /// `isInsideOfMaterial(Material.water)`
    pub eye_in_water: bool,
}

impl Default for PlayerHud {
    fn default() -> Self {
        Self {
            health: 20.0,
            max_health: 20.0,
            food: 20,
            armor: 0,
            experience: 0.0,
            level: 0,
            selected_slot: 0,
            survival: true,
            absorption: 0.0,
            hurt_resistant_time: 0,
            regeneration: false,
            poison: false,
            wither: false,
            hunger: false,
            hardcore: false,
            saturation: 5.0,
            air: 300,
            eye_in_water: false,
        }
    }
}

/// `Minecraft.getSystemTime`
fn system_time_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Numbers the F3 screen prints, gathered by the client each frame.
#[derive(Clone, Debug, Default)]
pub struct DebugInfo {
    pub fps: u32,
    pub chunk_updates: u32,
    pub chunks_drawn: usize,
    pub chunks_total: usize,
    pub render_distance: i32,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// `(sky, block)` light at the player's feet.
    pub light: (u8, u8),
    pub display: (u32, u32),
    pub gpu: String,
    pub backend: String,
    pub vsync: bool,
    pub limit_framerate: i32,
}

/// `GuiIngame.renderScoreboard` input: the objective title and (formatted name, points),
/// lowest score first.
#[derive(Clone, Debug, Default)]
pub struct Sidebar {
    pub title: String,
    pub lines: Vec<(String, i32)>,
}

#[derive(Clone, Debug)]
pub struct TabEntry {
    /// `getPlayerName`: the list display name, else the team-formatted profile name.
    pub name: String,
    pub ping: i32,
    pub spectator: bool,
    /// The list objective's points (display slot 0), if one is shown.
    pub score: Option<i32>,
}

/// `GuiPlayerTabOverlay.renderPlayerlist` input, already in `PlayerComparator` order.
#[derive(Clone, Debug, Default)]
pub struct TabList {
    pub entries: Vec<TabEntry>,
    pub hearts: bool,
    pub header: Option<String>,
    pub footer: Option<String>,
}

/// `titleFadeIn` / `titleDisplayTime` / `titleFadeOut`
#[derive(Clone, Copy, Debug)]
struct TitleTimes(i32, i32, i32);
impl Default for TitleTimes {
    /// `setDefaultTitlesTimes`
    fn default() -> Self {
        Self(10, 70, 20)
    }
}

struct ChatLine {
    text: String,
    /// `updateCounter` when added.
    tick: i32,
}

#[derive(Default)]
pub struct Hud {
    chat: Vec<ChatLine>,
    /// Integer client ticks (`updateCounter`).
    pub update_counter: i32,
    /// `GuiIngame.recordPlaying` / `recordPlayingUpFor`: the action bar message.
    action_bar: String,
    action_bar_time: i32,
    // renderPlayerStats damage flashing
    player_health: i32,
    last_player_health: i32,
    last_system_time: u64,
    health_update_counter: i64,
    displayed_title: String,
    displayed_subtitle: String,
    titles_timer: i32,
    title_times: TitleTimes,
    /// `renderSelectedItem`: the held stack's name and `remainingHighlightTicks`.
    pub highlight_name: String,
    pub highlight_ticks: i32,
    /// Set by the client each frame from its scoreboard.
    pub sidebar: Option<Sidebar>,
    /// `Some` while the player list key is held (and the list is worth showing).
    pub tab_list: Option<TabList>,
}

impl Hud {
    /// `GuiNewChat.printChatMessage`: newest first.
    pub fn add_chat(&mut self, text: impl Into<String>) {
        self.chat.insert(0, ChatLine { text: text.into(), tick: self.update_counter });
        self.chat.truncate(100);
    }

    /// `GuiIngame.setRecordPlaying(message, false)`
    pub fn set_action_bar(&mut self, text: String) {
        self.action_bar = text;
        self.action_bar_time = 60;
    }

    /// `GuiIngame.displayTitle`
    pub fn display_title(&mut self, title: Option<String>, subtitle: Option<String>, fade_in: i32, stay: i32, fade_out: i32) {
        let TitleTimes(fi, st, fo) = &mut self.title_times;
        if title.is_none() && subtitle.is_none() && fade_in < 0 && stay < 0 && fade_out < 0 {
            self.displayed_title.clear();
            self.displayed_subtitle.clear();
            self.titles_timer = 0;
        } else if let Some(t) = title {
            self.displayed_title = t;
            self.titles_timer = *fi + *st + *fo;
        } else if let Some(s) = subtitle {
            self.displayed_subtitle = s;
        } else {
            if fade_in >= 0 {
                *fi = fade_in;
            }
            if stay >= 0 {
                *st = stay;
            }
            if fade_out >= 0 {
                *fo = fade_out;
            }
            if self.titles_timer > 0 {
                self.titles_timer = *fi + *st + *fo;
            }
        }
    }

    /// `setDefaultTitlesTimes`
    pub fn default_title_times(&mut self) {
        self.title_times = TitleTimes::default();
    }

    /// `GuiIngame.updateTick` (the held-item highlight is counted by the client).
    pub fn tick(&mut self) {
        if self.action_bar_time > 0 {
            self.action_bar_time -= 1;
        }
        if self.titles_timer > 0 {
            self.titles_timer -= 1;
            if self.titles_timer <= 0 {
                self.displayed_title.clear();
                self.displayed_subtitle.clear();
            }
        }
        self.update_counter += 1;
    }

    /// `renderGameOverlay`. `hotbar_items` draws the hotbar stacks (`renderHotbarItem`).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        ui: &mut Ui,
        b: &mut GuiBatch,
        sr: Scaled,
        p: &PlayerHud,
        debug: &DebugInfo,
        chat_open: bool,
        hotbar_items: &mut dyn FnMut(&mut Ui, &mut GuiBatch),
    ) {
        let (w, h) = (sr.width, sr.height);
        if !ui.settings.hide_gui {
            self.hotbar(b, sr, p);
            hotbar_items(ui, b);
            // Crosshair: inverted against the world behind it.
            b.color = [1.0; 4];
            if !ui.settings.show_debug {
                b.blend = Blend::Invert;
                b.textured_rect(ICONS, w / 2 - 7, h / 2 - 7, 0, 0, 16, 16);
                b.blend = Blend::Alpha;
            }
            if p.survival {
                self.player_stats(b, sr, p);
                self.exp_bar(ui, b, sr, p, w / 2 - 91);
            }
            if ui.settings.held_item_tooltips {
                self.selected_item(ui, b, sr, p);
            }
        }
        if ui.settings.show_debug {
            debug_overlay(ui, b, sr, debug);
        }
        if !ui.settings.hide_gui && self.action_bar_time > 0 {
            let pt = ui.ticks - self.update_counter as f32;
            let f2 = self.action_bar_time as f32 - pt;
            let l1 = ((f2 * 255.0 / 20.0) as i32).min(255);
            if l1 > 8 {
                b.blend = Blend::Alpha;
                let x = w / 2 - ui.font.width(&self.action_bar) / 2;
                let text = self.action_bar.clone();
                ui.font.draw(b, &text, x as f32, (h - 68 - 4) as f32, 0xFFFFFF + ((l1 as u32) << 24 & 0xFF000000));
            }
        }
        if !ui.settings.hide_gui || chat_open {
            let pt = ui.ticks - self.update_counter as f32;
            self.titles(ui, b, sr, pt);
            if let Some(s) = self.sidebar.take() {
                render_scoreboard(ui, b, sr, &s);
                self.sidebar = Some(s);
            }
            self.draw_chat(ui, b, h, chat_open);
            if let Some(t) = self.tab_list.take() {
                render_player_list(ui, b, w, &t);
                self.tab_list = Some(t);
            }
        }
    }

    /// `renderSelectedItem`
    fn selected_item(&self, ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud) {
        if self.highlight_ticks <= 0 || self.highlight_name.is_empty() {
            return;
        }
        let s = &self.highlight_name;
        let i = (sr.width - ui.font.width(s)) / 2;
        let mut j = sr.height - 59;
        if !p.survival {
            j += 14;
        }
        let k = ((self.highlight_ticks as f32 * 256.0 / 10.0) as i32).min(255);
        if k > 0 {
            b.blend = Blend::Alpha;
            let s = s.clone();
            ui.font.draw_shadow(b, &s, i as f32, j as f32, 0xFFFFFF_u32.wrapping_add((k as u32) << 24));
        }
    }

    /// The title and subtitle (`titlesTimer`), scaled 4× and 2× about the screen centre.
    fn titles(&self, ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, pt: f32) {
        if self.titles_timer <= 0 {
            return;
        }
        let TitleTimes(fade_in, stay, fade_out) = self.title_times;
        let f3 = self.titles_timer as f32 - pt;
        let mut i2 = 255;
        if self.titles_timer > fade_out + stay {
            let f4 = (fade_in + stay + fade_out) as f32 - f3;
            i2 = (f4 * 255.0 / fade_in as f32) as i32;
        }
        if self.titles_timer <= fade_out {
            i2 = (f3 * 255.0 / fade_out as f32) as i32;
        }
        let i2 = i2.clamp(0, 255);
        if i2 <= 8 {
            return;
        }
        let color = 0xFFFFFF | ((i2 as u32) << 24 & 0xFF000000);
        let centre = [(sr.width / 2) as f32, (sr.height / 2) as f32];
        let saved = b.transform;
        b.blend = Blend::Alpha;
        b.transform = Some((4.0, centre));
        let t = self.displayed_title.clone();
        ui.font.draw_shadow(b, &t, (-ui.font.width(&t) / 2) as f32, -10.0, color);
        b.transform = Some((2.0, centre));
        let t = self.displayed_subtitle.clone();
        ui.font.draw_shadow(b, &t, (-ui.font.width(&t) / 2) as f32, 5.0, color);
        b.transform = saved;
    }

    /// `renderTooltip` (the hotbar)
    fn hotbar(&self, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud) {
        b.color = [1.0; 4];
        let i = sr.width / 2;
        b.textured_rect(WIDGETS, i - 91, sr.height - 22, 0, 0, 182, 22);
        b.textured_rect(WIDGETS, i - 91 - 1 + p.selected_slot as i32 * 20, sr.height - 22 - 1, 0, 22, 24, 22);
    }

    /// `renderPlayerStats` (no mount health: riding is not modelled yet).
    fn player_stats(&mut self, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud) {
        use mc_core::math::ceiling_float_int;
        let i = ceiling_float_int(p.health);
        let flag = self.health_update_counter > self.update_counter as i64 && (self.health_update_counter - self.update_counter as i64) / 3 % 2 == 1;
        let now = system_time_ms();
        if i < self.player_health && p.hurt_resistant_time > 0 {
            self.last_system_time = now;
            self.health_update_counter = (self.update_counter + 20) as i64;
        } else if i > self.player_health && p.hurt_resistant_time > 0 {
            self.last_system_time = now;
            self.health_update_counter = (self.update_counter + 10) as i64;
        }
        if now.saturating_sub(self.last_system_time) > 1000 {
            self.player_health = i;
            self.last_player_health = i;
            self.last_system_time = now;
        }
        self.player_health = i;
        let j = self.last_player_health;
        let mut rng = JavaRandom::new(self.update_counter.wrapping_mul(312871) as i64);
        // flag1 is never set in 1.8.9, so the previous food level never flashes.
        let k = p.food;
        let i1 = sr.width / 2 - 91;
        let j1 = sr.width / 2 + 91;
        let k1 = sr.height - 39;
        let f = p.max_health;
        let f1 = p.absorption;
        let l1 = ceiling_float_int((f + f1) / 2.0 / 10.0);
        let i2 = (10 - (l1 - 2)).max(3);
        let j2 = k1 - (l1 - 1) * i2 - 10;
        let mut f2 = f1;
        let k2 = p.armor;
        let l2 = if p.regeneration { self.update_counter % ceiling_float_int(f + 5.0) } else { -1 };
        for i3 in 0..10 {
            if k2 > 0 {
                let j3 = i1 + i3 * 8;
                if i3 * 2 + 1 < k2 {
                    b.textured_rect(ICONS, j3, j2, 34, 9, 9, 9);
                }
                if i3 * 2 + 1 == k2 {
                    b.textured_rect(ICONS, j3, j2, 25, 9, 9, 9);
                }
                if i3 * 2 + 1 > k2 {
                    b.textured_rect(ICONS, j3, j2, 16, 9, 9, 9);
                }
            }
        }
        for i6 in (0..ceiling_float_int((f + f1) / 2.0)).rev() {
            let j6 = if p.poison {
                16 + 36
            } else if p.wither {
                16 + 72
            } else {
                16
            };
            let k3 = flag as i32;
            let l3 = ceiling_float_int((i6 + 1) as f32 / 10.0) - 1;
            let i4 = i1 + i6 % 10 * 8;
            let mut j4 = k1 - l3 * i2;
            if i <= 4 {
                j4 += rng.next_int(2);
            }
            if i6 == l2 {
                j4 -= 2;
            }
            let k4 = if p.hardcore { 5 } else { 0 };
            b.textured_rect(ICONS, i4, j4, 16 + k3 * 9, 9 * k4, 9, 9);
            if flag {
                if i6 * 2 + 1 < j {
                    b.textured_rect(ICONS, i4, j4, j6 + 54, 9 * k4, 9, 9);
                }
                if i6 * 2 + 1 == j {
                    b.textured_rect(ICONS, i4, j4, j6 + 63, 9 * k4, 9, 9);
                }
            }
            if f2 > 0.0 {
                if f2 == f1 && f1 % 2.0 == 1.0 {
                    b.textured_rect(ICONS, i4, j4, j6 + 153, 9 * k4, 9, 9);
                } else {
                    b.textured_rect(ICONS, i4, j4, j6 + 144, 9 * k4, 9, 9);
                }
                f2 -= 2.0;
            } else {
                if i6 * 2 + 1 < i {
                    b.textured_rect(ICONS, i4, j4, j6 + 36, 9 * k4, 9, 9);
                }
                if i6 * 2 + 1 == i {
                    b.textured_rect(ICONS, i4, j4, j6 + 45, 9 * k4, 9, 9);
                }
            }
        }
        for k6 in 0..10 {
            let mut i7 = k1;
            let (l7, j8) = if p.hunger { (16 + 36, 13) } else { (16, 0) };
            if p.saturation <= 0.0 && self.update_counter % (k * 3 + 1) == 0 {
                i7 = k1 + (rng.next_int(3) - 1);
            }
            let i9 = j1 - k6 * 8 - 9;
            b.textured_rect(ICONS, i9, i7, 16 + j8 * 9, 27, 9, 9);
            if k6 * 2 + 1 < k {
                b.textured_rect(ICONS, i9, i7, l7 + 36, 27, 9, 9);
            }
            if k6 * 2 + 1 == k {
                b.textured_rect(ICONS, i9, i7, l7 + 45, 27, 9, 9);
            }
        }
        if p.eye_in_water {
            let l6 = p.air;
            let k7 = mc_core::math::ceiling_double_int((l6 - 2) as f64 * 10.0 / 300.0);
            let i8 = mc_core::math::ceiling_double_int(l6 as f64 * 10.0 / 300.0) - k7;
            for l8 in 0..k7 + i8 {
                let u = if l8 < k7 { 16 } else { 25 };
                b.textured_rect(ICONS, j1 - l8 * 8 - 9, j2, u, 18, 9, 9);
            }
        }
    }

    /// `renderExpBar`
    fn exp_bar(&self, ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud, x: i32) {
        let j = 182;
        let k = (p.experience * (j + 1) as f32) as i32;
        let l = sr.height - 32 + 3;
        b.textured_rect(ICONS, x, l, 0, 64, j, 5);
        if k > 0 {
            b.textured_rect(ICONS, x, l, 0, 69, k, 5);
        }
        if p.level > 0 {
            let s = p.level.to_string();
            let l1 = (sr.width - ui.font.width(&s)) / 2;
            let i1 = sr.height - 31 - 4;
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                ui.font.draw(b, &s, (l1 + dx) as f32, (i1 + dy) as f32, 0);
            }
            ui.font.draw(b, &s, l1 as f32, i1 as f32, 8453920);
        }
    }

    /// `GuiNewChat.drawChat`: visibility, opacity, scale, width and line count from the chat options.
    fn draw_chat(&self, ui: &mut Ui, b: &mut GuiBatch, screen_h: i32, chat_open: bool) {
        let st = &ui.settings;
        if st.chat_visibility == 2 {
            return;
        }
        // calculateChatboxHeight / calculateChatboxWidth
        let height = (if chat_open { st.chat_height_focused } else { st.chat_height_unfocused } * 160.0 + 20.0).floor() as i32;
        let max_lines = (height / 9) as usize;
        let opacity = st.chat_opacity * 0.9 + 0.1;
        let scale = st.chat_scale;
        let width = ((st.chat_width * 280.0 + 40.0).floor() as f32 / scale).ceil() as i32;
        let saved = b.transform;
        b.transform = Some((scale, [2.0, (screen_h - 48 + 20) as f32]));
        for (l1, line) in self.chat.iter().take(max_lines).enumerate() {
            let age = self.update_counter - line.tick;
            if age >= 200 && !chat_open {
                continue;
            }
            let mut d0 = (age as f64 / 200.0).clamp(0.0, 1.0);
            d0 = 1.0 - d0;
            d0 *= 10.0;
            d0 = d0.clamp(0.0, 1.0);
            d0 *= d0;
            let mut l2 = (255.0 * d0) as i32;
            if chat_open {
                l2 = 255;
            }
            l2 = (l2 as f32 * opacity) as i32;
            if l2 > 3 {
                let i2 = -(l1 as i32) * 9;
                b.rect(0, i2 - 9, width + 4, i2, ((l2 / 2) as u32) << 24);
                ui.font.draw_shadow(b, &line.text, 0.0, (i2 - 8) as f32, 16777215 + ((l2 as u32) << 24));
            }
        }
        b.transform = saved;
    }
}

/// `GuiIngame.renderScoreboard`
fn render_scoreboard(ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, s: &Sidebar) {
    let fh = FONT_HEIGHT;
    let mut i = ui.font.width(&s.title);
    for (name, points) in &s.lines {
        i = i.max(ui.font.width(&format!("{name}: \u{a7}c{points}")));
    }
    let i1 = s.lines.len() as i32 * fh;
    let j1 = sr.height / 2 + i1 / 3;
    let k1 = 3;
    let l1 = sr.width - i - k1;
    for (j, (name, points)) in s.lines.iter().enumerate() {
        let j = j as i32 + 1;
        let s2 = format!("\u{a7}c{points}");
        let k = j1 - j * fh;
        let l = sr.width - k1 + 2;
        b.rect(l1 - 2, k, l, k + fh, 1342177280);
        // The text color's 0x20 alpha is moot: drawRect leaves blending off, so it draws opaque.
        ui.font.draw(b, name, l1 as f32, k as f32, 0xFFFFFFFF);
        let w2 = ui.font.width(&s2);
        ui.font.draw(b, &s2, (l - w2) as f32, k as f32, 0xFFFFFFFF);
        if j == s.lines.len() as i32 {
            b.rect(l1 - 2, k - fh - 1, l, k - 1, 1610612736);
            b.rect(l1 - 2, k - 1, l, k, 1342177280);
            let tw = ui.font.width(&s.title);
            ui.font.draw(b, &s.title, (l1 + i / 2 - tw / 2) as f32, (k - fh) as f32, 0xFFFFFFFF);
        }
    }
}

/// `GuiPlayerTabOverlay.renderPlayerlist` (no skin heads: they need an encrypted connection or
/// the integrated server).
fn render_player_list(ui: &mut Ui, b: &mut GuiBatch, width: i32, t: &TabList) {
    let fh = FONT_HEIGHT;
    let mut i = 0;
    let mut j = 0;
    for e in &t.entries {
        i = i.max(ui.font.width(&e.name));
        if let (Some(v), false) = (e.score, t.hearts) {
            j = j.max(ui.font.width(&format!(" {v}")));
        }
    }
    let list = &t.entries[..t.entries.len().min(80)];
    let l3 = list.len() as i32;
    let mut i4 = l3;
    let mut j4 = 1;
    while i4 > 20 {
        j4 += 1;
        i4 = (l3 + j4 - 1) / j4;
    }
    let heads = false;
    let has_objective = list.iter().any(|e| e.score.is_some());
    let l = if has_objective {
        if t.hearts {
            90
        } else {
            j
        }
    } else {
        0
    };
    let i1 = (j4 * ((if heads { 9 } else { 0 }) + i + l + 13)).min(width - 50) / j4;
    let j1 = width / 2 - (i1 * j4 + (j4 - 1) * 5) / 2;
    let mut k1 = 10;
    let mut l1 = i1 * j4 + (j4 - 1) * 5;
    let header = t.header.as_ref().map(|h| ui.font.list_to_width(h, width - 50));
    let footer = t.footer.as_ref().map(|f| ui.font.list_to_width(f, width - 50));
    for s in header.iter().chain(footer.iter()).flatten() {
        l1 = l1.max(ui.font.width(s));
    }
    b.blend = Blend::Alpha;
    if let Some(lines) = &header {
        b.rect(width / 2 - l1 / 2 - 1, k1 - 1, width / 2 + l1 / 2 + 1, k1 + lines.len() as i32 * fh, i32::MIN as u32);
        for s in lines {
            let w = ui.font.width(s);
            ui.font.draw_shadow(b, s, (width / 2 - w / 2) as f32, k1 as f32, 0xFFFFFFFF);
            k1 += fh;
        }
        k1 += 1;
    }
    b.rect(width / 2 - l1 / 2 - 1, k1 - 1, width / 2 + l1 / 2 + 1, k1 + i4 * 9, i32::MIN as u32);
    for k4 in 0..l3 {
        let l4 = k4 / i4;
        let i5 = k4 % i4;
        let j2 = j1 + l4 * i1 + l4 * 5;
        let k2 = k1 + i5 * 9;
        b.rect(j2, k2, j2 + i1, k2 + 8, 553648127);
        b.color = [1.0; 4];
        let e = &list[k4 as usize];
        if e.spectator {
            let s1 = format!("\u{a7}o{}", e.name);
            ui.font.draw_shadow(b, &s1, j2 as f32, k2 as f32, 0x90FFFFFF);
        } else {
            ui.font.draw_shadow(b, &e.name, j2 as f32, k2 as f32, 0xFFFFFFFF);
        }
        if let (Some(v), false) = (e.score, e.spectator) {
            let k5 = j2 + i + 1;
            let l5 = k5 + l;
            if l5 - k5 > 5 {
                scoreboard_value(ui, b, t.hearts, v, k2, k5, l5);
            }
        }
        // drawPing
        let jj = match e.ping {
            p if p < 0 => 5,
            p if p < 150 => 0,
            p if p < 300 => 1,
            p if p < 600 => 2,
            p if p < 1000 => 3,
            _ => 4,
        };
        b.textured_rect(ICONS, j2 + i1 - 11, k2, 0, 176 + jj * 8, 10, 8);
    }
    if let Some(lines) = &footer {
        k1 = k1 + i4 * 9 + 1;
        b.rect(width / 2 - l1 / 2 - 1, k1 - 1, width / 2 + l1 / 2 + 1, k1 + lines.len() as i32 * fh, i32::MIN as u32);
        for s in lines {
            let w = ui.font.width(s);
            ui.font.draw_shadow(b, s, (width / 2 - w / 2) as f32, k1 as f32, 0xFFFFFFFF);
            k1 += fh;
        }
    }
}

/// `drawScoreboardValues` (hearts without the change flash: that needs per-player history).
fn scoreboard_value(ui: &mut Ui, b: &mut GuiBatch, hearts: bool, i: i32, y: i32, x0: i32, x1: i32) {
    if !hearts {
        let s1 = format!("\u{a7}e{i}");
        let w = ui.font.width(&s1);
        ui.font.draw_shadow(b, &s1, (x1 - w) as f32, y as f32, 16777215);
        return;
    }
    let last = i;
    let j = ((i.max(last) as f32) / 2.0).ceil() as i32;
    let k = ((i / 2) as f32).ceil().max(((last / 2) as f32).ceil()).max(10.0) as i32;
    if j <= 0 {
        return;
    }
    let f = ((x1 - x0 - 4) as f32 / k as f32).min(9.0);
    if f > 3.0 {
        let rect = |b: &mut GuiBatch, x: f32, u: i32| {
            const F: f32 = 0.003_906_25;
            b.textured_rect_uv(ICONS, x, y as f32, 9.0, 9.0, [u as f32 * F, 0.0, (u + 9) as f32 * F, 9.0 * F]);
        };
        for l in j..k {
            rect(b, x0 as f32 + l as f32 * f, 16);
        }
        for j1 in 0..j {
            let x = x0 as f32 + j1 as f32 * f;
            rect(b, x, 16);
            if j1 * 2 + 1 < i {
                rect(b, x, if j1 >= 10 { 160 } else { 52 });
            }
            if j1 * 2 + 1 == i {
                rect(b, x, if j1 >= 10 { 169 } else { 61 });
            }
        }
    } else {
        let f1 = (i as f32 / 20.0).clamp(0.0, 1.0);
        let i1 = (((1.0 - f1) * 255.0) as i32) << 16 | ((f1 * 255.0) as i32) << 8;
        let mut s = format!("{}", java_float(i as f32 / 2.0));
        if x1 - ui.font.width(&format!("{s}hp")) >= x0 {
            s.push_str("hp");
        }
        let w = ui.font.width(&s);
        ui.font.draw_shadow(b, &s, ((x1 + x0) / 2 - w / 2) as f32, y as f32, i1 as u32);
    }
}

/// Java `Float.toString` for the half-integers the heart score prints.
fn java_float(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// `GuiOverlayDebug`
fn debug_overlay(ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, d: &DebugInfo) {
    let [x, y, z] = d.pos;
    let (bx, by, bz) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let facing = mc_core::Facing::from_horizontal_index(mc_core::math::floor_double((d.yaw * 4.0 / 360.0) as f64 + 0.5) & 3);
    let towards = match facing {
        mc_core::Facing::North => "Towards negative Z",
        mc_core::Facing::South => "Towards positive Z",
        mc_core::Facing::West => "Towards negative X",
        mc_core::Facing::East => "Towards positive X",
        _ => "Invalid",
    };
    let limit = if d.limit_framerate >= 260 { "inf".to_string() } else { d.limit_framerate.to_string() };
    let left = [
        "Minecraft 1.8.9 (1.8.9/vanilla)".to_string(),
        format!(
            "{} fps ({} chunk update{}) T: {}{}",
            d.fps,
            d.chunk_updates,
            if d.chunk_updates != 1 { "s" } else { "" },
            limit,
            if d.vsync { " vsync" } else { "" }
        ),
        format!("C: {}/{} D: {}, L: 0", d.chunks_drawn, d.chunks_total, d.render_distance),
        "E: 0/0, B: 0, I: 0".to_string(),
        "P: 0. T: All: 0".to_string(),
        format!("MultiplayerChunkCache: {}, {}", d.chunks_total, d.chunks_total),
        String::new(),
        format!("XYZ: {:.3} / {:.5} / {:.3}", x, y, z),
        format!("Block: {bx} {by} {bz}"),
        format!("Chunk: {} {} {} in {} {} {}", bx & 15, by & 15, bz & 15, bx >> 4, by >> 4, bz >> 4),
        format!(
            "Facing: {} ({}) ({:.1} / {:.1})",
            facing.name(),
            towards,
            mc_core::math::wrap_angle_to_180_float(d.yaw),
            mc_core::math::wrap_angle_to_180_float(d.pitch)
        ),
        "Biome: Plains".to_string(),
        format!("Light: {} ({} sky, {} block)", d.light.0.max(d.light.1), d.light.0, d.light.1),
        "Local Difficulty: 0.00 (Day 0)".to_string(),
    ];
    for (i, s) in left.iter().enumerate() {
        if s.is_empty() {
            continue;
        }
        let k = ui.font.width(s);
        let i1 = 2 + FONT_HEIGHT * i as i32;
        b.rect(1, i1 - 1, 2 + k + 1, i1 + FONT_HEIGHT - 1, 0x90505050);
        ui.font.draw(b, s, 2.0, i1 as f32, 14737632);
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    let right = [
        format!("Rust: {} 64bit", env!("CARGO_PKG_VERSION")),
        String::new(),
        String::new(),
        String::new(),
        format!("CPU: {cpus}x"),
        String::new(),
        format!("Display: {}x{} ({})", d.display.0, d.display.1, d.backend),
        d.gpu.clone(),
    ];
    for (i, s) in right.iter().enumerate() {
        if s.is_empty() {
            continue;
        }
        let k = ui.font.width(s);
        let l = sr.width - 2 - k;
        let i1 = 2 + FONT_HEIGHT * i as i32;
        b.rect(l - 1, i1 - 1, l + k + 1, i1 + FONT_HEIGHT - 1, 0x90505050);
        ui.font.draw(b, s, l as f32, i1 as f32, 14737632);
    }
}

/// `java.util.Random`, for HUD jitter that should match vanilla frame for frame.
pub struct JavaRandom(i64);

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self((seed ^ 0x5DEECE66D) & ((1 << 48) - 1))
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self, bound: i32) -> i32 {
        if bound & -bound == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits - val + (bound - 1) >= 0 {
                return val;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_known_sequence() {
        // new Random(42).nextInt(10) == 0, then 3
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(10), 0);
        assert_eq!(r.next_int(10), 3);
    }
}
