//! `GuiScreenResourcePacks`: the available/selected pack lists (`GuiResourcePackAvailable`,
//! `GuiResourcePackSelected` over `GuiListExtended`/`GuiSlot`), `ResourcePackListEntry*` and
//! the incompatible-pack `GuiYesNo`.

use crate::batch::GuiBatch;
use crate::screens::{click_buttons, default_background, dirt, draw_buttons, Action, Key, Screen, OPTIONS_BACKGROUND};
use crate::widgets::Button;
use crate::Ui;
use mc_assets::repository::{self, PackEntry};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

const RESOURCE_PACKS: &str = "textures/gui/resource_packs.png";
/// `GuiSlot` constructor: `top` 32, `slotHeight` 36, list width 200.
const TOP: i32 = 32;
const SLOT_H: i32 = 36;
const LIST_W: i32 = 200;
/// `setHasListHeader(true, (int)(FONT_HEIGHT * 1.5F))`
const HEADER: i32 = 13;

type Pending = (String, u32, u32, Vec<u8>);
static TEXTURES: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Pack icons the screen wants as GUI textures (`DynamicTexture`); the client uploads them
/// before the next draw.
pub fn take_textures() -> Vec<(String, u32, u32, Vec<u8>)> {
    std::mem::take(&mut *TEXTURES.lock().unwrap())
}

/// `ResourcePackListEntryFound` or `ResourcePackListEntryDefault`.
#[derive(Clone)]
struct Entry {
    id: u32,
    /// `None` for the default pack.
    pack: Option<PackEntry>,
    name: String,
    /// `getFormattedText` of the description.
    description: String,
    icon: String,
}

impl Entry {
    /// `func_183019_a`: the pack format (1 for the default pack).
    fn format(&self) -> i32 {
        self.pack.as_ref().map_or(1, |p| p.format)
    }

    /// `func_148310_d`
    fn interactive(&self) -> bool {
        self.pack.is_some()
    }
}

struct Confirm {
    id: u32,
    newer: bool,
}

struct Drag {
    list: usize,
    y: i32,
    mult: f32,
}

#[derive(Default)]
pub struct ResourcePacks {
    buttons: Vec<Button>,
    avail: Vec<Entry>,
    sel: Vec<Entry>,
    /// `scroll[0]` available, `[1]` selected: `GuiSlot.amountScrolled`.
    scroll: [f32; 2],
    changed: bool,
    built: bool,
    confirm: Option<Confirm>,
    confirm_buttons: Vec<Button>,
    drag: Option<Drag>,
    mouse: (i32, i32),
    w: i32,
    h: i32,
}

impl ResourcePacks {
    fn left(&self, list: usize) -> i32 {
        if list == 0 { self.w / 2 - 4 - LIST_W } else { self.w / 2 + 4 }
    }

    fn bottom(&self) -> i32 {
        self.h - 55 + 4
    }

    fn entries(&self, list: usize) -> &Vec<Entry> {
        if list == 0 { &self.avail } else { &self.sel }
    }

    /// `func_148135_f`
    fn max_scroll(&self, list: usize) -> i32 {
        let content = self.entries(list).len() as i32 * SLOT_H + HEADER;
        (content - (self.bottom() - TOP - 4)).max(0)
    }

    fn bind_scroll(&mut self, list: usize) {
        let m = self.max_scroll(list) as f32;
        self.scroll[list] = self.scroll[list].clamp(0.0, m);
    }

    /// `getSlotIndexFromScreenCoords`
    fn slot_at(&self, list: usize, mx: i32, my: i32) -> Option<usize> {
        let left = self.left(list);
        let k = my - TOP - HEADER + self.scroll[list] as i32 - 4;
        let l = k / SLOT_H;
        let scrollbar_x = left + LIST_W - 6;
        (mx < scrollbar_x && mx >= left && mx <= left + LIST_W && l >= 0 && k >= 0 && (l as usize) < self.entries(list).len()).then_some(l as usize)
    }

