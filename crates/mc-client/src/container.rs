//! Server-opened windows (`GuiContainer` subclasses over a remote `Container`): the layout per
//! window type, client-side `slotClick` prediction, and the C0E clicks it queues for the server.
use crate::inventory::Catalogue;
use mc_gui::{
    batch::GuiBatch,
    screens::{Action, Key, Screen},
    ItemStack, Ui,
};
use std::sync::{Arc, Mutex};

/// A click to send: `C0EPacketClickWindow` minus the transaction id, which the client assigns.
#[derive(Clone, Debug, PartialEq)]
pub struct Click {
    pub slot: i16,
    pub button: i8,
    pub mode: i8,
    /// What `slotClick` returned (the clicked slot's stack before the click).
    pub item: Option<ItemStack>,
}

/// The window state shared between the screen and the network handler.
pub struct Window {
    pub id: u8,
    pub kind: String,
    pub title: String,
    /// The container's own slots (not the player's inventory part).
    pub slots: Vec<Option<ItemStack>>,
    pub cursor: Option<ItemStack>,
    /// `WindowProperty` values (furnace progress etc.).
    pub props: [i16; 4],
    pub clicks: Vec<Click>,
    pub closed: bool,
    /// `Container.transactionID`
    pub transaction: i16,
}

impl Window {
    /// `OpenWindow`: `slots` is the storage size; non-storage windows have fixed sizes.
    pub fn new(id: u8, kind: &str, title: String, slots: u8) -> Window {
        let n = match kind {
            "minecraft:crafting_table" => 10,
            "minecraft:enchanting_table" => 2,
            "minecraft:anvil" => 3,
            _ => slots as usize,
        };
        Window { id, kind: kind.into(), title, slots: vec![None; n], cursor: None, props: [0; 4], clicks: Vec::new(), closed: false, transaction: 0 }
    }

    /// Window slot index → storage: container slots first, then main inventory, then hotbar.
    pub fn get<'a>(&'a mut self, ui: &'a mut Ui, i: usize) -> Option<&'a mut Option<ItemStack>> {
        if self.kind == "player" {
            // ContainerPlayer: 0 result, 1-4 crafting grid, 5-8 armor, 9-35 main, 36-44 hotbar.
            return match i {
                0 => self.slots.get_mut(0),
                1..=4 => Some(&mut ui.crafting[i - 1]),
                5..=8 => Some(&mut ui.armor[i - 5]),
                9..=35 => Some(&mut ui.inventory[i]),
                36..=44 => Some(&mut ui.inventory[i - 36]),
                _ => None,
            };
        }
        let n = self.slots.len();
        if i < n {
            self.slots.get_mut(i)
        } else if i < n + 27 {
            Some(&mut ui.inventory[9 + i - n])
        } else if i < n + 36 {
            Some(&mut ui.inventory[i - n - 27])
        } else {
            None
        }
    }

    pub fn total(&self) -> usize {
        if self.kind == "player" {
            return 45;
        }
        self.slots.len() + 36
    }

    /// `SetSlot` / `WindowItems` for this window.
    pub fn set(&mut self, ui: &mut Ui, i: usize, s: Option<ItemStack>) {
        if let Some(slot) = self.get(ui, i) {
            *slot = s;
        }
    }

    /// Slots that only accept output (crafting result, furnace output).
    fn output_slot(&self, i: usize) -> bool {
        match self.kind.as_str() {
            "minecraft:crafting_table" => i == 0,
            "minecraft:furnace" => i == 2,
            "player" => i == 0,
            _ => false,
        }
    }
}

pub struct ContainerScreen {
    pub window: Arc<Mutex<Window>>,
    catalogue: Arc<Catalogue>,
    left: i32,
    top: i32,
    hover: Option<usize>,
    /// `GuiInventory` (an `InventoryEffectRenderer`) rather than a plain `GuiContainer`.
    is_player: bool,
}

struct Layout {
    texture: &'static str,
    w: i32,
    h: i32,
    /// Container slot positions.
    slots: Vec<(i32, i32)>,
    /// Top-left of the 3×9 main inventory; the hotbar is 58 below it.
    inv: (i32, i32),
    /// Generic chest: rows (two-part background).
    rows: Option<i32>,
}

