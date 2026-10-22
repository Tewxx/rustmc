struct GuiGlobals {
    // x, y = scaled GUI size (ortho projection), z = 1 when the target is sRGB
    screen: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: GuiGlobals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

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

@fragment
fn fs_alpha_test(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv) * in.color;
    if (c.a <= 0.1) {
        discard;
    }
    return encode(c);
}
