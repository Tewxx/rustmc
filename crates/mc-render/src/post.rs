//! Vanilla 1.8.9 post-processing on wgpu: ShaderGroup / Shader / ShaderManager / Framebuffer.
//!
//! A `shaders/post/*.json` chain is an ordered list of full-screen passes over named render targets;
//! each pass is a `shaders/program/*.json` program that draws one quad. `minecraft:main` is the
//! caller's framebuffer, every other target is a texture we own and ping-pong.
//!
//! Vanilla is GL: texture V points up and the projection flips y, so a program's `texCoord` is in
//! "GL space". We keep the same texCoord maths and flip V when sampling (`smp`), which makes an
//! input -> output blit a pixel-exact copy.
//!
//! `load` only parses (no `Device` yet); GPU objects are built on the first `render` and dropped by
//! `resize`/`set_main_format`.

use std::collections::HashMap;
use wgpu::util::DeviceExt;

// ---------------------------------------------------------------- JSON

/// Just enough JSON for the shader files (`serde_json` is only a dev-dependency of this crate).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self { Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v), _ => None }
    }
    pub fn str(&self) -> Option<&str> { match self { Json::Str(s) => Some(s), _ => None } }
    pub fn num(&self) -> Option<f64> { match self { Json::Num(n) => Some(*n), _ => None } }
    pub fn arr(&self) -> Option<&[Json]> { match self { Json::Arr(a) => Some(a), _ => None } }
    pub fn obj(&self) -> Option<&[(String, Json)]> { match self { Json::Obj(kv) => Some(kv), _ => None } }
    /// A number array, or a plain number as a one-element array (`"values": 4.0` is tolerated).
    pub fn floats(&self) -> Vec<f32> {
        match self {
            Json::Arr(a) => a.iter().filter_map(Json::num).map(|n| n as f32).collect(),
            Json::Num(n) => vec![*n as f32],
            _ => Vec::new(),
        }
    }
    pub fn get_str(&self, key: &str) -> Option<&str> { self.get(key).and_then(Json::str) }
    pub fn get_u32(&self, key: &str) -> Option<u32> { self.get(key).and_then(Json::num).map(|n| n.max(1.0) as u32) }
    pub fn get_floats(&self, key: &str) -> Vec<f32> { self.get(key).map(Json::floats).unwrap_or_default() }
}

pub fn parse_json(text: &str) -> Result<Json, String> {
    let mut p = Parser { b: text.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() { return Err(format!("trailing data at {}", p.i)); }
    Ok(v)
}

struct Parser<'a> { b: &'a [u8], i: usize }

impl Parser<'_> {
    fn ws(&mut self) { while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n') { self.i += 1 } }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&c) { self.i += 1; Ok(()) } else { Err(format!("expected '{}' at {}", c as char, self.i)) }
    }
    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.b.get(self.i).copied() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => Err(format!("unexpected value at {}", self.i)),
        }
    }
    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(w.as_bytes()) { self.i += w.len(); Ok(v) } else { Err(format!("bad literal at {}", self.i)) }
    }
    fn object(&mut self) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut kv = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b'}') { self.i += 1; return Ok(Json::Obj(kv)); }
        loop {
            self.ws();
            let key = self.string()?;
            self.eat(b':')?;
            kv.push((key, self.value()?));
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => { self.i += 1; return Ok(Json::Obj(kv)); }
                _ => return Err(format!("expected ',' or '}}' at {}", self.i)),
            }
        }
    }
    fn array(&mut self) -> Result<Json, String> {
        self.eat(b'[')?;
        let mut v = Vec::new();
        self.ws();
        if self.b.get(self.i) == Some(&b']') { self.i += 1; return Ok(Json::Arr(v)); }
        loop {
            v.push(self.value()?);
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => { self.i += 1; return Ok(Json::Arr(v)); }
                _ => return Err(format!("expected ',' or ']' at {}", self.i)),
            }
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let Some(&c) = self.b.get(self.i) else { return Err("unterminated string".into()) };
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else { return Err("unterminated escape".into()) };
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4).ok_or("short \\u")?;
                            let s = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                            let n = u32::from_str_radix(s, 16).map_err(|e| e.to_string())?;
                            self.i += 4;
                            out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        other => return Err(format!("bad escape \\{}", other as char)),
                    }
                }
                _ => {
                    // Copy the raw byte run (UTF-8 passes through unchanged).
                    let start = self.i - 1;
                    let mut end = self.i;
                    while let Some(&n) = self.b.get(end) { if n == b'"' || n == b'\\' { break } end += 1 }
                    out.push_str(std::str::from_utf8(&self.b[start..end]).map_err(|e| e.to_string())?);
                    self.i = end;
                }
            }
        }
    }
    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') { self.i += 1 }
        while self.b.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) { self.i += 1 }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
        s.parse::<f64>().map(Json::Num).map_err(|e| format!("bad number {s}: {e}"))
    }
}