impl ContainerScreen {
    pub fn new(window: Arc<Mutex<Window>>, catalogue: Arc<Catalogue>) -> Self {
        Self { window, catalogue, left: 0, top: 0, hover: None, is_player: false }
    }

    fn layout(w: &Window) -> Layout {
        let n = w.slots.len() as i32;
        match w.kind.as_str() {
            "player" => Layout {
                texture: "textures/gui/container/inventory.png",
                w: 176,
                h: 166,
                slots: std::iter::once((154, 28))
                    .chain((0..4).map(|i| (98 + i % 2 * 18, 18 + i / 2 * 18)))
                    .chain((0..4).map(|i| (8, 8 + i * 18)))
                    .collect(),
                inv: (8, 84),
                rows: None,
            },
            "minecraft:hopper" => Layout {
                texture: "textures/gui/container/hopper.png",
                w: 176,
                h: 133,
                slots: (0..n).map(|i| (44 + i * 18, 20)).collect(),
                inv: (8, 51),
                rows: None,
            },
            "minecraft:dispenser" | "minecraft:dropper" => Layout {
                texture: "textures/gui/container/dispenser.png",
                w: 176,
                h: 166,
                slots: (0..n).map(|i| (62 + i % 3 * 18, 17 + i / 3 * 18)).collect(),
                inv: (8, 84),
                rows: None,
            },
            "minecraft:crafting_table" => Layout {
                texture: "textures/gui/container/crafting_table.png",
                w: 176,
                h: 166,
                slots: std::iter::once((124, 35)).chain((0..9).map(|i| (30 + i % 3 * 18, 17 + i / 3 * 18))).collect(),
                inv: (8, 84),
                rows: None,
            },
            "minecraft:furnace" => Layout {
                texture: "textures/gui/container/furnace.png",
                w: 176,
                h: 166,
                slots: vec![(56, 17), (56, 53), (116, 35)],
                inv: (8, 84),
                rows: None,
            },
            _ => {
                // GuiChest (also the fallback for window types without their own GUI here).
                let rows = ((n + 8) / 9).clamp(1, 6);
                let off = (rows - 4) * 18;
                Layout {
                    texture: "textures/gui/container/generic_54.png",
                    w: 176,
                    h: 114 + rows * 18,
                    slots: (0..n).map(|i| (8 + i % 9 * 18, 18 + i / 9 * 18)).collect(),
                    inv: (8, 103 + off),
                    rows: Some(rows),
                }
            }
        }
    }

    fn positions(l: &Layout) -> Vec<(i32, i32)> {
        let mut v = l.slots.clone();
        for i in 0..27 {
            v.push((l.inv.0 + i % 9 * 18, l.inv.1 + i / 9 * 18));
        }
        for i in 0..9 {
            v.push((l.inv.0 + i * 18, l.inv.1 + 58));
        }
        v
    }

    fn at(&self, l: &Layout, mx: i32, my: i32) -> Option<usize> {
        let (x, y) = (mx - self.left, my - self.top);
        Self::positions(l).into_iter().position(|(sx, sy)| (sx - 1..sx + 17).contains(&x) && (sy - 1..sy + 17).contains(&y))
    }

    fn max(&self, s: ItemStack) -> u8 {
        self.catalogue.items[s.item].max.max(1)
    }

