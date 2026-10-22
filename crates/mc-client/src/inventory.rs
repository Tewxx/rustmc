//! Creative catalogue extracted from CreativeTabs and RenderItem in the reference Java client.
use glam::{Mat3, Vec3};
use mc_assets::{
    model::ModelLoader,
    AssetSource,
};
use mc_gui::{
    batch::GuiBatch,
    screens::{Action, Key, Screen},
    ItemStack, Ui,
};
use mc_render::models::BlockModels;
use std::{collections::HashMap, sync::Arc};

pub struct ItemDef {
    pub id: u16,
    pub meta: u16,
    pub max: u8,
    pub name: String,
    pub title: String,
    pub block: u16,
    pub tabs: u16,
    pub icon: Vec<IconQuad>,
}
pub struct IconQuad {
    pub texture: String,
    pub pos: [[f32; 2]; 4],
    pub uv: [[f32; 2]; 4],
    pub color: [[u8; 4]; 4],
    /// Tint index 0 quads take the stack's own color (dyed leather, potion damage values).
    pub tint0: Option<f32>,
}
pub struct Catalogue {
    pub items: Vec<ItemDef>,
}

impl Catalogue {
    #[cfg(test)]
    pub fn metadata() -> Self {
        Self {
            items: include_str!("creative.tsv")
                .lines()
                .map(|line| {
                    let f: Vec<_> = line.split('\t').collect();
                    ItemDef {
                        id: f[1].parse().unwrap(),
                        meta: f[2].parse().unwrap(),
                        max: f[3].parse().unwrap(),
                        name: f[4].into(),
                        title: f[5].into(),
                        block: f[7].parse().unwrap(),
                        tabs: 1 << f[0].parse::<u16>().unwrap(),
                        icon: vec![],
                    }
                })
                .collect(),
        }
    }
    pub fn load(assets: &AssetSource, models: &BlockModels) -> Self {
        let mut items: Vec<ItemDef> = Vec::new();
        let mut known: HashMap<(u16, u16, String), usize> = HashMap::new();
        let mut loader = ModelLoader::new(assets);
        // Registry items no creative tab lists (filled maps, written books, spawners...) still
        // arrive from servers; give them entries outside every tab (bit 15).
        let listed: std::collections::HashSet<u16> = include_str!("creative.tsv").lines().filter_map(|l| l.split('\t').nth(1)?.parse().ok()).collect();
        let lang = mc_gui::lang::Lang::load(assets, "en_US");
        let extra: Vec<String> = mc_data::item::Item::all()
            .iter()
            .filter(|i| !listed.contains(&i.id))
            .map(|i| {
                let short = i.name.trim_start_matches("minecraft:");
                let title = lang.get(&format!("{}.name", i.unlocalized));
                let block = i.block.and_then(mc_data::Block::by_name).map_or(0, |b| b.id);
                format!("15\t{}\t0\t{}\t{}\t{title}\t{short}\t{block}", i.id, i.max_stack, i.name)
            })
            .collect();
        for line in include_str!("creative.tsv").lines().chain(extra.iter().map(String::as_str)) {
            let f: Vec<_> = line.split('\t').collect();
            if f.len() != 8 {
                continue;
            }
            let tab: usize = f[0].parse().unwrap();
            let id: u16 = f[1].parse().unwrap();
            let meta: u16 = f[2].parse().unwrap();
            let key = (id, meta, f[5].to_owned());
            if let Some(&i) = known.get(&key) {
                items[i].tabs |= 1 << tab;
                continue;
            }
            let block = f[7].parse().unwrap();
            let mut icon = Vec::new();
            if let Some(model) = models.items.get(id, meta).filter(|m| !m.builtin) {
                let tint = |i: i32| mc_render::items::item_color(id, meta, i, &models.colors);
                icon = mc_render::items::gui_quads(model, tint)
                    .into_iter()
                    .map(|q| IconQuad { texture: "rustmc:block_atlas".into(), pos: q.pos, uv: q.uv, color: q.color, tint0: (q.tint_index == 0).then_some(q.shade) })
                    .collect();
            }
            if matches!(id, 54 | 130 | 146) {
                let texture = match id {
                    130 => "ender",
                    146 => "trapped",
                    _ => "normal",
                };
                let texture = format!("textures/entity/chest/{texture}.png");
                icon = entity_cube(&texture, [1., 6., 1.], [14., 10., 14.], [0., 19.], [64., 64.], 0.7, [255; 3]);
                icon.extend(entity_cube(
                    &texture,
                    [1., 1., 1.],
                    [14., 5., 14.],
                    [0., 0.],
                    [64., 64.],
                    0.7,
                    [255; 3],
                ));
            } else if id == 397 {
                let texture = match meta {
                    0 => "skeleton/skeleton",
                    1 => "skeleton/wither_skeleton",
                    2 => "zombie/zombie",
                    3 => "steve",
                    _ => "creeper/creeper",
                };
                let texture = format!("textures/entity/{texture}.png");
                let height = if matches!(meta, 2 | 3) { 64. } else { 32. };
                icon = entity_cube(&texture, [4., 4., 4.], [8., 8., 8.], [0., 0.], [64., height], 1.1, [255; 3]);
            } else if id == 425 {
                let colors: [u32; 16] = [
                    0x1e1b1b, 0xb3312c, 0x3b511a, 0x51301a, 0x253192, 0x7b2fbe, 0x287697, 0xababab, 0x434343, 0xd88198, 0x41cd34, 0xdecf2a,
                    0x6689d3, 0xc354cd, 0xeb8844, 0xf0f0f0,
                ];
                let c = colors[meta as usize & 15];
                let color = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                icon = entity_cube(
                    "textures/entity/banner_base.png",
                    [2., -4., 7.],
                    [12., 24., 1.],
                    [0., 0.],
                    [64., 64.],
                    0.5,
                    color,
                );
            }
            if icon.is_empty() {
                let texture = loader
                    .get(&format!("item/{}", f[6]))
                    .map(|m| m.resolve_texture("#layer0"))
                    .filter(|s| !s.contains("missing"))
                    .unwrap_or_else(|| format!("items/{}", f[6]));
                let path = format!("textures/{texture}.png");
                icon.push(IconQuad {
                    texture: path,
                    pos: [[0., 16.], [16., 16.], [16., 0.], [0., 0.]],
                    uv: [[0., 1.], [1., 1.], [1., 0.], [0., 0.]],
                    color: [[255; 4]; 4],
                    tint0: None,
                });
            }
            known.insert(key, items.len());
            items.push(ItemDef {
                id,
                meta,
                max: f[3].parse().unwrap(),
                name: f[4].into(),
                title: f[5].into(),
                block,
                tabs: 1 << tab,
                icon,
            });
        }
        Self { items }
    }
    /// A fresh stack of a catalogue entry (its own metadata, no NBT).
    pub fn stack(&self, item: usize, count: u8) -> ItemStack {
        ItemStack::new(item, count, self.items[item].meta as i16)
    }