// ---------------------------------------------------------------- model

/// One entry of `"targets"`: a framebuffer we own (vanilla's `addTarget`).
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub name: String,
    /// `None` means "main framebuffer size", like vanilla's `width`/`height` defaults.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// `"auxtargets"`: an extra sampler bound to another target (or a texture, which we skip).
#[derive(Clone, Debug, PartialEq)]
pub struct AuxTarget {
    pub name: String,
    pub id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A uniform and its values, as written in either JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct UniformValue {
    pub name: String,
    pub values: Vec<f32>,
}

/// One `"passes"` entry: a program drawn from `intarget` into `outtarget`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub name: String,
    pub intarget: String,
    pub outtarget: String,
    pub auxtargets: Vec<AuxTarget>,
    pub uniforms: Vec<UniformValue>,
}

/// A whole `shaders/post/*.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub targets: Vec<Target>,
    pub passes: Vec<Pass>,
}

pub const MAIN: &str = "minecraft:main";

impl Chain {
    pub fn parse(text: &str) -> Result<Chain, String> {
        let json = parse_json(text)?;
        let mut targets = Vec::new();
        for t in json.get("targets").and_then(Json::arr).unwrap_or(&[]) {
            let target = match t {
                Json::Str(name) => Target { name: name.clone(), width: None, height: None },
                other => Target {
                    name: other.get_str("name").ok_or("target without name")?.to_owned(),
                    width: other.get_u32("width"),
                    height: other.get_u32("height"),
                },
            };
            targets.push(target);
        }
        let mut passes = Vec::new();
        for p in json.get("passes").and_then(Json::arr).ok_or("no passes")? {
            let name = p.get_str("name").ok_or("pass without name")?.to_owned();
            // Vanilla defaults the targets to "minecraft:main".
            let intarget = p.get_str("intarget").unwrap_or(MAIN).to_owned();
            let outtarget = p.get_str("outtarget").unwrap_or(MAIN).to_owned();
            let mut auxtargets = Vec::new();
            for a in p.get("auxtargets").and_then(Json::arr).unwrap_or(&[]) {
                auxtargets.push(AuxTarget {
                    name: a.get_str("name").unwrap_or("AuxSampler").to_owned(),
                    id: a.get_str("id").unwrap_or_default().to_owned(),
                    width: a.get_u32("width"),
                    height: a.get_u32("height"),
                });
            }
            let mut uniforms = Vec::new();
            for u in p.get("uniforms").and_then(Json::arr).unwrap_or(&[]) {
                let values = if u.get("values").is_some() { u.get_floats("values") } else { u.get_floats("value") };
                if let Some(name) = u.get_str("name") { uniforms.push(UniformValue { name: name.to_owned(), values }) }
            }
            passes.push(Pass { name, intarget, outtarget, auxtargets, uniforms });
        }
        Ok(Chain { targets, passes })
    }
}

/// A `shaders/program/*.json` (vanilla `ShaderManager` + `Shader`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blend {
    pub src: wgpu::BlendFactor,
    pub dst: wgpu::BlendFactor,
    pub func: wgpu::BlendOperation,
}

