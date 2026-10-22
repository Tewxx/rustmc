//! `GuiInventory` over `ContainerPlayer`: the survival inventory opened with E.
use crate::inventory::Catalogue;
use mc_gui::{
    batch::GuiBatch,
    screens::{Action, Key, Screen},
    ItemStack, Ui,
};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    /// `InventoryPlayer` index (0..9 hotbar, 9..36 main).
    Inv(usize),
    Armor(usize),
    Craft(usize),
}

pub struct SurvivalInventory {
    catalogue: Arc<Catalogue>,
    left: i32,
    top: i32,
    pub cursor: Option<ItemStack>,
    hover: Option<Slot>,
}

const ARMOR_SUFFIX: [&str; 4] = ["_helmet", "_chestplate", "_leggings", "_boots"];

impl SurvivalInventory {
    pub fn new(catalogue: Arc<Catalogue>) -> Self {
        Self { catalogue, left: 0, top: 0, cursor: None, hover: None }
    }

    /// Container slots in `ContainerPlayer` order with their positions inside the 176×166 window.
    fn slots() -> Vec<(Slot, i32, i32)> {
        let mut v = Vec::new();
        for i in 0..4 {
            v.push((Slot::Craft(i), 88 + (i % 2) as i32 * 18, 26 + (i / 2) as i32 * 18));
        }
        for i in 0..4 {
            v.push((Slot::Armor(i), 8, 8 + i as i32 * 18));
        }
        for i in 9..36 {
            v.push((Slot::Inv(i), 8 + ((i - 9) % 9) as i32 * 18, 84 + ((i - 9) / 9) as i32 * 18));
        }
        for i in 0..9 {
            v.push((Slot::Inv(i), 8 + i as i32 * 18, 142));
        }
        v
    }

    fn at(&self, mx: i32, my: i32) -> Option<Slot> {
        let (x, y) = (mx - self.left, my - self.top);
        Self::slots()
            .into_iter()
            .find(|&(_, sx, sy)| (sx - 1..sx + 17).contains(&x) && (sy - 1..sy + 17).contains(&y))
            .map(|(s, _, _)| s)
    }

    fn get<'a>(ui: &'a mut Ui, s: Slot) -> &'a mut Option<ItemStack> {
        match s {
            Slot::Inv(i) => &mut ui.inventory[i],
            Slot::Armor(i) => &mut ui.armor[i],
            Slot::Craft(i) => &mut ui.crafting[i],
        }
    }

    fn max(&self, s: ItemStack) -> u8 {
        self.catalogue.items[s.item].max.max(1)
    }

    /// `Slot.isItemValid` / `getSlotStackLimit`
    fn accepts(&self, slot: Slot, s: ItemStack) -> bool {
        match slot {
            Slot::Armor(i) => {
                let name = &self.catalogue.items[s.item].name;
                name.ends_with(ARMOR_SUFFIX[i]) || i == 0 && name == "minecraft:pumpkin" || i == 0 && name == "minecraft:skull"
            }
            _ => true,
        }
    }

    fn limit(&self, slot: Slot, s: ItemStack) -> u8 {
        if matches!(slot, Slot::Armor(_)) {
            1
        } else {
            self.max(s)
        }
    }

    fn same(a: ItemStack, b: ItemStack) -> bool {
        a.stacks_with(&b)
    }

    /// `Container.slotClick` mode 0 (`PICKUP`).
    fn pickup(&mut self, ui: &mut Ui, slot: Slot, right: bool) {
        let cur = *Self::get(ui, slot);
        match (self.cursor, cur) {
            (None, None) => {}
            (None, Some(s)) => {
                let n = if right { s.count.div_ceil(2) } else { s.count };
                self.cursor = Some(ItemStack { count: n, ..s });
                *Self::get(ui, slot) = (s.count > n).then_some(ItemStack { count: s.count - n, ..s });
            }
            (Some(c), None) => {
                if !self.accepts(slot, c) {
                    return;
                }
                let n = if right { 1 } else { c.count.min(self.limit(slot, c)) };
                *Self::get(ui, slot) = Some(ItemStack { count: n, ..c });
                self.cursor = (c.count > n).then_some(ItemStack { count: c.count - n, ..c });
            }
            (Some(c), Some(s)) => {
                if Self::same(c, s) {
                    let room = self.limit(slot, s).saturating_sub(s.count);
                    let n = if right { 1.min(room) } else { c.count.min(room) };
                    *Self::get(ui, slot) = Some(ItemStack { count: s.count + n, ..s });
                    self.cursor = (c.count > n).then_some(ItemStack { count: c.count - n, ..c });
                } else if self.accepts(slot, c) && c.count <= self.limit(slot, c) {
                    *Self::get(ui, slot) = Some(c);
                    self.cursor = Some(s);
                }
            }
        }
    }

    /// `Container.mergeItemStack` over inventory indices in order.
    fn merge(&self, ui: &mut Ui, mut s: ItemStack, range: impl Iterator<Item = usize> + Clone) -> Option<ItemStack> {
        let max = self.max(s);
        for i in range.clone() {
            if let Some(t) = ui.inventory[i] {
                if Self::same(t, s) && t.count < max {
                    let n = s.count.min(max - t.count);
                    ui.inventory[i] = Some(ItemStack { count: t.count + n, ..t });
                    s.count -= n;
                    if s.count == 0 {
                        return None;
                    }
                }
            }
        }
        for i in range {
            if ui.inventory[i].is_none() {
                ui.inventory[i] = Some(s);
                return None;
            }
        }
        Some(s)
    }

    /// `ContainerPlayer.transferStackInSlot` (shift-click).
    fn transfer(&self, ui: &mut Ui, slot: Slot) {
        let Some(s) = *Self::get(ui, slot) else { return };
        let main = 9..36;
        let hotbar = 0..9;
        let rest = match slot {
            Slot::Craft(_) | Slot::Armor(_) => self.merge(ui, s, main.clone().chain(hotbar.clone())),
            Slot::Inv(i) => {
                let armor = (0..4).find(|&a| ui.armor[a].is_none() && self.accepts(Slot::Armor(a), s) && !matches!(self.catalogue.items[s.item].name.as_str(), "minecraft:pumpkin" | "minecraft:skull"));
                if let Some(a) = armor {
                    ui.armor[a] = Some(ItemStack { count: 1, ..s });
                    (s.count > 1).then_some(ItemStack { count: s.count - 1, ..s })
                } else if i >= 9 {
                    self.merge(ui, s, hotbar)
                } else {
                    self.merge(ui, s, main)
                }
            }
        };
        *Self::get(ui, slot) = rest;
    }
}