    /// Catalogue entry for an item id and damage value.
    pub fn index_of(&self, id: u16, meta: u16) -> Option<usize> {
        self.items.iter().position(|i| i.id == id && i.meta == meta).or_else(|| self.items.iter().position(|i| i.id == id))
    }
    pub fn find(&self, name: &str, meta: u16) -> Option<usize> {
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        self.items
            .iter()
            .position(|i| (i.name.trim_start_matches("minecraft:") == name || i.id.to_string() == name) && i.meta == meta)
    }
    /// The entry whose model draws `stack`: potions follow the splash bit of the damage value
    /// (`ItemPotion` picks `bottle_splash` / `bottle_drinkable`).
    pub fn render_def(&self, stack: &ItemStack) -> &ItemDef {
        let item = &self.items[stack.item];
        let splash = mc_data::potion::is_splash(stack.damage as i32);
        if item.id == 373 && splash != mc_data::potion::is_splash(item.meta as i32) {
            if let Some(other) = self.items.iter().find(|i| i.id == 373 && i.meta != 0 && mc_data::potion::is_splash(i.meta as i32) == splash) {
                return other;
            }
        }
        item
    }

    pub fn draw(&self, ui: &mut Ui, b: &mut GuiBatch, stack: ItemStack, x: i32, y: i32) {
        let item = self.render_def(&stack);
        // Item.getColorFromItemStack for the NBT/damage-dependent cases.
        let tint = if item.id == 373 && stack.damage as u16 != item.meta {
            Some(mc_render::items::potion_color(stack.damage as u16))
        } else {
            crate::tooltip::armor_dye(self, &stack)
        };
        for q in &item.icon {
            let color = match (tint, q.tint0) {
                (Some(c), Some(shade)) => {
                    let ch = |v: u32| ((v & 255) as f32 * shade) as u8;
                    [[ch(c >> 16), ch(c >> 8), ch(c), 255]; 4]
                }
                _ => q.color,
            };
            b.quad(Some(&q.texture), true, q.pos.map(|p| [p[0] + x as f32, p[1] + y as f32]), q.uv, color);
        }
        // RenderItem.renderEffect: two glint passes in color -8372020 over the item's texels.
        if crate::tooltip::has_effect(self, &stack) {
            let saved = b.blend;
            b.blend = mc_gui::batch::Blend::Glint;
            for q in &item.icon {
                b.quad(Some(&q.texture), true, q.pos.map(|p| [p[0] + x as f32, p[1] + y as f32]), q.uv, [[0x80, 0x40, 0xCC, 255]; 4]);
            }
            b.blend = saved;
        }
        // RenderItem.renderItemOverlayIntoGUI
        if stack.count > 1 {
            let s = stack.count.to_string();
            let w = ui.font.width(&s);
            ui.font.draw_shadow(b, &s, (x + 17 - w) as f32, (y + 9) as f32, 0xffffffff);
        }
        if crate::tooltip::is_damaged(self, &stack) {
            let max = mc_data::item::Item::by_id(item.id).map_or(1, |i| i.max_damage) as f64;
            let d = stack.damage as f64;
            let j = (13.0 - d * 13.0 / max + 0.5).floor() as i32;
            let i = (255.0 - d * 255.0 / max + 0.5).floor() as i32;
            let (x, y) = (x + 2, y + 13);
            let argb = |r: i32, g: i32, b: i32| 0xff00_0000 | ((r as u32 & 255) << 16) | ((g as u32 & 255) << 8) | (b as u32 & 255);
            b.rect(x, y, x + 13, y + 2, argb(0, 0, 0));
            b.rect(x, y, x + 12, y + 1, argb((255 - i) / 4, 64, 0));
            b.rect(x, y, x + j, y + 1, argb(255 - i, i, 0));
        }
    }
}

