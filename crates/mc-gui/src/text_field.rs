//! `GuiTextField`: cursor, selection, word jumps, clipboard and the horizontal scroll that keeps
//! the cursor visible. Indices are in `char`s (Java's UTF-16 indices for BMP text).

use crate::batch::{Blend, GuiBatch};
use crate::screens::Key;
use crate::Ui;

pub struct TextField {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    text: Vec<char>,
    pub max_len: usize,
    cursor_counter: i32,
    pub background: bool,
    pub can_lose_focus: bool,
    pub focused: bool,
    pub enabled: bool,
    line_scroll: usize,
    cursor: usize,
    selection_end: usize,
    pub enabled_color: u32,
    pub disabled_color: u32,
}

impl TextField {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            text: Vec::new(),
            max_len: 32,
            cursor_counter: 0,
            background: true,
            can_lose_focus: true,
            focused: false,
            enabled: true,
            line_scroll: 0,
            cursor: 0,
            selection_end: 0,
            enabled_color: 14737632,
            disabled_color: 7368816,
        }
    }

    /// Moves the box to a new layout (`initGui` recreating it) and keeps text and cursor.
    pub fn relayout(&mut self, x: i32, y: i32, width: i32, height: i32) {
        (self.x, self.y, self.width, self.height) = (x, y, width, height);
    }

    pub fn tick(&mut self) {
        self.cursor_counter += 1;
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// `setText`: truncated to `max_len`, cursor to the end.
    pub fn set_text(&mut self, ui: &Ui, s: &str) {
        self.text = s.chars().take(self.max_len).collect();
        self.set_cursor_end(ui);
    }

    pub fn selected_text(&self) -> String {
        let (i, j) = self.selection();
        self.text[i..j].iter().collect()
    }

    fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.selection_end), self.cursor.max(self.selection_end))
    }

    /// `writeText`: replaces the selection with the allowed characters of `s`.
    pub fn write_text(&mut self, ui: &Ui, s: &str) {
        let filtered: Vec<char> = s.chars().filter(|&c| allowed(c)).collect();
        let (i, j) = self.selection();
        let k = (self.max_len as isize - self.text.len() as isize + (j - i) as isize).max(0) as usize;
        let mut out: Vec<char> = self.text[..i].to_vec();
        let l = filtered.len().min(k);
        out.extend_from_slice(&filtered[..l]);
        out.extend_from_slice(&self.text[j..]);
        self.text = out;
        let target = i as isize - self.selection_end as isize + l as isize;
        self.move_cursor_by(ui, target);
    }

    /// `deleteWords`
    pub fn delete_words(&mut self, ui: &Ui, n: i32) {
        if self.text.is_empty() {
            return;
        }
        if self.selection_end != self.cursor {
            self.write_text(ui, "");
        } else {
            let to = self.nth_word_from(n, self.cursor, true) as isize;
            self.delete_from_cursor(ui, to - self.cursor as isize);
        }
    }

    /// `deleteFromCursor`
    pub fn delete_from_cursor(&mut self, ui: &Ui, n: isize) {
        if self.text.is_empty() {
            return;
        }
        if self.selection_end != self.cursor {
            self.write_text(ui, "");
            return;
        }
        let back = n < 0;
        let i = if back { self.cursor as isize + n } else { self.cursor as isize };
        let j = if back { self.cursor as isize } else { self.cursor as isize + n };
        let mut s: Vec<char> = if i >= 0 { self.text[..i as usize].to_vec() } else { Vec::new() };
        if (j as usize) < self.text.len() {
            s.extend_from_slice(&self.text[j.max(0) as usize..]);
        }
        self.text = s;
        if back {
            self.move_cursor_by(ui, n);
        }
    }

    /// `func_146197_a`: the position `n` words away from `pos`.
    pub fn nth_word_from(&self, n: i32, pos: usize, skip_spaces: bool) -> usize {
        let mut i = pos;
        for _ in 0..n.unsigned_abs() {
            if n > 0 {
                let l = self.text.len();
                match self.text[i.min(l)..].iter().position(|&c| c == ' ') {
                    None => i = l,
                    Some(p) => {
                        i += p;
                        while skip_spaces && i < l && self.text[i] == ' ' {
                            i += 1;
                        }
                    }
                }
            } else {
                while skip_spaces && i > 0 && self.text[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && self.text[i - 1] != ' ' {
                    i -= 1;
                }
            }
        }
        i
    }

    pub fn move_cursor_by(&mut self, ui: &Ui, n: isize) {
        self.set_cursor(ui, (self.selection_end as isize + n).max(0) as usize);
    }

    pub fn set_cursor(&mut self, ui: &Ui, p: usize) {
        self.cursor = p.min(self.text.len());
        self.set_selection_pos(ui, self.cursor);
    }

    pub fn set_cursor_end(&mut self, ui: &Ui) {
        self.set_cursor(ui, self.text.len());
    }

    /// `getWidth`: the inner width when the border is drawn.
    pub fn inner_width(&self) -> i32 {
        if self.background {
            self.width - 8
        } else {
            self.width
        }
    }

    fn tail(&self, from: usize) -> String {
        self.text[from.min(self.text.len())..].iter().collect()
    }

    /// `setSelectionPos`, which also scrolls the visible window over the text.
    pub fn set_selection_pos(&mut self, ui: &Ui, p: usize) {
        let i = self.text.len();
        let p = p.min(i);
        self.selection_end = p;
        if self.line_scroll > i {
            self.line_scroll = i;
        }
        let j = self.inner_width();
        let s = ui.font.trim(&self.tail(self.line_scroll), j, false);
        let k = s.chars().count() + self.line_scroll;
        let mut scroll = self.line_scroll as isize;
        if p == self.line_scroll {
            let all: String = self.text.iter().collect();
            scroll -= ui.font.trim(&all, j, true).chars().count() as isize;
        }
        if p > k {
            scroll += (p - k) as isize;
        } else if p as isize <= scroll {
            scroll -= scroll - p as isize;
        }
        self.line_scroll = scroll.clamp(0, i as isize) as usize;
    }

    /// `textboxKeyTyped` for the non-character keys; returns whether the key was used.
    pub fn key(&mut self, ui: &Ui, key: Key) -> bool {
        if !self.focused {
            return false;
        }
        let (shift, ctrl) = (ui.shift_down, ui.ctrl_down);
        match key {
            Key::Combo('a') => {
                self.set_cursor_end(ui);
                self.set_selection_pos(ui, 0);
            }
            Key::Combo('c') => crate::clipboard::set(&self.selected_text()),
            Key::Combo('v') => {
                if self.enabled {
                    self.write_text(ui, &crate::clipboard::get());
                }
            }
            Key::Combo('x') => {
                crate::clipboard::set(&self.selected_text());
                if self.enabled {
                    self.write_text(ui, "");
                }
            }
            Key::Backspace => {
                if self.enabled {
                    if ctrl {
                        self.delete_words(ui, -1);
                    } else {
                        self.delete_from_cursor(ui, -1);
                    }
                }
            }
            Key::Home => {
                if shift {
                    self.set_selection_pos(ui, 0);
                } else {
                    self.set_cursor(ui, 0);
                }
            }
            Key::Left => {
                if shift {
                    let to = if ctrl { self.nth_word_from(-1, self.selection_end, true) } else { self.selection_end.saturating_sub(1) };
                    self.set_selection_pos(ui, to);
                } else if ctrl {
                    let to = self.nth_word_from(-1, self.cursor, true);
                    self.set_cursor(ui, to);
                } else {
                    self.move_cursor_by(ui, -1);
                }
            }
            Key::Right => {
                if shift {
                    let to = if ctrl { self.nth_word_from(1, self.selection_end, true) } else { self.selection_end + 1 };
                    self.set_selection_pos(ui, to);
                } else if ctrl {
                    let to = self.nth_word_from(1, self.cursor, true);
                    self.set_cursor(ui, to);
                } else {
                    self.move_cursor_by(ui, 1);
                }
            }
            Key::End => {
                if shift {
                    self.set_selection_pos(ui, self.text.len());
                } else {
                    self.set_cursor_end(ui);
                }
            }
            Key::Delete => {
                if self.enabled {
                    if ctrl {
                        self.delete_words(ui, 1);
                    } else {
                        self.delete_from_cursor(ui, 1);
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// The character half of `textboxKeyTyped`.
    pub fn typed(&mut self, ui: &Ui, c: char) -> bool {
        if !self.focused || !allowed(c) {
            return false;
        }
        if self.enabled {
            self.write_text(ui, &c.to_string());
        }
        true
    }

    /// `mouseClicked`: focus follows the click, and a left click places the cursor.
    pub fn mouse_clicked(&mut self, ui: &Ui, mx: i32, my: i32, button: i32) {
        let inside = mx >= self.x && mx < self.x + self.width && my >= self.y && my < self.y + self.height;
        if self.can_lose_focus {
            self.set_focused(inside);
        }
        if self.focused && inside && button == 0 {
            let mut i = mx - self.x;
            if self.background {
                i -= 4;
            }
            let s = ui.font.trim(&self.tail(self.line_scroll), self.inner_width(), false);
            let n = ui.font.trim(&s, i, false).chars().count();
            self.set_cursor(ui, n + self.line_scroll);
        }
    }

    pub fn set_focused(&mut self, f: bool) {
        if f && !self.focused {
            self.cursor_counter = 0;
        }
        self.focused = f;
    }

    /// `drawTextBox`
    pub fn draw(&self, ui: &mut Ui, b: &mut GuiBatch) {
        if self.background {
            b.rect(self.x - 1, self.y - 1, self.x + self.width + 1, self.y + self.height + 1, 0xFFA0_A0A0);
            b.rect(self.x, self.y, self.x + self.width, self.y + self.height, 0xFF00_0000);
        }
        let color = if self.enabled { self.enabled_color } else { self.disabled_color };
        let j = self.cursor as isize - self.line_scroll as isize;
        let mut k = self.selection_end as isize - self.line_scroll as isize;
        let s: Vec<char> = ui.font.trim(&self.tail(self.line_scroll), self.inner_width(), false).chars().collect();
        let visible = j >= 0 && j as usize <= s.len();
        let blink = self.focused && self.cursor_counter / 6 % 2 == 0 && visible;
        let l = if self.background { self.x + 4 } else { self.x };
        let i1 = if self.background { self.y + (self.height - 8) / 2 } else { self.y };
        let mut j1 = l;
        if k > s.len() as isize {
            k = s.len() as isize;
        }
        if !s.is_empty() {
            let s1: String = if visible { s[..j as usize].iter().collect() } else { s.iter().collect() };
            j1 = ui.font.draw_shadow(b, &s1, l as f32, i1 as f32, color | 0xFF00_0000);
        }
        let mid = self.cursor < self.text.len() || self.text.len() >= self.max_len;
        let mut k1 = j1;
        if !visible {
            k1 = if j > 0 { l + self.width } else { l };
        } else if mid {
            k1 = j1 - 1;
            j1 -= 1;
        }
        if !s.is_empty() && visible && (j as usize) < s.len() {
            let rest: String = s[j as usize..].iter().collect();
            ui.font.draw_shadow(b, &rest, j1 as f32, i1 as f32, color | 0xFF00_0000);
        }
        if blink {
            if mid {
                b.rect(k1, i1 - 1, k1 + 1, i1 + 1 + 9, 0xFFD0_D0D0);
            } else {
                ui.font.draw_shadow(b, "_", k1 as f32, i1 as f32, color | 0xFF00_0000);
            }
        }
        if k != j {
            let pre: String = s[..k.max(0) as usize].iter().collect();
            let l1 = l + ui.font.width(&pre);
            self.draw_selection(b, k1, i1 - 1, l1 - 1, i1 + 1 + 9);
        }
    }

    /// `drawCursorVertical`: `GL_OR_REVERSE` with blue turns the black box white and the text
    /// blue. wgpu has no logic ops, so the box is inverted (black to white, text to black).
    fn draw_selection(&self, b: &mut GuiBatch, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32) {
        if x1 < x2 {
            std::mem::swap(&mut x1, &mut x2);
        }
        if y1 < y2 {
            std::mem::swap(&mut y1, &mut y2);
        }
        let right = self.x + self.width;
        x2 = x2.min(right);
        x1 = x1.min(right);
        let saved = b.blend;
        b.blend = Blend::Invert;
        b.rect_f(x2 as f32, y2 as f32, x1 as f32, y1 as f32, [255, 255, 255, 255]);
        b.blend = saved;
    }
}

/// `ChatAllowedCharacters.isAllowedCharacter`
pub fn allowed(c: char) -> bool {
    c != '\u{a7}' && c >= ' ' && c != '\u{7f}'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui() -> Ui {
        Ui::new(&mc_assets::AssetSource::new(), crate::settings::Settings::default())
    }

    #[test]
    fn typing_selection_and_words() {
        let ui = ui();
        let mut f = TextField::new(0, 0, 200, 20);
        f.max_len = 100;
        f.set_focused(true);
        for c in "hello big world".chars() {
            f.typed(&ui, c);
        }
        assert_eq!(f.text(), "hello big world");
        assert_eq!(f.nth_word_from(-1, f.cursor(), true), 10);
        f.delete_words(&ui, -1);
        assert_eq!(f.text(), "hello big ");
        f.set_cursor(&ui, 0);
        f.set_selection_pos(&ui, 5);
        assert_eq!(f.selected_text(), "hello");
        f.write_text(&ui, "bye");
        assert_eq!(f.text(), "bye big ");
        assert_eq!(f.cursor(), 3);
    }

    #[test]
    fn max_length_truncates_writes() {
        let ui = ui();
        let mut f = TextField::new(0, 0, 200, 20);
        f.max_len = 4;
        f.set_focused(true);
        f.write_text(&ui, "abcdef");
        assert_eq!(f.text(), "abcd");
    }
}