impl Default for Blend {
    /// `add / srcalpha / 1-srcalpha`, the blend every vanilla post program uses.
    fn default() -> Self { Blend { src: wgpu::BlendFactor::SrcAlpha, dst: wgpu::BlendFactor::OneMinusSrcAlpha, func: wgpu::BlendOperation::Add } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub fragment: String,
    pub vertex: String,
    pub blend: Blend,
    pub samplers: Vec<String>,
    pub attributes: Vec<String>,
    pub uniforms: Vec<UniformValue>,
}

impl Program {
    pub fn parse(text: &str) -> Result<Program, String> {
        let json = parse_json(text)?;
        let blend = match json.get("blend") {
            Some(b) => Blend {
                src: factor(b.get_str("srcrgb").unwrap_or("srcalpha"))?,
                dst: factor(b.get_str("dstrgb").unwrap_or("1-srcalpha"))?,
                func: operation(b.get_str("func").unwrap_or("add"))?,
            },
            None => Blend::default(),
        };
        let names = |key: &str| -> Vec<String> {
            json.get(key).and_then(Json::arr).unwrap_or(&[]).iter()
                .filter_map(|v| v.get_str("name").or_else(|| v.str()).map(str::to_owned)).collect()
        };
        let mut uniforms = Vec::new();
        for u in json.get("uniforms").and_then(Json::arr).unwrap_or(&[]) {
            let values = if u.get("values").is_some() { u.get_floats("values") } else { u.get_floats("value") };
            if let Some(name) = u.get_str("name") { uniforms.push(UniformValue { name: name.to_owned(), values }) }
        }
        Ok(Program {
            fragment: json.get_str("fragment").unwrap_or("blit").to_owned(),
            vertex: json.get_str("vertex").unwrap_or("blit").to_owned(),
            blend,
            samplers: names("samplers"),
            attributes: json.get("attributes").and_then(Json::arr).unwrap_or(&[]).iter().filter_map(Json::str).map(str::to_owned).collect(),
            uniforms,
        })
    }
}

fn factor(name: &str) -> Result<wgpu::BlendFactor, String> {
    use wgpu::BlendFactor as F;
    Ok(match name {
        "0" | "zero" => F::Zero,
        "1" | "one" => F::One,
        "srccolor" => F::Src,
        "1-srccolor" | "oneminussrccolor" => F::OneMinusSrc,
        "dstcolor" => F::Dst,
        "1-dstcolor" | "oneminusdstcolor" => F::OneMinusDst,
        "srcalpha" | "srcalphasaturate" => F::SrcAlpha,
        "1-srcalpha" | "oneminussrcalpha" => F::OneMinusSrcAlpha,
        "dstalpha" => F::DstAlpha,
        "1-dstalpha" | "oneminusdstalpha" => F::OneMinusDstAlpha,
        other => return Err(format!("unknown blend factor {other}")),
    })
}

fn operation(name: &str) -> Result<wgpu::BlendOperation, String> {
    Ok(match name {
        "add" => wgpu::BlendOperation::Add,
        "subtract" => wgpu::BlendOperation::Subtract,
        "reverse_subtract" | "reversesubtract" => wgpu::BlendOperation::ReverseSubtract,
        "min" => wgpu::BlendOperation::Min,
        "max" => wgpu::BlendOperation::Max,
        other => return Err(format!("unknown blend func {other}")),
    })
}

// ---------------------------------------------------------------- uniforms

/// Every uniform name a vanilla program declares, in buffer order. Each is one `vec4<f32>`, so a
/// program can read `.x`/`.xy`/`.xyz` regardless of the type its JSON declares; unused names stay 0.
pub const UNIFORMS: &[&str] = &[
    "ProjMat", "InSize", "OutSize", "ScreenSize", "Time", "AuxSize0", "AuxSize1", "AuxSize2", "AuxSize3",
    "ColorModulate", "BlurDir", "Radius", "Saturation", "DesaturationFactor", "InverseAmount", "MosaicSize",
    "Resolution", "LumaRamp", "LumaLevel", "VxOffset", "SpanMax", "ReduceMul", "SubPixelShift", "InScale",
    "InOffset", "InRotation", "Phosphor", "Green", "Creeper", "Notch", "Gray", "ColorScale", "Offset",
    "ConvergeX", "ConvergeY", "RadialConvergeX", "RadialConvergeY", "Scissor", "Vignette", "Tint",
];

fn uniform_index(name: &str) -> Option<usize> { UNIFORMS.iter().position(|n| *n == name) }

/// Floats in the uniform buffer: 4 per name.
pub const UNIFORM_SLOTS: usize = 4;
const UNIFORM_BYTES: u64 = (UNIFORMS.len() * UNIFORM_SLOTS * 4) as u64;

// ---------------------------------------------------------------- programs (WGSL)

/// Programs whose vanilla GLSL we ported 1:1.
fn fragment_body(program: &str) -> Option<&'static str> {
    Some(match program {
        // shaders/program/blit.fsh: `gl_FragColor = texture2D(DiffuseSampler, texCoord) * ColorModulate;`
        "blit" => "    return smp(tx0, i.tc) * u.ColorModulate;",
        // shaders/program/blur.fsh: a separable box blur of `Radius` texels along `BlurDir`.
        "blur" => BLUR_BODY,
        _ => return None,
    })
}

