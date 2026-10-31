//! Screens the network handler opens outside the ordinary menu set: `GuiEditSign`,
//! `GuiScreenBook` (read-only) and `GuiMerchant`. They live here so the client can reach
//! `ItemStack` NBT and the window/click model, which the plain `mc-gui` screens do not.
use crate::container::{ContainerScreen, Window};
use crate::inventory::Catalogue;
use crate::tooltip;
use mc_core::BlockPos;
use mc_gui::{
    batch::GuiBatch,
    component::{split_text, ClickAction, Component},
    screens::{default_background, Action, Key, Screen, Trade},
    widgets::Button,
    ItemStack, Ui,
};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------- GuiEditSign

/// `GuiEditSign`
pub(crate) struct SignScreen {
    pos: BlockPos,
    /// The original `TileEntitySign.signText` JSON, kept for lines the editor never touched.
    raw: [String; 4],
    /// `TileEntitySign.signText` after `getUnformattedText`.
    lines: [String; 4],
    /// Lines replaced by `new ChatComponentText(s)`, like `GuiEditSign.keyTyped`.
    edited: [bool; 4],
    /// `editLine`
    edit_line: usize,
    /// `updateCounter`, for the blinking caret.
    update_counter: i32,
    done: Button,
    /// [`Screen::close_action`] runs once; the same screen must not send twice.
    sent: bool,
}

impl SignScreen {
    pub(crate) fn new(pos: BlockPos, raw: [String; 4], lines: [String; 4]) -> Self {
        Self { pos, raw, lines, edited: [false; 4], edit_line: 0, update_counter: 0, done: Button::new(0, 0, 0, ""), sent: false }
    }

    fn line_mut(&mut self) -> &mut String {
        &mut self.lines[self.edit_line]
    }

    /// `keyTyped`'s Backspace branch (`s.substring(0, s.length() - 1)`), counting UTF-16
    /// code units like Java's `String`.
    fn backspace(&mut self) {
        let s = self.line_mut();
        let mut units: Vec<u16> = s.encode_utf16().collect();
        units.pop();
        *s = String::from_utf16_lossy(&units);
        self.edited[self.edit_line] = true;
    }
}

