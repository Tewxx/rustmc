@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

// ---- panorama cube (rendered into a small offscreen target, then blurred up)

struct PanoGlobals {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> pano: PanoGlobals;

struct PanoOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_pano(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>) -> PanoOut {
    var out: PanoOut;
    out.clip = pano.view_proj * vec4<f32>(pos, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_pano(in: PanoOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}


// ---- rotateAndBlurSkybox: each call draws the copied image three times at
// u offsets of -1, 0, +1 texels with alpha 1, 1/2, 1/3 (= an even 3-tap box),
// transposed. Seven calls alternate the blur between the original x and y axes.

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_blit(@builtin(vertex_index) i: u32) -> BlitOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: BlitOut;
    out.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}

fn box3(uv: vec2<f32>, step: vec2<f32>) -> vec4<f32> {
    let c = textureSample(tex, samp, uv - step) + textureSample(tex, samp, uv) + textureSample(tex, samp, uv + step);
    return vec4<f32>(c.rgb / 3.0, 1.0);
}

@fragment
fn fs_blur_x(in: BlitOut) -> @location(0) vec4<f32> {
    return box3(in.uv, vec2<f32>(1.0 / 256.0, 0.0));
}

@fragment
fn fs_blur_y(in: BlitOut) -> @location(0) vec4<f32> {
    return box3(in.uv, vec2<f32>(0.0, 1.0 / 256.0));
}