/// `blur.fsh`, ported line for line (the loop bound and the divide by `Radius * 2 + 1` included).
const BLUR_BODY: &str = r#"    var radius = u.Radius.x;
    if (!(radius >= 0.0)) { radius = 0.0; }
    radius = min(radius, 256.0);
    var blurred = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    var totalAlpha = 0.0;
    var totalSamples = 0.0;
    var r = -radius;
    loop {
        if (r > radius) { break; }
        let sampleValue = smp(tx0, i.tc + i.oneTexel * r * u.BlurDir.xy);
        // Accumulate average alpha
        totalAlpha = totalAlpha + sampleValue.a;
        totalSamples = totalSamples + 1.0;
        // Accumulate smoothed blur
        blurred = blurred + sampleValue;
        r = r + 1.0;
    }
    return vec4<f32>(blurred.rgb / (radius * 2.0 + 1.0), totalAlpha);"#;

/// Vanilla programs whose `.fsh` we could not read in this environment: their passes are drawn with
/// the `blit` body (a straight copy) instead of the original effect.
///
/// TODO: port `antialias`, `art`(outline_watercolor/outline_combine/blobs2), `bits`, `blobs`,
/// `blobs2`, `bumpy`, `color_convolve`, `creeper`, `deconverge`, `desaturate`, `downscale`,
/// `entity_outline`, `entity_sobel`, `flip`, `fxaa`, `green`, `invert`, `notch`, `ntsc_decode`,
/// `ntsc_encode`, `outline`, `pencil`, `phosphor`, `scan_pincushion`, `sobel`, `spiderclip`,
/// `watercolor`, `wobble` from `assets/minecraft/shaders/program/*.fsh` — each one only needs its
/// `main` body swapped into `fragment_body` (the vertex stage below already matches `blit.vsh` /
/// `blobs.vsh`, and `InSize`/`OutSize`/`ScreenSize`/`Time`/`AuxSize*` are uploaded for them).
pub const UNPORTED: &[&str] = &[
    "antialias", "bits", "blobs", "blobs2", "bumpy", "color_convolve", "creeper", "deconverge", "desaturate",
    "downscale", "entity_outline", "entity_sobel", "flip", "fxaa", "green", "invert", "notch", "ntsc_decode",
    "ntsc_encode", "outline", "outline_combine", "outline_soft", "outline_watercolor", "pencil", "phosphor",
    "scan_pincushion", "sobel", "spiderclip", "watercolor", "wobble",
];

fn wgsl(program: &str) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str("// generated from a shaders/program/*.json by mc_render::post\n");
    s.push_str("struct Uniforms {\n");
    for name in UNIFORMS { s.push_str(&format!("    {name}: vec4<f32>,\n")) }
    s.push_str("}\n\n@group(0) @binding(0) var<uniform> u: Uniforms;\n");
    s.push_str("@group(0) @binding(1) var tx0: texture_2d<f32>;\n");
    s.push_str("@group(0) @binding(2) var tx1: texture_2d<f32>;\n");
    s.push_str("@group(0) @binding(3) var tx2: texture_2d<f32>;\n");
    s.push_str("@group(0) @binding(4) var tx3: texture_2d<f32>;\n");
    s.push_str("@group(0) @binding(5) var tx4: texture_2d<f32>;\n");
    s.push_str("@group(0) @binding(6) var sam: sampler;\n\n");
    // GL samples with V up; our textures have row 0 at the top, so flip V here (once, for every program).
    s.push_str("fn smp(t: texture_2d<f32>, tc: vec2<f32>) -> vec4<f32> { return textureSample(t, sam, vec2<f32>(tc.x, 1.0 - tc.y)); }\n\n");
    s.push_str(VERTEX);
    s.push_str("@fragment\nfn fs(i: Vary) -> @location(0) vec4<f32> {\n");
    s.push_str(fragment_body(program).unwrap_or("    return smp(tx0, i.tc) * u.ColorModulate;"));
    s.push_str("\n}\n");
    s
}

/// `blit.vsh` / `blobs.vsh`: `texCoord = Position / OutSize` with the y flip the projection undoes,
/// plus `oneTexel = 1 / InSize`. `corner` is (0,0) at the top-left of the target, y pointing down.
const VERTEX: &str = r#"struct Vary {
    @builtin(position) pos: vec4<f32>,
    @location(0) tc: vec2<f32>,
    @location(1) oneTexel: vec2<f32>,
};

@vertex
fn vs(@location(0) corner: vec2<f32>) -> Vary {
    var in_size = max(u.InSize.xy, vec2<f32>(1.0, 1.0));
    // ProjMat is 2/w, -2/h with a -1/+1 translate: Position.y grows downwards, NDC.y up.
    var ndc = vec2<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0);
    var o: Vary;
    o.pos = vec4<f32>(ndc, 0.0, 1.0);
    o.tc = vec2<f32>(ndc.x * 0.5 + 0.5, ndc.y * 0.5 + 0.5);
    o.oneTexel = vec2<f32>(1.0, 1.0) / in_size;
    return o;
}

