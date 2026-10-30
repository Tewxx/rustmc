//! Vanilla 1.8.9 GUI: font renderer, widgets, screens and the in-game HUD.
//!
//! Everything draws into a [`batch::GuiBatch`] in scaled GUI units; `mc-render`
//! turns that into GPU draws.

pub mod batch;
pub mod clipboard;
pub mod component;
pub mod font;
pub mod hud;
pub mod keys;
pub mod lan;
pub mod lang;
pub mod resource_packs;
pub mod multiplayer;
pub mod screens;
pub mod server_data;
pub mod settings;
pub mod text_field;
pub mod widgets;

/// A client `ItemStack`. `item` is the catalogue entry that draws it; `damage` is the stack's own
/// metadata (`itemDamage`) and `tag` its NBT, both sent on the wire as they are.
///
/// `obj` stands in for Java object identity: copies made with `..stack` (vanilla mutating the
/// same object, e.g. `--stackSize`) keep it, new stacks get a fresh one. Equality ignores it.
#[derive(Clone, Copy, Debug)]
pub struct ItemStack {
    pub item: usize,
    pub count: u8,
    pub damage: i16,
    pub tag: Option<mc_core::nbt::Interned>,
    pub obj: u32,
}

impl PartialEq for ItemStack {
    fn eq(&self, o: &Self) -> bool {
        self.item == o.item && self.count == o.count && self.damage == o.damage && self.tag == o.tag
    }
}
impl Eq for ItemStack {}

impl ItemStack {
    pub fn new(item: usize, count: u8, damage: i16) -> Self {
        Self { item, count, damage, tag: None, obj: Self::fresh_obj() }
    }

    /// A new object identity.
    pub fn fresh_obj() -> u32 {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Java `==`: the same object.
    pub fn same_object(&self, o: &ItemStack) -> bool {
        self.obj == o.obj
    }

    /// `areItemsEqual && areItemStackTagsEqual`: may share a slot.
    pub fn stacks_with(&self, o: &ItemStack) -> bool {
        self.item == o.item && self.damage == o.damage && self.tag == o.tag
    }
}

/// One active potion effect as `InventoryEffectRenderer` shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectView {
    /// `statusIconIndex`, or -1.
    pub icon: i32,
    /// Translation key of the potion name.
    pub name: &'static str,
    pub amplifier: i32,
    /// `Potion.getDurationString`
    pub duration: String,
}

use font::FontRenderer;
use lang::Lang;
use mc_assets::AssetSource;
use settings::Settings;

/// `ScaledResolution`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scaled {
    pub width: i32,
    pub height: i32,
    pub width_f: f64,
    pub height_f: f64,
    pub factor: i32,
}

impl Scaled {
    pub fn new(display_w: u32, display_h: u32, gui_scale: i32) -> Self {
        let (w, h) = (display_w as i32, display_h as i32);
        let limit = if gui_scale == 0 { 1000 } else { gui_scale };
        let mut factor = 1;
        while factor < limit && w / (factor + 1) >= 320 && h / (factor + 1) >= 240 {
            factor += 1;
        }
        let (wf, hf) = (w as f64 / factor as f64, h as f64 / factor as f64);
        Self { width: wf.ceil() as i32, height: hf.ceil() as i32, width_f: wf, height_f: hf, factor }
    }
}

/// Shared GUI state (`Minecraft` fields the GUI reads).
pub struct Ui {
    /// Slots 0..9 are the hotbar; 9..36 are the main player inventory.
    pub inventory: [Option<ItemStack>; 36],
    /// Helmet, chestplate, leggings, boots (`ContainerPlayer` slots 5..9).
    pub armor: [Option<ItemStack>; 4],
    /// The 2×2 crafting grid of the survival inventory.
    pub crafting: [Option<ItemStack>; 4],
    /// Stacks thrown by the player (Q, clicking outside a container); the client spawns them.
    pub drops: Vec<ItemStack>,
    /// `GuiNewChat.sentMessages`: chat input history.
    pub sent_messages: Vec<String>,
    pub selected_slot: usize,
    pub shift_down: bool,
    /// `GuiScreen.isCtrlKeyDown`
    pub ctrl_down: bool,
    /// LWJGL code of the key behind the current `Screen::key` call (for rebinding).
    pub key_code: i32,
    pub font: FontRenderer,
    pub lang: Lang,
    pub settings: Settings,
    pub splash: String,
    /// True when a world is loaded (pause menu, world-tinted backgrounds).
    pub in_world: bool,
    /// The world belongs to a server connection (the pause menu says Disconnect).
    pub remote: bool,
    /// The local player's name, for screens that prefill it (`GuiMultiplayer`'s edit box).
    pub player_name: String,
    /// Client ticks (20/s) plus partial tick, for animations.
    pub ticks: f32,
    /// Sound events GUI widgets played (`playPressSound`), drained by the client.
    pub sounds: Vec<&'static str>,
    /// `thePlayer.getActivePotionEffects()` in iteration order.
    pub effects: Vec<EffectView>,
    /// The default pack's `pack.png` (`DefaultResourcePack.getPackImage`).
    pub pack_icon: Option<Vec<u8>>,
    /// Set by `GuiScreenResourcePacks` when Done applies a changed selection: the client runs
    /// `Minecraft.refreshResources`.
    pub pack_reload: bool,
    /// Set by the "Open resource pack folder" button.
    pub open_folder: Option<std::path::PathBuf>,
}

