//! Immediate-mode 2D batch in GUI units, mirroring `Gui`'s drawing helpers.
//! The renderer merges consecutive commands that share texture and state.

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
    /// `GL_GREATER 0.1` alpha test (on for textured GUI draws, off for `drawRect`/gradients).
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
        Self { items: Vec::new(), color: [1.0; 4], blend: Blend::Alpha, transform: None }
    }

    pub fn clear(&mut self) {
        self.items.clear();
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
        let moved: Vec<GuiVertex>;
        let verts = match self.transform {
            Some((s, o)) => {
                moved = verts.iter().map(|v| GuiVertex { pos: [v.pos[0] * s + o[0], v.pos[1] * s + o[1]], ..*v }).collect();
                &moved[..]
            }
            None => verts,
        };
        let blend = self.blend;
        if let Some(Item::Draw(last)) = self.items.last_mut() {
            if last.texture.as_deref() == texture && last.blend == blend && last.alpha_test == alpha_test && last.linear == linear {
                last.vertices.extend_from_slice(verts);
                return;
            }
        }
        self.items.push(Item::Draw(DrawCmd {
            texture: texture.map(str::to_string),
            blend,
            alpha_test,
            linear,
            vertices: verts.to_vec(),
        }));
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
        let c = argb(color);
        let saved = self.blend;
        self.blend = Blend::Alpha;
        self.quad(None, false, [[l, b], [r, b], [r, t], [l, t]], [[0.0; 2]; 4], [c; 4]);
        self.blend = saved;
    }

    /// Float version of `drawRect` used by the font renderer's underline/strikethrough.
    pub fn rect_f(&mut self, l: f32, t: f32, r: f32, b: f32, color: [u8; 4]) {
        self.quad(None, false, [[l, b], [r, b], [r, t], [l, t]], [[0.0; 2]; 4], [color; 4]);
    }

    /// `Gui.drawGradientRect`: vertical gradient, top color to bottom color.
    pub fn gradient(&mut self, left: i32, top: i32, right: i32, bottom: i32, start: u32, end: u32) {
        let (l, t, r, b) = (left as f32, top as f32, right as f32, bottom as f32);
        let (s, e) = (argb(start), argb(end));
        let saved = self.blend;
        self.blend = Blend::Alpha;
        self.quad(None, false, [[r, t], [l, t], [l, b], [r, b]], [[0.0; 2]; 4], [s, s, e, e]);
        self.blend = saved;
    }
}
