//! `GuiAchievement`: the slide-in achievement toast at the top right, and the `AchievementList`
//! table it draws from.
//!
//! `Minecraft.runGameLoop` calls `guiAchievement.updateAchievementWindow()` after the screen and
//! independently of `hideGUI`, so the toast covers every overlay and still shows with the HUD
//! hidden. [`Hud::render`](crate::hud::Hud::render) draws it with the HUD pass;
//! `NetHandlerPlayClient.handleStatistics` decides when to display one.
//!
//! The toast timing is `notificationTime` in `Minecraft.getSystemTime` milliseconds: a normal
//! toast lives for 3000 ms, while `displayUnformattedAchievement` (the open-inventory hint) starts
//! 2500 ms in the future and never expires on its own.

use crate::batch::GuiBatch;
use crate::font::FONT_HEIGHT;
use crate::lang::Lang;
use crate::{Scaled, Ui};
use std::collections::HashMap;

/// `textures/gui/achievement/achievement_background.png`
const BACKGROUND: &str = "textures/gui/achievement/achievement_background.png";

/// `AchievementList.openInventory`'s stat id: displayed specially by both the toast hint and
/// `handleStatistics` (it clears `showInventoryAchievementHint`).
pub const OPEN_INVENTORY: &str = "achievement.openInventory";

/// One `net.minecraft.stats.Achievement`: a `StatBase` carrying the icon stack, the display cell
/// and the parent that must be unlocked first.
#[derive(Clone, Copy, Debug)]
pub struct Achievement {
    /// `StatBase.statId`: the key the server sends and the client's stat map is keyed by.
    pub stat_id: &'static str,
    /// `unlocalizedName`: the name key is `achievement.<name>`, the description key
    /// `achievement.<name>.desc`.
    pub name: &'static str,
    /// `theItemStack`: registry item id (a block's `ItemBlock` id for blocks) and damage value.
    pub item: u16,
    pub damage: i16,
    /// `displayColumn` / `displayRow`.
    pub column: i32,
    pub row: i32,
    /// `parentAchievement`, by stat id.
    pub parent: Option<&'static str>,
    /// `isSpecial`: the spiked frame in the achievements screen.
    pub special: bool,
    /// `initIndependentStat`: no prerequisites (`canUnlockAchievement` is always true).
    pub independent: bool,
}

impl Achievement {
    /// `StatBase.getStatName`'s translation key.
    pub fn name_key(&self) -> String {
        format!("achievement.{}", self.name)
    }

    /// `Achievement.achievementDescription`'s translation key.
    pub fn description_key(&self) -> String {
        format!("achievement.{}.desc", self.name)
    }

    /// `getStatName().getUnformattedText()`: the translated name, formatting stripped (the keys
    /// carry none).
    pub fn stat_name(&self, lang: &Lang) -> String {
        lang.get(&self.name_key())
    }

    /// `Achievement.getDescription`. `openInventory` is the only achievement `Minecraft` gives a
    /// `statStringFormatter`; it fills the inventory key's display name into `%1$s`.
    pub fn description(&self, lang: &Lang, inventory_key: i32) -> String {
        if self.stat_id == OPEN_INVENTORY {
            lang.format(&self.description_key(), &[&crate::keys::key_name(inventory_key)])
        } else {
            lang.get(&self.description_key())
        }
    }

    /// The parent achievement, resolved.
    pub fn parent_achievement(&self) -> Option<&'static Achievement> {
        self.parent.and_then(by_stat_id)
    }

    /// `theItemStack`, once the client resolved the icon's catalogue entry
    /// (`Catalogue::index_of(item, damage)`); the damage carries skull and Notch-apple variants.
    pub fn icon_stack(&self, catalogue_item: usize) -> crate::ItemStack {
        crate::ItemStack::new(catalogue_item, 1, self.damage)
    }
}