    fn furnace_rules() -> &'static serde_json::Value {
        static RULES: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        RULES.get_or_init(|| serde_json::from_str(include_str!("furnace_rules.json")).unwrap())
    }

    fn fuel(&self, s: ItemStack) -> bool {
        let id = self.catalogue.items[s.item].id as u64;
        Self::furnace_rules()["fuels"].as_array().unwrap().iter().any(|v| v.as_u64() == Some(id))
    }

    fn smeltable(&self, s: ItemStack) -> bool {
        let id = self.catalogue.items[s.item].id as u64;
        Self::furnace_rules()["smelting"].as_array().unwrap().iter().any(|v| {
            v[0].as_u64() == Some(id) && (v[1].as_i64() == Some(32767) || v[1].as_i64() == Some(s.damage as i64))
        })
    }

    fn accepts(&self, w: &Window, i: usize, s: ItemStack) -> bool {
        if w.output_slot(i) { return false; }
        if w.kind == "minecraft:furnace" && i == 1 {
            return self.fuel(s) || self.catalogue.items[s.item].id == 325;
        }
        if w.kind == "player" && (5..=8).contains(&i) {
            return crate::item_use::armor_slot(self.catalogue.items[s.item].id) == Some(8 - i);
        }
        true
    }

    /// `Container.slotClick` mode 0 (`PICKUP`) on window slot `i`.
    fn pickup(&self, w: &mut Window, ui: &mut Ui, i: usize, right: bool) {
        let cursor = w.cursor;
        let valid = cursor.is_some_and(|c| self.accepts(w, i, c));
        let slot_limit = if (w.kind == "player" && (5..=8).contains(&i)) ||
            (w.kind == "minecraft:furnace" && i == 1 && cursor.is_some_and(|c| self.catalogue.items[c.item].id == 325)) { 1 } else { 64 };
        let Some(cur) = w.get(ui, i).map(|s| *s) else { return };
        let max_of = |s: ItemStack| self.max(s);
        let (new_slot, new_cursor) = match (cursor, cur) {
            (None, None) => return,
            (None, Some(s)) => {
                let n = if right { s.count.div_ceil(2) } else { s.count };
                ((s.count > n).then_some(ItemStack { count: s.count - n, ..s }), Some(ItemStack { count: n, ..s }))
            }
            (Some(c), None) if valid => {
                let n = if right { 1 } else { c.count.min(max_of(c)).min(slot_limit) };
                (Some(ItemStack { count: n, ..c }), (c.count > n).then_some(ItemStack { count: c.count - n, ..c }))
            }
            (Some(_), None) => return,
            (Some(c), Some(s)) if !valid => {
                // Taking from an output slot adds onto a matching cursor.
                if max_of(c) > 1 && c.stacks_with(&s) && c.count as u16 + s.count as u16 <= max_of(c) as u16 {
                    (None, Some(ItemStack { count: c.count + s.count, ..c }))
                } else {
                    return;
                }
            }
            (Some(c), Some(s)) if c.stacks_with(&s) => {
                let room = max_of(s).min(slot_limit).saturating_sub(s.count);
                let n = if right { 1.min(room) } else { c.count.min(room) };
                (Some(ItemStack { count: s.count + n, ..s }), (c.count > n).then_some(ItemStack { count: c.count - n, ..c }))
            }
            (Some(c), Some(s)) if c.count <= slot_limit => (Some(c), Some(s)),
            _ => return,
        };
        if let Some(slot) = w.get(ui, i) {
            *slot = new_slot;
        }
        w.cursor = new_cursor;
    }

    /// `Container.mergeItemStack(stack, start, end, reverse)`.
    fn merge(&self, w: &mut Window, ui: &mut Ui, mut s: ItemStack, range: Vec<usize>) -> Option<ItemStack> {
        let max = self.max(s);
        for &i in &range {
            if let Some(Some(t)) = w.get(ui, i).map(|x| *x) {
                if t.stacks_with(&s) && t.count < max {
                    let n = s.count.min(max - t.count);
                    w.set(ui, i, Some(ItemStack { count: t.count + n, ..t }));
                    s.count -= n;
                    if s.count == 0 {
                        return None;
                    }
                }
            }
        }
        for &i in &range {
            if w.get(ui, i).is_some_and(|x| x.is_none()) {
                w.set(ui, i, Some(s));
                return None;
            }
        }
        Some(s)
    }

    /// `transferStackInSlot` for storage containers: container ↔ inventory (to the inventory
    /// back to front).
    fn transfer(&self, w: &mut Window, ui: &mut Ui, i: usize) -> Option<ItemStack> {
        let s = w.get(ui, i).and_then(|x| *x)?;
        if w.kind == "player" {
            // ContainerPlayer.transferStackInSlot
            const SUFFIX: [&str; 4] = ["_helmet", "_chestplate", "_leggings", "_boots"];
            let name = self.catalogue.items[s.item].name.clone();
            let armor = SUFFIX.iter().position(|x| name.ends_with(x)).map(|a| 5 + a).filter(|&a| w.get(ui, a).is_some_and(|x| x.is_none()));
            let range: Vec<usize> = match (i, armor) {
                (0, _) => (9..45).rev().collect(),
                (1..=8, _) => (9..45).collect(),
                (_, Some(a)) => vec![a],
                (9..=35, _) => (36..45).collect(),
                _ => (9..36).collect(),
            };
            let rest = self.merge(w, ui, s, range);
            w.set(ui, i, rest);
            return (rest.map_or(0, |r| r.count) != s.count).then_some(s);
        }
        if w.kind == "minecraft:furnace" {
            let range = if i == 2 { (3..39).rev().collect() }
                else if i < 2 { (3..39).collect() }
                else if self.smeltable(s) { vec![0] }
                else if self.fuel(s) { vec![1] }
                else if i < 30 { (30..39).collect() }
                else { (3..30).collect() };
            let rest = self.merge(w, ui, s, range);
            w.set(ui, i, rest);
            return (rest.map_or(0, |r| r.count) != s.count).then_some(s);
        }
        let n = w.slots.len();
        let range: Vec<usize> = if i < n { (n..w.total()).rev().collect() } else { (0..n).filter(|&k| !w.output_slot(k)).collect() };
        let rest = self.merge(w, ui, s, range);
        w.set(ui, i, rest);
        (rest.map_or(0, |r| r.count) != s.count).then_some(s)
    }

    fn click_slot(&mut self, ui: &mut Ui, i: usize, button: i8, mode: i8) {
        let mut w = self.window.lock().unwrap();
        let item = w.get(ui, i).and_then(|s| *s);
        let returned = match mode {
            0 => { self.pickup(&mut w, ui, i, button == 1); item }
            1 => self.transfer(&mut w, ui, i),
            2 if (0..9).contains(&button) => {
                let hot_index = button as usize;
                let hot = ui.inventory[hot_index];
                let player_slot = if w.kind == "player" { i >= 5 && i < 45 } else { i >= w.slots.len() && i < w.total() };
                let valid = hot.is_none_or(|s| self.accepts(&w, i, s));
                let empty = ui.inventory.iter().position(Option::is_none);
                let direct = hot.is_none() || (player_slot && valid);
                if let Some(here) = item {
                    if direct {
                        w.set(ui, i, hot);
                        ui.inventory[hot_index] = Some(here);
                    } else if let (Some(old), Some(empty)) = (hot, empty) {
                        ui.inventory[hot_index] = Some(here);
                        w.set(ui, i, None);
                        // InventoryPlayer.addItemStackToInventory first fills compatible partial stacks.
                        let mut left = old.count;
                        for t in ui.inventory.iter_mut().flatten() {
                            if t.stacks_with(&old) && t.count < self.max(old) {
                                let n = left.min(self.max(old) - t.count); t.count += n; left -= n;
                                if left == 0 { break; }
                            }
                        }
                        if left > 0 { ui.inventory[empty] = Some(ItemStack { count: left, ..old }); }
                    }
                } else if hot.is_some() && valid {
                    w.set(ui, i, hot);
                    ui.inventory[hot_index] = None;
                }
                None
            }
            _ => None,
        };
        w.clicks.push(Click { slot: i as i16, button, mode, item: returned });
    }

    fn mouse(&mut self, ui: &mut Ui, mx: i32, my: i32, right: bool) -> Action {
        let l = Self::layout(&self.window.lock().unwrap());
        let outside = mx < self.left || my < self.top || mx >= self.left + l.w || my >= self.top + l.h;
        let button = right as i8;
        match self.at(&l, mx, my) {
            Some(i) => self.click_slot(ui, i, button, if ui.shift_down { 1 } else { 0 }),
            None if outside => {
                // Slot -999: drop the cursor stack (or one item).
                let mut w = self.window.lock().unwrap();
                if let Some(c) = w.cursor {
                    w.cursor = if right { (c.count > 1).then_some(ItemStack { count: c.count - 1, ..c }) } else { None };
                    w.clicks.push(Click { slot: -999, button, mode: 0, item: None });
                }
            }
            None => {}
        }
        Action::None
    }
}