    /// `isMouseYWithinSlotBounds`
    fn in_bounds(&self, list: usize, mx: i32, my: i32) -> bool {
        my >= TOP && my <= self.bottom() && mx >= self.left(list) && mx <= self.left(list) + LIST_W
    }

    fn can_up(&self, list: usize, i: usize) -> bool {
        i > 0 && self.entries(list)[i - 1].interactive()
    }

    fn can_down(&self, list: usize, i: usize) -> bool {
        i + 1 < self.entries(list).len() && self.entries(list)[i + 1].interactive()
    }

    fn build_lists(&mut self, ui: &Ui) {
        let dir = ui.settings.game_dir().join("resourcepacks");
        repository::ensure_dir(&dir);
        let all = repository::scan(&dir);
        let (selected, _) = repository::restore_selection(&all, &ui.settings.resource_packs, &ui.settings.incompatible_resource_packs);
        let queue_icon = |bytes: Option<&[u8]>| {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let icon = format!("rustmc:packicon/{id}");
            if let Some(img) = bytes.and_then(|b| image::load_from_memory(b).ok()) {
                let img = img.to_rgba8();
                TEXTURES.lock().unwrap().push((icon.clone(), img.width(), img.height(), img.into_raw()));
            }
            (id, icon)
        };
        let mk = |p: &PackEntry| {
            // Entry.updateResourcePack: a pack without a readable pack.png uses the default image.
            let (id, icon) = queue_icon(p.icon.as_deref().filter(|b| image::load_from_memory(b).is_ok()).or(ui.pack_icon.as_deref()));
            let description = match &p.description {
                Some(d) => describe(d),
                None => "\u{a7}cInvalid pack.mcmeta (or missing 'pack' section)".to_string(),
            };
            Entry { id, pack: Some(p.clone()), name: p.name.clone(), description, icon }
        };
        self.avail = all.iter().filter(|p| !selected.iter().any(|s| s.name == p.name)).map(mk).collect();
        self.sel = selected.iter().rev().map(mk).collect();
        let (id, icon) = queue_icon(ui.pack_icon.as_deref());
        self.sel.push(Entry { id, pack: None, name: "Default".into(), description: "The default look of Minecraft".into(), icon });
    }

    /// Finds an entry by id: `(list, index)`.
    fn find(&self, id: u32) -> Option<(usize, usize)> {
        (0..2).find_map(|l| self.entries(l).iter().position(|e| e.id == id).map(|i| (l, i)))
    }

    /// `ResourcePackListEntry.mousePressed`
    fn entry_pressed(&mut self, list: usize, i: usize, rx: i32, ry: i32) -> bool {
        let e = self.entries(list)[i].clone();
        if !e.interactive() || rx > 32 {
            return false;
        }
        if list == 0 {
            self.changed = true;
            let fmt = e.format();
            if fmt != 1 {
                self.confirm = Some(Confirm { id: e.id, newer: fmt > 1 });
            } else {
                self.avail.remove(i);
                self.sel.insert(0, e);
            }
            return true;
        }
        if rx < 16 {
            self.sel.remove(i);
            self.avail.insert(0, e);
            self.changed = true;
            return true;
        }
        if rx > 16 && ry < 16 && self.can_up(list, i) {
            self.sel.swap(i, i - 1);
            self.changed = true;
            return true;
        }
        if rx > 16 && ry > 16 && self.can_down(list, i) {
            self.sel.swap(i, i + 1);
            self.changed = true;
            return true;
        }
        false
    }