/// `AchievementList.achievementList`, in registration order.
pub static ACHIEVEMENTS: [Achievement; 34] = [
    Achievement { stat_id: "achievement.openInventory", name: "openInventory", item: 340, damage: 0, column: 0, row: 0, parent: None, special: false, independent: true },
    Achievement { stat_id: "achievement.mineWood", name: "mineWood", item: 17, damage: 0, column: 2, row: 1, parent: Some("achievement.openInventory"), special: false, independent: false },
    Achievement { stat_id: "achievement.buildWorkBench", name: "buildWorkBench", item: 58, damage: 0, column: 4, row: -1, parent: Some("achievement.mineWood"), special: false, independent: false },
    Achievement { stat_id: "achievement.buildPickaxe", name: "buildPickaxe", item: 270, damage: 0, column: 4, row: 2, parent: Some("achievement.buildWorkBench"), special: false, independent: false },
    Achievement { stat_id: "achievement.buildFurnace", name: "buildFurnace", item: 61, damage: 0, column: 3, row: 4, parent: Some("achievement.buildPickaxe"), special: false, independent: false },
    Achievement { stat_id: "achievement.acquireIron", name: "acquireIron", item: 265, damage: 0, column: 1, row: 4, parent: Some("achievement.buildFurnace"), special: false, independent: false },
    Achievement { stat_id: "achievement.buildHoe", name: "buildHoe", item: 290, damage: 0, column: 2, row: -3, parent: Some("achievement.buildWorkBench"), special: false, independent: false },
    Achievement { stat_id: "achievement.makeBread", name: "makeBread", item: 297, damage: 0, column: -1, row: -3, parent: Some("achievement.buildHoe"), special: false, independent: false },
    Achievement { stat_id: "achievement.bakeCake", name: "bakeCake", item: 354, damage: 0, column: 0, row: -5, parent: Some("achievement.buildHoe"), special: false, independent: false },
    Achievement { stat_id: "achievement.buildBetterPickaxe", name: "buildBetterPickaxe", item: 274, damage: 0, column: 6, row: 2, parent: Some("achievement.buildPickaxe"), special: false, independent: false },
    Achievement { stat_id: "achievement.cookFish", name: "cookFish", item: 350, damage: 0, column: 2, row: 6, parent: Some("achievement.buildFurnace"), special: false, independent: false },
    Achievement { stat_id: "achievement.onARail", name: "onARail", item: 66, damage: 0, column: 2, row: 3, parent: Some("achievement.acquireIron"), special: true, independent: false },
    Achievement { stat_id: "achievement.buildSword", name: "buildSword", item: 268, damage: 0, column: 6, row: -1, parent: Some("achievement.buildWorkBench"), special: false, independent: false },
    Achievement { stat_id: "achievement.killEnemy", name: "killEnemy", item: 352, damage: 0, column: 8, row: -1, parent: Some("achievement.buildSword"), special: false, independent: false },
    Achievement { stat_id: "achievement.killCow", name: "killCow", item: 334, damage: 0, column: 7, row: -3, parent: Some("achievement.buildSword"), special: false, independent: false },
    Achievement { stat_id: "achievement.flyPig", name: "flyPig", item: 329, damage: 0, column: 9, row: -3, parent: Some("achievement.killCow"), special: true, independent: false },
    Achievement { stat_id: "achievement.snipeSkeleton", name: "snipeSkeleton", item: 261, damage: 0, column: 7, row: 0, parent: Some("achievement.killEnemy"), special: true, independent: false },
    Achievement { stat_id: "achievement.diamonds", name: "diamonds", item: 56, damage: 0, column: -1, row: 5, parent: Some("achievement.acquireIron"), special: false, independent: false },
    Achievement { stat_id: "achievement.diamondsToYou", name: "diamondsToYou", item: 264, damage: 0, column: -1, row: 2, parent: Some("achievement.diamonds"), special: false, independent: false },
    Achievement { stat_id: "achievement.portal", name: "portal", item: 49, damage: 0, column: -1, row: 7, parent: Some("achievement.diamonds"), special: false, independent: false },
    Achievement { stat_id: "achievement.ghast", name: "ghast", item: 370, damage: 0, column: -4, row: 8, parent: Some("achievement.portal"), special: true, independent: false },
    Achievement { stat_id: "achievement.blazeRod", name: "blazeRod", item: 369, damage: 0, column: 0, row: 9, parent: Some("achievement.portal"), special: false, independent: false },
    Achievement { stat_id: "achievement.potion", name: "potion", item: 373, damage: 0, column: 2, row: 8, parent: Some("achievement.blazeRod"), special: false, independent: false },
    Achievement { stat_id: "achievement.theEnd", name: "theEnd", item: 381, damage: 0, column: 3, row: 10, parent: Some("achievement.blazeRod"), special: true, independent: false },
    Achievement { stat_id: "achievement.theEnd2", name: "theEnd2", item: 122, damage: 0, column: 4, row: 13, parent: Some("achievement.theEnd"), special: true, independent: false },
    Achievement { stat_id: "achievement.enchantments", name: "enchantments", item: 116, damage: 0, column: -4, row: 4, parent: Some("achievement.diamonds"), special: false, independent: false },
    Achievement { stat_id: "achievement.overkill", name: "overkill", item: 276, damage: 0, column: -4, row: 1, parent: Some("achievement.enchantments"), special: true, independent: false },
    Achievement { stat_id: "achievement.bookcase", name: "bookcase", item: 47, damage: 0, column: -3, row: 6, parent: Some("achievement.enchantments"), special: false, independent: false },
    Achievement { stat_id: "achievement.breedCow", name: "breedCow", item: 296, damage: 0, column: 7, row: -5, parent: Some("achievement.killCow"), special: false, independent: false },
    Achievement { stat_id: "achievement.spawnWither", name: "spawnWither", item: 397, damage: 1, column: 7, row: 12, parent: Some("achievement.theEnd2"), special: false, independent: false },
    Achievement { stat_id: "achievement.killWither", name: "killWither", item: 399, damage: 0, column: 7, row: 10, parent: Some("achievement.spawnWither"), special: false, independent: false },
    Achievement { stat_id: "achievement.fullBeacon", name: "fullBeacon", item: 138, damage: 0, column: 7, row: 8, parent: Some("achievement.killWither"), special: true, independent: false },
    Achievement { stat_id: "achievement.exploreAllBiomes", name: "exploreAllBiomes", item: 313, damage: 0, column: 4, row: 8, parent: Some("achievement.theEnd"), special: true, independent: false },
    Achievement { stat_id: "achievement.overpowered", name: "overpowered", item: 322, damage: 1, column: 6, row: 4, parent: Some("achievement.buildBetterPickaxe"), special: true, independent: false },
];

