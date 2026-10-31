//! Immediate-mode 2D batch in GUI units, mirroring `Gui`'s drawing helpers.
//! The renderer merges consecutive commands that share texture and state.

/// `FontRenderer`'s `ascii.png`. Flat [`GuiBatch::rect`] fills sample a white texel in this
/// texture (see [`WHITE_TEXEL_UV`]) so a fill and the text drawn over it share one command.
pub const ASCII_TEX: &str = "textures/font/ascii.png";

/// Centre of an opaque white texel inside the space glyph cell of `ascii.png`.
///
/// `FontRenderer` draws U+0020 without sampling the sheet, so that cell may be repainted:
/// `mc-render` makes this texel white when it uploads the font, which gives a fill exactly the
/// `(255, 255, 255, 255)` result of the 1×1 white fallback texture. Normalized coordinates keep
/// it inside cell 32 for any sheet size.
pub const WHITE_TEXEL_UV: [f32; 2] = [0.5 / 128.0, 16.5 / 128.0];

/// `Gui.drawRect`'s vertex alpha reaches the `GL_GREATER 0.1` alpha test, which discards
/// fragments at or below 25/255. A fill above that threshold may use the alpha-tested pipeline
/// without losing any fragment.
fn alpha_tested(alpha: u8) -> bool {
    alpha > 25
}

/// `GlStateManager.tryBlendFuncSeparate` modes the GUI uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// (SRC_ALPHA, ONE_MINUS_SRC_ALPHA)
    Alpha,
    /// (ONE_MINUS_DST_COLOR, ONE_MINUS_SRC_COLOR): the inverted crosshair.
    Invert,
    /// `RenderItem.renderEffect`: the enchantment glint over an item's opaque texels (the
    /// texture is the item's own, used as the `GL_EQUAL` depth mask).
    Glint,
}

#[derive(Clone, Copy, Debug)]
pub struct GuiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct DrawCmd {
    /// Resource path under `assets/minecraft/` (e.g. `textures/gui/widgets.png`), or `None` for flat color.
    pub texture: Option<String>,
    pub blend: Blend,
    /// `GL_GREATER 0.1` alpha test (on for textured GUI draws and for fills opaque enough that
    /// the test cannot discard them; off for faint `drawRect`/gradients).
    pub alpha_test: bool,
    /// Linear filtering (only the blurred panorama uses it).
    pub linear: bool,
    pub vertices: Vec<GuiVertex>,
}

/// A special pass the GPU side renders itself.
#[derive(Clone, Copy, Debug)]
pub enum Special {
    /// Title-screen panorama at the given time in ticks.
    Panorama { ticks: f32 },
}

#[derive(Clone, Debug)]
pub enum Item {
    Draw(DrawCmd),
    Special(Special),
}

#[derive(Default)]
pub struct GuiBatch {
    pub items: Vec<Item>,
    spare: Vec<DrawCmd>,
    /// `GlStateManager.color`, multiplied into textured draws.
    pub color: [f32; 4],
    pub blend: Blend,
    /// Scale then offset applied to every vertex (a `glScale`/`glTranslate` for overlays).
    pub transform: Option<(f32, [f32; 2])>,
}

impl Default for Blend {
    fn default() -> Self {
        Blend::Alpha
    }
}

pub fn argb(c: u32) -> [u8; 4] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8]
}

impl GuiBatch {
    pub fn new() -> Self {
        Self { items: Vec::new(), spare: Vec::new(), color: [1.0; 4], blend: Blend::Alpha, transform: None }
    }

    pub fn clear(&mut self) {
        for item in self.items.drain(..) {
            if let Item::Draw(mut draw) = item {
                draw.vertices.clear();
                if self.spare.len() < 256 { self.spare.push(draw); }
            }
        }
        self.color = [1.0; 4];
        self.blend = Blend::Alpha;
        self.transform = None;
    }