    /// `ResourcePackListEntry.drawEntry`
    #[allow(clippy::too_many_arguments)]
    fn draw_entry(&self, ui: &mut Ui, b: &mut GuiBatch, list: usize, i: usize, x: i32, y: i32, hovered: bool, mx: i32, my: i32) {
        let e = &self.entries(list)[i];
        let fmt = e.format();
        if fmt != 1 {
            b.color = [1.0; 4];
            b.rect(x - 1, y - 1, x + LIST_W - 9, y + (SLOT_H - 4) + 1, -8978432i32 as u32);
        }
        b.color = [1.0; 4];
        b.textured_rect_uv(&e.icon, x as f32, y as f32, 32.0, 32.0, [0.0, 0.0, 1.0, 1.0]);
        let mut s = e.name.clone();
        let mut s1 = e.description.clone();
        if hovered && e.interactive() {
            b.rect(x, y, x + 32, y + 32, -1601138544i32 as u32);
            b.color = [1.0; 4];
            let (j, k) = (mx - x, my - y);
            if fmt < 1 {
                s = ui.lang.get("resourcePack.incompatible");
                s1 = ui.lang.get("resourcePack.incompatible.old");
            } else if fmt > 1 {
                s = ui.lang.get("resourcePack.incompatible");
                s1 = ui.lang.get("resourcePack.incompatible.new");
            }
            let tex = |b: &mut GuiBatch, u: i32, v: i32| b.textured_rect(RESOURCE_PACKS, x, y, u, v, 32, 32);
            if list == 0 {
                tex(b, 0, if j < 32 { 32 } else { 0 });
            } else {
                tex(b, 32, if j < 16 { 32 } else { 0 });
                if self.can_up(list, i) {
                    tex(b, 96, if j < 32 && j > 16 && k < 16 { 32 } else { 0 });
                }
                if self.can_down(list, i) {
                    tex(b, 64, if j < 32 && j > 16 && k > 16 { 32 } else { 0 });
                }
            }
        }
        if ui.font.width(&s) > 157 {
            s = ui.font.trim_to_width(&s, 157 - ui.font.width("...")) + "...";
        }
        ui.font.draw_shadow(b, &s, (x + 32 + 2) as f32, (y + 1) as f32, 16777215);
        for (l, line) in ui.font.list_to_width(&s1, 157).iter().take(2).enumerate() {
            ui.font.draw_shadow(b, line, (x + 32 + 2) as f32, (y + 12 + 10 * l as i32) as f32, 8421504);
        }
    }

    /// `GuiSlot.drawScreen` for one list.
    fn draw_list(&mut self, ui: &mut Ui, b: &mut GuiBatch, list: usize, mx: i32, my: i32) {
        self.bind_scroll(list);
        let (left, right, top, bottom) = (self.left(list), self.left(list) + LIST_W, TOP, self.bottom());
        let amount = self.scroll[list] as i32;
        // The list body: options background at gray 32, scrolling with the content.
        let f = 32.0;
        let (l, r, t, bo) = (left as f32, right as f32, top as f32, bottom as f32);
        let c = [[32, 32, 32, 255]; 4];
        let v = |y: f32| (y + amount as f32) / f;
        b.quad(Some(OPTIONS_BACKGROUND), false, [[l, bo], [r, bo], [r, t], [l, t]], [[l / f, v(bo)], [r / f, v(bo)], [r / f, v(t)], [l / f, v(t)]], c);
        let x = left + 2;
        let y0 = top + 4 - amount;
        // drawListHeader
        let title = ui.lang.get(if list == 0 { "resourcePack.available.title" } else { "resourcePack.selected.title" });
        let s = format!("\u{a7}n\u{a7}l{title}");
        let hx = x + LIST_W / 2 - ui.font.width(&s) / 2;
        ui.font.draw(b, &s, hx as f32, (top + 3).min(y0) as f32, 16777215);
        let hover = self.slot_at(list, mx, my);
        for i in 0..self.entries(list).len() {
            let y = y0 + i as i32 * SLOT_H + HEADER;
            self.draw_entry(ui, b, list, i, x, y, hover == Some(i), mx, my);
        }
        // overlayBackground(0, top) and (bottom, height) at gray 64.
        let ov = |b: &mut GuiBatch, y0: i32, y1: i32| {
            let (a, z) = (y0 as f32, y1 as f32);
            let w = LIST_W as f32 / f;
            b.quad(Some(OPTIONS_BACKGROUND), false, [[l, z], [r, z], [r, a], [l, a]], [[0.0, z / f], [w, z / f], [w, a / f], [0.0, a / f]], [[64, 64, 64, 255]; 4]);
        };
        ov(b, 0, top);
        ov(b, bottom, self.h);
        b.gradient(left, top, right, top + 4, 0xFF00_0000, 0x0000_0000);
        b.gradient(left, bottom - 4, right, bottom, 0x0000_0000, 0xFF00_0000);
        let max = self.max_scroll(list);
        if max > 0 {
            let (sx, view) = (right - 6, bottom - top);
            let content = self.entries(list).len() as i32 * SLOT_H + HEADER;
            let bar = (view * view / content).clamp(32, view - 8);
            let by = (amount * (view - bar) / max + top).max(top);
            b.rect(sx, top, sx + 6, bottom, 0xFF00_0000);
            b.rect(sx, by, sx + 6, by + bar, 0xFF80_8080);
            b.rect(sx, by, sx + 5, by + bar - 1, 0xFFC0_C0C0);
        }
    }