impl Screen for SignScreen {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        // GuiEditSign.initGui: Keyboard.enableRepeatEvents(true) has no Rust equivalent.
        self.done = Button::new(0, w / 2 - 100, h / 4 + 120, ui.lang.get("gui.done"));
    }

    fn tick(&mut self) {
        self.update_counter += 1;
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        default_background(ui, b, w, h);
        let title = ui.lang.get("sign.edit");
        ui.font.draw_centered(b, &title, w / 2, 40, 0xffffffff);
        // The vanilla screen renders the tile entity with `TileEntitySignRenderer`; in this 2D
        // screen the sign item texture stands in for the block, scaled 6× (16 texels → 96 px).
        const T: f32 = 16.0 / 256.0;
        let (sx, sy) = (w / 2 - 48, 56);
        b.textured_rect_uv("textures/items/sign.png", sx as f32, sy as f32, 96.0, 96.0, [0.0, 0.0, T, T]);
        for (i, line) in self.lines.iter().enumerate() {
            let y = sy + 8 + i as i32 * 20;
            let x = w / 2 - ui.font.width(line) / 2;
            // Untouched lines keep the server component's colours; an edited line is the plain
            // `ChatComponentText` GuiEditSign installed.
            let text = if self.edited[i] { line.clone() } else { crate::chat::format(&self.raw[i], &ui.lang) };
            ui.font.draw(b, &text, x as f32, y as f32, 0xff000000);
            if i == self.edit_line && self.update_counter / 6 % 2 == 0 {
                // The caret blink without a text cursor: an underline under the active line.
                let x1 = w / 2 - 45;
                b.rect(x1, y + 10, x1 + ui.font.width(line).max(2), y + 11, 0xff000000);
            }
        }
        self.done.draw(ui, b, mx, my);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        if self.done.press(ui, mx, my) {
            Action::Close
        } else {
            Action::None
        }
    }

    fn key(&mut self, _ui: &mut Ui, key: Key) -> Action {
        match key {
            Key::Escape => {
                // `keyTyped` replaces the edited line before `actionPerformed(doneBtn)` runs.
                self.edited[self.edit_line] = true;
                Action::Close
            }
            // 200 / 208 / 28 / 156
            Key::Up => {
                self.edit_line = (self.edit_line + 3) & 3;
                // `keyTyped` ends by replacing the line it edited with `ChatComponentText(s)`.
                self.edited[self.edit_line] = true;
                Action::None
            }
            Key::Down | Key::Enter => {
                self.edit_line = (self.edit_line + 1) & 3;
                self.edited[self.edit_line] = true;
                Action::None
            }
            Key::Backspace => {
                self.backspace();
                Action::None
            }
            _ => Action::None,
        }
    }

    fn typed(&mut self, ui: &mut Ui, c: char) {
        // `ChatAllowedCharacters.isAllowedCharacter && getStringWidth(s + typedChar) <= 90`.
        if c == '\u{a7}' || (c as u32) < 32 || c == '\u{7f}' {
            return;
        }
        let mut s = self.lines[self.edit_line].clone();
        s.push(c);
        if ui.font.width(&s) <= 90 {
            self.lines[self.edit_line] = s;
            self.edited[self.edit_line] = true;
        }
    }

    fn close_action(&mut self) -> Action {
        if self.sent {
            return Action::None;
        }
        self.sent = true;
        // `C12PacketUpdateSign` carries `IChatComponent.Serializer.componentToJson` of each
        // `TileEntitySign.signText` entry. Untouched lines keep their original component; an
        // edited line is the `{"text": "…"}` form of `ChatComponentText`.
        let lines = std::array::from_fn(|i| {
            if self.edited[i] {
                serde_json::json!({ "text": self.lines[i] }).to_string()
            } else {
                self.raw[i].clone()
            }
        });
        Action::UpdateSign { pos: self.pos, lines }
    }
}

// ---------------------------------------------------------------- GuiScreenBook

/// `GuiScreenBook` opened read-only from `MC|BOpen`.
pub(crate) struct BookScreen {
    /// Raw `pages` strings (`IChatComponent` JSON on a valid signed book).
    pages: Vec<String>,
    total: usize,
    index: usize,
    valid: bool,
    /// `field_175386_A`: the wrapped component lines of the current page.
    lines: Vec<Component>,
    lines_page: Option<usize>,
    left: i32,
    top: i32,
}

impl BookScreen {
    /// Reads `ItemStack`'s `pages`/`title`/`author` like `new GuiScreenBook(player, book, false)`.
    pub(crate) fn from_stack(stack: &ItemStack) -> Self {
        let mut pages = Vec::new();
        let mut valid = false;
        if let Some(tag) = stack.tag {
            let nbt = tag.get();
            if let Some(mc_core::nbt::Tag::List(8, list)) = nbt.get("pages") {
                pages = list.iter().filter_map(mc_core::nbt::Tag::as_str).map(str::to_string).collect();
            }
            valid = book_tag_valid(&nbt);
        }
        let total = pages.len().max(1);
        Self { pages, total, index: 0, valid, lines: Vec::new(), lines_page: None, left: 0, top: 2 }
    }

    fn page_lines(&mut self, ui: &Ui) -> &[Component] {
        if self.lines_page != Some(self.index) {
            let raw = self.pages.get(self.index).map_or("\"\"", String::as_str);
            let comp = if self.valid {
                crate::chat::parse(raw, &ui.lang)
            } else {
                Component::text("\u{a7}4* Invalid book tag *")
            };
            self.lines = split_text(&comp, 116, &ui.font, true, true, true);
            self.lines_page = Some(self.index);
        }
        &self.lines
    }

