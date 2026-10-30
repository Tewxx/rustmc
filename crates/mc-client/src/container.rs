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
    pub props: [i16; 7],
    pub clicks: Vec<Click>,
    pub closed: bool,
    /// `Container.transactionID`
    pub transaction: i16,
    pub level: i32,
    drag: Option<(i8, Vec<usize>)>,
}

impl Window {
    /// `OpenWindow`: `slots` is the storage size; non-storage windows have fixed sizes.
    pub fn new(id: u8, kind: &str, title: String, slots: u8) -> Window {
        let n = match kind {
            "minecraft:crafting_table" => 10,
            "minecraft:enchanting_table" => 2,
            "minecraft:anvil" => 3,
            "minecraft:beacon" => 1,
            "minecraft:brewing_stand" => 4,
            _ => slots as usize,
        };
        Window { id, kind: kind.into(), title, slots: vec![None; n], cursor: None, props: [0, 0, 0, 0, -1, -1, -1], clicks: Vec::new(), closed: false, transaction: 0, level: 0, drag: None }
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
            "minecraft:furnace" | "minecraft:anvil" => i == 2,
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
    last_click: Option<(usize, bool, std::time::Instant)>,
    dragging: Option<(u8, Vec<usize>)>,
    pub(crate) creative: bool,
    shift_clicked: Option<ItemStack>,
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
        Self { window, catalogue, left: 0, top: 0, hover: None, is_player: false, last_click: None, dragging: None, creative: false, shift_clicked: None }
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
            "minecraft:anvil" => Layout {
                texture: "textures/gui/container/anvil.png", w: 176, h: 166,
                slots: vec![(27, 47), (76, 47), (134, 47)], inv: (8, 84), rows: None,
            },
            "minecraft:brewing_stand" => Layout {
                texture: "textures/gui/container/brewing_stand.png", w: 176, h: 166,
                slots: vec![(56, 46), (79, 53), (102, 46), (79, 17)], inv: (8, 84), rows: None,
            },
            "minecraft:enchanting_table" => Layout {
                texture: "textures/gui/container/enchanting_table.png", w: 176, h: 166,
                slots: vec![(15, 47), (35, 47)], inv: (8, 84), rows: None,
            },
            "minecraft:beacon" => Layout {
                texture: "textures/gui/container/beacon.png", w: 230, h: 219,
                slots: vec![(136, 110)], inv: (36, 137), rows: None,
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

    fn brewing_ingredient(&self, s: ItemStack) -> bool {
        let id = self.catalogue.items[s.item].id as u64;
        Self::furnace_rules()["brewing"].as_array().unwrap().iter().any(|v| {
            v[0].as_u64() == Some(id) && (v[1].as_i64() == Some(32767) || v[1].as_i64() == Some(s.damage as i64))
        })
    }

    fn accepts(&self, w: &Window, i: usize, s: ItemStack) -> bool {
        if w.output_slot(i) { return false; }
        if w.kind == "minecraft:enchanting_table" && i == 1 {
            return self.catalogue.items[s.item].id == 351 && s.damage == 4;
        }
        if w.kind == "minecraft:brewing_stand" && i < 4 {
            return if i < 3 { matches!(self.catalogue.items[s.item].id, 373 | 374) } else { self.brewing_ingredient(s) };
        }
        if w.kind == "minecraft:beacon" && i == 0 {
            return matches!(self.catalogue.items[s.item].id, 264 | 265 | 266 | 388);
        }
        if w.kind == "minecraft:furnace" && i == 1 {
            return self.fuel(s) || self.catalogue.items[s.item].id == 325;
        }
        if w.kind == "player" && (5..=8).contains(&i) {
            return crate::item_use::armor_slot(self.catalogue.items[s.item].id) == Some(8 - i);
        }
        true
    }

    fn can_enchant(&self, w: &Window, choice: usize) -> bool {
        let cost = w.props[choice] as i32;
        let lapis = w.slots[1].map_or(0, |s| s.count as usize);
        let needed = choice + 1;
        cost > 0 && w.slots[0].is_some() && (self.creative || lapis >= needed && w.level >= needed as i32 && w.level >= cost)
    }

    fn can_take(&self, w: &Window, i: usize) -> bool {
        w.kind != "minecraft:anvil" || i != 2 ||
            (w.props[0] > 0 && (self.creative || w.level >= w.props[0] as i32))
    }

    fn crafting_grid(w: &Window) -> Option<usize> {
        match w.kind.as_str() { "player" => Some(2), "minecraft:crafting_table" => Some(3), _ => None }
    }

    fn recipes() -> &'static serde_json::Value {
        static DATA: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
        DATA.get_or_init(|| serde_json::from_str(include_str!("crafting_recipes.json")).unwrap())
    }

    fn recraft(&self, w: &mut Window, ui: &mut Ui) {
        let Some(size) = Self::crafting_grid(w) else { return };
        let grid: Vec<_> = (1..=size * size).map(|i| *w.get(ui, i).unwrap()).collect();
        let matches = |input: &serde_json::Value, actual: Option<ItemStack>| -> bool {
            match (input.as_array(), actual) {
                (None, None) => true,
                (Some(a), Some(s)) => self.catalogue.items[s.item].id == a[0].as_u64().unwrap() as u16 &&
                    (a[2].as_i64().unwrap() == 32767 || s.damage as i64 == a[2].as_i64().unwrap()),
                _ => false,
            }
        };
        let mut result = None;
        let items: Vec<_> = grid.iter().flatten().copied().collect();
        if items.len() == 2 && items.iter().all(|s| s.count == 1) {
            let id = self.catalogue.items[items[0].item].id;
            if id == self.catalogue.items[items[1].item].id {
                if let Some(item) = mc_data::item::Item::by_id(id).filter(|i| i.is_damageable()) {
                    let max = item.max_damage as i32;
                    let damage = (items[0].damage as i32 + items[1].damage as i32 - max - max * 5 / 100).max(0) as i16;
                    result = Some(ItemStack::new(items[0].item, 1, damage));
                }
            }
        }
        for recipe in Self::recipes()["recipes"].as_array().unwrap() {
            if result.is_some() { break; }
            let inputs = recipe["inputs"].as_array().unwrap();
            let matched = if let Some(width) = recipe["width"].as_u64() {
                let width = width as usize;
                let height = recipe["height"].as_u64().unwrap() as usize;
                (0..=3-width).any(|x| (0..=3-height).any(|y| [true, false].into_iter().any(|mirror| {
                    (0..3).all(|gy| (0..3).all(|gx| {
                        let actual = if gx < size && gy < size { grid[gy * size + gx] } else { None };
                        let expected = if gx >= x && gx < x + width && gy >= y && gy < y + height {
                            let dx = gx - x; &inputs[(gy-y)*width + if mirror { width-dx-1 } else { dx }]
                        } else { &serde_json::Value::Null };
                        matches(expected, actual)
                    }))
                })))
            } else {
                let mut remaining = inputs.clone();
                let mut ok = true;
                for item in grid.iter().flatten() {
                    if let Some(i) = remaining.iter().position(|r| matches(r, Some(*item))) { remaining.remove(i); }
                    else { ok = false; break; }
                }
                ok && remaining.is_empty()
            };
            if matched {
                let output = recipe["output"].as_array().unwrap();
                let id = output[0].as_u64().unwrap() as u16;
                let damage = output[2].as_i64().unwrap() as i16;
                result = self.catalogue.index_of(id, damage as u16).or_else(|| self.catalogue.index_of(id, 0)).map(|item| {
                    let mut stack = ItemStack::new(item, output[1].as_u64().unwrap() as u8, damage);
                    if recipe["copy_nbt"].as_bool() == Some(true) { stack.tag = grid.iter().flatten().filter_map(|s| s.tag).last(); }
                    stack
                });
                break;
            }
        }
        w.set(ui, 0, result);
    }

    fn crafted(&self, w: &mut Window, ui: &mut Ui) {
        let Some(size) = Self::crafting_grid(w) else { return };
        for i in 1..=size * size {
            let Some(stack) = *w.get(ui, i).unwrap() else { continue };
            let id = self.catalogue.items[stack.item].id;
            let remainder = Self::recipes()["containers"].as_array().unwrap().iter().find_map(|r| {
                (r[0].as_u64() == Some(id as u64)).then(|| r[1].as_u64().unwrap() as u16)
            }).and_then(|id| self.catalogue.index_of(id, 0)).map(|item| ItemStack::new(item, 1, 0));
            w.set(ui, i, (stack.count > 1).then_some(ItemStack { count: stack.count - 1, ..stack }));
            if let Some(remainder) = remainder {
                if w.get(ui, i).unwrap().is_none() { w.set(ui, i, Some(remainder)); }
                else {
                    let range = if w.kind == "player" { (9..45).collect() } else { (10..46).collect() };
                    self.merge(w, ui, remainder, range);
                }
            }
        }
        self.recraft(w, ui);
    }

    /// `Container.slotClick` mode 0 (`PICKUP`) on window slot `i`.
    fn pickup(&self, w: &mut Window, ui: &mut Ui, i: usize, right: bool) {
        if !self.can_take(w, i) && w.get(ui, i).is_some_and(|s| s.is_some()) { return; }
        let cursor = w.cursor;
        let valid = cursor.is_some_and(|c| self.accepts(w, i, c));
        let slot_limit = if (w.kind == "minecraft:brewing_stand" && i < 3) || (w.kind == "minecraft:enchanting_table" && i == 0) || (w.kind == "minecraft:beacon" && i == 0) || (w.kind == "player" && (5..=8).contains(&i)) ||
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
        if !self.can_take(w, i) { return None; }
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
        if w.kind == "minecraft:anvil" {
            let range = if i == 2 { (3..39).rev().collect() }
                else if i < 2 { (3..39).collect() }
                else { (0..2).collect() };
            let rest = self.merge(w, ui, s, range);
            w.set(ui, i, rest);
            return (rest.map_or(0, |r| r.count) != s.count).then_some(s);
        }
        if w.kind == "minecraft:enchanting_table" {
            let rest = if i < 2 { self.merge(w, ui, s, (2..38).rev().collect()) }
                else if self.catalogue.items[s.item].id == 351 && s.damage == 4 { self.merge(w, ui, s, vec![1]) }
                else if w.slots[0].is_none() {
                    let placed = if s.count == 1 { s } else { ItemStack { count: 1, tag: None, obj: ItemStack::fresh_obj(), ..s } };
                    w.slots[0] = Some(placed);
                    (s.count > 1).then_some(ItemStack { count: s.count - 1, ..s })
                } else { Some(s) };
            w.set(ui, i, rest);
            return (rest.map_or(0, |r| r.count) != s.count).then_some(s);
        }
        if w.kind == "minecraft:brewing_stand" {
            let range = if i < 4 { (4..40).rev().collect() }
                else if w.slots[3].is_none() && self.brewing_ingredient(s) { vec![3] }
                else if matches!(self.catalogue.items[s.item].id, 373 | 374) { (0..3).collect() }
                else if i < 31 { (31..40).collect() }
                else { (4..31).collect() };
            let rest = self.merge(w, ui, s, range);
            w.set(ui, i, rest);
            return (rest.map_or(0, |r| r.count) != s.count).then_some(s);
        }
        if w.kind == "minecraft:crafting_table" {
            let range = if i == 0 { (10..46).rev().collect() }
                else if i < 10 { (10..46).collect() }
                else if i < 37 { (37..46).collect() }
                else { (10..37).collect() };
            let rest = self.merge(w, ui, s, range);
            w.set(ui, i, rest);
            return (rest.map_or(0, |r| r.count) != s.count).then_some(s);
        }
        if w.kind == "minecraft:beacon" {
            let range = if i == 0 { (1..37).rev().collect() }
                else if s.count == 1 && self.accepts(w, 0, s) && w.slots[0].is_none() { vec![0] }
                else if i < 28 { (28..37).collect() }
                else { (1..28).collect() };
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

    fn drag_click(&mut self, ui: &mut Ui, slot: i16, button: i8) {
        let mut w = self.window.lock().unwrap();
        let stage = button & 3;
        let mode = (button >> 2) & 3;
        if w.cursor.is_none() {
            w.drag = None;
        } else if stage == 0 && w.drag.is_none() && (mode <= 1 || mode == 2 && self.creative) {
            w.drag = Some((mode, Vec::new()));
        } else if stage == 1 && w.drag.as_ref().is_some_and(|(m, _)| *m == mode) && slot >= 0 {
            let i = slot as usize;
            let cursor = w.cursor.unwrap();
            let existing = w.get(ui, i).map(|v| *v);
            let can_add = existing.is_some_and(|v| v.is_none_or(|s| s.stacks_with(&cursor) && s.count <= self.max(cursor)))
                && self.accepts(&w, i, cursor);
            if can_add {
                let (_, slots) = w.drag.as_mut().unwrap();
                if slots.len() < cursor.count as usize && !slots.contains(&i) { slots.push(i); }
            }
        } else if stage == 2 && w.drag.as_ref().is_some_and(|(m, _)| *m == mode) {
            let (_, slots) = w.drag.take().unwrap();
            let crafting_changed = Self::crafting_grid(&w).is_some_and(|n| slots.iter().any(|&i| i >= 1 && i <= n*n));
            let mut cursor = w.cursor.unwrap();
            if !slots.is_empty() {
                let amount = if mode == 0 { cursor.count / slots.len() as u8 } else if mode == 1 { 1 } else { self.max(cursor) };
                let mut left = cursor.count as i16;
                for i in slots {
                    let current = w.get(ui, i).and_then(|v| *v);
                    let old = current.map_or(0, |s| s.count);
                    let limit = if w.kind == "minecraft:brewing_stand" && i < 3 || w.kind == "minecraft:enchanting_table" && i == 0 || w.kind == "minecraft:beacon" && i == 0 || w.kind == "player" && (5..=8).contains(&i) ||
                        w.kind == "minecraft:furnace" && i == 1 && self.catalogue.items[cursor.item].id == 325 { 1 } else { 64 };
                    let new_count = old.saturating_add(amount).min(limit).min(self.max(cursor));
                    left -= (new_count - old) as i16;
                    w.set(ui, i, Some(ItemStack { count: new_count, ..cursor }));
                }
                cursor.count = left.max(0) as u8;
                w.cursor = (left > 0).then_some(cursor);
                if crafting_changed { self.recraft(&mut w, ui); }
            }
        } else {
            w.drag = None;
        }
        w.clicks.push(Click { slot, button, mode: 5, item: None });
    }

    #[cfg(test)]
    pub(crate) fn replay_click(&mut self, ui: &mut Ui, slot: i16, button: i8, mode: i8) {
        if slot == -999 && mode == 0 && self.window.lock().unwrap().cursor.is_some() {
            self.mouse(ui, -1, -1, button == 1);
            self.release_at(ui, -1, -1, button as u8);
        } else {
            self.click_slot(ui, slot as usize, button, mode);
        }
    }

    fn click_slot(&mut self, ui: &mut Ui, i: usize, button: i8, mode: i8) {
        let mut w = self.window.lock().unwrap();
        let item = w.get(ui, i).and_then(|s| *s);
        let grid_size = Self::crafting_grid(&w);
        let grid_before: Vec<_> = grid_size.map(|n| (1..=n*n).map(|j| *w.get(ui, j).unwrap()).collect()).unwrap_or_default();
        if w.drag.take().is_some() {
            w.clicks.push(Click { slot: i as i16, button, mode, item: None });
            return;
        }
        let returned = match mode {
            0 => { self.pickup(&mut w, ui, i, button == 1); item }
            1 if i == 0 && grid_size.is_some() => {
                let first = self.transfer(&mut w, ui, i);
                if let Some(original) = first {
                    loop {
                        self.crafted(&mut w, ui);
                        if !w.slots[0].is_some_and(|s| s.item == original.item) || self.transfer(&mut w, ui, i).is_none() { break; }
                    }
                }
                first
            }
            1 => self.transfer(&mut w, ui, i),
            2 if (0..9).contains(&button) && self.can_take(&w, i) => {
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
            3 if self.creative && w.cursor.is_none() => {
                if let Some(s) = item { w.cursor = Some(ItemStack { count: self.max(s), obj: ItemStack::fresh_obj(), ..s }); }
                None
            }
            4 if w.cursor.is_none() && self.can_take(&w, i) => {
                if let Some(stack) = item {
                    let count = if button == 0 { 1 } else { stack.count };
                    w.set(ui, i, (stack.count > count).then_some(ItemStack { count: stack.count - count, ..stack }));
                }
                None
            }
            6 => {
                if item.is_none() || !self.can_take(&w, i) {
                    if let Some(mut cursor) = w.cursor {
                        let max = self.max(cursor);
                        for pass in 0..2 {
                            let mut indices: Vec<usize> = (0..w.total()).collect();
                            if button != 0 { indices.reverse(); }
                            for index in indices {
                                if index == 0 && (w.kind == "player" || w.kind == "minecraft:crafting_table") { continue; }
                                if !self.can_take(&w, index) { continue; }
                                let Some(stack) = w.get(ui, index).and_then(|v| *v) else { continue };
                                if !stack.stacks_with(&cursor) || (pass == 0 && stack.count == self.max(stack)) { continue; }
                                let take = stack.count.min(max.saturating_sub(cursor.count));
                                if take == 0 { break; }
                                cursor.count += take;
                                w.set(ui, index, (stack.count > take).then_some(ItemStack { count: stack.count - take, ..stack }));
                            }
                        }
                        w.cursor = Some(cursor);
                    }
                }
                None
            }
            _ => None,
        };
        if i == 0 && grid_size.is_some() && mode != 1 && item.is_some() {
            let remaining = w.get(ui, 0).and_then(|s| *s);
            if remaining.map_or(0, |s| s.count) < item.unwrap().count && matches!(mode, 0 | 2 | 4) { self.crafted(&mut w, ui); }
        }
        if let Some(n) = grid_size {
            let grid_after: Vec<_> = (1..=n*n).map(|j| *w.get(ui, j).unwrap()).collect();
            if grid_before != grid_after { self.recraft(&mut w, ui); }
        }
        w.clicks.push(Click { slot: i as i16, button, mode, item: returned });
    }

    fn beacon_buttons(w: &Window) -> Vec<(i32, i32, i16, i16)> {
        let mut buttons = vec![(53, 22, 1, 0), (77, 22, 3, 0), (53, 47, 11, 1), (77, 47, 8, 1), (65, 72, 5, 2), (144, 47, 10, 3)];
        if w.props[1] > 0 { buttons.push((168, 47, w.props[1], 3)); }
        buttons
    }

    fn mouse(&mut self, ui: &mut Ui, mx: i32, my: i32, right: bool) -> Action {
        let l = Self::layout(&self.window.lock().unwrap());
        if !right {
            let mut w = self.window.lock().unwrap();
            if w.kind == "minecraft:beacon" {
                let x = mx - self.left; let y = my - self.top;
                if (164..186).contains(&x) && (107..129).contains(&y) && w.slots[0].is_some() && w.props[1] > 0 {
                    return Action::Beacon { window_id: w.id, primary: w.props[1] as i32, secondary: w.props[2] as i32 };
                }
                if (190..212).contains(&x) && (107..129).contains(&y) { w.closed = true; return Action::Close; }
                for (bx, by, effect, tier) in Self::beacon_buttons(&w) {
                    if (bx..bx + 22).contains(&x) && (by..by + 22).contains(&y) {
                        if tier < w.props[0] { w.props[if tier < 3 { 1 } else { 2 }] = effect; }
                        return Action::None;
                    }
                }
            }
            if w.kind == "minecraft:enchanting_table" {
                for choice in 0..3 {
                    if (self.left + 60..self.left + 168).contains(&mx) && (self.top + 14 + 19 * choice..self.top + 33 + 19 * choice).contains(&my) {
                        if self.can_enchant(&w, choice as usize) { return Action::Enchant { window_id: w.id, choice: choice as u8 }; }
                        return Action::None;
                    }
                }
            }
        }
        let outside = mx < self.left || my < self.top || mx >= self.left + l.w || my >= self.top + l.h;
        let button = right as i8;
        match self.at(&l, mx, my) {
            Some(i) => {
                let now = std::time::Instant::now();
                let double = !right && self.last_click.is_some_and(|(slot, previous_right, at)|
                    slot == i && !previous_right && now.duration_since(at).as_millis() < 250);
                self.last_click = if double { None } else { Some((i, right, now)) };
                if double && ui.shift_down {
                    self.shift_clicked = self.window.lock().unwrap().get(ui, i).and_then(|slot| *slot);
                    self.click_slot(ui, i, button, 1);
                    if let Some(shifted) = self.shift_clicked {
                        let candidates = {
                            let mut w = self.window.lock().unwrap();
                            let group = |index: usize| -> usize {
                                match w.kind.as_str() {
                                    "player" => if index == 0 { 0 } else if index < 5 { 1 } else { 2 },
                                    "minecraft:crafting_table" => if index == 0 { 0 } else if index < 10 { 1 } else { 2 },
                                    _ => if index < w.slots.len() { 0 } else { 1 },
                                }
                            };
                            let target = group(i);
                            let same_inventory: Vec<usize> = (0..w.total()).filter(|&j| group(j) == target).collect();
                            same_inventory.into_iter().filter(|&j| w.get(ui, j).and_then(|slot| *slot).is_some_and(|stack| stack.stacks_with(&shifted))).collect::<Vec<_>>()
                        };
                        for j in candidates { self.click_slot(ui, j, button, 1); }
                    }
                } else if double {
                    self.click_slot(ui, i, button, 6);
                } else if self.window.lock().unwrap().cursor.is_some() {
                    self.dragging = Some((right as u8, Vec::new()));
                } else {
                    if ui.shift_down { self.shift_clicked = self.window.lock().unwrap().get(ui, i).and_then(|slot| *slot); }
                    self.click_slot(ui, i, button, if ui.shift_down { 1 } else { 0 });
                }
            }
            None if outside => {
                if self.window.lock().unwrap().cursor.is_some() {
                    self.dragging = Some((right as u8, Vec::new()));
                }
            }
            None => {}
        }
        Action::None
    }
}

impl Screen for ContainerScreen {
    fn mouse_move(&mut self, ui: &mut Ui, mx: i32, my: i32) {
        let Some((_, slots)) = self.dragging.as_ref() else { return };
        let w = self.window.lock().unwrap();
        let Some(cursor) = w.cursor else { return };
        if slots.len() >= cursor.count as usize { return; }
        let layout = Self::layout(&w);
        drop(w);
        let Some(i) = self.at(&layout, mx, my) else { return };
        if slots.contains(&i) { return; }
        let mut w = self.window.lock().unwrap();
        let existing = w.get(ui, i).map(|v| *v);
        let can_add = existing.is_some_and(|v| v.is_none_or(|s| s.stacks_with(&cursor) && s.count <= self.max(cursor))) && self.accepts(&w, i, cursor);
        drop(w);
        if can_add { self.dragging.as_mut().unwrap().1.push(i); }
    }

    fn release_at(&mut self, ui: &mut Ui, mx: i32, my: i32, button: u8) {
        let Some((drag_button, slots)) = self.dragging.take() else { return };
        if button != drag_button { return; }
        let right = button == 1;
        if !slots.is_empty() {
            let mode = button as i8 * 4;
            self.drag_click(ui, -999, mode);
            for i in slots { self.drag_click(ui, i as i16, mode + 1); }
            self.drag_click(ui, -999, mode + 2);
        } else {
            let layout = Self::layout(&self.window.lock().unwrap());
            if let Some(i) = self.at(&layout, mx, my) {
                self.click_slot(ui, i, button as i8, if button == 2 { 3 } else if ui.shift_down { 1 } else { 0 });
            } else if mx < self.left || my < self.top || mx >= self.left + layout.w || my >= self.top + layout.h {
                let mut w = self.window.lock().unwrap();
                if let Some(c) = w.cursor {
                    w.cursor = if right { (c.count > 1).then_some(ItemStack { count: c.count - 1, ..c }) } else { None };
                    w.clicks.push(Click { slot: -999, button: right as i8, mode: 0, item: None });
                }
            }
        }
    }

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
        if win.kind == "minecraft:beacon" {
            for (bx, by, effect, tier) in Self::beacon_buttons(&win) {
                let enabled = tier < win.props[0];
                let selected = win.props[if tier < 3 { 1 } else { 2 }] == effect;
                let hover = (x + bx..x + bx + 22).contains(&mx) && (y + by..y + by + 22).contains(&my);
                let u = if !enabled { 44 } else if selected { 22 } else if hover { 66 } else { 0 };
                b.textured_rect(l.texture, x + bx, y + by, u, 219, 22, 22);
                if let Some(p) = mc_data::potion::POTIONS.iter().find(|p| p.id == effect as u8) {
                    b.textured_rect("textures/gui/container/inventory.png", x + bx + 2, y + by + 2, p.icon % 8 * 18, 198 + p.icon / 8 * 18, 18, 18);
                }
            }
            for (bx, icon, enabled) in [(164, 90, win.slots[0].is_some() && win.props[1] > 0), (190, 112, true)] {
                let hover = (x + bx..x + bx + 22).contains(&mx) && (y + 107..y + 129).contains(&my);
                let u = if !enabled { 44 } else if hover { 66 } else { 0 };
                b.textured_rect(l.texture, x + bx, y + 107, u, 219, 22, 22);
                b.textured_rect(l.texture, x + bx + 2, y + 109, icon, 220, 18, 18);
            }
            for (n, id) in [388, 264, 266, 265].iter().enumerate() {
                if let Some(item) = self.catalogue.index_of(*id, 0) {
                    self.catalogue.draw(ui, b, ItemStack::new(item, 1, 0), x + 42 + n as i32 * 22, y + 109);
                }
            }
            let primary = ui.lang.get("tile.beacon.primary");
            let secondary = ui.lang.get("tile.beacon.secondary");
            ui.font.draw_centered(b, &primary, x + 62, y + 10, 0xffe0e0e0);
            ui.font.draw_centered(b, &secondary, x + 169, y + 10, 0xffe0e0e0);
        }
        if win.kind == "minecraft:enchanting_table" {
            for choice in 0..3 {
                let cost = win.props[choice];
                let hovered = (x + 60..x + 168).contains(&mx) && (y + 14 + 19 * choice as i32..y + 33 + 19 * choice as i32).contains(&my);
                let enabled = self.can_enchant(&win, choice);
                let v = if cost == 0 || !enabled { 185 } else if hovered { 204 } else { 166 };
                b.textured_rect(l.texture, x + 60, y + 14 + 19 * choice as i32, 0, v, 108, 19);
                if cost > 0 {
                    let text = cost.to_string();
                    let color = if enabled { 0xff80ff20 } else { 0xff404020 };
                    let px = x + 154 - ui.font.width(&text);
                    ui.font.draw(b, &text, px as f32, (y + 23 + 19 * choice as i32) as f32, color);
                }
            }
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
            "minecraft:furnace" | "minecraft:brewing_stand" | "minecraft:dispenser" | "minecraft:dropper" => l.w / 2 - ui.font.width(&win.title) / 2,
            "minecraft:crafting_table" => 28,
            _ => 8,
        };
        let title = win.title.clone();
        if win.kind == "player" {
            // GuiInventory: only "Crafting" above the grid.
            ui.font.draw(b, &title, (x + 86) as f32, (y + 16) as f32, 0xff404040);
        } else if win.kind != "minecraft:beacon" {
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
    fn pick_item(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        let layout = Self::layout(&self.window.lock().unwrap());
        if let Some(i) = self.at(&layout, mx, my) {
            if self.creative && self.window.lock().unwrap().cursor.is_some() { self.dragging = Some((2, Vec::new())); }
            else { self.click_slot(ui, i, 2, 3); }
        }
        Action::None
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        match key {
            Key::Escape | Key::Inventory => {
                self.window.lock().unwrap().closed = true;
                Action::Close
            }
            Key::PickItem => {
                if let Some(i) = self.hover { self.click_slot(ui, i, 2, 3); }
                Action::None
            }
            Key::Drop => {
                if self.window.lock().unwrap().cursor.is_none() {
                    if let Some(i) = self.hover {
                        if self.window.lock().unwrap().get(ui, i).is_some_and(|slot| slot.is_some()) {
                            self.click_slot(ui, i, ui.ctrl_down as i8, 4);
                        }
                    }
                }
                Action::None
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
            {
                let mut win = w.lock().unwrap();
                win.level = fixture["level"].as_i64().unwrap() as i32;
                win.props[0] = fixture["initial_cost"].as_i64().unwrap_or(0) as i16;
                for (i, initial) in fixture["initial_slots"].as_array().unwrap().iter().enumerate() {
                    win.set(&mut ui, i, decode(initial));
                }
            }
            let mut screen = ContainerScreen::new(w.clone(), cat.clone());
            screen.creative = fixture["creative"].as_bool().unwrap_or(false);
            for step in fixture["steps"].as_array().unwrap() {
                let slot = step["slot"].as_i64().unwrap() as i16;
                let button = step["button"].as_i64().unwrap() as i8;
                let mode = step["mode"].as_i64().unwrap() as i8;
                if mode == 5 { screen.drag_click(&mut ui, slot, button); }
                else { screen.click_slot(&mut ui, slot as usize, button, mode); }
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
    fn beacon_selection_respects_pyramid_level_and_payment() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new(); assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let payment = cat.index_of(265, 0).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let w = Arc::new(Mutex::new(Window::new(7, "minecraft:beacon", String::new(), 0)));
        let mut screen = ContainerScreen::new(w.clone(), cat);
        w.lock().unwrap().props[0] = 1;
        screen.mouse(&mut ui, 54, 48, false);
        assert_eq!(w.lock().unwrap().props[1], 0);
        screen.mouse(&mut ui, 54, 23, false);
        assert_eq!(w.lock().unwrap().props[1], 1);
        assert!(matches!(screen.mouse(&mut ui, 165, 108, false), Action::None));
        w.lock().unwrap().slots[0] = Some(ItemStack::new(payment, 1, 0));
        w.lock().unwrap().props[0] = 4;
        screen.mouse(&mut ui, 145, 48, false);
        assert!(matches!(screen.mouse(&mut ui, 165, 108, false), Action::Beacon { window_id: 7, primary: 1, secondary: 10 }));
    }

    #[test]
    fn java_enchant_choices_and_screen_action() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new(); assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let sword = cat.index_of(267, 0).unwrap();
        let lapis = cat.index_of(351, 4).or_else(|| cat.index_of(351, 0)).unwrap();
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("../tests/data/enchant_choices.json")).unwrap();
        for row in fixtures.as_array().unwrap() {
            let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
            let w = Arc::new(Mutex::new(Window::new(7, "minecraft:enchanting_table", String::new(), 0)));
            {
                let mut win = w.lock().unwrap();
                win.level = row["level"].as_i64().unwrap() as i32;
                win.props[0] = row["cost"].as_i64().unwrap() as i16;
                win.slots[0] = Some(ItemStack::new(sword, 1, 0));
                let n = row["lapis"].as_u64().unwrap() as u8;
                if n > 0 { win.slots[1] = Some(ItemStack::new(lapis, n, 4)); }
            }
            let mut screen = ContainerScreen::new(w, cat.clone());
            screen.creative = row["creative"].as_bool().unwrap();
            let action = screen.mouse(&mut ui, 61, 15, false);
            assert_eq!(matches!(action, Action::Enchant { window_id: 7, choice: 0 }), row["allowed"].as_bool().unwrap());
        }
    }

    #[test]
    fn screen_double_click_and_drop_key_queue_vanilla_modes() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let stone = cat.index_of(1, 0).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let window = Arc::new(Mutex::new(Window::new(2, "minecraft:chest", String::new(), 27)));
        window.lock().unwrap().slots[0] = Some(ItemStack::new(stone, 8, 0));
        window.lock().unwrap().slots[1] = Some(ItemStack::new(stone, 10, 0));
        let mut screen = ContainerScreen::new(window.clone(), cat);
        screen.mouse(&mut ui, 9, 19, false);
        screen.mouse(&mut ui, 9, 19, false);
        assert_eq!(window.lock().unwrap().cursor.unwrap().count, 18);
        assert_eq!(window.lock().unwrap().clicks.last().unwrap().mode, 6);
        window.lock().unwrap().cursor = None;
        window.lock().unwrap().slots[1] = Some(ItemStack::new(stone, 10, 0));
        screen.hover = Some(1);
        ui.ctrl_down = true;
        screen.key(&mut ui, Key::Drop);
        let win = window.lock().unwrap();
        assert_eq!((win.clicks.last().unwrap().mode, win.clicks.last().unwrap().button), (4, 1));
        assert!(win.slots[1].is_none());
    }

    #[test]
    fn shift_double_click_does_not_repeat_when_source_is_empty() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let stone = cat.index_of(1, 0).unwrap();
        let other = cat.index_of(4, 0).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        ui.shift_down = true;
        let window = Arc::new(Mutex::new(Window::new(2, "minecraft:chest", String::new(), 27)));
        for (i, s) in [(0, ItemStack::new(stone, 10, 0)), (1, ItemStack::new(stone, 5, 0)), (2, ItemStack::new(other, 4, 0)), (3, ItemStack::new(stone, 2, 0))] {
            window.lock().unwrap().slots[i] = Some(s);
        }
        let mut screen = ContainerScreen::new(window.clone(), cat);
        screen.click(&mut ui, 9, 19);
        screen.click(&mut ui, 9, 19);
        let win = window.lock().unwrap();
        assert_eq!(win.clicks.iter().map(|c| c.slot).collect::<Vec<_>>(), [0, 0]);
        assert_eq!(win.slots[2].unwrap().item, other);
        assert_eq!(win.slots[1].unwrap().count, 5);
        assert_eq!(ui.inventory[8].unwrap().count, 10);
    }

    #[test]
    fn drag_splits_stacks_and_sends_vanilla_sequence() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let models = mc_render::models::BlockModels::load(&assets);
        let cat = Arc::new(Catalogue::load(&assets, &models));
        let stone = cat.index_of(1, 0).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let window = Arc::new(Mutex::new(Window::new(2, "minecraft:chest", String::new(), 27)));
        window.lock().unwrap().cursor = Some(ItemStack::new(stone, 9, 0));
        let mut screen = ContainerScreen::new(window.clone(), cat);
        screen.click(&mut ui, 27, 19);
        screen.mouse_move(&mut ui, 27, 19);
        screen.mouse_move(&mut ui, 45, 19);
        screen.release_at(&mut ui, 45, 19, 0);
        let win = window.lock().unwrap();
        assert_eq!(win.slots[1].unwrap().count, 4);
        assert_eq!(win.slots[2].unwrap().count, 4);
        assert_eq!(win.cursor.unwrap().count, 1);
        let sent: Vec<_> = win.clicks.iter().map(|c| (c.slot, c.button, c.mode)).collect();
        assert_eq!(sent, [(-999, 0, 5), (1, 1, 5), (2, 1, 5), (-999, 2, 5)]);
        drop(win);
        window.lock().unwrap().cursor = Some(ItemStack::new(stone, 64, 0));
        screen.creative = true;
        screen.pick_item(&mut ui, 63, 19);
        screen.mouse_move(&mut ui, 63, 19);
        screen.mouse_move(&mut ui, 81, 19);
        screen.release_at(&mut ui, 81, 19, 2);
        let win = window.lock().unwrap();
        assert_eq!(win.slots[3].unwrap().count, 64);
        assert_eq!(win.slots[4].unwrap().count, 64);
        assert!(win.cursor.is_none());
        assert_eq!(win.clicks.last().unwrap().button, 10);
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