    /// `GuiYesNoCallback.confirmClicked` of the incompatible-pack prompt; the screen is shown
    /// again afterwards (`initGui`: scroll back to 0, lists kept because `changed`).
    fn answer(&mut self, yes: bool) {
        if let Some(c) = self.confirm.take() {
            if yes {
                if let Some((0, i)) = self.find(c.id) {
                    let e = self.avail.remove(i);
                    self.sel.insert(0, e);
                }
            }
        }
        self.scroll = [0.0; 2];
    }
}

impl Screen for ResourcePacks {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.w = w;
        self.h = h;
        self.buttons = vec![
            Button::sized(2, w / 2 - 154, h - 48, 150, 20, ui.lang.get("resourcePack.openFolder")),
            Button::sized(1, w / 2 + 4, h - 48, 150, 20, ui.lang.get("gui.done")),
        ];
        self.confirm_buttons = vec![
            Button::sized(0, w / 2 - 155, h / 6 + 96, 150, 20, ui.lang.get("gui.yes")),
            Button::sized(1, w / 2 - 155 + 160, h / 6 + 96, 150, 20, ui.lang.get("gui.no")),
        ];
        if !self.changed || !self.built {
            self.build_lists(ui);
            self.built = true;
        }
        self.drag = None;
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        self.mouse = (mx, my);
        if let Some(c) = &self.confirm {
            // GuiYesNo(title, message): drawDefaultBackground, lines at 70 and 90.
            let kind = if c.newer { "new" } else { "old" };
            let (t1, t2) = (ui.lang.get("resourcePack.incompatible.confirm.title"), ui.lang.get(&format!("resourcePack.incompatible.confirm.{kind}")));
            default_background(ui, b, w, h);
            ui.font.draw_centered(b, &t1, w / 2, 70, 16777215);
            ui.font.draw_centered(b, &t2, w / 2, 90, 16777215);
            draw_buttons(&mut self.confirm_buttons, ui, b, mx, my);
            return;
        }
        // GuiSlot.handleMouseInput's drag-scrolling.
        if let Some(d) = &mut self.drag {
            self.scroll[d.list] -= (my - d.y) as f32 * d.mult;
            d.y = my;
        }
        dirt(b, 0, h, w, 64, 0.0);
        self.draw_list(ui, b, 0, mx, my);
        self.draw_list(ui, b, 1, mx, my);
        let title = ui.lang.get("resourcePack.title");
        ui.font.draw_centered(b, &title, w / 2, 16, 16777215);
        let info = ui.lang.get("resourcePack.folderInfo");
        ui.font.draw_centered(b, &info, w / 2 - 77, h - 26, 8421504);
        draw_buttons(&mut self.buttons, ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.mouse = (mx, my);
        if self.confirm.is_some() {
            match click_buttons(&mut self.confirm_buttons, ui, mx, my) {
                Some(0) => self.answer(true),
                Some(1) => self.answer(false),
                _ => {}
            }
            return Action::None;
        }
        match click_buttons(&mut self.buttons, ui, mx, my) {
            Some(2) => {
                let dir = ui.settings.game_dir().join("resourcepacks");
                repository::ensure_dir(&dir);
                ui.open_folder = Some(dir);
                return Action::None;
            }
            Some(1) => {
                if self.changed {
                    // Selected entries top to bottom, reversed: lowest priority first.
                    let packs: Vec<&PackEntry> = self.sel.iter().filter_map(|e| e.pack.as_ref()).rev().collect();
                    ui.settings.resource_packs = packs.iter().map(|p| p.name.clone()).collect();
                    ui.settings.incompatible_resource_packs = packs.iter().filter(|p| !p.compatible()).map(|p| p.name.clone()).collect();
                    ui.settings.save();
                    ui.pack_reload = true;
                }
                return Action::Close;
            }
            _ => {}
        }
        for list in 0..2 {
            if !self.in_bounds(list, mx, my) {
                continue;
            }
            if let Some(i) = self.slot_at(list, mx, my) {
                let k = TOP + 4 - self.scroll[list] as i32 + i as i32 * SLOT_H + HEADER;
                if self.entry_pressed(list, i, mx - (self.left(list) + 2), my - k) {
                    // setEnabled(false): no drag for this press.
                    return Action::None;
                }
            }
        }
        // Press-and-drag scrolling (initialClickY / scrollMultiplier).
        for list in 0..2 {
            if self.in_bounds(list, mx, my) {
                let left = self.left(list);
                let mult = if mx >= left + LIST_W - 6 && mx <= left + LIST_W {
                    let (view, content) = (self.bottom() - TOP, self.entries(list).len() as i32 * SLOT_H + HEADER);
                    let bar = (view * view / content).clamp(32, view - 8);
                    -1.0 / ((view - bar) as f32 / self.max_scroll(list).max(1) as f32)
                } else {
                    1.0
                };
                self.drag = Some(Drag { list, y: my, mult });
            }
        }
        Action::None
    }

