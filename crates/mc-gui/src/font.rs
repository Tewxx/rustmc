//! Port of `FontRenderer`: `ascii.png` glyphs with measured widths, unicode page
//! fallback via `glyph_sizes.bin`, `§` formatting codes and drop shadows.

use crate::batch::GuiBatch;
use mc_assets::AssetSource;

const ASCII: &str = "\u{00c0}\u{00c1}\u{00c2}\u{00c8}\u{00ca}\u{00cb}\u{00cd}\u{00d3}\u{00d4}\u{00d5}\u{00da}\u{00df}\u{00e3}\u{00f5}\u{011f}\u{0130}\u{0131}\u{0152}\u{0153}\u{015e}\u{015f}\u{0174}\u{0175}\u{017e}\u{0207}\u{0000}\u{0000}\u{0000}\u{0000}\u{0000}\u{0000}\u{0000} !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\u{0000}\u{00c7}\u{00fc}\u{00e9}\u{00e2}\u{00e4}\u{00e0}\u{00e5}\u{00e7}\u{00ea}\u{00eb}\u{00e8}\u{00ef}\u{00ee}\u{00ec}\u{00c4}\u{00c5}\u{00c9}\u{00e6}\u{00c6}\u{00f4}\u{00f6}\u{00f2}\u{00fb}\u{00f9}\u{00ff}\u{00d6}\u{00dc}\u{00f8}\u{00a3}\u{00d8}\u{00d7}\u{0192}\u{00e1}\u{00ed}\u{00f3}\u{00fa}\u{00f1}\u{00d1}\u{00aa}\u{00ba}\u{00bf}\u{00ae}\u{00ac}\u{00bd}\u{00bc}\u{00a1}\u{00ab}\u{00bb}\u{2591}\u{2592}\u{2593}\u{2502}\u{2524}\u{2561}\u{2562}\u{2556}\u{2555}\u{2563}\u{2551}\u{2557}\u{255d}\u{255c}\u{255b}\u{2510}\u{2514}\u{2534}\u{252c}\u{251c}\u{2500}\u{253c}\u{255e}\u{255f}\u{255a}\u{2554}\u{2569}\u{2566}\u{2560}\u{2550}\u{256c}\u{2567}\u{2568}\u{2564}\u{2565}\u{2559}\u{2558}\u{2552}\u{2553}\u{256b}\u{256a}\u{2518}\u{250c}\u{2588}\u{2584}\u{258c}\u{2590}\u{2580}\u{03b1}\u{03b2}\u{0393}\u{03c0}\u{03a3}\u{03c3}\u{03bc}\u{03c4}\u{03a6}\u{0398}\u{03a9}\u{03b4}\u{221e}\u{2205}\u{2208}\u{2229}\u{2261}\u{00b1}\u{2265}\u{2264}\u{2320}\u{2321}\u{00f7}\u{2248}\u{00b0}\u{2219}\u{00b7}\u{221a}\u{207f}\u{00b2}\u{25a0}\u{0000}";

const ASCII_TEX: &str = "textures/font/ascii.png";
pub const FONT_HEIGHT: i32 = 9;

#[derive(Clone)]
pub struct FontRenderer {
    char_width: [i32; 256],
    glyph_width: Vec<u8>,
    color_code: [u32; 32],
    /// `ASCII` as UTF-16 units, for vanilla's `indexOf`.
    table: Vec<u16>,
    rng: u64,
}

struct State {
    x: f32,
    y: f32,
    /// Current RGBA (0–1).
    color: [f32; 4],
    base: [f32; 4],
    random: bool,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

impl FontRenderer {
    pub fn load(assets: &AssetSource) -> Self {
        let color_code = std::array::from_fn(|i| {
            let j = ((i >> 3) & 1) as u32 * 85;
            let mut k = ((i >> 2) & 1) as u32 * 170 + j;
            let mut l = ((i >> 1) & 1) as u32 * 170 + j;
            let mut i1 = (i & 1) as u32 * 170 + j;
            if i == 6 {
                k += 85;
            }
            if i >= 16 {
                k /= 4;
                l /= 4;
                i1 /= 4;
            }
            ((k & 255) << 16) | ((l & 255) << 8) | (i1 & 255)
        });
        let mut char_width = [0; 256];
        if let Some(img) = assets.read(ASCII_TEX).and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8()) {
            // readFontTexture
            let (w, h) = (img.width() as i32, img.height() as i32);
            let (cell_h, cell_w) = (h / 16, w / 16);
            let scale = 8.0 / cell_w as f32;
            for (c, width) in char_width.iter_mut().enumerate() {
                let (col, row) = (c as i32 % 16, c as i32 / 16);
                let mut i2 = cell_w - 1;
                while i2 >= 0 {
                    let x = col * cell_w + i2;
                    let empty = (0..cell_h).all(|k2| img.get_pixel(x as u32, (row * cell_w + k2) as u32)[3] == 0);
                    if !empty {
                        break;
                    }
                    i2 -= 1;
                }
                i2 += 1;
                *width = (0.5 + (i2 as f32 * scale) as f64) as i32 + 1;
            }
            char_width[32] = 4;
        }
        let glyph_width = assets.read("font/glyph_sizes.bin").unwrap_or_else(|| vec![0; 65536]);
        Self { char_width, glyph_width, color_code, table: ASCII.encode_utf16().collect(), rng: 0x5DEECE66D }
    }