/// `AchievementList.achievementList`.
pub fn list() -> &'static [Achievement] {
    &ACHIEVEMENTS
}

/// Look up a stat id from an `S37PacketStatistics` entry.
pub fn by_stat_id(stat_id: &str) -> Option<&'static Achievement> {
    ACHIEVEMENTS.iter().find(|a| a.stat_id == stat_id)
}

/// `StatFileWriter.readStat`: the stored value, 0 when absent.
pub fn read_stat(stats: &HashMap<String, i32>, stat_id: &str) -> i32 {
    stats.get(stat_id).copied().unwrap_or(0)
}

/// `StatFileWriter.hasAchievementUnlocked`: `readStat(achievement) > 0`.
pub fn has_achievement_unlocked(stats: &HashMap<String, i32>, achievement: &Achievement) -> bool {
    read_stat(stats, achievement.stat_id) > 0
}

/// `StatFileWriter.canUnlockAchievement`: the parent is unlocked, or there is none. Only local stat
/// changes (`increaseStat`) consult it; `handleStatistics` trusts the server's values.
pub fn can_unlock_achievement(stats: &HashMap<String, i32>, achievement: &Achievement) -> bool {
    achievement.parent_achievement().is_none_or(|p| has_achievement_unlocked(stats, p))
}

/// Draws the achievement's item stack at a GUI position: `RenderItem.renderItemAndEffectIntoGUI`.
/// Item models live in `mc-client`'s catalogue, so the client supplies the drawing.
pub type ItemIcon = Box<dyn FnMut(&mut Ui, &mut GuiBatch, i32, i32)>;