"#;

// ---------------------------------------------------------------- chain

/// A parsed chain plus its (lazily created) GPU resources.
pub struct PostChain {
    chain: Chain,
    programs: Vec<Program>,
    /// Per pass: uniform name -> values, i.e. program JSON defaults overridden by the pass's uniforms.
    defaults: Vec<Vec<(usize, [f32; 4])>>,
    overrides: HashMap<String, Vec<f32>>,
    width: u32,
    height: u32,
    main_format: wgpu::TextureFormat,
    gpu: Option<Gpu>,
    /// `ShaderGroup.field_148036_j` / `field_148035_a`: vanilla's cyclic shader clock.
    time: f32,
    last_partial: f32,
}

struct Gpu {
    targets: HashMap<String, TargetRt>,
    passes: Vec<PassRt>,
    corners: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    _dummy_tex: wgpu::Texture,
}

struct TargetRt {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

struct PassRt {
    uniform: wgpu::Buffer,
    pipeline: Option<wgpu::RenderPipeline>,
    binding: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

const INTERNAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl PostChain {
    /// Parse a `shaders/post/*.json`; `programs` resolves a pass name to its `shaders/program/*.json`.
    pub fn load(text: &str, programs: impl Fn(&str) -> Option<String>, width: u32, height: u32) -> Result<PostChain, String> {
        let chain = Chain::parse(text)?;
        let mut loaded = Vec::with_capacity(chain.passes.len());
        let mut defaults = Vec::with_capacity(chain.passes.len());
        for pass in &chain.passes {
            let json = programs(&pass.name).ok_or_else(|| format!("missing program {}", pass.name))?;
            let program = Program::parse(&json)?;
            let mut values: Vec<(usize, [f32; 4])> = Vec::new();
            // Program JSON defaults first, then the pass's overrides (vanilla addUniform + initUniform).
            for u in program.uniforms.iter().chain(pass.uniforms.iter()) {
                let Some(index) = uniform_index(&u.name) else { continue };
                let mut slot = values.iter().find(|(i, _)| *i == index).map(|(_, v)| *v).unwrap_or([0.0; 4]);
                for (k, v) in u.values.iter().take(UNIFORM_SLOTS).enumerate() { slot[k] = *v }
                match values.iter_mut().find(|(i, _)| *i == index) {
                    Some(entry) => entry.1 = slot,
                    None => values.push((index, slot)),
                }
            }
            loaded.push(program);
            defaults.push(values);
        }
        Ok(PostChain {
            chain,
            programs: loaded,
            defaults,
            overrides: HashMap::new(),
            width: width.max(1),
            height: height.max(1),
            main_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            gpu: None,
            time: 0.0,
            last_partial: 0.0,
        })
    }

    /// The format of the `minecraft:main` view handed to `render` (the client's framebuffer).
    pub fn set_main_format(&mut self, format: wgpu::TextureFormat) {
        if self.main_format != format { self.main_format = format; self.gpu = None; }
    }

    /// `ShaderGroup.func_148026_a`: new display size, targets not pinned by the JSON follow it.
    pub fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) != (self.width, self.height) { self.width = w; self.height = h; self.gpu = None; }
    }

    /// `ShaderUniform.func_148090_a`: an override applied to every pass that declares the name.
    pub fn set_uniform(&mut self, name: &str, values: &[f32]) { self.overrides.insert(name.to_owned(), values.to_vec()); }
    pub fn uniform(&self, name: &str) -> Option<&[f32]> { self.overrides.get(name).map(Vec::as_slice) }
    pub fn clear_uniform(&mut self, name: &str) { self.overrides.remove(name); }
    pub fn passes(&self) -> &[Pass] { &self.chain.passes }
    pub fn targets(&self) -> &[Target] { &self.chain.targets }
    pub fn size(&self) -> (u32, u32) { (self.width, self.height) }