    fn next_visible(&self) -> bool {
        self.index + 1 < self.total
    }

    fn prev_visible(&self) -> bool {
        self.index > 0
    }
}

/// `ItemWritableBook.isNBTValid` + `ItemEditableBook.validBookTagContents`.
fn book_tag_valid(nbt: &mc_core::nbt::Compound) -> bool {
    let Some(mc_core::nbt::Tag::List(8, pages)) = nbt.get("pages") else { return false };
    for page in pages {
        match page {
            mc_core::nbt::Tag::String(s) if s.chars().count() <= 32767 => {}
            _ => return false,
        }
    }
    let Some(mc_core::nbt::Tag::String(title)) = nbt.get("title") else { return false };
    title.chars().count() <= 32 && matches!(nbt.get("author"), Some(mc_core::nbt::Tag::String(_)))
}

impl Screen for BookScreen {
    fn init(&mut self, _ui: &Ui, w: i32, _h: i32) {
        self.left = (w - 192) / 2;
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, _h: i32, mx: i32, my: i32) {
        // `GuiScreenBook.drawScreen` draws the book straight over the world: no dim background.
        self.left = (w - 192) / 2;
        let (x, y) = (self.left, self.top);
        b.color = [1.0; 4];
        b.textured_rect("textures/gui/book.png", x, y, 0, 0, 192, 192);
        // `book.pageIndicator` with (page + 1, total)
        let (a, c) = ((self.index + 1).to_string(), self.total.to_string());
        let indicator = ui.lang.format("book.pageIndicator", &[&a, &c]);
        let iw = ui.font.width(&indicator);
        ui.font.draw_shadow(b, &indicator, (x - iw + 192 - 44) as f32, (y + 16) as f32, 0xff000000);
        let lines = self.page_lines(ui).to_vec();
        let max = (128 / 9).min(lines.len() as i32);
        for i in 0..max as usize {
            let line = lines[i].formatted();
            ui.font.draw_shadow(b, &line, (x + 36) as f32, (y + 32 + i as i32 * 9) as f32, 0xff000000);
        }
        self.draw_page_button(b, x + 120, y + 156, true, mx, my, self.next_visible());
        self.draw_page_button(b, x + 38, y + 156, false, mx, my, self.prev_visible());
        let done = ui.lang.get("gui.done");
        let (bx, by) = (w / 2 - 100, 196);
        let hovered = (bx..bx + 200).contains(&mx) && (by..by + 20).contains(&my);
        b.textured_rect("textures/gui/widgets.png", bx, by, 0, if hovered { 66 } else { 46 }, 200, 20);
        let color = if hovered { 0xffffa0 } else { 0xe0e0e0 };
        ui.font.draw_centered(b, &done, w / 2 - 100 + 100, by + 6, color);
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        // `GuiScreenBook.mouseClicked`: a change_page link is handled before the widgets.
        if let Some(page) = self.page_link_at(ui, mx, my) {
            self.index = page;
            return Action::None;
        }
        let (x, y) = (self.left, self.top);
        if (x + 120..x + 143).contains(&mx) && (y + 156..y + 169).contains(&my) {
            if self.next_visible() {
                self.index += 1;
            }
            return Action::None;
        }
        if (x + 38..x + 61).contains(&mx) && (y + 156..y + 169).contains(&my) {
            if self.prev_visible() {
                self.index -= 1;
            }
            return Action::None;
        }
        let bx = self.done_left();
        // The Done button covers 200×20 at (w/2 - 100, 196).
        if (bx..bx + 200).contains(&mx) && (196..216).contains(&my) {
            return Action::Close;
        }
        Action::None
    }

    fn key(&mut self, _ui: &mut Ui, key: Key) -> Action {
        // `GuiScreenBook.keyTyped` defers to `GuiScreen.keyTyped` for Escape and the inventory key.
        if key == Key::Escape || key == Key::Inventory {
            Action::Close
        } else {
            Action::None
        }
    }
}