    fn ascii_index(&self, c: u16) -> Option<usize> {
        self.table.iter().position(|&t| t == c)
    }

    fn glyph(&self, c: u16) -> u8 {
        self.glyph_width.get(c as usize).copied().unwrap_or(0)
    }

    /// `getCharWidth`
    pub fn char_width(&self, c: u16) -> i32 {
        if c == 167 {
            return -1;
        }
        if c == 32 {
            return 4;
        }
        if let Some(i) = self.ascii_index(c).filter(|_| c > 0) {
            return self.char_width[i];
        }
        let g = self.glyph(c);
        if g != 0 {
            let mut j = (g >> 4) as i32;
            let mut k = (g & 15) as i32;
            if k > 7 {
                k = 15;
                j = 0;
            }
            k += 1;
            return (k - j) / 2 + 1;
        }
        0
    }

    /// `getStringWidth`
    pub fn width(&self, text: &str) -> i32 {
        let s: Vec<u16> = text.encode_utf16().collect();
        let (mut w, mut bold) = (0, false);
        let mut j = 0;
        while j < s.len() {
            let mut k = self.char_width(s[j]);
            if k < 0 && j < s.len() - 1 {
                j += 1;
                match s[j] {
                    108 | 76 => bold = true,
                    114 | 82 => bold = false,
                    _ => {}
                }
                k = 0;
            }
            w += k;
            if bold && k > 0 {
                w += 1;
            }
            j += 1;
        }
        w
    }

    /// `listFormattedStringToWidth`: wraps at spaces (or mid-word), carrying the active
    /// formatting codes onto each continuation line.
    pub fn list_to_width(&self, text: &str, width: i32) -> Vec<String> {
        let mut out = Vec::new();
        let mut s: Vec<u16> = text.encode_utf16().collect();
        loop {
            // A first character wider than the line would recurse forever in vanilla; take it.
            let i = self.size_to_width(&s, width).max(1);
            if s.len() <= i {
                out.push(String::from_utf16_lossy(&s));
                break;
            }
            let head = s[..i].to_vec();
            let skip = matches!(s[i], 32 | 10) as usize;
            let mut rest: Vec<u16> = format_from_string(&head);
            rest.extend_from_slice(&s[i + skip..]);
            out.push(String::from_utf16_lossy(&head));
            s = rest;
        }
        // String.split drops trailing empty strings.
        while out.len() > 1 && out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out
    }

    /// `sizeStringToWidth`
    fn size_to_width(&self, s: &[u16], width: i32) -> usize {
        let i = s.len();
        let (mut j, mut k, mut l, mut bold) = (0, 0usize, None::<usize>, false);
        while k < i {
            let c = s[k];
            match c {
                10 => {}
                167 => {
                    if k < i - 1 {
                        k += 1;
                        let c1 = s[k];
                        if c1 == 108 || c1 == 76 {
                            bold = true;
                        } else if c1 == 114 || c1 == 82 || is_format_color(c1) {
                            bold = false;
                        }
                    }
                }
                _ => {
                    if c == 32 {
                        l = Some(k);
                    }
                    j += self.char_width(c);
                    if bold {
                        j += 1;
                    }
                }
            }
            if c == 10 {
                // The newline itself is consumed by the split.
                l = Some(k);
                k = l.unwrap();
                return k;
            }
            if j > width {
                break;
            }
            k += 1;
        }
        match l {
            Some(l) if k != i && l < k => l,
            _ => k,
        }
    }

    /// `trimStringToWidth` (forward only).
    pub fn trim_to_width(&self, text: &str, width: i32) -> String {
        self.trim(text, width, false)
    }

