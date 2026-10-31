//! Drawing leaves. Real GUI code ends in `FontRenderer` and `Tessellator`/`WorldRenderer`; they land here as
//! `Draw` commands in screen units, for the client's UI layer to replay (`take_draw`).
use crate::stubs::{register, Ret};
use jni::objects::GlobalRef;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Vertex {
    pub pos: [f64; 3],
    pub uv: [f64; 2],
    pub rgba: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    Text {
        text: String,
        x: f32,
        y: f32,
        /// GUI units per font unit after the model-view (and, in the world pass, projection) transform.
        scale: f32,
        color: i32,
        shadow: bool,
    },
    /// `GL_LINES`/`LINE_STRIP`/`LINE_LOOP` as independent segments (vertex pairs), `width` in framebuffer pixels
    /// (`glLineWidth`).
    Lines { width: f32, verts: Vec<Vertex> },
    /// One `Tessellator.draw()`: GL mode (7 = quads), its vertices, and the bound texture path
    /// (`textures/gui/widgets.png` for minecraft, `modid:path` for other domains).
    Vertices {
        mode: i32,
        texture: Option<String>,
        verts: Vec<Vertex>,
        /// `GL_BLEND` factors `(src, dst)` in force when the batch was flushed; `None` while blending is off.
        blend: Option<(i32, i32)>,
    },
    /// A quad textured with a framebuffer copy (`glCopyTexSubImage2D` into a mod texture): vanilla
    /// `GuiMainMenu.renderSkybox`'s blurred panorama, which mods' title screens copy. The client draws its own.
    Backdrop,
    /// `RenderItem.renderItemIntoGUI`/`renderItemAndEffectIntoGUI`: a 16x16 item icon with its top-left corner at
    /// (x, y) GUI units after the model-view transform, `scale` GUI units per icon unit.
    Item { id: u16, meta: u16, x: f32, y: f32, scale: f32, glint: bool },
}

#[derive(Default)]
struct Builder {
    mode: i32,
    verts: Vec<Vertex>,
    cur: Vertex,
    offset: [f64; 3],
}

static SINK: Mutex<Vec<Draw>> = Mutex::new(Vec::new());
static BUILDER: Mutex<Option<Builder>> = Mutex::new(None);

const IDENTITY: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];
const MODELVIEW: i32 = 0x1700;
const PROJECTION: i32 = 0x1701;
const TEXTURE: i32 = 0x1702;
const INVALID_ENUM: i32 = 0x0500;
const INVALID_VALUE: i32 = 0x0501;
const STACK_OVERFLOW: i32 = 0x0503;
const STACK_UNDERFLOW: i32 = 0x0504;

/// OpenGL post-multiplies column-major matrices (`T * S * vertex`, not `S * T * vertex`).
fn multiply(a: [f64; 16], b: [f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| (0..4).map(|k| a[k * 4 + i % 4] * b[i / 4 * 4 + k]).sum())
}

fn transform(m: [f64; 16], p: [f64; 3], w: f64) -> [f64; 3] {
    std::array::from_fn(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r] * w)
}

struct MatrixStack {
    current: [f64; 16],
    saved: Vec<[f64; 16]>,
}
impl MatrixStack {
    const fn new() -> Self {
        Self {
            current: IDENTITY,
            saved: Vec::new(),
        }
    }
}

/// GUI geometry is emitted in GUI units after the model-view transform. Projection is
/// retained separately; it is not a replacement for a world-space/native OpenGL backend.
struct Gl {
    texture: Option<String>,
    texture_2d: bool,
    color: [f32; 4],
    modelview: MatrixStack,
    projection: MatrixStack,
    texture_matrix: MatrixStack,
    matrix_mode: i32,
    error: i32,
    immediate: bool,
    line_width: f32,
    world: Option<World>,
}

/// A `RenderWorldLastEvent` pass: geometry goes through the camera's projection to GUI units.
struct World {
    gui: [f64; 2],
    saved: ([f64; 16], [f64; 16]),
}

/// Vertices this close to the eye plane (clip w) are behind the camera for drawing purposes.
const NEAR_W: f64 = 0.05;