// Helpers so the Done button geometry stays in one place; the screen has no window width
// field, so the button hit test recomputes from the current left edge.
fn ui_width(s: &BookScreen) -> i32 {
    s.left * 2 + 192
}

impl BookScreen {
    fn done_left(&self) -> i32 {
        ui_width(self) / 2 - 100
    }

    fn draw_page_button(&self, b: &mut GuiBatch, x: i32, y: i32, next: bool, mx: i32, my: i32, visible: bool) {
        if !visible {
            return;
        }
        let hover = (x..x + 23).contains(&mx) && (y..y + 13).contains(&my);
        let u = if hover { 23 } else { 0 };
        let v = if next { 192 } else { 205 };
        b.textured_rect("textures/gui/book.png", x, y, u, v, 23, 13);
    }

    /// `func_175385_b`: the component piece under the mouse on the current page.
    fn page_link_at(&self, ui: &Ui, mx: i32, my: i32) -> Option<usize> {
        let i = mx - self.left - 36;
        let j = my - self.top - 32;
        if i < 0 || j < 0 || i > 116 {
            return None;
        }
        let k = (128 / 9).min(self.lines.len() as i32);
        if j >= 9 * k + k {
            return None;
        }
        let line = self.lines.get((j / 9) as usize)?;
        let mut x = 0;
        for piece in &line.0 {
            x += ui.font.width(&piece.text);
            if x > i {
                let click = piece.click.as_ref()?;
                return match click.action {
                    ClickAction::ChangePage => click.value.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).filter(|n| *n < self.total),
                    _ => None,
                };
            }
        }
        None
    }
}

// ---------------------------------------------------------------- GuiMerchant

/// `GuiMerchant`: `ContainerMerchant`'s slots, click prediction and `SlotMerchantResult` live in
/// [`ContainerScreen`]; this screen adds the recipe buttons, the selected recipe's preview and
/// its tooltips.
pub(crate) struct MerchantScreen {
    container: ContainerScreen,
}

impl MerchantScreen {
    pub(crate) fn new(window: Arc<Mutex<Window>>, catalogue: Arc<Catalogue>) -> Self {
        Self { container: ContainerScreen::new(window, catalogue) }
    }

    /// `GuiMerchant.actionPerformed`: move the selection (clamped to the list), then
    /// `ContainerMerchant.setCurrentRecipeIndex` and the `MC|TrSel` payload.
    fn select(&mut self, delta: i32) -> Action {
        let n = self.container.trade_count() as i32;
        let mut selected = self.container.selected_recipe() as i32 + delta;
        if delta > 0 && selected >= n {
            selected = n - 1;
        }
        if selected < 0 {
            selected = 0;
        }
        let selected = selected.max(0) as usize;
        self.container.set_recipe_index(selected);
        Action::MerchantSelect(selected as i32)
    }

    /// `GuiMerchant.MerchantButton.drawButton`: 12×19, the texture column at 176 + 12·state and
    /// the row at 19 for the previous button.
    fn draw_button(&self, b: &mut GuiBatch, x: i32, y: i32, next: bool, mx: i32, my: i32, enabled: bool) {
        let hover = (x..x + 12).contains(&mx) && (y..y + 19).contains(&my);
        let u = if !enabled { 24 } else if hover { 12 } else { 0 };
        let v = if next { 0 } else { 19 };
        b.textured_rect("textures/gui/container/villager.png", x, y, 176 + u, v, 12, 19);
    }
}

impl Screen for MerchantScreen {
    fn init(&mut self, ui: &Ui, w: i32, h: i32) {
        self.container.init(ui, w, h);
    }