/// Builds a toast [`ItemIcon`] from the client's catalogue: `item` is
/// `Catalogue::index_of(achievement.item, achievement.damage)` and `draw` is `Catalogue::draw`.
pub fn item_icon<F>(achievement: &Achievement, item: Option<usize>, mut draw: F) -> Option<ItemIcon>
where
    F: FnMut(&mut Ui, &mut GuiBatch, crate::ItemStack, i32, i32) + 'static,
{
    item.map(|item| {
        let stack = achievement.icon_stack(item);
        Box::new(move |ui: &mut Ui, b: &mut GuiBatch, x: i32, y: i32| draw(ui, b, stack, x, y)) as ItemIcon
    })
}

/// `Minecraft.getSystemTime`
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// `GuiAchievement`: the toast's text, the achievement it shows and its `notificationTime`.
#[derive(Default)]
pub struct AchievementToast {
    /// `theAchievement`.
    achievement: Option<&'static Achievement>,
    /// `achievementTitle`.
    title: String,
    /// `achievementDescription`.
    description: String,
    /// `notificationTime`, in `getSystemTime` milliseconds; 0 shows nothing.
    notification_time: u64,
    /// `permanentNotification`: the open-inventory hint, which does not expire.
    permanent: bool,
    /// `theAchievement.theItemStack`, drawn through the client's item renderer.
    icon: Option<ItemIcon>,
    /// `NetHandlerPlayClient.field_147308_k`: one statistics packet has been handled on this
    /// connection, so the open-inventory hint is no longer offered (`Hud` is rebuilt by
    /// `reset_world_state`, giving each connection a fresh value).
    pub statistics_seen: bool,
}

impl AchievementToast {
    /// `GuiAchievement.displayAchievement`: "Achievement get!" over the stat name, yellow/white,
    /// for 3000 ms.
    pub fn display_achievement(&mut self, ui: &Ui, achievement: &'static Achievement, icon: Option<ItemIcon>) {
        self.display_achievement_at(ui, achievement, icon, now_ms());
    }

    /// [`Self::display_achievement`] at an explicit `getSystemTime` (for tests).
    pub fn display_achievement_at(&mut self, ui: &Ui, achievement: &'static Achievement, icon: Option<ItemIcon>, now: u64) {
        self.title = ui.lang.get("achievement.get");
        self.description = achievement.stat_name(&ui.lang);
        self.notification_time = now;
        self.achievement = Some(achievement);
        self.icon = icon;
        self.permanent = false;
    }

    /// `GuiAchievement.displayUnformattedAchievement`: the stat name over the description, starting
    /// 2500 ms from now and staying until replaced or cleared.
    pub fn display_unformatted_achievement(&mut self, ui: &Ui, achievement: &'static Achievement, icon: Option<ItemIcon>) {
        self.display_unformatted_achievement_at(ui, achievement, icon, now_ms());
    }

    /// [`Self::display_unformatted_achievement`] at an explicit `getSystemTime` (for tests).
    pub fn display_unformatted_achievement_at(&mut self, ui: &Ui, achievement: &'static Achievement, icon: Option<ItemIcon>, now: u64) {
        self.title = achievement.stat_name(&ui.lang);
        self.description = achievement.description(&ui.lang, ui.settings.key("key.inventory"));
        self.notification_time = now + 2500;
        self.achievement = Some(achievement);
        self.icon = icon;
        self.permanent = true;
    }

    /// `GuiAchievement.clearAchievements`: `loadWorld(null)` calls it when leaving a world.
    pub fn clear_achievements(&mut self) {
        self.achievement = None;
        self.notification_time = 0;
        self.icon = None;
    }

    /// `theAchievement != null && notificationTime != 0` (the player check is the caller's).
    pub fn is_showing(&self) -> bool {
        self.achievement.is_some() && self.notification_time != 0
    }