    fn release(&mut self, _: &mut Ui) {
        self.drag = None;
    }

    fn key(&mut self, _: &mut Ui, key: Key) -> Action {
        if key == Key::Escape { Action::Close } else { Action::None }
    }

    fn scroll(&mut self, delta: i32) {
        if self.confirm.is_some() {
            return;
        }
        let (mx, my) = self.mouse;
        if let Some(list) = (0..2).find(|&l| self.in_bounds(l, mx, my)) {
            self.scroll[list] -= (delta.signum() * SLOT_H / 2) as f32;
            self.bind_scroll(list);
        }
    }
}

/// `getFormattedText` of a `pack.mcmeta` description: a string, or an object/array of text
/// pieces with `color` and the bold/italic/underlined/strikethrough/obfuscated flags.
fn describe(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(describe).collect(),
        Value::Object(o) => {
            let mut code = String::new();
            let colors = ["black", "dark_blue", "dark_green", "dark_aqua", "dark_red", "dark_purple", "gold", "gray", "dark_gray", "blue", "green", "aqua", "red", "light_purple", "yellow", "white"];
            if let Some(i) = o.get("color").and_then(Value::as_str).and_then(|c| colors.iter().position(|n| *n == c)) {
                code.push_str(&format!("\u{a7}{i:x}"));
            }
            for (k, c) in [("obfuscated", 'k'), ("bold", 'l'), ("strikethrough", 'm'), ("underlined", 'n'), ("italic", 'o')] {
                if o.get(k).and_then(Value::as_bool) == Some(true) {
                    code.push('\u{a7}');
                    code.push(c);
                }
            }
            let text = o.get("text").and_then(Value::as_str).unwrap_or("");
            let extra: String = o.get("extra").and_then(Value::as_array).map(|a| a.iter().map(describe).collect()).unwrap_or_default();
            if text.is_empty() && extra.is_empty() { String::new() } else { format!("{code}{text}\u{a7}r{extra}") }
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: u32, name: &str, format: i32) -> Entry {
        let pack = PackEntry { name: name.into(), path: name.into(), format, description: None, icon: None };
        Entry { id, pack: Some(pack), name: name.into(), description: String::new(), icon: String::new() }
    }

    fn screen() -> ResourcePacks {
        let mut s = ResourcePacks { w: 427, h: 240, ..Default::default() };
        s.avail = vec![found(1, "a", 1), found(2, "old", 0)];
        let default = Entry { id: 9, pack: None, name: "Default".into(), description: String::new(), icon: String::new() };
        s.sel = vec![found(3, "b", 1), found(4, "c", 1), default];
        s
    }

    fn names(l: &[Entry]) -> Vec<&str> {
        l.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn layout_matches_java() {
        let s = screen();
        // availableResourcePacksList.setSlotXBoundsFromLeft(width / 2 - 4 - 200); selected: width / 2 + 4.
        assert_eq!((s.left(0), s.left(1)), (427 / 2 - 204, 427 / 2 + 4));
        // GuiResourcePackList: top 32, bottom height - 55 + 4.
        assert_eq!(s.bottom(), 240 - 51);
        // First entry: x = left + 2, y = top + 4 + headerPadding.
        assert_eq!(s.slot_at(1, s.left(1) + 10, TOP + HEADER + 4 + 1), Some(0));
        assert_eq!(s.slot_at(1, s.left(1) + 10, TOP + HEADER + 3), None);
        assert_eq!(s.slot_at(1, s.left(1) + 195, TOP + HEADER + 10), None, "scrollbar column");
    }

    #[test]
    fn moving_entries() {
        let mut s = screen();
        // Add "a": left arrow area, format 1 moves it to the top of the selected list.
        assert!(s.entry_pressed(0, 0, 10, 10));
        assert_eq!(names(&s.sel), ["a", "b", "c", "Default"]);
        assert!(s.changed);
        // Move c up (right half, upper quarter), then down.
        assert!(s.entry_pressed(1, 2, 20, 5));
        assert_eq!(names(&s.sel), ["a", "c", "b", "Default"]);
        assert!(s.entry_pressed(1, 1, 20, 25));
        assert_eq!(names(&s.sel), ["a", "b", "c", "Default"]);
        // The last real pack cannot move below the default entry; the default entry is inert.
        assert!(!s.entry_pressed(1, 2, 20, 25));
        assert!(!s.entry_pressed(1, 3, 10, 10));
        // Remove: left half of the selected entry.
        assert!(s.entry_pressed(1, 0, 5, 5));
        assert_eq!(names(&s.avail), ["a", "old"]);
    }

    #[test]
    fn incompatible_needs_confirmation() {
        let mut s = screen();
        assert!(s.entry_pressed(0, 1, 10, 10));
        assert!(s.confirm.as_ref().is_some_and(|c| !c.newer), "pack_format 0 is older");
        assert_eq!(names(&s.sel), ["b", "c", "Default"], "not moved before answering");
        s.answer(false);
        assert_eq!(names(&s.sel), ["b", "c", "Default"]);
        assert!(s.entry_pressed(0, 1, 10, 10));
        s.answer(true);
        assert_eq!(names(&s.sel), ["old", "b", "c", "Default"]);
        assert!(s.changed && s.confirm.is_none());
    }
}