    fn ensure(&mut self, device: &wgpu::Device) {
        if self.gpu.is_some() { return }
        let mut targets = HashMap::new();
        for t in &self.chain.targets {
            if t.name == MAIN { continue }
            let w = t.width.unwrap_or(self.width).max(1);
            let h = t.height.unwrap_or(self.height).max(1);
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&format!("post target {}", t.name)),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: INTERNAL_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            targets.insert(t.name.clone(), TargetRt { tex, view, width: w, height: h });
        }
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("post dummy"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: INTERNAL_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        // Counter-clockwise-in-NDC pairs of a top-left origin quad (vanilla draws one GL_QUADS).
        let corners: Vec<[f32; 2]> = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let corners = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("post quad"),
            contents: bytemuck::cast_slice(&corners),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        for binding in 1..=5 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("post"), entries: &entries });
        // Vanilla framebuffers are GL_NEAREST + clamp to edge (Framebuffer.setFramebufferFilter(9728)).
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let passes = self.chain.passes.iter().map(|_| PassRt {
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post uniforms"),
                size: UNIFORM_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            pipeline: None,
            binding: None,
        }).collect();
        self.gpu = Some(Gpu { targets, passes, corners, layout, sampler, dummy, _dummy_tex: dummy_tex });
    }

    fn pipeline(&mut self, device: &wgpu::Device, pass: usize) -> Option<wgpu::RenderPipeline> {
        let gpu = self.gpu.as_mut()?;
        if let Some(p) = &gpu.passes[pass].pipeline { return Some(p.clone()) }
        let desc = &self.chain.passes[pass];
        let format = if desc.outtarget == MAIN { self.main_format } else { INTERNAL_FORMAT };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("post {}", desc.name)),
            source: wgpu::ShaderSource::Wgsl(wgsl(&self.programs[pass].fragment).into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[&gpu.layout],
            push_constant_ranges: &[],
        });
        let blend = self.programs[pass].blend;
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("post {}", desc.name)),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Vanilla tryBlendFuncSeparate(src, dst, 1, 0).
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: blend.src, dst_factor: blend.dst, operation: blend.func },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        gpu.passes[pass].pipeline = Some(pipeline.clone());
        Some(pipeline)
    }

    /// Upload `pass`'s uniforms: JSON defaults, then mod overrides, then the values `Shader.loadShader`
    /// sets every frame (InSize/OutSize/ScreenSize/Time/AuxSize*), so those win like in vanilla.
    fn upload(&mut self, queue: &wgpu::Queue, pass: usize, in_size: (u32, u32), out_size: (u32, u32)) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        let mut values = [0f32; UNIFORMS.len() * UNIFORM_SLOTS];
        for (index, slot) in &self.defaults[pass] {
            values[index * UNIFORM_SLOTS..index * UNIFORM_SLOTS + UNIFORM_SLOTS].copy_from_slice(slot);
        }
        for (name, vs) in &self.overrides {
            let Some(index) = uniform_index(name) else { continue };
            for (k, v) in vs.iter().take(UNIFORM_SLOTS).enumerate() { values[index * UNIFORM_SLOTS + k] = *v }
        }
        let mut set = |name: &str, v: [f32; 4]| {
            if let Some(index) = uniform_index(name) { values[index * UNIFORM_SLOTS..index * UNIFORM_SLOTS + 4].copy_from_slice(&v) }
        };
        set("InSize", [in_size.0 as f32, in_size.1 as f32, 0.0, 0.0]);
        set("OutSize", [out_size.0 as f32, out_size.1 as f32, 0.0, 0.0]);
        set("ScreenSize", [self.width as f32, self.height as f32, 0.0, 0.0]);
        set("Time", [self.time / 20.0, 0.0, 0.0, 0.0]);
        for (i, aux) in self.chain.passes[pass].auxtargets.iter().take(4).enumerate() {
            let size = gpu.targets.get(&aux.id).map(|t| (t.width, t.height)).unwrap_or((1, 1));
            set(["AuxSize0", "AuxSize1", "AuxSize2", "AuxSize3"][i], [size.0 as f32, size.1 as f32, 0.0, 0.0]);
        }
        queue.write_buffer(&gpu.passes[pass].uniform, 0, bytemuck::cast_slice(&values));
    }

    /// Draw the chain into `input` (vanilla `ShaderGroup.loadShaderGroup` + `EntityRenderer`).
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder,
                  input: &wgpu::TextureView, partial_ticks: f32) {
        // ShaderGroup.field_148036_j: +1 (minus the last partial) per frame, then /20 as "Time".
        if partial_ticks < 1.0 { self.time += 1.0 - self.last_partial; self.last_partial = partial_ticks }
        else { self.last_partial = 1.0 }
        self.ensure(device);
        let main_size = (self.width, self.height);
        for pass in 0..self.chain.passes.len() {
            let desc = &self.chain.passes[pass];
            if desc.intarget == desc.outtarget {
                eprintln!("[post] pass {} reads and writes {}", desc.name, desc.outtarget);
                continue;
            }
            // A pass may not sample the view it renders into, so collect the views first.
            let (in_view, in_size, out_view, out_size) = {
                let Some(gpu) = self.gpu.as_ref() else { return };
                let (in_view, in_size) = if desc.intarget == MAIN {
                    (input.clone(), main_size)
                } else {
                    match gpu.targets.get(&desc.intarget) { Some(t) => (t.view.clone(), (t.width, t.height)), None => continue }
                };
                let (out_view, out_size) = if desc.outtarget == MAIN {
                    (input.clone(), main_size)
                } else {
                    match gpu.targets.get(&desc.outtarget) { Some(t) => (t.view.clone(), (t.width, t.height)), None => continue }
                };
                (in_view, in_size, out_view, out_size)
            };
            self.upload(queue, pass, in_size, out_size);
            let Some(pipeline) = self.pipeline(device, pass) else { continue };
            let desc = &self.chain.passes[pass];
            let gpu = self.gpu.as_mut().unwrap();
            if gpu.passes[pass].binding.as_ref().is_none_or(|(view, _)| *view != in_view) {
                let mut entries = vec![
                    wgpu::BindGroupEntry { binding: 0, resource: gpu.passes[pass].uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&in_view) },
                ];
                for (k, aux) in desc.auxtargets.iter().take(4).enumerate() {
                    let view = gpu.targets.get(&aux.id).map(|t| &t.view).unwrap_or(&gpu.dummy);
                    entries.push(wgpu::BindGroupEntry { binding: 2 + k as u32, resource: wgpu::BindingResource::TextureView(view) });
                }
                entries.push(wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&gpu.sampler) });
                let mut bound = desc.auxtargets.len().min(4) as u32;
                while bound < 4 {
                    entries.push(wgpu::BindGroupEntry { binding: 2 + bound, resource: wgpu::BindingResource::TextureView(&gpu.dummy) });
                    bound += 1;
                }
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("post"), layout: &gpu.layout, entries: &entries });
                gpu.passes[pass].binding = Some((in_view.clone(), bind_group));
            }
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rp.set_pipeline(&pipeline);
            rp.set_bind_group(0, &gpu.passes[pass].binding.as_ref().unwrap().1, &[]);
            rp.set_vertex_buffer(0, gpu.corners.slice(..));
            rp.draw(0..6, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUR: &str = r#"{
        "targets": [ "swap" ],
        "passes": [
            { "name": "blur", "intarget": "minecraft:main", "outtarget": "swap",
              "uniforms": [ { "name": "BlurDir", "values": [ 1.0, 0.0 ] }, { "name": "Radius", "values": [ 20.0 ] } ] },
            { "name": "blur", "intarget": "swap", "outtarget": "minecraft:main",
              "uniforms": [ { "name": "BlurDir", "values": [ 0.0, 1.0 ] }, { "name": "Radius", "values": [ 20.0 ] } ] }
        ]
    }"#;

    const BLUR_PROGRAM: &str = r#"{
        "blend": { "func": "add", "srcrgb": "srcalpha", "dstrgb": "1-srcalpha" },
        "vertex": "blur", "fragment": "blur",
        "samplers": [ { "name": "DiffuseSampler" } ],
        "attributes": [ "Position" ],
        "uniforms": [
            { "name": "ProjMat", "type": "matrix4x4", "count": 16, "values": [ 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1 ] },
            { "name": "InSize", "type": "float", "count": 2, "values": [ 1.0, 1.0 ] },
            { "name": "OutSize", "type": "float", "count": 2, "values": [ 1.0, 1.0 ] },
            { "name": "BlurDir", "type": "float", "count": 2, "values": [ 1.0, 0.0 ] },
            { "name": "Radius", "type": "float", "count": 1, "values": [ 1.0 ] }
        ]
    }"#;

    #[test]
    fn parses_json_scalars_objects_and_escapes() {
        assert_eq!(parse_json("1.5").unwrap(), Json::Num(1.5));
        assert_eq!(parse_json(" \"a\\nb\" ").unwrap(), Json::Str("a\nb".into()));
        assert_eq!(parse_json("-2e3").unwrap(), Json::Num(-2000.0));
        assert_eq!(parse_json("[true, null, false]").unwrap(), Json::Arr(vec![Json::Bool(true), Json::Null, Json::Bool(false)]));
        let obj = parse_json("{\"a\": [1, 2], \"b\": {\"c\": \"d\"}}").unwrap();
        assert_eq!(obj.get("a").and_then(Json::arr).map(<[Json]>::len), Some(2));
        assert_eq!(obj.get("b").and_then(|b| b.get_str("c")), Some("d"));
        assert!(parse_json("{").is_err());
        assert!(parse_json("[1,]").is_err());
    }

    #[test]
    fn parses_post_chain_in_pass_order() {
        let chain = Chain::parse(BLUR).unwrap();
        assert_eq!(chain.targets, vec![Target { name: "swap".into(), width: None, height: None }]);
        assert_eq!(chain.passes.len(), 2);
        assert_eq!(chain.passes[0].intarget, MAIN);
        assert_eq!(chain.passes[0].outtarget, "swap");
        assert_eq!(chain.passes[1].intarget, "swap");
        assert_eq!(chain.passes[1].outtarget, MAIN);
        assert_eq!(chain.passes[0].uniforms[1].values, vec![20.0]);
        assert_eq!(chain.passes[1].uniforms[0].values, vec![0.0, 1.0]);
    }

    #[test]
    fn parses_program_blend_samplers_and_uniforms() {
        let p = Program::parse(BLUR_PROGRAM).unwrap();
        assert_eq!(p.fragment, "blur");
        assert_eq!(p.vertex, "blur");
        assert_eq!(p.samplers, vec!["DiffuseSampler".to_string()]);
        assert_eq!(p.uniforms.len(), 5);
        assert_eq!(p.blend.src, wgpu::BlendFactor::SrcAlpha);
        assert_eq!(p.blend.dst, wgpu::BlendFactor::OneMinusSrcAlpha);
    }

    #[test]
    fn pass_uniforms_override_program_defaults() {
        let chain = PostChain::load(BLUR, |name| (name == "blur").then(|| BLUR_PROGRAM.to_string()), 1920, 1080).unwrap();
        let radius = uniform_index("Radius").unwrap();
        let blur_dir = uniform_index("BlurDir").unwrap();
        assert_eq!(chain.defaults[0].iter().find(|(i, _)| *i == radius).unwrap().1, [20.0, 0.0, 0.0, 0.0]);
        assert_eq!(chain.defaults[0].iter().find(|(i, _)| *i == blur_dir).unwrap().1, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(chain.defaults[1].iter().find(|(i, _)| *i == blur_dir).unwrap().1, [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(chain.targets()[0].name, "swap");
    }

    #[test]
    fn missing_program_is_an_error() {
        assert!(PostChain::load(BLUR, |_| None, 8, 8).is_err());
    }

    #[test]
    fn every_program_assembles_a_fragment_entry_point() {
        for name in ["blur", "blit", "notch", "fxaa"] {
            let src = wgsl(name);
            assert!(src.contains("@fragment\nfn fs(i: Vary)"), "{name}");
            assert!(src.contains("@vertex\nfn vs("), "{name}");
            for u in ["Radius: vec4<f32>", "BlurDir: vec4<f32>", "InSize: vec4<f32>", "Time: vec4<f32>"] {
                assert!(src.contains(u), "{name}: {u}");
            }
        }
        // Ported programs keep their own body; the rest fall back to blit.
        assert!(wgsl("blur").contains("totalAlpha"));
        assert!(wgsl("wobble").contains("u.ColorModulate"));
        assert!(fragment_body("blit").is_some());
        assert!(fragment_body("wobble").is_none());
    }

    #[test]
    fn uniforms_resize_and_overrides_round_trip() {
        let mut chain = PostChain::load(BLUR, |_| Some(BLUR_PROGRAM.into()), 320, 240).unwrap();
        assert_eq!(chain.size(), (320, 240));
        chain.set_uniform("Radius", &[3.0]);
        assert_eq!(chain.uniform("Radius"), Some(&[3.0f32][..]));
        chain.resize(640, 480);
        assert_eq!(chain.size(), (640, 480));
        chain.clear_uniform("Radius");
        assert!(chain.uniform("Radius").is_none());
    }

    /// Builds every ported pipeline on a real device when one is available.
    #[test]
    fn pipelines_build_when_a_gpu_is_available() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = crate::headless_adapter(&instance) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let mut chain = PostChain::load(BLUR, |_| Some(BLUR_PROGRAM.into()), 64, 64).unwrap();
        chain.set_main_format(wgpu::TextureFormat::Rgba8Unorm);
        let main = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("main"),
            size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = main.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("post") });
        chain.render(&device, &queue, &mut enc, &view, 1.0);
        queue.submit([enc.finish()]);
        let _ = device.poll(wgpu::Maintain::Wait);
    }
}