    /// `GuiAchievement.updateAchievementWindow`'s `d1` at `now`, or `None` once a normal toast has
    /// expired (which resets `notificationTime`, like vanilla).
    pub fn slide(&mut self, now: u64) -> Option<f64> {
        self.achievement?;
        if self.notification_time == 0 {
            return None;
        }
        let mut d0 = (now as i64 - self.notification_time as i64) as f64 / 3000.0;
        if !self.permanent {
            if d0 < 0.0 || d0 > 1.0 {
                self.notification_time = 0;
                return None;
            }
        } else if d0 > 0.5 {
            // The hint stays fully out; it is only removed by a later toast or `clearAchievements`.
            d0 = 0.5;
        }
        Some(slide_curve(d0))
    }

    /// `GuiAchievement.updateAchievementWindow`'s drawing.
    pub fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, sr: Scaled) {
        self.draw_at(ui, b, sr, now_ms());
    }

    /// [`Self::draw`] at an explicit `getSystemTime` (for tests).
    pub fn draw_at(&mut self, ui: &mut Ui, b: &mut GuiBatch, sr: Scaled, now: u64) {
        let Some(d1) = self.slide(now) else { return };
        let i = sr.width - 160;
        let j = toast_y(d1);
        b.color = [1.0; 4];
        b.textured_rect(BACKGROUND, i, j, 96, 202, 160, 32);
        if self.permanent {
            // `drawSplitString(achievementDescription, i + 30, j + 7, 120, -1)`
            let description = self.description.clone();
            let mut y = j + 7;
            for line in ui.font.list_to_width(&description, 120) {
                ui.font.draw(b, &line, (i + 30) as f32, y as f32, 0xFFFF_FFFF);
                y += FONT_HEIGHT;
            }
        } else {
            // `drawString` (no shadow): title yellow (`-256`), description white (`-1`).
            let (title, description) = (self.title.clone(), self.description.clone());
            ui.font.draw(b, &title, (i + 30) as f32, (j + 7) as f32, 0xFFFF_FF00);
            ui.font.draw(b, &description, (i + 30) as f32, (j + 18) as f32, 0xFFFF_FFFF);
        }
        if let Some(icon) = &mut self.icon {
            icon(ui, b, i + 8, j + 8);
        }
    }
}

/// `updateAchievementWindow`'s easing of `d0` (`elapsed / 3000`) into the drop distance `d1`
/// (1 = fully hidden above the screen, 0 = fully down).
pub fn slide_curve(d0: f64) -> f64 {
    let mut d1 = d0 * 2.0;
    if d1 > 1.0 {
        d1 = 2.0 - d1;
    }
    d1 *= 4.0;
    d1 = 1.0 - d1;
    if d1 < 0.0 {
        d1 = 0.0;
    }
    d1 = d1 * d1;
    d1 * d1
}

