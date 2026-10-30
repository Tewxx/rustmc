//! Drawing leaves. Real GUI code ends in `FontRenderer` and `Tessellator`/`WorldRenderer`; they land here as
//! `Draw` commands in screen units, for the client's UI layer to replay (`take_draw`).
use crate::stubs::{register, Ret};
use jni::objects::GlobalRef;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Vertex { pub pos: [f64; 3], pub uv: [f64; 2], pub rgba: [f32; 4] }

#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    Text { text: String, x: f32, y: f32, color: i32, shadow: bool },
    /// One `Tessellator.draw()`: GL mode (7 = quads), its vertices, and the bound texture path
    /// (e.g. `textures/gui/widgets.png`) if texturing was enabled and the texture is from the `minecraft` domain.
    Vertices { mode: i32, texture: Option<String>, verts: Vec<Vertex> },
}

#[derive(Default)]
struct Builder { mode: i32, verts: Vec<Vertex>, cur: Vertex }

static SINK: Mutex<Vec<Draw>> = Mutex::new(Vec::new());
static BUILDER: Mutex<Option<Builder>> = Mutex::new(None);

/// The slice of GL state that decides how recorded vertices look.
struct Gl { texture: Option<String>, texture_2d: bool, color: [f32; 4] }
static GL: Mutex<Gl> = Mutex::new(Gl { texture: None, texture_2d: true, color: [1.0; 4] });
static TEXT_WIDTH: Mutex<Option<Box<dyn Fn(&str) -> i32 + Send>>> = Mutex::new(None);

/// Draw commands recorded since the last call.
pub fn take_draw() -> Vec<Draw> { std::mem::take(&mut *SINK.lock().unwrap()) }

