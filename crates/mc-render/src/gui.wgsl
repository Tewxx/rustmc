struct GuiGlobals {
    // x, y = scaled GUI size (ortho projection), z = 1 when the target is sRGB
    screen: vec4<f32>,
    // x, y = renderEffect's texture offsets: (time % 3000) / 3000 / 8, (time % 4873) / 4873 / 8
    glint: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: GuiGlobals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var glint_tex: texture_2d<f32>;
@group(2) @binding(1) var glint_samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    // glOrtho(0, w, h, 0, ...)
    out.clip = vec4<f32>(pos.x / g.screen.x * 2.0 - 1.0, 1.0 - pos.y / g.screen.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

fn encode(c: vec4<f32>) -> vec4<f32> {
    if (g.screen.z > 0.5) {
        let lo = c.rgb / 12.92;
        let hi = pow((c.rgb + 0.055) / 1.055, vec3<f32>(2.4));
        return vec4<f32>(select(hi, lo, c.rgb <= vec3<f32>(0.04045)), c.a);
    }
    return c;
}

@fragment
fn fs_plain(in: VsOut) -> @location(0) vec4<f32> {
    return encode(textureSample(tex, samp, in.uv) * in.color);
}

// Texture matrix scale(8) . translate(f, 0) . rotate(deg) applied to the item's atlas UV.
fn glint_uv(uv: vec2<f32>, f: f32, deg: f32) -> vec2<f32> {
    let r = radians(deg);
    let c = cos(r);
    let s = sin(r);
    return 8.0 * (vec2<f32>(c * uv.x - s * uv.y, s * uv.x + c * uv.y) + vec2<f32>(f, 0.0));
}

// Both renderEffect passes at once: blendFunc(SRC_COLOR, ONE) adds c * c per pass.
@fragment
fn fs_glint(in: VsOut) -> @location(0) vec4<f32> {
    let mask = textureSample(tex, samp, in.uv);
    let a = textureSampleLevel(glint_tex, glint_samp, glint_uv(in.uv, g.glint.x, -50.0), 0.0) * in.color;
    let b = textureSampleLevel(glint_tex, glint_samp, glint_uv(in.uv, -g.glint.y, 10.0), 0.0) * in.color;
    if (mask.a <= 0.1) {
        discard;
    }
    return encode(vec4<f32>(a.rgb * a.rgb + b.rgb * b.rgb, 1.0));
}

@fragment
fn fs_alpha_test(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv) * in.color;
    if (c.a <= 0.1) {
        discard;
    }
    return encode(c);
}