    /// `trimStringToWidth(text, width, reverse)`: the longest prefix (or suffix) that fits,
    /// skipping `§` codes and counting bold's extra pixel.
    pub fn trim(&self, text: &str, width: i32, reverse: bool) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out: std::collections::VecDeque<char> = std::collections::VecDeque::new();
        let (mut i, mut code, mut bold) = (0, false, false);
        let n = chars.len() as isize;
        let (mut l, k) = if reverse { (n - 1, -1) } else { (0, 1) };
        while l >= 0 && l < n && i < width {
            let c0 = chars[l as usize];
            let i1 = self.char_width(c0 as u32 as u16);
            if code {
                code = false;
                if c0 != 'l' && c0 != 'L' {
                    if c0 == 'r' || c0 == 'R' {
                        bold = false;
                    }
                } else {
                    bold = true;
                }
            } else if i1 < 0 {
                code = true;
            } else {
                i += i1;
                if bold {
                    i += 1;
                }
            }
            if i > width {
                break;
            }
            if reverse {
                out.push_front(c0);
            } else {
                out.push_back(c0);
            }
            l += k;
        }
        out.into_iter().collect()
    }

    pub fn draw(&mut self, b: &mut GuiBatch, text: &str, x: f32, y: f32, color: u32) -> i32 {
        self.render_string(b, text, x, y, color, false)
    }

    pub fn draw_shadow(&mut self, b: &mut GuiBatch, text: &str, x: f32, y: f32, color: u32) -> i32 {
        let i = self.render_string(b, text, x + 1.0, y + 1.0, color, true);
        i.max(self.render_string(b, text, x, y, color, false))
    }

    /// `Gui.drawCenteredString` (always shadowed).
    pub fn draw_centered(&mut self, b: &mut GuiBatch, text: &str, x: i32, y: i32, color: u32) {
        let w = self.width(text);
        self.draw_shadow(b, text, (x - w / 2) as f32, y as f32, color);
    }

    fn render_string(&mut self, b: &mut GuiBatch, text: &str, x: f32, y: f32, mut color: u32, shadow: bool) -> i32 {
        if color & 0xFC00_0000 == 0 {
            color |= 0xFF00_0000;
        }
        if shadow {
            color = ((color & 0x00FC_FCFC) >> 2) | (color & 0xFF00_0000);
        }
        let c = [
            ((color >> 16) & 255) as f32 / 255.0,
            ((color >> 8) & 255) as f32 / 255.0,
            (color & 255) as f32 / 255.0,
            ((color >> 24) & 255) as f32 / 255.0,
        ];
        let mut st = State { x, y, color: c, base: c, random: false, bold: false, italic: false, underline: false, strike: false };
        self.render_at_pos(b, text, shadow, &mut st);
        st.x as i32
    }

    fn next_random(&mut self, bound: usize) -> usize {
        // java.util.Random LCG; exact sequence doesn't matter for obfuscated text.
        self.rng = (self.rng.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        ((self.rng >> 17) as usize) % bound
    }

    fn render_at_pos(&mut self, b: &mut GuiBatch, text: &str, shadow: bool, st: &mut State) {
        let s: Vec<u16> = text.encode_utf16().collect();
        let mut i = 0;
        while i < s.len() {
            let mut c0 = s[i];
            if c0 == 167 && i + 1 < s.len() {
                let code = (s[i + 1] as u8 as char).to_ascii_lowercase();
                let i1 = "0123456789abcdefklmnor".find(code).map_or(-1, |p| p as i32);
                if i1 < 16 {
                    st.random = false;
                    st.bold = false;
                    st.strike = false;
                    st.underline = false;
                    st.italic = false;
                    let mut idx = if !(0..=15).contains(&i1) { 15 } else { i1 } as usize;
                    if shadow {
                        idx += 16;
                    }
                    let j1 = self.color_code[idx];
                    st.color = [((j1 >> 16) & 255) as f32 / 255.0, ((j1 >> 8) & 255) as f32 / 255.0, (j1 & 255) as f32 / 255.0, st.base[3]];
                } else {
                    match i1 {
                        16 => st.random = true,
                        17 => st.bold = true,
                        18 => st.strike = true,
                        19 => st.underline = true,
                        20 => st.italic = true,
                        _ => {
                            st.random = false;
                            st.bold = false;
                            st.strike = false;
                            st.underline = false;
                            st.italic = false;
                            st.color = st.base;
                        }
                    }
                }
                i += 2;
                continue;
            }

            let mut j = self.ascii_index(c0);
            if st.random {
                if let Some(_) = j {
                    let k = self.char_width(c0);
                    loop {
                        let r = self.next_random(self.table.len());
                        let c1 = self.table[r];
                        if k == self.char_width(c1) {
                            c0 = c1;
                            j = Some(r);
                            break;
                        }
                    }
                }
            }
            let f1 = 1.0;
            // Unicode glyphs have their own 0.5px shadow offset instead of the 1px one.
            let flag = (c0 == 0 || j.is_none()) && shadow;
            if flag {
                st.x -= f1;
                st.y -= f1;
            }
            let mut f = self.render_char(b, c0, st);
            if flag {
                st.x += f1;
                st.y += f1;
            }
            if st.bold {
                st.x += f1;
                if flag {
                    st.x -= f1;
                    st.y -= f1;
                }
                self.render_char(b, c0, st);
                st.x -= f1;
                if flag {
                    st.x += f1;
                    st.y += f1;
                }
                f += 1.0;
            }
            let col = st.color.map(|v| (v * 255.0) as u8);
            let half = (FONT_HEIGHT / 2) as f32;
            if st.strike {
                b.rect_f(st.x, st.y + half - 1.0, st.x + f, st.y + half, col);
            }
            if st.underline {
                b.rect_f(st.x - 1.0, st.y + FONT_HEIGHT as f32 - 1.0, st.x + f, st.y + FONT_HEIGHT as f32, col);
            }
            st.x += (f as i32) as f32;
            i += 1;
        }
    }

    /// `renderChar`: returns the advance.
    fn render_char(&self, b: &mut GuiBatch, c: u16, st: &State) -> f32 {
        if c == 32 {
            return 4.0;
        }
        match self.ascii_index(c) {
            Some(i) => self.render_default_char(b, i, st),
            None => self.render_unicode_char(b, c, st),
        }
    }

    fn render_default_char(&self, b: &mut GuiBatch, ch: usize, st: &State) -> f32 {
        let i = (ch % 16 * 8) as f32;
        let j = (ch / 16 * 8) as f32;
        let k = if st.italic { 1.0 } else { 0.0 };
        let l = self.char_width[ch];
        let f = l as f32 - 0.01;
        let (x, y) = (st.x, st.y);
        let col = st.color.map(|v| (v * 255.0) as u8);
        // GL_TRIANGLE_STRIP TL, BL, TR, BR → quad TL, BL, BR, TR.
        b.quad(
            Some(ASCII_TEX),
            true,
            [[x + k, y], [x - k, y + 7.99], [x + f - 1.0 - k, y + 7.99], [x + f - 1.0 + k, y]],
            [
                [i / 128.0, j / 128.0],
                [i / 128.0, (j + 7.99) / 128.0],
                [(i + f - 1.0) / 128.0, (j + 7.99) / 128.0],
                [(i + f - 1.0) / 128.0, j / 128.0],
            ],
            [col; 4],
        );
        l as f32
    }

    fn render_unicode_char(&self, b: &mut GuiBatch, c: u16, st: &State) -> f32 {
        let g = self.glyph(c);
        if g == 0 {
            return 0.0;
        }
        let page = format!("textures/font/unicode_page_{:02x}.png", c / 256);
        let start = (g >> 4) as f32;
        let end = ((g & 15) + 1) as f32;
        let f2 = (c % 16 * 16) as f32 + start;
        let f3 = ((c & 255) / 16 * 16) as f32;
        let f4 = end - start - 0.02;
        let f5 = if st.italic { 1.0 } else { 0.0 };
        let (x, y) = (st.x, st.y);
        let col = st.color.map(|v| (v * 255.0) as u8);
        b.quad(
            Some(&page),
            true,
            [[x + f5, y], [x - f5, y + 7.99], [x + f4 / 2.0 - f5, y + 7.99], [x + f4 / 2.0 + f5, y]],
            [
                [f2 / 256.0, f3 / 256.0],
                [f2 / 256.0, (f3 + 15.98) / 256.0],
                [(f2 + f4) / 256.0, (f3 + 15.98) / 256.0],
                [(f2 + f4) / 256.0, f3 / 256.0],
            ],
            [col; 4],
        );
        (end - start) / 2.0 + 1.0
    }
}

fn is_format_color(c: u16) -> bool {
    (48..=57).contains(&c) || (97..=102).contains(&c) || (65..=70).contains(&c)
}

fn is_format_special(c: u16) -> bool {
    (107..=111).contains(&c) || (75..=79).contains(&c) || c == 114 || c == 82
}

/// `getFormatFromString`: the color and style codes in effect at the end of `text`.
pub fn format_from_string(text: &[u16]) -> Vec<u16> {
    let mut s = Vec::new();
    for i in 0..text.len().saturating_sub(1) {
        if text[i] == 167 {
            let c = text[i + 1];
            if is_format_color(c) {
                s = vec![167, c];
            } else if is_format_special(c) {
                s.extend_from_slice(&[167, c]);
            }
        }
    }
    s
}
