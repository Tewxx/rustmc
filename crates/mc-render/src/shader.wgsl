struct Globals {
    view_proj: mat4x4<f32>,
    camera: vec4<f32>,
    fog_color: vec4<f32>,
    // x = fog start, y = fog end, z = 1 when the render target is sRGB
    fog: vec4<f32>,
};


@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
@group(0) @binding(3) var lightmap: texture_2d<f32>;
@group(0) @binding(4) var lightmap_sampler: sampler;
// Per-section origins, indexed by instance index (= arena slot).
@group(1) @binding(0) var<storage, read> origins: array<vec4<f32>>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light_uv: vec2<f32>,
    @location(3) dist: f32,
    @location(4) @interpolate(flat) alpha_test: f32,
    @location(5) @interpolate(flat) translucent: f32,
};

@vertex
fn vs_main(
    @builtin(instance_index) slot: u32,
    @location(0) packed_pos: vec4<u32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) light_in: vec2<f32>,
) -> VsOut {
    let pos = vec3<f32>(packed_pos.xyz) / 2048.0 - 4.0;
    // Layer flag in color.a: 0 solid, 1 cutout, ~0.5 cutout-mipped, ~0.25 translucent.
    let flag = color.a;
    let alpha_test = select(select(0.0, 0.5, flag > 0.1), 1.0, flag > 0.9);
    let translucent = select(0.0, 1.0, abs(flag - 0.25) < 0.05);
    let light = light_in;
    // Camera-relative so far-out coordinates keep f32 precision.
    let rel = pos + origins[slot].xyz - globals.camera.xyz;
    var out: VsOut;
    out.clip = globals.view_proj * vec4<f32>(rel, 1.0);
    out.uv = uv;
    out.color = vec4<f32>(color.rgb, 1.0);
    // Vanilla lightmap texture matrix: (coord + 8) / 256, coord in 0..240.
    out.light_uv = (light * 255.0 + 8.0) / 256.0;
    out.dist = length(rel);
    out.alpha_test = alpha_test;
    out.translucent = translucent;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // CUTOUT samples level 0 only (setBlurMipmap(false, false)); everything else is mipmapped.
    var tex = textureSample(atlas, atlas_sampler, in.uv);
    if (in.alpha_test > 0.75) {
        tex = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    }
    // Vanilla alpha test (GL_GREATER 0.1) for every layer except SOLID.
    if (in.alpha_test > 0.25 && tex.a <= 0.1) {
        discard;
    }
    let light = textureSample(lightmap, lightmap_sampler, in.light_uv).rgb;
    // All in vanilla's raw color space, like the fixed-function pipeline.
    let base = tex.rgb * in.color.rgb * light;
    let f = clamp((in.dist - globals.fog.x) / (globals.fog.y - globals.fog.x), 0.0, 1.0);
    var rgb = mix(base, globals.fog_color.rgb, f);
    if (globals.fog.z > 0.5) {
        // The target re-encodes to sRGB on write; pre-decode so the stored value is the raw one.
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb, select(1.0,tex.a,in.translucent>0.5));
}