impl Screen for SurvivalInventory {
    fn entity_preview(&self) -> Option<(i32, i32, i32)> {
        Some((self.left + 51, self.top + 75, 30))
    }
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.left = ui.effect_gui_left(w, 176);
        self.top = (h - 166) / 2;
    }
    fn pauses(&self) -> bool {
        false
    }
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        mc_gui::screens::default_background(ui, b, w, h);
        // GuiInventory.updateScreen re-runs updateActivePotionEffects.
        self.left = ui.effect_gui_left(w, 176);
        let (x, y) = (self.left, self.top);
        b.textured_rect("textures/gui/container/inventory.png", x, y, 0, 0, 176, 166);
        let title = ui.lang.get("container.crafting");
        ui.font.draw(b, &title, (x + 86) as f32, (y + 16) as f32, 0xff404040);
        self.hover = self.at(mx, my);
        for (slot, sx, sy) in Self::slots() {
            if let Some(s) = *Self::get(ui, slot) {
                self.catalogue.draw(ui, b, s, x + sx, y + sy);
            }
            if self.hover == Some(slot) {
                b.rect(x + sx, y + sy, x + sx + 16, y + sy + 16, 0x80ffffff);
            }
        }
        if let Some(s) = self.cursor {
            self.catalogue.draw(ui, b, s, mx - 8, my - 8);
        } else if let Some(s) = self.hover.and_then(|h| *Self::get(ui, h)) {
            let lines = crate::tooltip::render_lines(&self.catalogue, &ui.lang, &s, ui.settings.advanced_item_tooltips);
            ui.draw_hovering_text(b, &lines, mx, my, w, h);
        }
        ui.draw_effects(b, x, y);
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
                // onContainerClosed: the held stack and the crafting grid are thrown.
                ui.drops.extend(self.cursor.take());
                for c in &mut ui.crafting {
                    ui.drops.extend(c.take());
                }
                Action::Close
            }
            Key::Number(n) => {
                if self.cursor.is_none() {
                    if let Some(slot) = self.hover {
                        let hot = ui.inventory[n];
                        let here = *Self::get(ui, slot);
                        if hot.is_none_or(|h| self.accepts(slot, h)) {
                            *Self::get(ui, slot) = hot;
                            ui.inventory[n] = here;
                        }
                    }
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

impl SurvivalInventory {
    fn mouse(&mut self, ui: &mut Ui, mx: i32, my: i32, right: bool) -> Action {
        let outside = mx < self.left || my < self.top || mx >= self.left + 176 || my >= self.top + 166;
        match self.at(mx, my) {
            Some(slot) if ui.shift_down => self.transfer(ui, slot),
            Some(slot) => self.pickup(ui, slot, right),
            None if outside => {
                // Slot -999: throw the held stack (or one item on right click).
                if let Some(c) = self.cursor {
                    if right {
                        ui.drops.push(ItemStack { count: 1, ..c });
                        self.cursor = (c.count > 1).then_some(ItemStack { count: c.count - 1, ..c });
                    } else {
                        ui.drops.push(c);
                        self.cursor = None;
                    }
                }
            }
            None => {}
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pickup_split_merge_and_shift_click() {
        let Some(jar) = crate::find_jar() else { return };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let c = Arc::new(Catalogue::metadata());
        let stone = c.find("stone", 0).unwrap();
        let helmet = c.find("iron_helmet", 0).unwrap();
        let mut s = SurvivalInventory::new(c);
        s.init(&ui, 640, 360);
        ui.inventory[9] = Some(ItemStack::new(stone, 64, 0));
        s.pickup(&mut ui, Slot::Inv(9), true);
        assert_eq!((s.cursor.unwrap().count, ui.inventory[9].unwrap().count), (32, 32));
        s.pickup(&mut ui, Slot::Inv(10), true);
        assert_eq!(ui.inventory[10].unwrap().count, 1);
        s.pickup(&mut ui, Slot::Inv(9), false);
        assert_eq!((s.cursor.is_none(), ui.inventory[9].unwrap().count), (true, 63));
        s.cursor = None;
        s.transfer(&mut ui, Slot::Inv(9));
        assert_eq!(ui.inventory[0].unwrap().count, 63);
        ui.inventory[11] = Some(ItemStack::new(helmet, 1, 0));
        s.transfer(&mut ui, Slot::Inv(11));
        assert!(ui.armor[0].is_some() && ui.inventory[11].is_none());
        s.pickup(&mut ui, Slot::Armor(1), false);
        ui.inventory[12] = Some(ItemStack::new(stone, 1, 0));
        s.pickup(&mut ui, Slot::Inv(12), false);
        s.pickup(&mut ui, Slot::Armor(1), false);
        assert!(ui.armor[1].is_none(), "stone is not a chestplate");
    }
}