impl Ui {
    pub fn new(assets: &AssetSource, settings: Settings) -> Self {
        Self {
            inventory: [None; 36], armor: [None; 4], crafting: [None; 4], drops: Vec::new(), sent_messages: Vec::new(), selected_slot: 0, shift_down: false, ctrl_down: false, key_code: 0,
            font: FontRenderer::load(assets),
            lang: Lang::load(assets, "en_US"),
            settings,
            splash: random_splash(assets),
            in_world: false,
            remote: false,
            player_name: "Player".into(),
            ticks: 0.0,
            sounds: Vec::new(),
            effects: Vec::new(),
            pack_icon: assets.read_base("pack.png"),
            pack_reload: false,
            open_folder: None,
        }
    }

    /// `Minecraft.refreshResources`'s GUI half: re-read the font, language table and splash
    /// from the (possibly changed) resource pack stack while keeping player state and settings.
    pub fn reload(&mut self, assets: &AssetSource) {
        self.font = FontRenderer::load(assets);
        self.lang = Lang::load(assets, "en_US");
        self.splash = random_splash(assets);
        self.pack_icon = assets.read_base("pack.png");
    }

    /// `InventoryEffectRenderer.updateActivePotionEffects`: the container moves right to make room.
    pub fn effect_gui_left(&self, w: i32, x_size: i32) -> i32 {
        if self.effects.is_empty() {
            (w - x_size) / 2
        } else {
            160 + (w - x_size - 200) / 2
        }
    }

    /// `GuiScreen.drawHoveringText`: the purple-bordered tooltip box (screen size `w`×`h`).
    pub fn draw_hovering_text(&mut self, b: &mut batch::GuiBatch, lines: &[String], x: i32, y: i32, w: i32, h: i32) {
        if lines.is_empty() {
            return;
        }
        let i = lines.iter().map(|l| self.font.width(l)).max().unwrap_or(0);
        let mut l1 = x + 12;
        let mut i2 = y - 12;
        let mut k = 8;
        if lines.len() > 1 {
            k += 2 + (lines.len() as i32 - 1) * 10;
        }
        if l1 + i > w {
            l1 -= 28 + i;
        }
        if i2 + k + 6 > h {
            i2 = h - k - 6;
        }
        let l = 0xf0100010u32;
        b.gradient(l1 - 3, i2 - 4, l1 + i + 3, i2 - 3, l, l);
        b.gradient(l1 - 3, i2 + k + 3, l1 + i + 3, i2 + k + 4, l, l);
        b.gradient(l1 - 3, i2 - 3, l1 + i + 3, i2 + k + 3, l, l);
        b.gradient(l1 - 4, i2 - 3, l1 - 3, i2 + k + 3, l, l);
        b.gradient(l1 + i + 3, i2 - 3, l1 + i + 4, i2 + k + 3, l, l);
        let i1 = 0x505000ffu32;
        let j1 = (i1 & 0xfefefe) >> 1 | i1 & 0xff000000;
        b.gradient(l1 - 3, i2 - 3 + 1, l1 - 3 + 1, i2 + k + 3 - 1, i1, j1);
        b.gradient(l1 + i + 2, i2 - 3 + 1, l1 + i + 3, i2 + k + 3 - 1, i1, j1);
        b.gradient(l1 - 3, i2 - 3, l1 + i + 3, i2 - 3 + 1, i1, i1);
        b.gradient(l1 - 3, i2 + k + 2, l1 + i + 3, i2 + k + 3, j1, j1);
        for (k1, s1) in lines.iter().enumerate() {
            self.font.draw_shadow(b, s1, l1 as f32, i2 as f32, 0xffffffff);
            if k1 == 0 {
                i2 += 2;
            }
            i2 += 10;
        }
    }

    /// `InventoryEffectRenderer.drawActivePotionEffects`
    pub fn draw_effects(&mut self, b: &mut batch::GuiBatch, gui_left: i32, gui_top: i32) {
        if self.effects.is_empty() {
            return;
        }
        let (i, mut j) = (gui_left - 124, gui_top);
        let n = self.effects.len() as i32;
        let l = if n > 5 { 132 / (n - 1) } else { 33 };
        let tex = "textures/gui/container/inventory.png";
        for e in self.effects.clone() {
            b.color = [1.0; 4];
            b.textured_rect(tex, i, j, 0, 166, 140, 32);
            if e.icon >= 0 {
                b.textured_rect(tex, i + 6, j + 7, e.icon % 8 * 18, 198 + e.icon / 8 * 18, 18, 18);
            }
            let mut s1 = self.lang.get(e.name);
            if (1..=3).contains(&e.amplifier) {
                s1 = format!("{s1} {}", self.lang.get(&format!("enchantment.level.{}", e.amplifier + 1)));
            }
            self.font.draw_shadow(b, &s1, (i + 10 + 18) as f32, (j + 6) as f32, 0xffffffff);
            self.font.draw_shadow(b, &e.duration, (i + 10 + 18) as f32, (j + 6 + 10) as f32, 0xff7f7f7f);
            j += l;
        }
    }
}

/// A random line from `texts/splashes.txt`, like `GuiMainMenu`'s constructor.
fn random_splash(assets: &AssetSource) -> String {
    let lines: Vec<String> = assets
        .read("texts/splashes.txt")
        .map(|b| String::from_utf8_lossy(&b).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    if lines.is_empty() {
        return "missingno".into();
    }
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as usize);
    lines[seed % lines.len()].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_scale_matches_vanilla() {
        assert_eq!(Scaled::new(1280, 720, 0).factor, 3);
        assert_eq!(Scaled::new(1920, 1080, 0).factor, 4);
        assert_eq!(Scaled::new(1920, 1080, 2).factor, 2);
        assert_eq!(Scaled::new(854, 480, 0).factor, 2);
        let s = Scaled::new(1280, 720, 0);
        assert_eq!((s.width, s.height), (427, 240));
    }
}