    pub fn special(&mut self, s: Special) {
        self.items.push(Item::Special(s));
    }

    fn tint(&self) -> [u8; 4] {
        self.color.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8)
    }

    fn quads(&mut self, texture: Option<&str>, alpha_test: bool, linear: bool, verts: &[GuiVertex]) {
        let transform = self.transform;
        let extend = |out: &mut Vec<GuiVertex>| {
            if let Some((s, o)) = transform {
                out.extend(verts.iter().map(|v| GuiVertex { pos: [v.pos[0] * s + o[0], v.pos[1] * s + o[1]], ..*v }));
            } else { out.extend_from_slice(verts); }
        };
        let blend = self.blend;
        if let Some(Item::Draw(last)) = self.items.last_mut() {
            if last.texture.as_deref() == texture && last.blend == blend && last.alpha_test == alpha_test && last.linear == linear {
                extend(&mut last.vertices);
                return;
            }
        }
        let mut draw = self.spare.pop().unwrap_or_else(|| DrawCmd { texture: None, blend, alpha_test, linear, vertices: Vec::new() });
        match (texture, &mut draw.texture) {
            (Some(path), Some(old)) => { old.clear(); old.push_str(path); }
            (path, old) => *old = path.map(str::to_string),
        }
        draw.blend = blend; draw.alpha_test = alpha_test; draw.linear = linear;
        extend(&mut draw.vertices);
        self.items.push(Item::Draw(draw));
    }

    /// One quad: corners in GL_QUADS order, any winding.
    pub fn quad(&mut self, texture: Option<&str>, alpha_test: bool, pos: [[f32; 2]; 4], uv: [[f32; 2]; 4], color: [[u8; 4]; 4]) {
        let v: [GuiVertex; 4] = std::array::from_fn(|i| GuiVertex { pos: pos[i], uv: uv[i], color: color[i] });
        self.quads(texture, alpha_test, false, &v);
    }

    /// `Gui.drawTexturedModalRect`: texel coordinates on a 256×256 sheet.
    pub fn textured_rect(&mut self, texture: &str, x: i32, y: i32, u: i32, v: i32, w: i32, h: i32) {
        const F: f32 = 0.003_906_25;
        self.textured_rect_uv(
            texture,
            x as f32,
            y as f32,
            w as f32,
            h as f32,
            [u as f32 * F, v as f32 * F, (u + w) as f32 * F, (v + h) as f32 * F],
        );
    }

    /// Textured quad with explicit normalized UVs `[u0, v0, u1, v1]`.
    pub fn textured_rect_uv(&mut self, texture: &str, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4]) {
        let c = self.tint();
        self.quad(
            Some(texture),
            true,
            [[x, y + h], [x + w, y + h], [x + w, y], [x, y]],
            [[uv[0], uv[3]], [uv[2], uv[3]], [uv[2], uv[1]], [uv[0], uv[1]]],
            [c; 4],
        );
    }

    /// Like [`Self::textured_rect_uv`] but with a per-call color and linear filtering.
    pub fn textured_rect_linear(&mut self, texture: &str, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], color: [u8; 4]) {
        let v = [
            GuiVertex { pos: [x, y + h], uv: [uv[0], uv[3]], color },
            GuiVertex { pos: [x + w, y + h], uv: [uv[2], uv[3]], color },
            GuiVertex { pos: [x + w, y], uv: [uv[2], uv[1]], color },
            GuiVertex { pos: [x, y], uv: [uv[0], uv[1]], color },
        ];
        self.quads(Some(texture), false, true, &v);
    }

    /// `Gui.drawRect`: ARGB fill, corners in either order.
    pub fn rect(&mut self, left: i32, top: i32, right: i32, bottom: i32, color: u32) {
        let (l, r) = (left.min(right) as f32, left.max(right) as f32);
        let (t, b) = (top.min(bottom) as f32, top.max(bottom) as f32);
        if l == r || t == b {
            return;
        }
        let c = argb(color);
        let saved = self.blend;
        self.blend = Blend::Alpha;
        if alpha_tested(c[3]) {
            self.quad(Some(ASCII_TEX), true, [[l, b], [r, b], [r, t], [l, t]], [WHITE_TEXEL_UV; 4], [c; 4]);
        } else {
            self.quad(None, false, [[l, b], [r, b], [r, t], [l, t]], [[0.0; 2]; 4], [c; 4]);
        }
        self.blend = saved;
    }

    /// Float version of `drawRect` used by the font renderer's underline/strikethrough.
    pub fn rect_f(&mut self, l: f32, t: f32, r: f32, b: f32, color: [u8; 4]) {
        if l == r || t == b {
            return;
        }
        if alpha_tested(color[3]) {
            self.quad(Some(ASCII_TEX), true, [[l, b], [r, b], [r, t], [l, t]], [WHITE_TEXEL_UV; 4], [color; 4]);
        } else {
            self.quad(None, false, [[l, b], [r, b], [r, t], [l, t]], [[0.0; 2]; 4], [color; 4]);
        }
    }

    /// `Gui.drawGradientRect`: vertical gradient, top color to bottom color.
    pub fn gradient(&mut self, left: i32, top: i32, right: i32, bottom: i32, start: u32, end: u32) {
        let (l, t, r, b) = (left as f32, top as f32, right as f32, bottom as f32);
        if l == r || t == b {
            return;
        }
        let (s, e) = (argb(start), argb(end));
        let saved = self.blend;
        self.blend = Blend::Alpha;
        if alpha_tested(s[3].min(e[3])) {
            self.quad(Some(ASCII_TEX), true, [[r, t], [l, t], [l, b], [r, b]], [WHITE_TEXEL_UV; 4], [s, s, e, e]);
        } else {
            self.quad(None, false, [[r, t], [l, t], [l, b], [r, b]], [[0.0; 2]; 4], [s, s, e, e]);
        }
        self.blend = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One glyph quad as `FontRenderer::render_default_char` emits it.
    fn glyph(b: &mut GuiBatch) {
        b.quad(
            Some(ASCII_TEX),
            true,
            [[0.0, 0.0], [0.0, 8.0], [8.0, 8.0], [8.0, 0.0]],
            [[0.0; 2]; 4],
            [[255; 4]; 4],
        );
    }

    fn commands(b: &GuiBatch) -> Vec<&DrawCmd> {
        b.items.iter().filter_map(|i| match i { Item::Draw(d) => Some(d), Item::Special(_) => None }).collect()
    }

    #[test]
    fn fill_and_text_share_one_command() {
        let mut b = GuiBatch::new();
        b.rect(0, 0, 10, 10, 0x80FF_FFFF);
        glyph(&mut b);
        b.rect(0, 10, 10, 20, 0x80FF_FFFF);
        glyph(&mut b);
        let c = commands(&b);
        assert_eq!(c.len(), 1, "fills and the text over them must batch together");
        assert_eq!(c[0].texture.as_deref(), Some(ASCII_TEX));
        assert!(c[0].alpha_test);
        assert_eq!(c[0].vertices.len(), 16);
        for v in &c[0].vertices[..4] {
            assert_eq!(v.uv, WHITE_TEXEL_UV, "fills must sample the white texel");
        }
    }

    #[test]
    fn fill_keeps_the_old_pipeline_when_alpha_test_would_discard() {
        // 25/255 = 0.098 <= 0.1: alpha test would drop the quad, so it stays untextured.
        let mut b = GuiBatch::new();
        b.rect(0, 0, 4, 4, 25 << 24);
        let c = commands(&b);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].texture, None);
        assert!(!c[0].alpha_test);
        assert_eq!(c[0].vertices[0].uv, [0.0; 2]);

        // 26/255 = 0.102 > 0.1: safe on the alpha-tested white texel.
        let mut b = GuiBatch::new();
        b.rect(0, 0, 4, 4, 26 << 24);
        let c = commands(&b);
        assert_eq!(c[0].texture.as_deref(), Some(ASCII_TEX));
        assert!(c[0].alpha_test);
    }

    #[test]
    fn faint_fill_splits_the_font_batch() {
        let mut b = GuiBatch::new();
        b.rect(0, 0, 10, 10, 0x80FF_FFFF);
        glyph(&mut b);
        b.rect(0, 10, 10, 20, 0x0A00_0000);
        b.rect(0, 20, 10, 30, 0x80FF_FFFF);
        let c = commands(&b);
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].texture, None);
        assert!(!c[1].alpha_test);
        assert_eq!(c[2].texture.as_deref(), Some(ASCII_TEX));
    }

    #[test]
    fn gradient_uses_the_font_batch_only_when_both_ends_pass() {
        let mut b = GuiBatch::new();
        b.gradient(0, 0, 20, 20, 0x80FF_FFFF, 0xC0FF_FFFF);
        glyph(&mut b);
        let c = commands(&b);
        assert_eq!(c.len(), 1);
        assert!(c[0].alpha_test);

        let mut b = GuiBatch::new();
        b.gradient(0, 0, 20, 20, 0x80FF_FFFF, 0x00FF_FFFF);
        let c = commands(&b);
        assert_eq!(c[0].texture, None);
        assert!(!c[0].alpha_test);
    }

    #[test]
    fn vertex_positions_and_colours_are_unchanged() {
        let mut old = GuiBatch::new();
        old.quad(None, false, [[3.0, 9.0], [11.0, 9.0], [11.0, 1.0], [3.0, 1.0]], [[0.0; 2]; 4], [[1, 2, 3, 200]; 4]);
        let mut new = GuiBatch::new();
        new.rect(3, 1, 11, 9, 0xC801_0203);
        let (o, n) = (commands(&old), commands(&new));
        for i in 0..4 {
            assert_eq!(o[0].vertices[i].pos, n[0].vertices[i].pos);
            assert_eq!(o[0].vertices[i].color, n[0].vertices[i].color);
        }
    }

    #[test]
    fn real_font_text_joins_fill_batch() {
        // Guards the shared `ASCII_TEX` path: the font's glyph quads must match the fills.
        let mut font = crate::font::FontRenderer::load(&mc_assets::AssetSource::new());
        let mut b = GuiBatch::new();
        b.rect(0, 0, 20, 20, 0x80FF_FFFF);
        font.draw_shadow(&mut b, "Hi", 2.0, 2.0, 0xFFFF_FFFF);
        b.rect(0, 20, 20, 40, 0x80FF_FFFF);
        assert_eq!(commands(&b).len(), 1);
    }

    #[test]
    fn order_and_transform_are_preserved() {
        let mut b = GuiBatch::new();
        b.transform = Some((2.0, [1.0, 3.0]));
        b.rect(0, 0, 10, 10, 0x80FF_FFFF);
        glyph(&mut b);
        let c = commands(&b);
        assert_eq!(c.len(), 1);
        // The fill is still first and still scaled/offset: [l, b] = (0, 10).
        assert_eq!(c[0].vertices[0].pos, [1.0, 23.0]);
        assert_eq!(c[0].vertices[4].pos, [1.0, 3.0]);
    }

    #[test]
    fn zero_area_fills_draw_nothing() {
        let mut b = GuiBatch::new();
        b.rect(5, 5, 5, 20, 0xFFFF_FFFF);
        b.rect(5, 5, 20, 5, 0xFFFF_FFFF);
        b.rect_f(1.0, 1.0, 1.0, 4.0, [255; 4]);
        b.gradient(2, 2, 2, 9, 0xFFFF_FFFF, 0xFFFF_FFFF);
        assert!(b.items.is_empty());
    }
}