/// Plugs in the client's real font metrics; the default is a flat 6 px per character.
pub fn set_text_width(f: impl Fn(&str) -> i32 + Send + 'static) { *TEXT_WIDTH.lock().unwrap() = Some(Box::new(f)); }

/// Width of `s` including its `§` codes: the client font knows `§l` bold adds a pixel per character. Without a hook
/// (headless tests) it is a flat 6 px per visible character.
fn width(s: &str) -> i32 {
    if let Some(f) = TEXT_WIDTH.lock().unwrap().as_ref() { return f(s); }
    let (mut n, mut chars) = (0, s.chars());
    while let Some(ch) = chars.next() { if ch == '§' { chars.next(); } else { n += 6; } }
    n
}

/// `FontRenderer.trimStringToWidth`: the longest prefix (or suffix when `reverse`) that fits `max`.
fn trim(s: &str, max: i32, reverse: bool) -> String {
    let chars: Vec<char> = s.chars().collect();
    // Measure whole prefixes (or suffixes) so formatting codes keep their effect on the characters after them.
    let piece = |n: usize| -> String { if reverse { chars[chars.len() - n..].iter().collect() } else { chars[..n].iter().collect() } };
    let n = (0..=chars.len()).take_while(|&n| width(&piece(n)) <= max).last().unwrap_or(0);
    piece(n)
}

fn builder<R>(f: impl FnOnce(&mut Builder) -> R) -> R { f(BUILDER.lock().unwrap().get_or_insert_with(Default::default)) }

pub fn register_all() {
    const FR: &str = "net/minecraft/client/gui/FontRenderer.";
    register(&format!("{FR}func_78256_a"), |c| { let s = c.string(0)?.unwrap_or_default(); Ok(width(&s).into()) });
    register(&format!("{FR}func_78263_a"), |c| { let ch = c.char(0)?; Ok(width(&ch.to_string()).into()) });
    register(&format!("{FR}func_78269_a"), |c| { let (s, w) = (c.string(0)?.unwrap_or_default(), c.i32(1)?); Ok(trim(&s, w, false).into()) });
    register(&format!("{FR}func_78262_a"), |c| { let (s, w, rev) = (c.string(0)?.unwrap_or_default(), c.i32(1)?, c.bool(2)?); Ok(trim(&s, w, rev).into()) });
    register(&format!("{FR}func_78276_b"), |c| {
        let (text, x, y, color) = (c.string(0)?.unwrap_or_default(), c.i32(1)? as f32, c.i32(2)? as f32, c.i32(3)?);
        draw_text(text, x, y, color, false)
    });
    register(&format!("{FR}func_175063_a"), |c| {
        let (text, x, y, color) = (c.string(0)?.unwrap_or_default(), c.f32(1)?, c.f32(2)?, c.i32(3)?);
        draw_text(text, x, y, color, true)
    });
    register(&format!("{FR}func_175065_a"), |c| {
        let (text, x, y, color, shadow) = (c.string(0)?.unwrap_or_default(), c.f32(1)?, c.f32(2)?, c.i32(3)?, c.bool(4)?);
        draw_text(text, x, y, color, shadow)
    });

    // GL state is not recorded yet: every GlStateManager call is an accepted no-op.
    register("net/minecraft/client/renderer/GlStateManager.*", |_| Ok(Ret::Null));
    // Texture binding by ResourceLocation; only the `minecraft` domain can be replayed by the client.
    register("net/minecraft/client/renderer/texture/TextureManager.func_110577_a", |c| {
        let loc = c.obj(0)?;
        let texture = if loc.is_null() { None } else {
            let domain: String = { let d = c.env.get_field(&loc, "field_110626_a", "Ljava/lang/String;")?.l()?; c.env.get_string(&d.into())?.into() };
            let path: String = { let p = c.env.get_field(&loc, "field_110625_b", "Ljava/lang/String;")?.l()?; c.env.get_string(&p.into())?.into() };
            (domain == "minecraft").then_some(path)
        };
        GL.lock().unwrap().texture = texture;
        Ok(Ret::Null)
    });
    const GSM: &str = "net/minecraft/client/renderer/GlStateManager.";
    register(&format!("{GSM}func_179131_c"), |c| { let v = [c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?]; GL.lock().unwrap().color = v; Ok(Ret::Null) });
    register(&format!("{GSM}func_179124_c"), |c| { let v = [c.f32(0)?, c.f32(1)?, c.f32(2)?, 1.0]; GL.lock().unwrap().color = v; Ok(Ret::Null) });
    register(&format!("{GSM}func_179090_x"), |_| { GL.lock().unwrap().texture_2d = false; Ok(Ret::Null) });
    register(&format!("{GSM}func_179098_w"), |_| { GL.lock().unwrap().texture_2d = true; Ok(Ret::Null) });
    // UI click sounds: there is no audio path for mod UIs yet.
    register("net/minecraft/client/audio/SoundHandler.func_147682_a", |_| Ok(Ret::Null));
    register("net/minecraft/client/audio/PositionedSoundRecord.func_147674_a", |_| Ok(Ret::Null));
    register("org/lwjgl/input/Keyboard.enableRepeatEvents", |_| Ok(Ret::Null));

    const TESS: &str = "net/minecraft/client/renderer/Tessellator.";
    register(&format!("{TESS}func_178181_a"), |c| single(c, &TESSELLATOR, "net/minecraft/client/renderer/Tessellator"));
    register(&format!("{TESS}func_178180_c"), |c| single(c, &RENDERER, "net/minecraft/client/renderer/WorldRenderer"));
    register(&format!("{TESS}func_78381_a"), |_| {
        let (mode, verts) = builder(|b| (b.mode, std::mem::take(&mut b.verts)));
        let texture = { let gl = GL.lock().unwrap(); if gl.texture_2d { gl.texture.clone() } else { None } };
        SINK.lock().unwrap().push(Draw::Vertices { mode, texture, verts });
        Ok(Ret::Null)
    });

    // WorldRenderer's fluent setters return `this`.
    const WR: &str = "net/minecraft/client/renderer/WorldRenderer.";
    // A new batch starts from the current GL colour, as vertex formats without a colour element do in GL.
    register(&format!("{WR}func_181668_a"), |c| {
        let (mode, color) = (c.i32(0)?, GL.lock().unwrap().color);
        builder(|b| { b.mode = mode; b.verts.clear(); b.cur.rgba = color; });
        Ok(Ret::Null)
    });
    register(&format!("{WR}func_181662_b"), |c| { let p = [c.f64(0)?, c.f64(1)?, c.f64(2)?]; builder(|b| b.cur.pos = p); this(c) });
    register(&format!("{WR}func_181673_a"), |c| { let uv = [c.f64(0)?, c.f64(1)?]; builder(|b| b.cur.uv = uv); this(c) });
    register(&format!("{WR}func_181666_a"), |c| { let v = [c.f32(0)?, c.f32(1)?, c.f32(2)?, c.f32(3)?]; builder(|b| b.cur.rgba = v); this(c) });
    register(&format!("{WR}func_181669_b"), |c| {
        let v = [c.i32(0)? as f32 / 255.0, c.i32(1)? as f32 / 255.0, c.i32(2)? as f32 / 255.0, c.i32(3)? as f32 / 255.0];
        builder(|b| b.cur.rgba = v);
        this(c)
    });
    register(&format!("{WR}func_181663_c"), |c| this(c));
    register(&format!("{WR}func_181675_d"), |_| { builder(|b| { let v = b.cur; b.verts.push(v); }); Ok(Ret::Null) });
}

static TESSELLATOR: OnceLock<GlobalRef> = OnceLock::new();
static RENDERER: OnceLock<GlobalRef> = OnceLock::new();

fn single(c: &mut crate::stubs::Call<'_, '_>, cell: &'static OnceLock<GlobalRef>, class: &str) -> jni::errors::Result<Ret> {
    let obj = super::singleton(c.env, cell, class)?;
    Ok(Ret::Obj(c.env.new_global_ref(obj)?))
}

fn this(c: &mut crate::stubs::Call<'_, '_>) -> jni::errors::Result<Ret> { Ok(Ret::Obj(c.env.new_global_ref(&c.this)?)) }

fn draw_text(text: String, x: f32, y: f32, color: i32, shadow: bool) -> jni::errors::Result<Ret> {
    let end = x as i32 + width(&text);
    SINK.lock().unwrap().push(Draw::Text { text, x, y, color, shadow });
    Ok(end.into())
}