    fn draw(&mut self, ui: &mut Ui, b: &mut GuiBatch, w: i32, h: i32, mx: i32, my: i32) {
        // `GuiContainer.drawScreen`: panel, title, slots, cursor and the hovered slot's tooltip.
        self.container.draw(ui, b, w, h, mx, my);
        let (x, y) = self.container.origin();
        let selected = self.container.trade();
        // `GuiMerchant.drawScreen` previews the selected recipe above the three slots.
        if let Some(r) = &selected {
            for (s, dx) in [(r.buy, 36), (r.buy2, 62), (r.sell, 120)] {
                if let Some(s) = s {
                    self.container.catalogue.draw(ui, b, s, x + dx, y + 24);
                }
            }
        }
        // `drawGuiContainerBackgroundLayer`'s disabled-recipe scroll.
        if selected.as_ref().is_some_and(Trade::disabled) {
            b.textured_rect("textures/gui/container/villager.png", x + 83, y + 21, 212, 0, 28, 21);
            b.textured_rect("textures/gui/container/villager.png", x + 83, y + 51, 212, 0, 28, 21);
        }
        // `GuiMerchant.updateScreen`'s enabled flags; `initGui` starts both disabled.
        let n = self.container.trade_count();
        let sel = self.container.selected_recipe();
        self.draw_button(b, x + 147, y + 23, true, mx, my, sel + 1 < n);
        self.draw_button(b, x + 17, y + 23, false, mx, my, sel > 0);
        // `GuiMerchant.drawScreen`'s trade tooltips come after the container's and, like
        // vanilla, are not suppressed while the cursor holds a stack.
        if let Some(r) = &selected {
            for (dx, s) in [(36, r.buy), (62, r.buy2), (120, r.sell)] {
                if let Some(s) = s {
                    if (x + dx - 1..x + dx + 17).contains(&mx) && (y + 24 - 1..y + 24 + 17).contains(&my) {
                        let lines = tooltip::render_lines(&self.container.catalogue, &ui.lang, &s, ui.settings.advanced_item_tooltips);
                        ui.draw_hovering_text(b, &lines, mx, my, w, h);
                        return;
                    }
                }
            }
            // `isPointInRegion(83, 21, 28, 21) || isPointInRegion(83, 51, 28, 21)`.
            let over_scroll = (x + 83 - 1..x + 83 + 28 + 1).contains(&mx)
                && ((y + 21 - 1..y + 21 + 21 + 1).contains(&my) || (y + 51 - 1..y + 51 + 21 + 1).contains(&my));
            if r.disabled() && over_scroll {
                let text = vec![ui.lang.get("merchant.deprecated")];
                ui.draw_hovering_text(b, &text, mx, my, w, h);
            }
        }
    }

    fn click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        let (x, y) = self.container.origin();
        // `GuiMerchant.initGui`: next at (i + 120 + 27, j + 24 - 1), previous at (i + 36 - 19, ...).
        if (x + 147..x + 159).contains(&mx) && (y + 23..y + 42).contains(&my) {
            if self.container.selected_recipe() + 1 < self.container.trade_count() {
                return self.select(1);
            }
            return Action::None;
        }
        if (x + 17..x + 29).contains(&mx) && (y + 23..y + 42).contains(&my) {
            if self.container.selected_recipe() > 0 {
                return self.select(-1);
            }
            return Action::None;
        }
        self.container.click(ui, mx, my)
    }

    fn right_click(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.container.right_click(ui, mx, my)
    }

    fn pick_item(&mut self, ui: &mut Ui, mx: i32, my: i32) -> Action {
        self.container.pick_item(ui, mx, my)
    }

    fn release_at(&mut self, ui: &mut Ui, mx: i32, my: i32, button: u8) {
        self.container.release_at(ui, mx, my, button);
    }

    fn mouse_move(&mut self, ui: &mut Ui, mx: i32, my: i32) {
        self.container.mouse_move(ui, mx, my);
    }

    fn key(&mut self, ui: &mut Ui, key: Key) -> Action {
        // `GuiMerchant` adds no keys of its own: `GuiContainer.keyTyped` handles Escape, the
        // inventory key, pick block, drop and the hotbar number keys.
        self.container.key(ui, key)
    }

    fn merchant_recipes(&mut self, window_id: i32, recipes: Vec<Trade>) {
        self.container.merchant_recipes(window_id, recipes);
    }

    fn pauses(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------- MC|TrList

/// One `MerchantRecipe` before the wire `Slot`s are mapped through the item catalogue.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RawTrade {
    pub buy: Option<mc_protocol::Slot>,
    pub buy2: Option<mc_protocol::Slot>,
    pub sell: Option<mc_protocol::Slot>,
    pub uses: i32,
    pub max_uses: i32,
}