impl Screen for ContainerScreen {
    fn entity_preview(&self) -> Option<(i32, i32, i32)> {
        let l = Self::layout(&self.window.lock().unwrap());
        (l.texture == "textures/gui/container/inventory.png").then_some((self.left + 51, self.top + 75, 30))
    }
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        let l = Self::layout(&self.window.lock().unwrap());
        self.is_player = self.window.lock().unwrap().kind == "player";
        self.left = if self.is_player { ui.effect_gui_left(w, l.w) } else { (w - l.w) / 2 };
        self.top = (h - l.h) / 2;
    }
    fn pauses(&self) -> bool {
        false
    }
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        mc_gui::screens::default_background(ui, b, w, h);
        let win = self.window.lock().unwrap();
        let l = Self::layout(&win);
        if self.is_player {
            // GuiInventory.updateScreen re-runs updateActivePotionEffects.
            self.left = ui.effect_gui_left(w, l.w);
        }
        let (x, y) = (self.left, self.top);
        b.color = [1.0; 4];
        match l.rows {
            Some(rows) => {
                b.textured_rect(l.texture, x, y, 0, 0, l.w, rows * 18 + 17);
                b.textured_rect(l.texture, x, y + rows * 18 + 17, 0, 126, l.w, 96);
            }
            None => b.textured_rect(l.texture, x, y, 0, 0, l.w, l.h),
        }
        if win.kind == "minecraft:furnace" {
            // GuiFurnace: flame (burn time left) and arrow (cook progress).
            let (burn, total_burn, cook, total_cook) = (win.props[0] as i32, win.props[1] as i32, win.props[2] as i32, win.props[3] as i32);
            if burn > 0 {
                let k = burn * 13 / if total_burn == 0 { 200 } else { total_burn };
                b.textured_rect(l.texture, x + 56, y + 36 + 12 - k, 176, 12 - k, 14, k + 1);
            }
            let p = if total_cook != 0 && cook != 0 { cook * 24 / total_cook } else { 0 };
            b.textured_rect(l.texture, x + 79, y + 34, 176, 14, p + 1, 16);
        }
        let title_x = match win.kind.as_str() {
            "minecraft:furnace" | "minecraft:dispenser" | "minecraft:dropper" => l.w / 2 - ui.font.width(&win.title) / 2,
            "minecraft:crafting_table" => 28,
            _ => 8,
        };
        let title = win.title.clone();
        if win.kind == "player" {
            // GuiInventory: only "Crafting" above the grid.
            ui.font.draw(b, &title, (x + 86) as f32, (y + 16) as f32, 0xff404040);
        } else {
            ui.font.draw(b, &title, (x + title_x) as f32, (y + 6) as f32, 0xff404040);
            let inv = ui.lang.get("container.inventory");
            ui.font.draw(b, &inv, (x + 8) as f32, (y + l.h - 96 + 2) as f32, 0xff404040);
        }
        let cursor = win.cursor;
        drop(win);
        self.hover = self.at(&l, mx, my);
        let pos = Self::positions(&l);
        for (i, &(sx, sy)) in pos.iter().enumerate() {
            let s = self.window.lock().unwrap().get(ui, i).and_then(|s| *s);
            if let Some(s) = s {
                self.catalogue.draw(ui, b, s, x + sx, y + sy);
            }
            if self.hover == Some(i) {
                b.rect(x + sx, y + sy, x + sx + 16, y + sy + 16, 0x80ffffff);
            }
        }
        if let Some(s) = cursor {
            self.catalogue.draw(ui, b, s, mx - 8, my - 8);
        } else if let Some(s) = self.hover.and_then(|h| self.window.lock().unwrap().get(ui, h).and_then(|s| *s)) {
            let lines = crate::tooltip::render_lines(&self.catalogue, &ui.lang, &s, ui.settings.advanced_item_tooltips);
            ui.draw_hovering_text(b, &lines, mx, my, w, h);
        }
        if self.is_player {
            ui.draw_effects(b, x, y);
        }
    }
    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.mouse(ui, mx, my, false)
    }
    fn right_click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.mouse(ui, mx, my, true)
    }
    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        match key {
            Key::Escape | Key::Inventory => {
                self.window.lock().unwrap().closed = true;
                Action::Close
            }
            Key::Number(n) => {
                if self.window.lock().unwrap().cursor.is_none() {
                    if let Some(i) = self.hover {
                        self.click_slot(ui, i, n as i8, 2);
                    }
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_slot_click_traces() {
        let Some(jar) = crate::find_jar() else {
            assert_ne!(std::env::var("RUSTMC_REQUIRE_GOLDEN").as_deref(), Ok("1"), "slot-click replay needs the vanilla jar");
            eprintln!("skipping Java slot-click replay: vanilla jar not found");
            return;
        };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("../tests/data/containers.json")).unwrap();
        let decode = |v: &serde_json::Value| -> Option<ItemStack> {
            let a = v.as_array()?;
            let id = a[0].as_u64().unwrap() as u16;
            let damage = a[2].as_i64().unwrap() as i16;
            Some(ItemStack::new(cat.index_of(id, damage as u16).or_else(|| cat.index_of(id, 0)).unwrap(), a[1].as_u64().unwrap() as u8, damage))
        };
        for fixture in fixtures.as_array().unwrap() {
            let kind = fixture["kind"].as_str().unwrap();
            let name = fixture["name"].as_str().unwrap();
            let w = Arc::new(Mutex::new(Window::new(1, kind, String::new(), if kind == "player" { 5 } else if kind == "minecraft:furnace" { 3 } else { 27 })));
            let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
            for initial in fixture["initial"].as_array().unwrap() {
                let row = initial.as_array().unwrap();
                let s = decode(&serde_json::Value::Array(row[1..].to_vec()));
                w.lock().unwrap().set(&mut ui, row[0].as_u64().unwrap() as usize, s);
            }
            let mut screen = ContainerScreen::new(w.clone(), cat.clone());
            for step in fixture["steps"].as_array().unwrap() {
                let slot = step["slot"].as_u64().unwrap() as usize;
                screen.click_slot(&mut ui, slot, step["button"].as_i64().unwrap() as i8, step["mode"].as_i64().unwrap() as i8);
                let mut w = w.lock().unwrap();
                let returned = w.clicks.last().unwrap().item;
                assert_eq!(returned, decode(&step["returned"]), "{name} slot {slot}: return");
                assert_eq!(w.cursor, decode(&step["cursor"]), "{name} slot {slot}: cursor");
                for (i, expected) in step["slots"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(*w.get(&mut ui, i).unwrap(), decode(expected), "{name} slot {slot}: inventory {i}");
                }
            }
        }
    }

    #[test]
    fn chest_clicks_predict_and_queue_packets() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let stone = cat.index_of(1, 0).unwrap();
        let w = Arc::new(Mutex::new(Window::new(2, "minecraft:chest", "Chest".into(), 27)));
        w.lock().unwrap().slots[0] = Some(ItemStack::new(stone, 40, 0));
        let mut s = ContainerScreen::new(w.clone(), cat.clone());
        // Right click picks up half; the packet carries the pre-click stack.
        s.click_slot(&mut ui, 0, 1, 0);
        {
            let w = w.lock().unwrap();
            assert_eq!(w.cursor.unwrap().count, 20);
            assert_eq!(w.slots[0].unwrap().count, 20);
            assert_eq!(w.clicks[0], Click { slot: 0, button: 1, mode: 0, item: Some(ItemStack::new(stone, 40, 0)) });
        }
        // Drop it into the first hotbar slot (window slot 27 + 27 = 54).
        s.click_slot(&mut ui, 54, 0, 0);
        assert_eq!(ui.inventory[0].unwrap().count, 20);
        // Shift-click the rest from the chest: fills the inventory back to front, hotbar 8 first.
        s.click_slot(&mut ui, 0, 0, 1);
        assert!(w.lock().unwrap().slots[0].is_none());
        assert_eq!(ui.inventory[0].unwrap().count, 40);
        assert_eq!(w.lock().unwrap().clicks.len(), 3);
    }

    #[test]
    fn player_window_maps_container_player_slots() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let helmet = cat.find("minecraft:iron_helmet", 0).unwrap();
        ui.inventory[0] = Some(ItemStack::new(helmet, 1, 0));
        let w = Arc::new(Mutex::new(Window::new(0, "player", "Crafting".into(), 5)));
        let mut s = ContainerScreen::new(w.clone(), cat);
        // Hotbar 0 is window slot 36; shift-click equips the helmet into slot 5.
        s.click_slot(&mut ui, 36, 0, 1);
        assert!(ui.inventory[0].is_none());
        assert_eq!(ui.armor[0].map(|a| a.item), Some(helmet));
        assert_eq!(w.lock().unwrap().clicks[0].slot, 36);
    }
}
