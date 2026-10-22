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
}

impl Default for PlayerHud {
    fn default() -> Self {
        Self { health: 20.0, max_health: 20.0, food: 20, armor: 0, experience: 0.0, level: 0, selected_slot: 0, survival: true }
    }
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
}

impl Hud {
    /// `GuiNewChat.printChatMessage`: newest first.
    pub fn add_chat(&mut self, text: impl Into<String>) {
        self.chat.insert(0, ChatLine { text: text.into(), tick: self.update_counter });
        self.chat.truncate(100);
    }

    pub fn render(&mut self, ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud, debug: &DebugInfo, chat_open: bool) {
        let (w, h) = (sr.width, sr.height);
        if !ui.settings.hide_gui {
            self.hotbar(b, sr, p);
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
        }
        if ui.settings.show_debug {
            debug_overlay(ui, b, sr, debug);
        }
        if !ui.settings.hide_gui || chat_open {
            self.draw_chat(ui, b, h, chat_open);
        }
    }

    /// `renderTooltip` (the hotbar)
    fn hotbar(&self, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud) {
        b.color = [1.0; 4];
        let i = sr.width / 2;
        b.textured_rect(WIDGETS, i - 91, sr.height - 22, 0, 0, 182, 22);
        b.textured_rect(WIDGETS, i - 91 - 1 + p.selected_slot as i32 * 20, sr.height - 22 - 1, 0, 22, 24, 22);
    }

    /// `renderPlayerStats`, no potions, mount, air or damage flashing.
    fn player_stats(&self, b: &mut GuiBatch, sr: Scaled, p: &PlayerHud) {
        let health = mc_core::math::ceiling_float_int(p.health);
        let i1 = sr.width / 2 - 91;
        let j1 = sr.width / 2 + 91;
        let k1 = sr.height - 39;
        let f = p.max_health;
        let l1 = mc_core::math::ceiling_float_int(f / 2.0 / 10.0);
        let i2 = (10 - (l1 - 2)).max(3);
        let j2 = k1 - (l1 - 1) * i2 - 10;
        let k2 = p.armor;
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
        // Low health jitter uses Random seeded by updateCounter * 312871.
        let mut rng = JavaRandom::new((self.update_counter as i64).wrapping_mul(312871));
        for i6 in (0..mc_core::math::ceiling_float_int(f / 2.0)).rev() {
            let j6 = 16;
            let l3 = mc_core::math::ceiling_float_int((i6 + 1) as f32 / 10.0) - 1;
            let i4 = i1 + i6 % 10 * 8;
            let mut j4 = k1 - l3 * i2;
            if health <= 4 {
                j4 += rng.next_int(2);
            }
            b.textured_rect(ICONS, i4, j4, 16, 0, 9, 9);
            if i6 * 2 + 1 < health {
                b.textured_rect(ICONS, i4, j4, j6 + 36, 0, 9, 9);
            }
            if i6 * 2 + 1 == health {
                b.textured_rect(ICONS, i4, j4, j6 + 45, 0, 9, 9);
            }
        }
        for k6 in 0..10 {
            let i9 = j1 - k6 * 8 - 9;
            b.textured_rect(ICONS, i9, k1, 16, 27, 9, 9);
            if k6 * 2 + 1 < p.food {
                b.textured_rect(ICONS, i9, k1, 16 + 36, 27, 9, 9);
            }
            if k6 * 2 + 1 == p.food {
                b.textured_rect(ICONS, i9, k1, 16 + 45, 27, 9, 9);
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

    /// `GuiNewChat.drawChat` with default chat settings (width 320, opacity 1, scale 1).
    fn draw_chat(&self, ui: &mut Ui, b: &mut GuiBatch, screen_h: i32, chat_open: bool) {
        let max_lines = if chat_open { 20 } else { 10 };
        let width = 320;
        let (ox, oy) = (2, screen_h - 48 + 20);
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
            if l2 > 3 {
                let i2 = -(l1 as i32) * 9;
                b.rect(ox, oy + i2 - 9, ox + width + 4, oy + i2, ((l2 / 2) as u32) << 24);
                ui.font.draw_shadow(b, &line.text, ox as f32, (oy + i2 - 8) as f32, 16777215 + ((l2 as u32) << 24));
            }
        }
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