const TITLES: [&str; 12] = [
    "Building Blocks",
    "Decoration Blocks",
    "Redstone",
    "Transportation",
    "Miscellaneous",
    "Search Items",
    "Foodstuffs",
    "Tools",
    "Combat",
    "Brewing",
    "Materials",
    "Survival Inventory",
];
fn entity_cube(texture: &str, lo: [f32; 3], size: [f32; 3], uv: [f32; 2], sheet: [f32; 2], scale: f32, tint: [u8; 3]) -> Vec<IconQuad> {
    let [x, y, z] = lo;
    let [w, h, d] = size;
    let [u, v] = uv;
    let p = [
        [x, y, z],
        [x + w, y, z],
        [x + w, y + h, z],
        [x, y + h, z],
        [x, y, z + d],
        [x + w, y, z + d],
        [x + w, y + h, z + d],
        [x, y + h, z + d],
    ];
    let r = Mat3::from_rotation_x(30f32.to_radians()) * Mat3::from_rotation_y(45f32.to_radians());
    let points = p.map(|p| r * Vec3::new(p[0] - 8., 8. - p[1], p[2] - 8.));
    let mut faces = vec![
        ([5, 1, 2, 6], [u + d + w, v + d, u + d + w + d, v + d + h]),
        ([0, 4, 7, 3], [u, v + d, u + d, v + d + h]),
        ([5, 4, 0, 1], [u + d, v, u + d + w, v + d]),
        ([2, 3, 7, 6], [u + d + w, v + d, u + d + w + w, v]),
        ([1, 0, 3, 2], [u + d, v + d, u + d + w, v + d + h]),
        ([4, 5, 6, 7], [u + d + w + d, v + d, u + d + w + d + w, v + d + h]),
    ];
    faces.sort_by(|a, b| {
        a.0.iter()
            .map(|&i| points[i].z)
            .sum::<f32>()
            .total_cmp(&b.0.iter().map(|&i| points[i].z).sum::<f32>())
    });
    faces
        .into_iter()
        .map(|(idx, [u1, v1, u2, v2])| IconQuad {
            texture: texture.into(),
            pos: idx.map(|i| [8. + points[i].x * scale, 8. - points[i].y * scale]),
            uv: [
                [u2 / sheet[0], v1 / sheet[1]],
                [u1 / sheet[0], v1 / sheet[1]],
                [u1 / sheet[0], v2 / sheet[1]],
                [u2 / sheet[0], v2 / sheet[1]],
            ],
            color: [[tint[0], tint[1], tint[2], 255]; 4],
            tint0: None,
        })
        .collect()
}
pub struct Creative {
    catalogue: Arc<Catalogue>,
    tab: usize,
    row: usize,
    search: String,
    cursor: Option<ItemStack>,
    left: i32,
    top: i32,
    hover: Option<usize>,
    hot: Option<usize>,
    drag_scroll: bool,
}
impl Creative {
    pub fn new(catalogue: Arc<Catalogue>) -> Self {
        Self {
            catalogue,
            tab: 0,
            row: 0,
            search: String::new(),
            cursor: None,
            left: 0,
            top: 0,
            hover: None,
            hot: None,
            drag_scroll: false,
        }
    }
    fn filtered(&self) -> Vec<usize> {
        self.catalogue
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                i.tabs & (1 << self.tab) != 0
                    && (self.tab != 5
                        || format!("{} {}", i.title, i.name)
                            .to_lowercase()
                            .contains(&self.search.to_lowercase()))
            })
            .map(|(i, _)| i)
            .collect()
    }
    fn rows(&self) -> usize {
        self.filtered().len().div_ceil(9).saturating_sub(5)
    }
    fn hit(&self, mx: i32, my: i32) -> (Option<usize>, Option<usize>) {
        let (x, y) = (mx - self.left - 9, my - self.top);
        if !(0..160).contains(&x) || x % 18 >= 16 {
            return (None, None);
        }
        let column = (x / 18) as usize;
        if (112..128).contains(&y) {
            return (None, Some(column));
        }
        if self.tab == 11 {
            if (54..106).contains(&y) && (y - 54) % 18 < 16 {
                return (None, Some(9 + (y as usize - 54) / 18 * 9 + column));
            }
        } else if (18..106).contains(&y) && (y - 18) % 18 < 16 {
            return (
                self.filtered().get(self.row * 9 + (y as usize - 18) / 18 * 9 + column).copied(),
                None,
            );
        }
        (None, None)
    }
}
impl Screen for Creative {
    fn entity_preview(&self) -> Option<(i32, i32, i32)> {
        (self.tab == 11).then_some((self.left + 43, self.top + 45, 20))
    }
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.left = ui.effect_gui_left(w, 195);
        self.top = (h - 136) / 2;
    }
    fn pauses(&self) -> bool {
        false
    }
    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, _w: i32, _h: i32, mx: i32, my: i32) {
        mc_gui::screens::default_background(ui, b, _w, _h);
        let (x, y) = (self.left, self.top);
        self.hover = None;
        self.hot = None;
        for tab in 0..12 {
            let tx = x + (tab % 6) as i32 * 28 + if tab % 6 == 5 { 27 } else { 0 };
            let ty = if tab < 6 { y - 28 } else { y + 136 };
            let selected = tab == self.tab;
            b.textured_rect(
                "textures/gui/container/creative_inventory/tabs.png",
                tx,
                ty,
                (tab % 6) as i32 * 28,
                if tab < 6 {
                    if selected {
                        32
                    } else {
                        0
                    }
                } else if selected {
                    96
                } else {
                    64
                },
                28,
                32,
            );
            let names = [
                "brick_block",
                "red_flower",
                "redstone",
                "golden_rail",
                "lava_bucket",
                "compass",
                "apple",
                "iron_axe",
                "golden_sword",
                "potion",
                "stick",
                "chest",
            ];
            let icon = self
                .catalogue
                .find(names[tab], 0)
                .or_else(|| self.catalogue.items.iter().position(|i| i.tabs & (1 << tab) != 0));
            if let Some(item) = icon {
                self.catalogue.draw(ui, b, self.catalogue.stack(item, 1), tx + 6, ty + 7);
            }
        }
        let bg = if self.tab == 5 {
            "tab_item_search.png"
        } else if self.tab == 11 {
            "tab_inventory.png"
        } else {
            "tab_items.png"
        };
        b.textured_rect(&format!("textures/gui/container/creative_inventory/{bg}"), x, y, 0, 0, 195, 136);
        if self.tab == 5 {
            b.rect(x + 82, y + 6, x + 171, y + 17, 0xff000000);
            ui.font
                .draw_shadow(b, &format!("{}_", self.search), (x + 84) as f32, (y + 8) as f32, 0xffffffff);
        }
        ui.font.draw(b, TITLES[self.tab], (x + 8) as f32, (y + 6) as f32, 0xff404040);
        if self.tab != 11 {
            self.row = self.row.min(self.rows());
            for (slot, &item) in self.filtered().iter().skip(self.row * 9).take(45).enumerate() {
                let sx = x + 9 + (slot % 9) as i32 * 18;
                let sy = y + 18 + (slot / 9) as i32 * 18;
                self.catalogue.draw(ui, b, self.catalogue.stack(item, 1), sx, sy);
                if (sx..sx + 16).contains(&mx) && (sy..sy + 16).contains(&my) {
                    self.hover = Some(item);
                    b.rect(sx, sy, sx + 16, sy + 16, 0x80ffffff);
                }
            }
            let rows = self.rows();
            let sy = y
                + 18
                + if rows == 0 {
                    0
                } else {
                    (self.row as f32 / rows as f32 * 95.) as i32
                };
            b.textured_rect(
                "textures/gui/container/creative_inventory/tabs.png",
                x + 175,
                sy,
                if rows == 0 { 244 } else { 232 },
                0,
                12,
                15,
            );
            if self.drag_scroll {
                self.row = (((my - y - 25).clamp(0, 95) as f32 / 95.) * rows as f32).round() as usize;
            }
        } else {
            for slot in 9..36 {
                let sx = x + 9 + ((slot - 9) % 9) as i32 * 18;
                let sy = y + 54 + ((slot - 9) / 9) as i32 * 18;
                if let Some(s) = ui.inventory[slot] {
                    self.catalogue.draw(ui, b, s, sx, sy);
                }
                if (sx..sx + 16).contains(&mx) && (sy..sy + 16).contains(&my) {
                    self.hot = Some(slot);
                }
            }
        }
        for slot in 0..9 {
            let sx = x + 9 + slot as i32 * 18;
            let sy = y + 112;
            if let Some(s) = ui.inventory[slot] {
                self.catalogue.draw(ui, b, s, sx, sy);
            }
            if (sx..sx + 16).contains(&mx) && (sy..sy + 16).contains(&my) {
                self.hot = Some(slot);
                b.rect(sx, sy, sx + 16, sy + 16, 0x80ffffff);
            }
        }
        if let Some(s) = self.cursor {
            self.catalogue.draw(ui, b, s, mx - 8, my - 8);
        } else if let Some(s) = self.hover.map(|i| self.catalogue.stack(i, 1)).or_else(|| self.hot.and_then(|slot| ui.inventory[slot])) {
            let lines = crate::tooltip::render_lines(&self.catalogue, &ui.lang, &s, ui.settings.advanced_item_tooltips);
            ui.draw_hovering_text(b, &lines, mx, my, _w, _h);
        }
        ui.draw_effects(b, x, y);
    }
    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        for tab in 0..12 {
            let x = self.left + (tab % 6) as i32 * 28 + if tab % 6 == 5 { 27 } else { 0 };
            let y = if tab < 6 { self.top - 28 } else { self.top + 136 };
            if (x..x + 28).contains(&mx) && (y..y + 32).contains(&my) {
                self.tab = tab;
                self.row = 0;
                return Action::None;
            }
        }
        if (self.left + 175..self.left + 187).contains(&mx) && (self.top + 18..self.top + 128).contains(&my) {
            self.drag_scroll = true;
            return Action::None;
        }
        let (hover, hot) = self.hit(mx, my);
        if let Some(item) = hover {
            let count = if ui.shift_down { self.catalogue.items[item].max } else { 1 };
            if ui.shift_down {
                let slot = ui.inventory[..9].iter().position(Option::is_none).unwrap_or(ui.selected_slot);
                ui.inventory[slot] = Some(self.catalogue.stack(item, count));
            } else {
                self.cursor = Some(self.catalogue.stack(item, count));
            }
        } else if let Some(slot) = hot {
            if ui.shift_down {
                ui.inventory[slot] = None;
            } else if let (Some(held), Some(existing)) = (self.cursor, ui.inventory[slot]) {
                if held.stacks_with(&existing) {
                    let n = held.count.min(self.catalogue.items[held.item].max - existing.count);
                    ui.inventory[slot] = Some(ItemStack {
                        count: existing.count + n,
                        ..existing
                    });
                    self.cursor = if held.count > n {
                        Some(ItemStack {
                            count: held.count - n,
                            ..held
                        })
                    } else {
                        None
                    };
                } else {
                    std::mem::swap(&mut self.cursor, &mut ui.inventory[slot]);
                }
            } else {
                std::mem::swap(&mut self.cursor, &mut ui.inventory[slot]);
            }
        } else if mx < self.left || mx > self.left + 195 || my < self.top || my > self.top + 136 {
            self.cursor = None;
        }
        Action::None
    }
    fn right_click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        let (hover, hot) = self.hit(mx, my);
        if let Some(item) = hover {
            self.cursor = Some(self.catalogue.stack(item, self.catalogue.items[item].max));
        } else if let Some(slot) = hot {
            if let Some(held) = self.cursor {
                match ui.inventory[slot] {
                    None => {
                        ui.inventory[slot] = Some(ItemStack { count: 1, ..held });
                        self.cursor = if held.count > 1 {
                            Some(ItemStack {
                                count: held.count - 1,
                                ..held
                            })
                        } else {
                            None
                        };
                    }
                    Some(s) if s.stacks_with(&held) && s.count < self.catalogue.items[s.item].max => {
                        ui.inventory[slot] = Some(ItemStack { count: s.count + 1, ..s });
                        self.cursor = if held.count > 1 {
                            Some(ItemStack {
                                count: held.count - 1,
                                ..held
                            })
                        } else {
                            None
                        };
                    }
                    _ => {}
                }
            } else if let Some(s) = ui.inventory[slot] {
                let n = s.count.div_ceil(2);
                self.cursor = Some(ItemStack { count: n, ..s });
                ui.inventory[slot] = if s.count > n {
                    Some(ItemStack { count: s.count - n, ..s })
                } else {
                    None
                };
            }
        }
        Action::None
    }
    fn release(&mut self, _: &mut Ui) {
        self.drag_scroll = false;
    }
    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        match key {
            Key::Escape | Key::Inventory => Action::Close,
            Key::Number(slot) => {
                if let Some(item) = self.hover {
                    ui.inventory[slot] = Some(self.catalogue.stack(item, self.catalogue.items[item].max));
                }
                Action::None
            }
            Key::Backspace => {
                self.search.pop();
                self.row = 0;
                Action::None
            }
            _ => Action::None,
        }
    }
    fn typed(&mut self, c: char) {
        if self.tab == 5 && !c.is_control() && self.search.len() < 50 {
            self.search.push(c);
            self.row = 0;
        }
    }
    fn scroll(&mut self, delta: i32) {
        self.row = (self.row as i32 - delta).clamp(0, self.rows() as i32) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vanilla_catalogue_has_models_and_resources() {
        let Some(jar) = crate::find_jar() else {
            return;
        };
        let mut assets = AssetSource::new();
        assets.push(&jar).unwrap();
        let models = BlockModels::load(&assets);
        let c = Catalogue::load(&assets, &models);
        assert!(c.items.len() > 500);
        assert!(c.find("diamond_sword", 0).is_some());
        assert!(c.find("stone", 6).is_some());
        let missing: Vec<_> = c
            .items
            .iter()
            // Farmland and lit furnaces have no item model in vanilla either (missing texture).
            .filter(|i| !matches!(i.name.as_str(), "minecraft:farmland" | "minecraft:lit_furnace"))
            .filter(|i| {
                i.icon
                    .iter()
                    .any(|q| q.texture != "rustmc:block_atlas" && assets.read(&q.texture).is_none())
            })
            .map(|i| format!("{}:{} {}", i.name, i.meta, i.icon[0].texture))
            .collect();
        assert!(missing.is_empty(), "Missing icons: {missing:?}");
    }
    #[test]
    fn creative_clicks_hotbar_stacks_search_and_number_keys() {
        let Some(jar) = crate::find_jar() else {
            return;
        };
        let mut assets = AssetSource::new();
        assets.push(&jar).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let c = Arc::new(Catalogue::metadata());
        let mut screen = Creative::new(c.clone());
        screen.init(&ui, 640, 360);
        let (x, y) = (screen.left, screen.top);
        screen.click(&mut ui, x + 10, y + 19);
        screen.click(&mut ui, x + 10, y + 113);
        assert_eq!(ui.inventory[0].unwrap().count, 1);
        assert_eq!(c.items[ui.inventory[0].unwrap().item].name, "minecraft:stone");
        ui.shift_down = true;
        screen.click(&mut ui, x + 28, y + 19);
        ui.shift_down = false;
        assert_eq!(ui.inventory[1].unwrap().count, 64);
        screen.right_click(&mut ui, x + 28, y + 113);
        assert_eq!(ui.inventory[1].unwrap().count, 32);
        assert_eq!(screen.cursor.unwrap().count, 32);
        screen.click(&mut ui, x + 169, y - 24);
        assert_eq!(screen.tab, 5);
        for c in "diamond_sword".chars() {
            screen.typed(c);
        }
        let results = screen.filtered();
        assert!(!results.is_empty());
        assert!(results.iter().all(|&i| c.items[i].name == "minecraft:diamond_sword"));
        let mut b = GuiBatch::new();
        screen.draw(&mut ui, &mut b, 640, 360, x + 10, y + 19);
        screen.key(&mut ui, Key::Number(2));
        assert_eq!(c.items[ui.inventory[2].unwrap().item].name, "minecraft:diamond_sword");
        assert!(matches!(screen.key(&mut ui, Key::Inventory), Action::Close));
    }
}