fn clip4(m: [f64; 16], p: [f64; 3]) -> [f64; 4] {
    std::array::from_fn(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

/// Eye space to GUI units (y down), keeping NDC depth in z; `None` behind the near limit.
fn project(projection: [f64; 16], gui: [f64; 2], mut v: Vertex) -> Option<Vertex> {
    let c = clip4(projection, v.pos);
    if c[3] <= NEAR_W {
        return None;
    }
    v.pos = [(c[0] / c[3] * 0.5 + 0.5) * gui[0], (0.5 - c[1] / c[3] * 0.5) * gui[1], c[2] / c[3]];
    Some(v)
}

/// Clips a segment against the near limit in eye space, then projects both ends.
fn project_segment(projection: [f64; 16], gui: [f64; 2], a: Vertex, b: Vertex) -> Option<(Vertex, Vertex)> {
    let (wa, wb) = (clip4(projection, a.pos)[3], clip4(projection, b.pos)[3]);
    if wa <= NEAR_W && wb <= NEAR_W {
        return None;
    }
    let cut = |inside: Vertex, outside: Vertex, wi: f64, wo: f64| {
        let t = (wi - NEAR_W * 1.0001) / (wi - wo);
        let mut v = inside;
        v.pos = std::array::from_fn(|i| inside.pos[i] + (outside.pos[i] - inside.pos[i]) * t);
        v.rgba = std::array::from_fn(|i| inside.rgba[i] + (outside.rgba[i] - inside.rgba[i]) * t as f32);
        v
    };
    let (a, b) = if wa <= NEAR_W {
        (cut(b, a, wb, wa), b)
    } else if wb <= NEAR_W {
        (a, cut(a, b, wa, wb))
    } else {
        (a, b)
    };
    Some((project(projection, gui, a)?, project(projection, gui, b)?))
}

fn segments(mode: i32, v: &[Vertex]) -> Vec<(Vertex, Vertex)> {
    match mode {
        1 => v.chunks_exact(2).map(|p| (p[0], p[1])).collect(),
        3 => v.windows(2).map(|p| (p[0], p[1])).collect(),
        2 if v.len() >= 2 => v.windows(2).map(|p| (p[0], p[1])).chain([(v[v.len() - 1], v[0])]).collect(),
        _ => Vec::new(),
    }
}

/// Starts a `RenderWorldLastEvent` pass: `projection` and `modelview` are the camera's, column-major, with the
/// model-view placing the world relative to `RenderManager.renderPos` like vanilla `orientCamera`.
pub fn begin_world(projection: [f64; 16], modelview: [f64; 16], gui: (f64, f64)) {
    let mut gl = GL.lock().unwrap();
    let saved = (gl.projection.current, gl.modelview.current);
    gl.projection.current = projection;
    gl.modelview.current = modelview;
    gl.world = Some(World { gui: [gui.0, gui.1], saved });
}

/// Ends the world pass and restores the GUI matrices.
pub fn end_world() {
    let mut gl = GL.lock().unwrap();
    if let Some(w) = gl.world.take() {
        gl.projection.current = w.saved.0;
        gl.modelview.current = w.saved.1;
    }
}

impl Gl {
    const fn new() -> Self {
        Self {
            texture: None,
            texture_2d: true,
            color: [1.; 4],
            modelview: MatrixStack::new(),
            projection: MatrixStack::new(),
            texture_matrix: MatrixStack::new(),
            matrix_mode: MODELVIEW,
            error: 0,
            immediate: false,
            line_width: 1.,
            world: None,
        }
    }
    fn error(&mut self, error: i32) {
        if self.error == 0 {
            self.error = error;
        }
    }
    fn stack(&mut self) -> &mut MatrixStack {
        match self.matrix_mode {
            PROJECTION => &mut self.projection,
            TEXTURE => &mut self.texture_matrix,
            _ => &mut self.modelview,
        }
    }
    fn mode(&mut self, mode: i32) {
        if matches!(mode, MODELVIEW | PROJECTION | TEXTURE) {
            self.matrix_mode = mode;
        } else {
            self.error(INVALID_ENUM);
        }
    }
    fn push(&mut self) {
        if self.stack().saved.len() >= 127 {
            self.error(STACK_OVERFLOW);
            return;
        }
        let stack = self.stack();
        stack.saved.push(stack.current);
    }
    fn pop(&mut self) {
        if let Some(m) = self.stack().saved.pop() {
            self.stack().current = m;
        } else {
            self.error(STACK_UNDERFLOW);
        }
    }
    fn mult(&mut self, m: [f64; 16]) {
        let next = multiply(self.stack().current, m);
        if next.iter().all(|v| v.is_finite()) {
            self.stack().current = next;
        } else {
            self.error(INVALID_VALUE);
        }
    }
    fn translate(&mut self, p: [f64; 3]) {
        let mut m = IDENTITY;
        m[12..15].copy_from_slice(&p);
        self.mult(m);
    }
    fn scale(&mut self, p: [f64; 3]) {
        let mut m = IDENTITY;
        for i in 0..3 {
            m[i * 5] = p[i];
        }
        self.mult(m);
    }
    fn rotate(&mut self, angle: f64, axis: [f64; 3]) {
        if !angle.is_finite() || !axis.iter().all(|x| x.is_finite()) {
            self.error(INVALID_VALUE);
            return;
        }
        let length = axis[0].hypot(axis[1]).hypot(axis[2]);
        if length == 0. {
            return;
        }
        if !length.is_finite() {
            self.error(INVALID_VALUE);
            return;
        }
        let [x, y, z] = axis.map(|v| v / length);
        let (s, c) = angle.to_radians().sin_cos();
        let t = 1. - c;
        self.mult([
            x * x * t + c,
            y * x * t + z * s,
            z * x * t - y * s,
            0.,
            x * y * t - z * s,
            y * y * t + c,
            z * y * t + x * s,
            0.,
            x * z * t + y * s,
            y * z * t - x * s,
            z * z * t + c,
            0.,
            0.,
            0.,
            0.,
            1.,
        ]);
    }
    fn vertices(&mut self, vertices: &mut [Vertex]) -> bool {
        for v in vertices {
            v.pos = transform(self.modelview.current, v.pos, 1.);
            let uv = transform(self.texture_matrix.current, [v.uv[0], v.uv[1], 0.], 1.);
            v.uv = [uv[0], uv[1]];
            if !v.pos.iter().chain(v.uv.iter()).all(|x| x.is_finite()) {
                self.error(INVALID_VALUE);
                return false;
            }
        }
        true
    }
}
static GL: Mutex<Gl> = Mutex::new(Gl::new());
static UNSUPPORTED_CAPS: Mutex<Vec<i32>> = Mutex::new(Vec::new());
static TEXT_WIDTH: Mutex<Option<Box<dyn Fn(&str) -> i32 + Send>>> = Mutex::new(None);

/// Draw commands recorded since the last call.
/// The texture later draws sample (`None`: untextured).
pub(crate) fn bind_texture(texture: Option<String>) { GL.lock().unwrap().texture = texture; }

pub fn take_draw() -> Vec<Draw> {
    std::mem::take(&mut *SINK.lock().unwrap())
}

/// Plugs in the client's real font metrics; the default is a flat 6 px per character.
pub fn set_text_width(f: impl Fn(&str) -> i32 + Send + 'static) {
    *TEXT_WIDTH.lock().unwrap() = Some(Box::new(f));
}

/// Width of `s` including its `§` codes: the client font knows `§l` bold adds a pixel per character. Without a hook
/// (headless tests) it is a flat 6 px per visible character.
fn width(s: &str) -> i32 {
    if let Some(f) = TEXT_WIDTH.lock().unwrap().as_ref() {
        return f(s);
    }
    let (mut n, mut chars) = (0, s.chars());
    while let Some(ch) = chars.next() {
        if ch == '§' {
            chars.next();
        } else {
            n += 6;
        }
    }
    n
}

/// `FontRenderer.trimStringToWidth`: the longest prefix (or suffix when `reverse`) that fits `max`.
fn trim(s: &str, max: i32, reverse: bool) -> String {
    let chars: Vec<char> = s.chars().collect();
    // Measure whole prefixes (or suffixes) so formatting codes keep their effect on the characters after them.
    let piece = |n: usize| -> String {
        if reverse {
            chars[chars.len() - n..].iter().collect()
        } else {
            chars[..n].iter().collect()
        }
    };
    let n = (0..=chars.len())
        .take_while(|&n| width(&piece(n)) <= max)
        .last()
        .unwrap_or(0);
    piece(n)
}

fn builder<R>(f: impl FnOnce(&mut Builder) -> R) -> R {
    f(BUILDER.lock().unwrap().get_or_insert_with(Default::default))
}

pub fn register_all() {
    const FR: &str = "net/minecraft/client/gui/FontRenderer.";
    register(&format!("{FR}func_78256_a"), |c| {
        let s = c.string(0)?.unwrap_or_default();
        Ok(width(&s).into())
    });
    register(&format!("{FR}func_78263_a"), |c| {
        let ch = c.char(0)?;
        Ok(width(&ch.to_string()).into())
    });
    register(&format!("{FR}func_78269_a"), |c| {
        let (s, w) = (c.string(0)?.unwrap_or_default(), c.i32(1)?);
        Ok(trim(&s, w, false).into())
    });
    register(&format!("{FR}func_78262_a"), |c| {
        let (s, w, rev) = (c.string(0)?.unwrap_or_default(), c.i32(1)?, c.bool(2)?);
        Ok(trim(&s, w, rev).into())
    });
    register(&format!("{FR}func_78276_b"), |c| {
        let (text, x, y, color) = (
            c.string(0)?.unwrap_or_default(),
            c.i32(1)? as f32,
            c.i32(2)? as f32,
            c.i32(3)?,
        );
        draw_text(text, x, y, color, false)
    });
    register(&format!("{FR}func_175063_a"), |c| {
        let (text, x, y, color) = (
            c.string(0)?.unwrap_or_default(),
            c.f32(1)?,
            c.f32(2)?,
            c.i32(3)?,
        );
        draw_text(text, x, y, color, true)
    });
    register(&format!("{FR}func_175065_a"), |c| {
        let (text, x, y, color, shadow) = (
            c.string(0)?.unwrap_or_default(),
            c.f32(1)?,
            c.f32(2)?,
            c.i32(3)?,
            c.bool(4)?,
        );
        draw_text(text, x, y, color, shadow)
    });

    register_matrices();
    register_immediate();
    register_state();
    // Preserve the domain even though the native renderer may not have loaded that mod texture yet.
    register(
        "net/minecraft/client/renderer/texture/TextureManager.func_110577_a",
        |c| {
            let loc = c.obj(0)?;
            let texture = if loc.is_null() {
                None
            } else {
                let domain: String = {
                    let d = c
                        .env
                        .get_field(&loc, "field_110626_a", "Ljava/lang/String;")?
                        .l()?;
                    c.env.get_string(&d.into())?.into()
                };
                let path: String = {
                    let p = c
                        .env
                        .get_field(&loc, "field_110625_b", "Ljava/lang/String;")?
                        .l()?;
                    c.env.get_string(&p.into())?.into()
                };
                Some(super::texture::resolve(if domain == "minecraft" {
                    path
                } else {
                    format!("{domain}:{path}")
                }))
            };
            GL.lock().unwrap().texture = texture;
            Ok(Ret::Null)
        },
    );
    const GSM: &str = "net/minecraft/client/renderer/GlStateManager.";
    register(&format!("{GSM}func_179131_c"), |c| {
        let v = [c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?];
        GL.lock().unwrap().color = v;
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179124_c"), |c| {
        let v = [c.f32(0)?, c.f32(1)?, c.f32(2)?, 1.0];
        GL.lock().unwrap().color = v;
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179090_x"), |_| {
        GL.lock().unwrap().texture_2d = false;
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179098_w"), |_| {
        GL.lock().unwrap().texture_2d = true;
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179117_G"), |_| {
        GL.lock().unwrap().color = [1.; 4];
        Ok(Ret::Null)
    });
    // UI click sounds: there is no audio path for mod UIs yet.
    register(
        "net/minecraft/client/audio/SoundHandler.func_147682_a",
        |_| Ok(Ret::Null),
    );
    register(
        "net/minecraft/client/audio/PositionedSoundRecord.func_147674_a",
        |_| Ok(Ret::Null),
    );
    register("org/lwjgl/input/Keyboard.enableRepeatEvents", |_| {
        Ok(Ret::Null)
    });

    const TESS: &str = "net/minecraft/client/renderer/Tessellator.";
    register(&format!("{TESS}func_178181_a"), |c| {
        single(c, &TESSELLATOR, "net/minecraft/client/renderer/Tessellator")
    });
    register(&format!("{TESS}func_178180_c"), |c| {
        single(c, &RENDERER, "net/minecraft/client/renderer/WorldRenderer")
    });
    register(&format!("{TESS}func_78381_a"), |_| {
        let (mode, verts) = builder(|b| (b.mode, std::mem::take(&mut b.verts)));
        emit(mode, verts);
        Ok(Ret::Null)
    });

    // WorldRenderer's fluent setters return `this`.
    const WR: &str = "net/minecraft/client/renderer/WorldRenderer.";
    // A new batch starts from the current GL colour, as vertex formats without a colour element do in GL.
    register(&format!("{WR}func_181668_a"), |c| {
        let (mode, color) = (c.i32(0)?, GL.lock().unwrap().color);
        builder(|b| {
            b.mode = mode;
            b.verts.clear();
            b.cur.rgba = color;
        });
        Ok(Ret::Null)
    });
    register(&format!("{WR}func_181662_b"), |c| {
        let p = [c.f64(0)?, c.f64(1)?, c.f64(2)?];
        builder(|b| b.cur.pos = std::array::from_fn(|i| p[i] + b.offset[i]));
        this(c)
    });
    register(&format!("{WR}func_178969_c"), |c| {
        let p = [c.f64(0)?, c.f64(1)?, c.f64(2)?];
        builder(|b| b.offset = p);
        Ok(Ret::Null)
    });
    register(&format!("{WR}func_181673_a"), |c| {
        let uv = [c.f64(0)?, c.f64(1)?];
        builder(|b| b.cur.uv = uv);
        this(c)
    });
    register(&format!("{WR}func_181666_a"), |c| {
        let v = [c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?];
        builder(|b| b.cur.rgba = v);
        this(c)
    });
    register(&format!("{WR}func_181669_b"), |c| {
        let v = [
            c.i32(0)? as f32 / 255.0,
            c.i32(1)? as f32 / 255.0,
            c.i32(2)? as f32 / 255.0,
            c.i32(3)? as f32 / 255.0,
        ];
        builder(|b| b.cur.rgba = v);
        this(c)
    });
    register(&format!("{WR}func_181663_c"), |c| this(c));
    register(&format!("{WR}func_181675_d"), |_| {
        let vertex = builder(|b| b.cur);
        append_vertex(vertex);
        Ok(Ret::Null)
    });
}

static TESSELLATOR: OnceLock<GlobalRef> = OnceLock::new();
static RENDERER: OnceLock<GlobalRef> = OnceLock::new();

fn single(
    c: &mut crate::stubs::Call<'_, '_>,
    cell: &'static OnceLock<GlobalRef>,
    class: &str,
) -> jni::errors::Result<Ret> {
    let obj = super::singleton(c.env, cell, class)?;
    Ok(Ret::Obj(c.env.new_global_ref(obj)?))
}

fn this(c: &mut crate::stubs::Call<'_, '_>) -> jni::errors::Result<Ret> {
    Ok(Ret::Obj(c.env.new_global_ref(&c.this)?))
}

fn draw_text(text: String, x: f32, y: f32, color: i32, shadow: bool) -> jni::errors::Result<Ret> {
    // Draw::Text only carries an anchor. Glyph scale/rotation still needs a richer draw command.
    let end = (x as i32).saturating_add(width(&text));
    let mut gl = GL.lock().unwrap();
    let m = gl.modelview.current;
    let (p, unit) = (transform(m, [x as f64, y as f64, 0.], 1.), transform(m, [x as f64 + 1., y as f64, 0.], 1.));
    let anchor = match &gl.world {
        // Glyphs stay screen-aligned: the scale is how far one font unit along x lands on screen.
        Some(w) => match (
            project(gl.projection.current, w.gui, Vertex { pos: p, ..Default::default() }),
            project(gl.projection.current, w.gui, Vertex { pos: unit, ..Default::default() }),
        ) {
            (Some(a), Some(b)) => Some((a.pos[0], a.pos[1], (b.pos[0] - a.pos[0]).hypot(b.pos[1] - a.pos[1]))),
            _ => None,
        },
        None => Some((p[0], p[1], (unit[0] - p[0]).hypot(unit[1] - p[1]))),
    };
    let Some((x, y, scale)) = anchor else { return Ok(end.into()) };
    if !x.is_finite() || !y.is_finite() || !scale.is_finite() {
        gl.error(INVALID_VALUE);
        return Ok(end.into());
    }
    drop(gl);
    out(Draw::Text {
        text,
        x: x as f32,
        y: y as f32,
        scale: scale as f32,
        color,
        shadow,
    });
    Ok(end.into())
}

fn register_matrices() {
    const GSM: &str = "net/minecraft/client/renderer/GlStateManager.";
    const GL11: &str = "org/lwjgl/opengl/GL11.";
    for key in [format!("{GSM}func_179094_E"), format!("{GL11}glPushMatrix")] {
        register(&key, |_| {
            GL.lock().unwrap().push();
            Ok(Ret::Null)
        });
    }
    for key in [format!("{GSM}func_179121_F"), format!("{GL11}glPopMatrix")] {
        register(&key, |_| {
            GL.lock().unwrap().pop();
            Ok(Ret::Null)
        });
    }
    for key in [
        format!("{GSM}func_179096_D"),
        format!("{GL11}glLoadIdentity"),
    ] {
        register(&key, |_| {
            GL.lock().unwrap().stack().current = IDENTITY;
            Ok(Ret::Null)
        });
    }
    for key in [format!("{GSM}func_179128_n"), format!("{GL11}glMatrixMode")] {
        register(&key, |c| {
            let mode = c.i32(0)?;
            GL.lock().unwrap().mode(mode);
            Ok(Ret::Null)
        });
    }
    for key in [
        format!("{GSM}func_179109_b"),
        format!("{GSM}func_179137_b"),
        format!("{GL11}glTranslatef"),
        format!("{GL11}glTranslated"),
    ] {
        register(&key, |c| {
            let p = [c.f64(0)?, c.f64(1)?, c.f64(2)?];
            GL.lock().unwrap().translate(p);
            Ok(Ret::Null)
        });
    }
    for key in [
        format!("{GSM}func_179152_a"),
        format!("{GSM}func_179139_a"),
        format!("{GL11}glScalef"),
        format!("{GL11}glScaled"),
    ] {
        register(&key, |c| {
            let p = [c.f64(0)?, c.f64(1)?, c.f64(2)?];
            GL.lock().unwrap().scale(p);
            Ok(Ret::Null)
        });
    }
    for key in [
        format!("{GSM}func_179114_b"),
        format!("{GL11}glRotatef"),
        format!("{GL11}glRotated"),
    ] {
        register(&key, |c| {
            let (a, p) = (c.f64(0)?, [c.f64(1)?, c.f64(2)?, c.f64(3)?]);
            GL.lock().unwrap().rotate(a, p);
            Ok(Ret::Null)
        });
    }
    for key in [format!("{GSM}func_179130_a"), format!("{GL11}glOrtho")] {
        register(&key, |c| {
            let (l, r, b, t, n, f) = (
                c.f64(0)?,
                c.f64(1)?,
                c.f64(2)?,
                c.f64(3)?,
                c.f64(4)?,
                c.f64(5)?,
            );
            let mut gl = GL.lock().unwrap();
            if l == r || b == t || n == f {
                gl.error(INVALID_VALUE);
            } else {
                gl.mult([
                    2. / (r - l),
                    0.,
                    0.,
                    0.,
                    0.,
                    2. / (t - b),
                    0.,
                    0.,
                    0.,
                    0.,
                    -2. / (f - n),
                    0.,
                    -(r + l) / (r - l),
                    -(t + b) / (t - b),
                    -(f + n) / (f - n),
                    1.,
                ]);
            }
            Ok(Ret::Null)
        });
    }
    register(&format!("{GL11}glLineWidth"), |c| {
        let w = c.f32(0)?;
        let mut gl = GL.lock().unwrap();
        if w > 0. && w.is_finite() {
            gl.line_width = w;
        } else {
            gl.error(INVALID_VALUE);
        }
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glGetError"), |_| {
        let mut gl = GL.lock().unwrap();
        let error = std::mem::take(&mut gl.error);
        Ok(error.into())
    });
}

const MAX_VERTICES: usize = 262_144;

fn append_vertex(vertex: Vertex) {
    if !vertex
        .pos
        .iter()
        .chain(vertex.uv.iter())
        .all(|v| v.is_finite())
    {
        GL.lock().unwrap().error(INVALID_VALUE);
        return;
    }
    let full = builder(|b| {
        if b.verts.len() >= MAX_VERTICES {
            true
        } else {
            b.verts.push(vertex);
            false
        }
    });
    if full {
        GL.lock().unwrap().error(INVALID_VALUE);
    }
}

fn register_immediate() {
    const GL11: &str = "org/lwjgl/opengl/GL11.";
    register(&format!("{GL11}glBegin"), |c| {
        let mode = c.i32(0)?;
        let color = {
            let mut gl = GL.lock().unwrap();
            if gl.immediate {
                gl.error(0x0502);
                return Ok(Ret::Null);
            }
            gl.immediate = true;
            gl.color
        };
        builder(|b| {
            b.mode = mode;
            b.verts.clear();
            b.cur = Vertex {
                rgba: color,
                ..Default::default()
            };
        });
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glEnd"), |_| {
        {
            let mut gl = GL.lock().unwrap();
            if !gl.immediate {
                gl.error(0x0502);
                return Ok(Ret::Null);
            }
            gl.immediate = false;
        }
        let (mode, vertices) = builder(|b| (b.mode, std::mem::take(&mut b.verts)));
        emit(mode, vertices);
        Ok(Ret::Null)
    });
    for (name, count) in [
        ("glVertex2f", 2),
        ("glVertex2d", 2),
        ("glVertex3f", 3),
        ("glVertex3d", 3),
    ] {
        register(&format!("{GL11}{name}"), move |c| {
            let p = [
                c.f64(0)?,
                c.f64(1)?,
                if count == 3 { c.f64(2)? } else { 0. },
            ];
            let color = {
                let mut gl = GL.lock().unwrap();
                if !gl.immediate {
                    gl.error(0x0502);
                    return Ok(Ret::Null);
                }
                gl.color
            };
            let vertex = builder(|b| {
                b.cur.pos = p;
                b.cur.rgba = color;
                b.cur
            });
            append_vertex(vertex);
            Ok(Ret::Null)
        });
    }
    for name in ["glTexCoord2f", "glTexCoord2d"] {
        register(&format!("{GL11}{name}"), |c| {
            let uv = [c.f64(0)?, c.f64(1)?];
            builder(|b| b.cur.uv = uv);
            Ok(Ret::Null)
        });
    }
    for (name, count) in [
        ("glColor3f", 3),
        ("glColor3d", 3),
        ("glColor4f", 4),
        ("glColor4d", 4),
    ] {
        register(&format!("{GL11}{name}"), move |c| {
            let rgba = [
                c.f64(0)? as f32,
                c.f64(1)? as f32,
                c.f64(2)? as f32,
                if count == 4 { c.f64(3)? as f32 } else { 1. },
            ];
            let mut gl = GL.lock().unwrap();
            if rgba.iter().all(|v| v.is_finite()) {
                gl.color = rgba.map(|v| v.clamp(0., 1.));
            } else {
                gl.error(INVALID_VALUE);
            }
            Ok(Ret::Null)
        });
    }
    // glGetFloat(GL_MODELVIEW/PROJECTION/TEXTURE_MATRIX, buf): column-major from the current position, which LWJGL
    // leaves where it was (absolute puts). Other parameters keep the buffer unchanged, as before.
    register(&format!("{GL11}glGetFloat"), |c| {
        let pname = c.i32(0)?;
        let buf = c.obj(1)?;
        let m = {
            let gl = GL.lock().unwrap();
            match pname { 0x0ba6 => gl.modelview.current, 0x0ba7 => gl.projection.current, 0x0ba8 => gl.texture_matrix.current, _ => return Ok(Ret::Null) }
        };
        let at = c.env.call_method(&buf, "position", "()I", &[])?.i()?;
        for (i, v) in m.iter().enumerate() {
            c.env.call_method(&buf, "put", "(IF)Ljava/nio/FloatBuffer;", &[jni::objects::JValue::Int(at + i as i32), jni::objects::JValue::Float(*v as f32)])?;
        }
        Ok(Ret::Null)
    });
    // glLoadMatrix(buf): replaces the selected stack's top with 16 column-major floats from the position.
    register(&format!("{GL11}glLoadMatrix"), |c| {
        let buf = c.obj(0)?;
        let at = c.env.call_method(&buf, "position", "()I", &[])?.i()?;
        let mut m = [0f64; 16];
        for (i, v) in m.iter_mut().enumerate() {
            *v = c.env.call_method(&buf, "get", "(I)F", &[jni::objects::JValue::Int(at + i as i32)])?.f()? as f64;
        }
        GL.lock().unwrap().stack().current = m;
        Ok(Ret::Null)
    });
    for (name, enabled) in [("glEnable", true), ("glDisable", false)] {
        register(&format!("{GL11}{name}"), move |c| {
            let flag = c.i32(0)?;
            let mut gl = GL.lock().unwrap();
            if flag == 0x0de1 {
                gl.texture_2d = enabled;
            }
            // These are valid OpenGL capabilities, but the existing Draw format cannot
            // reproduce their state. Report that once without fabricating GL_INVALID_ENUM.
            else if matches!(
                flag,
                0x0be2
                    | 0x0b71
                    | 0x0bc0
                    | 0x0c11
                    | 0x0b44
                    | 0x0b50
                    | 0x0b60
                    | 0x0b20
                    | 0x0b10
                    | 0x8037
            ) {
                drop(gl);
                let mut seen = UNSUPPORTED_CAPS.lock().unwrap();
                if !seen.contains(&flag) {
                    seen.push(flag);
                    eprintln!(
                        "[forge] GL capability 0x{flag:x} is not represented by GUI draw replay"
                    );
                }
            } else {
                gl.error(INVALID_ENUM);
            }
            Ok(Ret::Null)
        });
    }
}

/// Normalize filled legacy primitives to the quad-only GUI command format. Each triangle
/// becomes one quad with a repeated last vertex: the replay's second triangle is degenerate.
fn filled_quads(mode: i32, vertices: &[Vertex]) -> Option<Vec<Vertex>> {
    let mut out = Vec::new();
    let mut tri = |a: usize, b: usize, c: usize| {
        out.extend([vertices[a], vertices[b], vertices[c], vertices[c]])
    };
    match mode {
        7 => return Some(vertices.chunks_exact(4).flatten().copied().collect()),
        4 => {
            for i in (0..vertices.len().saturating_sub(2)).step_by(3) {
                tri(i, i + 1, i + 2);
            }
        }
        5 => {
            for i in 0..vertices.len().saturating_sub(2) {
                if i % 2 == 0 {
                    tri(i, i + 1, i + 2);
                } else {
                    tri(i + 1, i, i + 2);
                }
            }
        }
        6 | 9 => {
            for i in 1..vertices.len().saturating_sub(1) {
                tri(0, i, i + 1);
            }
        }
        8 => {
            drop(tri);
            for i in (0..vertices.len().saturating_sub(3)).step_by(2) {
                out.extend([
                    vertices[i],
                    vertices[i + 1],
                    vertices[i + 3],
                    vertices[i + 2],
                ]);
            }
        }
        _ => return None,
    }
    Some(out)
}

fn emit(mode: i32, mut vertices: Vec<Vertex>) {
    let (texture, blend, world, line_width) = {
        let mut gl = GL.lock().unwrap();
        if !gl.vertices(&mut vertices) {
            return;
        }
        let world = gl.world.as_ref().map(|w| (gl.projection.current, w.gui));
        let blend = STATE.lock().unwrap().s.blend_pair();
        (if gl.texture_2d { gl.texture.clone() } else { None }, blend, world, gl.line_width)
    };
    if matches!(mode, 1..=3) {
        let segs = segments(mode, &vertices);
        let verts: Vec<Vertex> = match world {
            Some((p, gui)) => segs.into_iter().filter_map(|(a, b)| project_segment(p, gui, a, b)).flat_map(|(a, b)| [a, b]).collect(),
            None => segs.into_iter().flat_map(|(a, b)| [a, b]).collect(),
        };
        if !verts.is_empty() {
            out(Draw::Lines { width: line_width, verts });
        }
        return;
    }
    if let Some(mut verts) = filled_quads(mode, &vertices) {
        if let Some((p, gui)) = world {
            // A quad crossing the eye plane is dropped whole; filled boxes mods draw sit in front of the camera.
            verts = verts
                .chunks_exact(4)
                .filter_map(|q| q.iter().map(|v| project(p, gui, *v)).collect::<Option<Vec<_>>>())
                .flatten()
                .collect();
            if verts.is_empty() {
                return;
            }
        }
        out(Draw::Vertices {
            mode: 7,
            texture,
            verts,
            blend,
        });
    } else if world.is_none() {
        // Keep the mode visible to callers, rather than presenting unsupported points as quads.
        out(Draw::Vertices {
            mode,
            texture,
            verts: vertices,
            blend,
        });
    }
}

// ---- fixed-function state surface ------------------------------------------------------------
// `GlStateManager` (1.8.9) is a stub, so its caches live here: mods toggle and query the state, and the
// sink's draw commands carry what it can honour.

const UNITS: i32 = 8;
const TEXTURE0: i32 = 33984;
const CAP_TEXTURE_2D: i32 = 0x0de1;
const GL_INVALID_ENUM: i32 = 0x0500;
const GL_INVALID_OPERATION: i32 = 0x0502;

/// `GlStateManager`'s `*State` fields, with vanilla's defaults.
#[derive(Clone, Copy, PartialEq)]
struct States {
    alpha: bool,
    alpha_func: i32,
    alpha_ref: f32,
    blend: bool,
    blend_src: i32,
    blend_dst: i32,
    blend_src_a: i32,
    blend_dst_a: i32,
    depth: bool,
    depth_func: i32,
    depth_mask: bool,
    cull: bool,
    cull_face: i32,
    fog: bool,
    fog_mode: i32,
    fog_density: f32,
    fog_start: f32,
    fog_end: f32,
    lighting: bool,
    color_material: bool,
    normalize: bool,
    rescale_normal: bool,
    offset_fill: bool,
    offset_line: bool,
    offset_factor: f32,
    offset_units: f32,
    shade_model: i32,
    color_mask: [bool; 4],
    clear_color: [f32; 4],
    clear_depth: f64,
    clear_mask: i32,
    viewport: [i32; 4],
    scissor: [i32; 4],
    scissor_test: bool,
    line_smooth: bool,
    active: i32,
    texture_2d: [bool; 8],
    texture_name: [i32; 8],
    lights: [bool; 8],
}

impl States {
    const fn new() -> Self {
        Self {
            alpha: false, alpha_func: 519, alpha_ref: -1.,
            blend: false, blend_src: 1, blend_dst: 0, blend_src_a: 1, blend_dst_a: 0,
            depth: false, depth_func: 513, depth_mask: true,
            cull: false, cull_face: 1029,
            fog: false, fog_mode: 2048, fog_density: 1., fog_start: 0., fog_end: 1.,
            lighting: false, color_material: false, normalize: false, rescale_normal: false,
            offset_fill: false, offset_line: false, offset_factor: 0., offset_units: 0.,
            shade_model: 7425, color_mask: [true; 4],
            clear_color: [0.; 4], clear_depth: 1., clear_mask: 0,
            viewport: [0, 0, 854, 480], scissor: [0, 0, 854, 480], scissor_test: false,
            line_smooth: false, active: 0,
            texture_2d: [true; 8], texture_name: [0; 8], lights: [false; 8],
        }
    }
    fn unit(&self) -> usize { self.active.clamp(0, UNITS - 1) as usize }
    /// What a batch flushed now composes with; the separate alpha factors are folded onto the colour ones.
    fn blend_pair(&self) -> Option<(i32, i32)> { self.blend.then_some((self.blend_src, self.blend_dst)) }
    /// The flag a capability toggles, or `None` for capabilities the state surface does not track.
    fn cap(&mut self, cap: i32) -> Option<&mut bool> {
        let unit = self.unit();
        Some(match cap {
            0x0bc0 => &mut self.alpha,
            0x0be2 => &mut self.blend,
            0x0b44 => &mut self.cull,
            0x0b71 => &mut self.depth,
            0x0b60 => &mut self.fog,
            0x0b50 => &mut self.lighting,
            0x0b57 => &mut self.color_material,
            0x0ba1 => &mut self.normalize,
            0x803a => &mut self.rescale_normal,
            0x8037 => &mut self.offset_fill,
            0x2a02 => &mut self.offset_line,
            0x0c11 => &mut self.scissor_test,
            0x0b20 => &mut self.line_smooth,
            0x4000..=0x4007 => &mut self.lights[(cap - 0x4000) as usize],
            CAP_TEXTURE_2D => &mut self.texture_2d[unit],
            _ => return None,
        })
    }
}

/// State the sink keeps beside `Gl`: the attrib stacks, the framebuffer binding and pixel-store hints.
struct GlState {
    s: States,
    attribs: Vec<States>,
    client: Vec<States>,
    framebuffer: i32,
    pack_alignment: i32,
    unpack_alignment: i32,
}

impl GlState {
    const fn new() -> Self {
        Self { s: States::new(), attribs: Vec::new(), client: Vec::new(), framebuffer: 0, pack_alignment: 4, unpack_alignment: 4 }
    }
}

static STATE: Mutex<GlState> = Mutex::new(GlState::new());

/// Compiled display lists: the commands emitted between `glNewList` and `glEndList`.
struct Lists {
    next: i32,
    commands: Vec<(i32, Vec<Draw>)>,
    recording: Option<Record>,
}

struct Record {
    id: i32,
    execute: bool,
    commands: Vec<Draw>,
}

static LISTS: Mutex<Lists> = Mutex::new(Lists { next: 1, commands: Vec::new(), recording: None });

/// `glGenTextures` names, clear of `texture.rs`'s mod-texture ids (`1 << 20`); both are `rustmc/gl/<id>`.
static NEXT_TEXTURE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1 << 24);
static TEXTURE_SIZE: Mutex<Vec<(i32, i32, i32)>> = Mutex::new(Vec::new());
static CAPABILITIES: Mutex<Option<GlobalRef>> = Mutex::new(None);

fn with_state(f: impl FnOnce(&mut States)) { f(&mut STATE.lock().unwrap().s) }

fn active_texture(texture: i32) {
    with_state(move |s| s.active = (texture - TEXTURE0).clamp(0, UNITS - 1));
}

/// Applies an enable/disable; false for a capability the state surface does not know.
fn cap_state(cap: i32, on: bool) -> bool {
    let mut gl = GL.lock().unwrap();
    let mut state = STATE.lock().unwrap();
    let unit = state.s.unit();
    match state.s.cap(cap) {
        Some(slot) => *slot = on,
        None => return false,
    }
    if cap == CAP_TEXTURE_2D && unit == 0 { gl.texture_2d = on; }
    true
}

/// The full-window viewport (and scissor box) for a framebuffer of this size.
pub(crate) fn reset_viewport((w, h): (i32, i32)) { with_state(|s| { s.viewport = [0, 0, w, h]; s.scissor = [0, 0, w, h]; }); }

/// Textures filled from the framebuffer by `glCopyTexSubImage2D` (their `rustmc/gl/<id>` paths).
static FRAMEBUFFER_COPIES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Draws made while the viewport is smaller than the framebuffer go to an offscreen image that is only read back by
/// `glCopyTexSubImage2D` (renderSkybox's 256x256 pass); the client paints that image itself ([`Draw::Backdrop`]).
fn offscreen() -> bool {
    let (w, h) = super::minecraft::framebuffer_size();
    let v = STATE.lock().unwrap().s.viewport;
    w > 0 && h > 0 && (v[2] < w || v[3] < h)
}

/// Emits one command: into the list being compiled, and to the sink too unless that list is compile-only.
fn out(draw: Draw) {
    if offscreen() { return; }
    let draw = match draw {
        Draw::Vertices { texture: Some(t), .. } if FRAMEBUFFER_COPIES.lock().unwrap().contains(&t) => Draw::Backdrop,
        d => d,
    };
    let mut lists = LISTS.lock().unwrap();
    if let Some(rec) = lists.recording.as_mut() {
        rec.commands.push(draw.clone());
        if !rec.execute { return; }
    }
    drop(lists);
    SINK.lock().unwrap().push(draw);
}

fn gen_lists(range: i32) -> i32 {
    if range <= 0 { return 0; }
    let mut lists = LISTS.lock().unwrap();
    let first = lists.next;
    for id in first..first + range { lists.commands.push((id, Vec::new())); }
    lists.next += range;
    first
}

fn delete_lists(first: i32, range: i32) {
    let mut lists = LISTS.lock().unwrap();
    let end = first.saturating_add(range.max(0));
    lists.commands.retain(|(id, _)| *id < first || *id >= end);
}

/// `glNewList`: 0 when the list starts, else the GL error vanilla would raise.
fn new_list(id: i32, mode: i32) -> i32 {
    let mut lists = LISTS.lock().unwrap();
    if lists.recording.is_some() { return GL_INVALID_OPERATION; }
    let execute = match mode {
        0x1300 => false, // GL_COMPILE
        0x1301 => true,  // GL_COMPILE_AND_EXECUTE
        _ => return GL_INVALID_ENUM,
    };
    lists.recording = Some(Record { id, execute, commands: Vec::new() });
    0
}

fn end_list() -> i32 {
    let mut lists = LISTS.lock().unwrap();
    let Some(rec) = lists.recording.take() else { return GL_INVALID_OPERATION };
    match lists.commands.iter_mut().find(|(id, _)| *id == rec.id) {
        Some(slot) => slot.1 = rec.commands,
        None => lists.commands.push((rec.id, rec.commands)),
    }
    0
}

fn call_list(id: i32) {
    let commands = LISTS.lock().unwrap().commands.iter().find(|(k, _)| *k == id).map(|(_, v)| v.clone());
    for draw in commands.unwrap_or_default() { out(draw); }
}

/// `GLU.gluPerspective`: the frustum matrix multiplied into the current matrix.
fn glu_perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> [f64; 16] {
    let (n, f, aspect) = (f64::from(near), f64::from(far), f64::from(aspect));
    let radians = f64::from(fovy) / 2. * std::f64::consts::PI / 180.;
    let sine = radians.sin();
    let delta = f - n;
    // GLU leaves the matrix alone on a degenerate frustum.
    if delta == 0. || sine == 0. || aspect == 0. { return IDENTITY; }
    let cot = radians.cos() / sine;
    let mut m = [0.; 16];
    m[0] = cot / aspect;
    m[5] = cot;
    m[10] = -(f + n) / delta;
    m[11] = -1.;
    m[14] = -2. * f * n / delta;
    m
}

/// `glGetInteger` answered from the caches; the scalar form can only carry one value per pname.
fn get_integer(pname: i32) -> i32 {
    let gl = GL.lock().unwrap();
    let state = STATE.lock().unwrap();
    let s = &state.s;
    match pname {
        0x0ba0 => gl.matrix_mode,
        0x0ba3 => gl.modelview.saved.len() as i32 + 1,
        0x0ba4 => gl.projection.saved.len() as i32 + 1,
        0x0ba5 => gl.texture_matrix.saved.len() as i32 + 1,
        0x0ba2 => s.viewport[2],
        0x84e0 => TEXTURE0 + s.active,
        0x8069 => s.texture_name[s.unit()],
        0x0cf5 => state.unpack_alignment,
        0x0d05 => state.pack_alignment,
        0x0d33 => 16384, // GL_MAX_TEXTURE_SIZE
        0x8872 | 0x8b4c => 16,
        0x8869 => 16,
        0x8b49 | 0x8b4a => 4096,
        0x8b4d => 32,
        0x0d31 => 8,
        0x0d36 => 32,
        0x0d50 => 4,
        0x0db1 => 24,
        0x0c02 => 8,
        _ => 0,
    }
}

/// `RenderHelper.enable`/`disableStandardItemLighting`: the two item lights and colour material.
fn standard_lighting(on: bool) {
    with_state(move |s| {
        s.lighting = on;
        s.color_material = on;
        s.lights[0] = on;
        s.lights[1] = on;
        if on { s.shade_model = 7424; } // GL_FLAT
    });
}

fn capabilities(c: &mut crate::stubs::Call) -> jni::errors::Result<Ret> {
    let mut cache = CAPABILITIES.lock().unwrap();
    if let Some(caps) = cache.as_ref() { return Ok(Ret::Obj(caps.clone())); }
    let caps = crate::runtime::alloc(c.env, "org/lwjgl/opengl/ContextCapabilities")?;
    // The backend presents a GL 2.1 core context: these are the flags vanilla and mods branch on to pick
    // the core entry points this file implements. Everything else stays at its default (false).
    for name in ["OpenGL11", "OpenGL12", "OpenGL13", "OpenGL14", "OpenGL15", "OpenGL20", "OpenGL21"] {
        let _ = c.env.set_field(&caps, name, "Z", true.into());
    }
    let global = c.env.new_global_ref(&caps)?;
    *cache = Some(global.clone());
    Ok(Ret::Obj(global))
}

fn register_state() {
    const GSM: &str = "net/minecraft/client/renderer/GlStateManager.";
    const GL11: &str = "org/lwjgl/opengl/GL11.";

    fn set_flag(key: &str, value: bool, slot: fn(&mut States) -> &mut bool) {
        register(key, move |_| {
            with_state(move |s| *slot(s) = value);
            Ok(Ret::Null)
        });
    }
    // GlStateManager: cached enables.
    set_flag(&format!("{GSM}func_179147_l"), true, |s| &mut s.blend);
    set_flag(&format!("{GSM}func_179084_k"), false, |s| &mut s.blend);
    set_flag(&format!("{GSM}func_179141_d"), true, |s| &mut s.alpha);
    set_flag(&format!("{GSM}func_179118_c"), false, |s| &mut s.alpha);
    set_flag(&format!("{GSM}func_179097_i"), false, |s| &mut s.depth);
    set_flag(&format!("{GSM}func_179126_j"), true, |s| &mut s.depth);
    set_flag(&format!("{GSM}func_179140_f"), false, |s| &mut s.lighting);
    set_flag(&format!("{GSM}func_179145_e"), true, |s| &mut s.lighting);
    set_flag(&format!("{GSM}func_179142_g"), true, |s| &mut s.color_material);
    set_flag(&format!("{GSM}func_179091_B"), true, |s| &mut s.rescale_normal);
    set_flag(&format!("{GSM}func_179101_C"), false, |s| &mut s.rescale_normal);
    set_flag(&format!("{GSM}func_179089_o"), true, |s| &mut s.cull);
    set_flag(&format!("{GSM}func_179129_p"), false, |s| &mut s.cull);
    set_flag(&format!("{GSM}func_179088_q"), true, |s| &mut s.offset_fill);

    // Blend factors; `tryBlendFuncSeparate` only re-sends when something changed, as vanilla does.
    register(&format!("{GSM}func_179112_b"), |c| {
        let (src, dst) = (c.i32(0)?, c.i32(1)?);
        with_state(move |s| (s.blend_src, s.blend_dst, s.blend_src_a, s.blend_dst_a) = (src, dst, src, dst));
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179120_a"), |c| {
        let (src, dst, src_a, dst_a) = (c.i32(0)?, c.i32(1)?, c.i32(2)?, c.i32(3)?);
        with_state(move |s| {
            if (s.blend_src, s.blend_dst, s.blend_src_a, s.blend_dst_a) != (src, dst, src_a, dst_a) {
                (s.blend_src, s.blend_dst, s.blend_src_a, s.blend_dst_a) = (src, dst, src_a, dst_a);
            }
        });
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179092_a"), |c| {
        let (func, reference) = (c.i32(0)?, c.f32(1)?);
        with_state(move |s| (s.alpha_func, s.alpha_ref) = (func, reference));
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179132_a"), |c| {
        let mask = c.bool(0)?;
        with_state(move |s| s.depth_mask = mask);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179143_c"), |c| {
        let func = c.i32(0)?;
        with_state(move |s| s.depth_func = func);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179103_j"), |c| {
        let mode = c.i32(0)?;
        with_state(move |s| s.shade_model = mode);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179086_m"), |c| {
        let mask = c.i32(0)?;
        with_state(move |s| s.clear_mask = mask);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179082_a"), |c| {
        let color = [c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?];
        with_state(move |s| s.clear_color = color);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179083_b"), |c| {
        let v = [c.i32(0)?, c.i32(1)?, c.i32(2)?, c.i32(3)?];
        with_state(move |s| s.viewport = v);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179135_a"), |c| {
        let mask = [c.bool(0)?, c.bool(1)?, c.bool(2)?, c.bool(3)?];
        with_state(move |s| s.color_mask = mask);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179138_g"), |c| {
        active_texture(c.i32(0)?);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179093_d"), |c| {
        let mode = c.i32(0)?;
        with_state(move |s| s.fog_mode = mode);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179102_b"), |c| {
        let start = c.f32(0)?;
        with_state(move |s| s.fog_start = start);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179153_c"), |c| {
        let end = c.f32(0)?;
        with_state(move |s| s.fog_end = end);
        Ok(Ret::Null)
    });
    register(&format!("{GSM}func_179095_a"), |c| {
        let density = c.f32(0)?;
        with_state(move |s| s.fog_density = density);
        Ok(Ret::Null)
    });

    // GL11: the same state reached directly, plus the immediate-mode leaf calls the sink cannot honour.
    for (name, on) in [("glEnable", true), ("glDisable", false)] {
        register(&format!("{GL11}{name}"), move |c| {
            let cap = c.i32(0)?;
            if !cap_state(cap, on) { GL.lock().unwrap().error(GL_INVALID_ENUM); }
            Ok(Ret::Null)
        });
    }
    register(&format!("{GL11}glIsEnabled"), |c| {
        let cap = c.i32(0)?;
        let mut gl = GL.lock().unwrap();
        let mut state = STATE.lock().unwrap();
        // Vanilla toggles unit 0's texturing through the sink's flag, so answer from it there.
        let value = if cap == CAP_TEXTURE_2D && state.s.active == 0 {
            Some(gl.texture_2d)
        } else {
            state.s.cap(cap).map(|slot| *slot)
        };
        drop(state);
        if value.is_none() { gl.error(GL_INVALID_ENUM); }
        Ok(Ret::Bool(value.unwrap_or(false)))
    });
    register(&format!("{GL11}glGetInteger"), |c| Ok(get_integer(c.i32(0)?).into()));
    register(&format!("{GL11}glBlendFunc"), |c| {
        let (src, dst) = (c.i32(0)?, c.i32(1)?);
        with_state(move |s| (s.blend_src, s.blend_dst, s.blend_src_a, s.blend_dst_a) = (src, dst, src, dst));
        Ok(Ret::Null)
    });
    register("org/lwjgl/opengl/GL14.glBlendFuncSeparate", |c| {
        let (src, dst, src_a, dst_a) = (c.i32(0)?, c.i32(1)?, c.i32(2)?, c.i32(3)?);
        with_state(move |s| (s.blend_src, s.blend_dst, s.blend_src_a, s.blend_dst_a) = (src, dst, src_a, dst_a));
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glAlphaFunc"), |c| {
        let (func, reference) = (c.i32(0)?, c.f32(1)?);
        with_state(move |s| (s.alpha_func, s.alpha_ref) = (func, reference));
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glShadeModel"), |c| {
        let mode = c.i32(0)?;
        with_state(move |s| s.shade_model = mode);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glDepthFunc"), |c| {
        let func = c.i32(0)?;
        with_state(move |s| s.depth_func = func);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glDepthMask"), |c| {
        let mask = c.bool(0)?;
        with_state(move |s| s.depth_mask = mask);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glColorMask"), |c| {
        let mask = [c.bool(0)?, c.bool(1)?, c.bool(2)?, c.bool(3)?];
        with_state(move |s| s.color_mask = mask);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glClear"), |c| {
        let mask = c.i32(0)?;
        with_state(move |s| s.clear_mask = mask);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glClearColor"), |c| {
        let color = [c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?];
        with_state(move |s| s.clear_color = color);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glClearDepth"), |c| {
        let depth = c.f64(0)?;
        with_state(move |s| s.clear_depth = depth);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glViewport"), |c| {
        let v = [c.i32(0)?, c.i32(1)?, c.i32(2)?, c.i32(3)?];
        with_state(move |s| s.viewport = v);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glScissor"), |c| {
        let v = [c.i32(0)?, c.i32(1)?, c.i32(2)?, c.i32(3)?];
        with_state(move |s| s.scissor = v);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glCullFace"), |c| {
        let mode = c.i32(0)?;
        with_state(move |s| s.cull_face = mode);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glPolygonOffset"), |c| {
        let (factor, units) = (c.f32(0)?, c.f32(1)?);
        with_state(move |s| (s.offset_factor, s.offset_units) = (factor, units));
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glPixelStorei"), |c| {
        let (param, value) = (c.i32(0)?, c.i32(1)?);
        let mut state = STATE.lock().unwrap();
        match param {
            0x0cf5 => state.unpack_alignment = value,
            0x0d05 => state.pack_alignment = value,
            _ => {}
        }
        Ok(Ret::Null)
    });
    // glCopyTexSubImage2D: remember which texture holds a framebuffer copy (see Draw::Backdrop).
    register(&format!("{GL11}glCopyTexSubImage2D"), |_| {
        if let Some(t) = GL.lock().unwrap().texture.clone().filter(|t| t.starts_with("rustmc/gl/")) {
            let mut copies = FRAMEBUFFER_COPIES.lock().unwrap();
            if !copies.contains(&t) { copies.push(t); }
        }
        Ok(Ret::Null)
    });
    for name in ["glNormal3f", "glNormal3d", "glHint", "glTexSubImage2D"] {
        register(&format!("{GL11}{name}"), |_| Ok(Ret::Null));
    }

    // Attrib stacks: vanilla's `pushAttrib` only asks for the enable/lighting bits; we keep them all.
    for (name, stack) in [("glPushAttrib", true), ("glPushClientAttrib", true), ("glPopAttrib", false), ("glPopClientAttrib", false)] {
        register(&format!("{GL11}{name}"), move |_| {
            let mut gl = GL.lock().unwrap();
            let mut state = STATE.lock().unwrap();
            let client = name == "glPushClientAttrib" || name == "glPopClientAttrib";
            if stack {
                let snapshot = state.s;
                if client { state.client.push(snapshot); } else { state.attribs.push(snapshot); }
            } else {
                let popped = if client { state.client.pop() } else { state.attribs.pop() };
                match popped {
                    Some(prev) => {
                        state.s = prev;
                        gl.texture_2d = prev.texture_2d[0];
                    }
                    None => gl.error(GL_INVALID_OPERATION),
                }
            }
            Ok(Ret::Null)
        });
    }

    // Display lists.
    register(&format!("{GL11}glGenLists"), |c| Ok(gen_lists(c.i32(0)?).into()));
    register(&format!("{GL11}glDeleteLists"), |c| {
        delete_lists(c.i32(0)?, c.i32(1)?);
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glNewList"), |c| {
        let (id, mode) = (c.i32(0)?, c.i32(1)?);
        match new_list(id, mode) {
            0 => {}
            error => GL.lock().unwrap().error(error),
        }
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glEndList"), |_| {
        match end_list() {
            0 => {}
            error => GL.lock().unwrap().error(error),
        }
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glCallList"), |c| {
        call_list(c.i32(0)?);
        Ok(Ret::Null)
    });
    register("net/minecraft/client/renderer/GLAllocation.func_74526_a", |c| Ok(gen_lists(c.i32(0)?).into()));
    register("net/minecraft/client/renderer/GLAllocation.func_74523_b", |c| {
        delete_lists(c.i32(0)?, 1);
        Ok(Ret::Null)
    });

    // Textures: ids are the client's `rustmc/gl/<id>` GUI textures.
    register(&format!("{GL11}glGenTextures"), |_| {
        Ok(NEXT_TEXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed).into())
    });
    register(&format!("{GL11}glDeleteTextures"), |c| {
        let name = c.i32(0)?;
        TEXTURE_SIZE.lock().unwrap().retain(|(id, _, _)| *id != name);
        with_state(move |s| s.texture_name.iter_mut().for_each(|n| if *n == name { *n = 0 }));
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glBindTexture"), |c| {
        let (target, name) = (c.i32(0)?, c.i32(1)?);
        if target != CAP_TEXTURE_2D { return Ok(Ret::Null); }
        let mut gl = GL.lock().unwrap();
        let mut state = STATE.lock().unwrap();
        let unit = state.s.unit();
        state.s.texture_name[unit] = name;
        if unit == 0 { gl.texture = (name != 0).then(|| format!("rustmc/gl/{name}")); }
        Ok(Ret::Null)
    });
    register(&format!("{GL11}glTexImage2D"), |c| {
        let (level, width, height) = (c.i32(1)?, c.i32(3)?, c.i32(4)?);
        let state = STATE.lock().unwrap();
        let name = state.s.texture_name[state.s.unit()];
        drop(state);
        if name != 0 && level == 0 {
            let mut sizes = TEXTURE_SIZE.lock().unwrap();
            sizes.retain(|(id, _, _)| *id != name);
            sizes.push((name, width, height));
        }
        // The pixels themselves have no consumer here: `texture.rs` owns the upload queue.
        Ok(Ret::Null)
    });

    // Lighting helpers, display, framebuffers and the GLU projection helper.
    register("net/minecraft/client/renderer/RenderHelper.func_74519_b", |_| {
        standard_lighting(true);
        Ok(Ret::Null)
    });
    register("net/minecraft/client/renderer/RenderHelper.func_74518_a", |_| {
        standard_lighting(false);
        Ok(Ret::Null)
    });
    // The GUI variant rotates the item lights with a model-view push/pop around them: net zero here, where
    // only the light enables and colour material reach the sink.
    register("net/minecraft/client/renderer/RenderHelper.func_74520_c", |_| {
        standard_lighting(true);
        Ok(Ret::Null)
    });
    // The window's framebuffer size, whatever viewport a mod has set.
    register("org/lwjgl/opengl/Display.getWidth", |_| Ok(super::minecraft::framebuffer_size().0.into()));
    register("org/lwjgl/opengl/Display.getHeight", |_| Ok(super::minecraft::framebuffer_size().1.into()));
    register("org/lwjgl/opengl/Display.isCreated", |_| Ok(true.into()));
    register("org/lwjgl/opengl/Display.setDisplayMode", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/Display.update", |_| Ok(Ret::Null));
    register("org/lwjgl/opengl/DisplayMode.<init>", |_| Ok(Ret::Null));
    for class in ["GL30", "ARBFramebufferObject", "EXTFramebufferObject"] {
        for name in ["glBindFramebuffer", "glBindFramebufferEXT"] {
            register(&format!("org/lwjgl/opengl/{class}.{name}"), |c| {
                let framebuffer = c.i32(1)?;
                STATE.lock().unwrap().framebuffer = framebuffer;
                Ok(Ret::Null)
            });
        }
    }
    register("org/lwjgl/opengl/GL13.glActiveTexture", |c| {
        active_texture(c.i32(0)?);
        Ok(Ret::Null)
    });
    register("org/lwjgl/opengl/ARBMultitexture.glActiveTextureARB", |c| {
        active_texture(c.i32(0)?);
        Ok(Ret::Null)
    });
    register("org/lwjgl/opengl/GLContext.getCapabilities", |c| capabilities(c));
    register("org/lwjgl/util/glu/Project.gluPerspective", |c| {
        let (fovy, aspect, near, far) = (c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?);
        let m = glu_perspective(fovy, aspect, near, far);
        GL.lock().unwrap().mult(m);
        Ok(Ret::Null)
    });
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    fn capabilities_toggle_and_report_vanilla_defaults() {
        let mut s = States::new();
        assert_eq!(s.cap(0x0be2).map(|flag| *flag), Some(false));
        *s.cap(0x0be2).unwrap() = true;
        assert!(s.blend);
        assert_eq!(s.blend_pair(), Some((1, 0)));
        assert_eq!(s.cap(0x1234).map(|flag| *flag), None);
        assert_eq!((s.alpha_func, s.depth_func, s.shade_model), (519, 513, 7425));
    }

    #[test]
    fn texture_capabilities_follow_the_active_unit() {
        let mut s = States::new();
        s.active = 3;
        *s.cap(CAP_TEXTURE_2D).unwrap() = false;
        assert!(!s.texture_2d[3]);
        assert!(s.texture_2d[0]);
        assert_eq!(s.unit(), 3);
    }

    #[test]
    fn glu_perspective_is_the_glu_frustum() {
        let m = glu_perspective(70., 1.5, 0.05, 1000.);
        let cot = 1. / (70f64.to_radians() / 2.).tan();
        assert!((m[0] - cot / 1.5).abs() < 1e-12);
        assert!((m[5] - cot).abs() < 1e-12);
        // near is an f32 (0.05 is not exact), so compare at f32 precision.
        assert!((m[10] + (1000. + 0.05) / (1000. - 0.05)).abs() < 1e-6);
        assert_eq!(m[11], -1.);
        assert!((m[14] + 2. * 1000. * 0.05 / (1000. - 0.05)).abs() < 1e-6);
        assert_eq!(glu_perspective(70., 0., 1., 2.), IDENTITY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(got: [f64; 3], expected: [f64; 3]) {
        for i in 0..3 {
            assert!(
                (got[i] - expected[i]).abs() < 1e-9,
                "{got:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn legacy_matrix_order_and_push_pop_preserve_parent() {
        let mut gl = Gl::new();
        gl.translate([10., 20., 3.]);
        gl.push();
        gl.scale([2., 3., 4.]);
        close(
            transform(gl.modelview.current, [1., 2., 3.], 1.),
            [12., 26., 15.],
        );
        gl.pop();
        close(
            transform(gl.modelview.current, [1., 2., 3.], 1.),
            [11., 22., 6.],
        );
        gl.pop();
        assert_eq!(gl.error, STACK_UNDERFLOW);
        assert_eq!(gl.modelview.current[12], 10.);
    }

    #[test]
    fn normalized_axis_rotation_uses_opengl_handedness() {
        let mut gl = Gl::new();
        gl.rotate(90., [0., 0., 2.]);
        close(
            transform(gl.modelview.current, [1., 0., 0.], 1.),
            [0., 1., 0.],
        );
        gl.rotate(-90., [0., 0., 1.]);
        close(
            transform(gl.modelview.current, [1., 2., 3.], 1.),
            [1., 2., 3.],
        );
    }

    #[test]
    fn texture_and_projection_stacks_do_not_change_gui_position() {
        let mut gl = Gl::new();
        gl.mode(TEXTURE);
        gl.translate([0.25, 0.5, 0.]);
        gl.mode(PROJECTION);
        gl.scale([2., 2., 2.]);
        gl.mode(MODELVIEW);
        gl.translate([5., 6., 0.]);
        let mut vertices = [Vertex {
            pos: [1., 2., 0.],
            uv: [0., 0.],
            rgba: [1.; 4],
        }];
        assert!(gl.vertices(&mut vertices));
        assert_eq!(vertices[0].pos, [6., 8., 0.]);
        assert_eq!(vertices[0].uv, [0.25, 0.5]);
    }

    #[test]
    fn invalid_transforms_are_latched_without_corrupting_the_matrix() {
        let mut gl = Gl::new();
        gl.translate([f64::NAN, 0., 0.]);
        assert_eq!(gl.error, INVALID_VALUE);
        assert_eq!(gl.modelview.current, IDENTITY);
        gl.mode(123);
        assert_eq!(gl.error, INVALID_VALUE);
        assert_eq!(gl.matrix_mode, MODELVIEW);
        for _ in 0..200 {
            gl.push();
        }
        assert_eq!(gl.modelview.saved.len(), 127);
    }

    #[test]
    fn filled_primitive_conversion_preserves_triangle_order_and_attributes() {
        let vertices: Vec<_> = (0..5)
            .map(|i| Vertex {
                pos: [i as f64, 0., 0.],
                uv: [i as f64, 0.],
                rgba: [i as f32; 4],
            })
            .collect();
        let strip = filled_quads(5, &vertices).unwrap();
        let ids: Vec<_> = strip.iter().map(|v| v.pos[0] as usize).collect();
        assert_eq!(ids, [0, 1, 2, 2, 2, 1, 3, 3, 2, 3, 4, 4]);
        assert_eq!(strip[5], vertices[1]);
        assert_eq!(filled_quads(6, &vertices).unwrap().len(), 12);
        assert_eq!(
            filled_quads(8, &vertices).unwrap(),
            vec![vertices[0], vertices[1], vertices[3], vertices[2]]
        );
        assert!(filled_quads(1, &vertices).is_none());
        assert!(filled_quads(4, &[]).unwrap().is_empty());
    }

    /// A 90-degree perspective (gluPerspective), column-major, eye looking down -z.
    fn perspective() -> [f64; 16] {
        let (n, f) = (0.05, 256.);
        [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., (f + n) / (n - f), -1., 0., 0., 2. * f * n / (n - f), 0.]
    }

    fn at(x: f64, y: f64, z: f64) -> Vertex { Vertex { pos: [x, y, z], rgba: [1.; 4], ..Default::default() } }

    #[test]
    fn world_projection_centres_the_view_axis_and_clips_lines_at_the_eye() {
        let gui = [400., 200.];
        let ahead = project(perspective(), gui, at(0., 0., -10.)).unwrap();
        close(ahead.pos, [200., 100., ahead.pos[2]]);
        // 45 degrees right of the axis lands on the right edge; up is up on screen.
        close(project(perspective(), gui, at(10., 10., -10.)).unwrap().pos[..2].iter().copied().chain([0.]).collect::<Vec<_>>().try_into().unwrap(), [400., 0., 0.]);
        assert!(project(perspective(), gui, at(0., 0., 10.)).is_none(), "behind the camera");
        // A tracer from in front to behind is cut at the eye plane, not mirrored through it.
        let (a, b) = project_segment(perspective(), gui, at(0., 0., -1.), at(1., 0., 5.)).unwrap();
        close(a.pos[..2].iter().copied().chain([0.]).collect::<Vec<_>>().try_into().unwrap(), [200., 100., 0.]);
        assert!(b.pos[0] > 200. && b.pos[0].is_finite());
        assert!(project_segment(perspective(), gui, at(0., 0., 1.), at(1., 0., 5.)).is_none());
    }

    #[test]
    fn world_pass_emits_projected_lines_and_restores_gui_matrices() {
        take_draw();
        let mut modelview = IDENTITY;
        modelview[14] = -10.;
        begin_world(perspective(), modelview, (400., 200.));
        emit(1, vec![at(0., 0., 0.), at(10., 0., 0.)]);
        emit(7, vec![at(-1., -1., 0.), at(1., -1., 0.), at(1., 1., 0.), at(-1., 1., 0.)]);
        end_world();
        let drawn = take_draw();
        let Draw::Lines { width, verts } = &drawn[0] else { panic!("{drawn:?}") };
        assert_eq!((*width, verts.len()), (1., 2));
        close([verts[0].pos[0], verts[0].pos[1], 0.], [200., 100., 0.]);
        close([verts[1].pos[0], verts[1].pos[1], 0.], [400., 100., 0.]);
        let Draw::Vertices { mode: 7, verts, .. } = &drawn[1] else { panic!("{drawn:?}") };
        close([verts[0].pos[0], verts[0].pos[1], 0.], [180., 110., 0.]);
        let gl = GL.lock().unwrap();
        assert!(gl.world.is_none());
        assert_eq!(gl.modelview.current, IDENTITY);
    }
}