/// `int j = 0 - (int)(d1 * 36.0D)`: the background's top edge (negative is above the screen).
pub fn toast_y(d1: f64) -> i32 {
    -((d1 * 36.0) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn ui() -> Ui {
        Ui::new(&mc_assets::AssetSource::new(), crate::settings::Settings::default())
    }

    /// A `Lang` backed by a one-file temporary pack (under `target/`, so it works in sandboxes
    /// that deny the system temp directory).
    fn lang(text: &str) -> (Lang, std::path::PathBuf) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("achievement-lang-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let dir = root.join("assets/minecraft/lang");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("en_US.lang"), text).unwrap();
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&root).unwrap();
        (Lang::load(&assets, "en_US"), root)
    }

    #[test]
    fn achievement_list_matches_vanilla() {
        assert_eq!(list().len(), 34);
        let first = &list()[0];
        assert_eq!(first.stat_id, OPEN_INVENTORY);
        assert_eq!((first.name, first.column, first.row), ("openInventory", 0, 0));
        assert_eq!((first.item, first.damage), (340, 0));
        assert!(first.independent);
        assert!(first.parent.is_none());
        assert!(by_stat_id("achievement.theEnd2").is_some_and(|a| (a.item, a.damage, a.column, a.row) == (122, 0, 4, 13)));
        assert!(by_stat_id("achievement.spawnWither").is_some_and(|a| (a.item, a.damage) == (397, 1)));
        let skull = by_stat_id("achievement.spawnWither").unwrap().icon_stack(7);
        assert_eq!((skull.item, skull.count, skull.damage), (7, 1, 1));
        assert!(by_stat_id("achievement.overpowered").is_some_and(|a| (a.item, a.damage) == (322, 1)));
        assert!(by_stat_id("achievement.nope").is_none());
        // `AchievementList.minDisplayColumn..maxDisplayRow`
        let columns = list().iter().map(|a| a.column);
        let rows = list().iter().map(|a| a.row);
        assert_eq!((columns.clone().min(), columns.max(), rows.clone().min(), rows.max()), (Some(-4), Some(9), Some(-5), Some(13)));
        assert_eq!(list().iter().filter(|a| a.special).count(), 10);
        // Every parent resolves, and only `openInventory` is independent.
        for a in list() {
            assert_eq!(a.parent.is_some(), !a.independent, "{}", a.stat_id);
            if let Some(p) = a.parent {
                assert!(by_stat_id(p).is_some(), "{} -> {p}", a.stat_id);
            }
        }
    }

    #[test]
    fn unlock_checks_follow_stat_file_writer() {
        let mut stats = HashMap::new();
        let wood = by_stat_id("achievement.mineWood").unwrap();
        let inventory = by_stat_id(OPEN_INVENTORY).unwrap();
        assert!(!has_achievement_unlocked(&stats, wood));
        assert_eq!(read_stat(&stats, wood.stat_id), 0);
        assert!(can_unlock_achievement(&stats, inventory));
        assert!(!can_unlock_achievement(&stats, wood));
        stats.insert(OPEN_INVENTORY.to_string(), 1);
        assert!(has_achievement_unlocked(&stats, inventory));
        assert!(can_unlock_achievement(&stats, wood));
        stats.insert(OPEN_INVENTORY.to_string(), 0);
        assert!(!has_achievement_unlocked(&stats, inventory));
        assert!(!can_unlock_achievement(&stats, wood));
    }

    #[test]
    fn language_keys_and_open_inventory_formatter() {
        let (lang, root) = lang(
            "achievement.get=Achievement get!\n\
             achievement.openInventory=Taking Inventory\n\
             achievement.openInventory.desc=Press '%1$s' to open your inventory.\n\
             achievement.mineWood=Getting Wood\n\
             achievement.mineWood.desc=Attack a tree until a block of wood pops out\n",
        );
        let inventory = by_stat_id(OPEN_INVENTORY).unwrap();
        assert_eq!(inventory.stat_name(&lang), "Taking Inventory");
        assert_eq!(inventory.description(&lang, 18), "Press 'E' to open your inventory.");
        let wood = by_stat_id("achievement.mineWood").unwrap();
        assert_eq!(wood.stat_name(&lang), "Getting Wood");
        assert_eq!(wood.description(&lang, 18), "Attack a tree until a block of wood pops out");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn slide_curve_matches_update_achievement_window() {
        // d0 = elapsed / 3000; d1 = 1 leaves the toast 36 px above the screen.
        assert_eq!(slide_curve(0.0), 1.0);
        assert_eq!(slide_curve(0.125), 0.0);
        assert_eq!(slide_curve(0.5), 0.0);
        // Symmetric tail: 1/16 in and 15/16 in are 2.25 px up.
        assert_eq!(slide_curve(0.0625), 0.0625);
        assert_eq!(slide_curve(0.9375), 0.0625);
        assert_eq!(slide_curve(1.0), 1.0);
    }

    #[test]
    fn toast_y_is_truncated_towards_zero() {
        assert_eq!(toast_y(1.0), -36);
        assert_eq!(toast_y(0.0625), -2);
        assert_eq!(toast_y(0.0), 0);
    }

    #[test]
    fn draw_places_background_and_icon_like_gui_achievement() {
        use crate::batch::Item;
        use std::cell::Cell;
        use std::rc::Rc;

        let mut ui = ui();
        let mut b = GuiBatch::new();
        let sr = Scaled { width: 427, height: 240, width_f: 427.0, height_f: 240.0, factor: 3 };
        let skull = by_stat_id("achievement.spawnWither").unwrap();
        let icon_at = Rc::new(Cell::new((0, 0, 0)));
        let seen = icon_at.clone();
        let icon = item_icon(skull, Some(42), move |_ui, _b, stack, x, y| seen.set((x, y, stack.damage as i32)));
        let mut toast = AchievementToast::default();
        toast.display_achievement_at(&ui, skull, icon, 1000);
        // Fully retracted at t = 0: the 160x32 background starts `width - 160` across, 36 px up.
        toast.draw_at(&mut ui, &mut b, sr, 1000);
        let Item::Draw(background) = &b.items[0] else { panic!("expected a textured draw") };
        assert_eq!(background.texture.as_deref(), Some(BACKGROUND));
        assert_eq!(background.vertices[0].pos, [267.0, -4.0]);
        assert_eq!(background.vertices[1].pos, [427.0, -4.0]);
        assert_eq!(background.vertices[2].pos, [427.0, -36.0]);
        assert_eq!(background.vertices[3].pos, [267.0, -36.0]);
        // `renderItemAndEffectIntoGUI(theItemStack, i + 8, j + 8)`; the stack keeps the damage
        // value that selects the icon variant (the wither skeleton skull here).
        assert_eq!(icon_at.get(), (275, -28, 1));
        // Fully down, then gone.
        b.items.clear();
        toast.draw_at(&mut ui, &mut b, sr, 1375);
        let Item::Draw(background) = &b.items[0] else { panic!("expected a textured draw") };
        assert_eq!(background.vertices[3].pos, [267.0, 0.0]);
        b.items.clear();
        toast.draw_at(&mut ui, &mut b, sr, 4001);
        assert!(b.items.is_empty());
    }

    #[test]
    fn toast_lives_for_three_seconds() {
        let ui = ui();
        let inventory = by_stat_id(OPEN_INVENTORY).unwrap();
        let mut toast = AchievementToast::default();
        assert!(!toast.is_showing());
        toast.display_achievement_at(&ui, inventory, None, 1000);
        assert!(toast.is_showing());
        // Vanilla titles: yellow "achievement.get", white stat name (empty pack -> the keys).
        assert_eq!((toast.title.as_str(), toast.description.as_str()), ("achievement.get", "achievement.openInventory"));
        assert_eq!(toast.slide(1000), Some(1.0));
        assert_eq!(toast.slide(1375), Some(0.0));
        assert_eq!(toast.slide(2500), Some(0.0));
        assert_eq!(toast.slide(4000), Some(1.0));
        // d0 > 1: the next update clears notificationTime.
        assert_eq!(toast.slide(4001), None);
        assert!(!toast.is_showing());
    }

    #[test]
    fn unformatted_toast_starts_later_and_never_expires() {
        let ui = ui();
        let inventory = by_stat_id(OPEN_INVENTORY).unwrap();
        let mut toast = AchievementToast::default();
        toast.display_unformatted_achievement_at(&ui, inventory, None, 1000);
        assert!(toast.is_showing());
        assert!(toast.permanent);
        // notificationTime = now + 2500; the closed form gives a d1 far above the screen.
        assert!(toast.slide(1000).is_some_and(|d1| toast_y(d1) < -1000));
        assert_eq!(toast.slide(3500), Some(1.0));
        assert_eq!(toast.slide(3875), Some(0.0));
        assert_eq!(toast.slide(100_000), Some(0.0));
        assert!(toast.is_showing());
        toast.clear_achievements();
        assert!(!toast.is_showing());
        assert_eq!(toast.slide(100_000), None);
    }
}