/// `MerchantRecipeList.readFromBuf`: the leading window id, then the byte-counted recipes.
pub(crate) fn parse_merchant_list(data: &[u8]) -> Option<(i32, Vec<RawTrade>)> {
    let mut r = mc_protocol::Reader::new(data);
    let window_id = r.i32().ok()?;
    let n = r.u8().ok()? as usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let buy = r.slot().ok()?;
        let sell = r.slot().ok()?;
        let buy2 = if r.bool().ok()? { r.slot().ok()? } else { None };
        let disabled = r.bool().ok()?;
        let uses = r.i32().ok()?;
        let max_uses = r.i32().ok()?;
        out.push(RawTrade { buy, buy2, sell, uses: if disabled { max_uses } else { uses }, max_uses });
    }
    Some((window_id, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_protocol::Writer;

    fn slot(id: i16, count: i8, damage: i16) -> mc_protocol::Slot {
        mc_protocol::Slot { id, count, damage, nbt: None }
    }

    #[test]
    fn merchant_list_parses_window_id_and_disabled_multiplier() {
        let mut w = Writer::new();
        w.i32(7);
        w.u8(2);
        w.slot(Some(&slot(388, 3, 0)));
        w.slot(Some(&slot(278, 1, 0)));
        w.bool(false);
        w.bool(false);
        w.i32(0);
        w.i32(7);
        w.slot(Some(&slot(388, 1, 0)));
        w.slot(Some(&slot(373, 1, 0)));
        w.bool(true);
        w.slot(Some(&slot(388, 1, 0)));
        w.bool(true);
        w.i32(0);
        w.i32(4);
        let (id, list) = parse_merchant_list(&w.buf).unwrap();
        assert_eq!(id, 7);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].buy.as_ref().unwrap().id, 388);
        assert_eq!(list[0].uses, 0);
        assert_eq!(list[1].buy2.as_ref().unwrap().id, 388);
        // `compensateToolUses`: the disabled flag replaces `uses` with `maxUses`.
        assert_eq!((list[1].uses, list[1].max_uses), (4, 4));
    }

    #[test]
    fn merchant_list_stops_on_truncated_payload() {
        let mut w = Writer::new();
        w.i32(1);
        w.u8(1);
        w.slot(Some(&slot(388, 3, 0)));
        assert!(parse_merchant_list(&w.buf).is_none());
    }

    #[test]
    fn signed_book_validity_follows_writable_and_editable_tag_checks() {
        let nbt = |pages: Vec<mc_core::nbt::Tag>, title: &str, author: Option<&str>| {
            let mut c = mc_core::nbt::Compound::new();
            c.insert("pages".into(), mc_core::nbt::Tag::List(8, pages));
            c.insert("title".into(), mc_core::nbt::Tag::String(title.into()));
            if let Some(a) = author {
                c.insert("author".into(), mc_core::nbt::Tag::String(a.into()));
            }
            c
        };
        let page = mc_core::nbt::Tag::String("{}".into());
        assert!(book_tag_valid(&nbt(vec![page.clone()], "Title", Some("A"))));
        assert!(!book_tag_valid(&nbt(vec![page.clone()], "x".repeat(33).as_str(), Some("A"))));
        assert!(!book_tag_valid(&nbt(vec![page], "Title", None)));
        assert!(!book_tag_valid(&nbt(vec![mc_core::nbt::Tag::Int(1)], "T", Some("A"))));
    }

    #[test]
    fn sign_close_serialises_plain_lines_once() {
        let raw = [r#""a""#.to_string(), r#""b""#.to_string(), r#""""#.to_string(), r#"{"text":"d","color":"red"}"#.to_string()];
        let plain = ["a".to_string(), "b".to_string(), String::new(), "d".to_string()];
        let mut s = SignScreen::new(BlockPos::new(1, 2, 3), raw, plain);
        // Editing the first line replaces only that component; untouched lines keep their JSON.
        s.lines[0] = "ab".into();
        s.edited[0] = true;
        match s.close_action() {
            Action::UpdateSign { pos, lines } => {
                assert_eq!(pos, BlockPos::new(1, 2, 3));
                assert_eq!(lines[0], r#"{"text":"ab"}"#);
                assert_eq!(lines[3], r#"{"text":"d","color":"red"}"#);
            }
            _ => panic!("sign did not queue C12"),
        }
        assert!(matches!(s.close_action(), Action::None), "onGuiClosed must not send twice");
    }

    #[test]
    fn merchant_buttons_follow_gui_merchant_states() {
        let mut ui = Ui::new(&mc_assets::AssetSource::new(), mc_gui::settings::Settings::default());
        let w = Arc::new(Mutex::new(Window::new(1, "minecraft:villager", String::new(), 3)));
        let mut s = MerchantScreen::new(w, Arc::new(Catalogue::metadata()));
        s.init(&ui, 400, 300);
        // `GuiMerchant.initGui` puts next at (i+147, j+23) and previous at (i+17, j+23); both
        // start disabled and `updateScreen` only enables them once a recipe list is present.
        let (x, y) = (62, 67);
        assert!(matches!(s.click(&mut ui, x + 148, y + 24), Action::None));
        // One recipe leaves both buttons disabled.
        s.merchant_recipes(1, vec![Trade::default()]);
        assert!(matches!(s.click(&mut ui, x + 148, y + 24), Action::None));
        assert!(matches!(s.click(&mut ui, x + 18, y + 24), Action::None));
        // Three recipes: next sends MC|TrSel and moves the selection; at the end it is disabled.
        s.merchant_recipes(1, vec![Trade::default(), Trade::default(), Trade::default()]);
        assert!(matches!(s.click(&mut ui, x + 148, y + 24), Action::MerchantSelect(1)));
        assert!(matches!(s.click(&mut ui, x + 148, y + 24), Action::MerchantSelect(2)));
        assert!(matches!(s.click(&mut ui, x + 148, y + 24), Action::None));
        // Previous clamps at 0 and then disables itself.
        assert!(matches!(s.click(&mut ui, x + 18, y + 24), Action::MerchantSelect(1)));
        assert!(matches!(s.click(&mut ui, x + 18, y + 24), Action::MerchantSelect(0)));
        assert!(matches!(s.click(&mut ui, x + 18, y + 24), Action::None));
        // A shorter list arrives: `NpcMerchant.setRecipes` keeps the client's selected index
        // (vanilla `updateScreen` only recomputes the button flags), so previous stays enabled.
        s.select(1);
        s.select(1);
        assert_eq!(s.container.selected_recipe(), 2);
        s.merchant_recipes(1, vec![Trade::default()]);
        assert_eq!(s.container.selected_recipe(), 2);
        assert!(matches!(s.click(&mut ui, x + 18, y + 24), Action::MerchantSelect(1)));
        // `MC|TrSel` for another window is ignored.
        s.merchant_recipes(2, vec![Trade::default(), Trade::default(), Trade::default()]);
        assert_eq!(s.container.trade_count(), 1);
    }
}
