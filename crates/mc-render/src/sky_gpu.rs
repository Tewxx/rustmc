use crate::{sky::Sky, Camera};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use mc_assets::AssetSource;
use wgpu::util::DeviceExt;

/// Uniform blocks written per frame: dome, void plane, sunrise fan, sun, moon, stars and the
/// two `renderCloudsFancy` passes (its no-colour depth prepass and the colour pass).
const MAX_DRAWS: usize = 8;
/// `renderCloudsFancy`'s 8x8 cell grid with top, bottom and four 8-segment sides per cell.
const MAX_CLOUD_QUADS: usize = 64 * (2 + 8 * 4);

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug, Pod, Zeroable)]
struct Vertex {
    /// Camera-relative model position; `RenderGlobal` draws the sky/cloud lists with the camera
    /// already at the origin.
    pos: [f32; 3],
    uv: [f32; 2],
    /// `(rgb, alpha)` multiplier on the draw colour: cloud faces store their 0.7/0.8/0.9/1.0
    /// shade here, the sunrise fan its centre/rim alpha.
    tint: [f32; 2],
}

impl Vertex {
    fn new(pos: [f32; 3], uv: [f32; 2], tint: [f32; 2]) -> Self {
        Self { pos, uv, tint }
    }
}

/// Per-draw constants that used to be baked into the vertex stream. The static sky, star and
/// cloud geometry only carries a model position, uv and tint; everything that changes per frame
/// (view-projection, celestial rotation, colours, fog and the cloud scroll) lives here.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Draw {
    /// Model to clip transform, including the camera view projection.
    transform: [[f32; 4]; 4],
    color: [f32; 4],
    /// `(fog start, fog end, fog density, 1 when fog applies)` of `EntityRenderer.setupFog`.
    fog: [f32; 4],
    /// Model translation applied before the transform and the fog distance.
    translate: [f32; 4],
    /// `(u, v, kind, unused)`; `kind` is 0 flat, 1 sun, 2 moon, 3 clouds and selects the
    /// sampled texture and whether the vertex shader pins depth to the far plane.
    uv_off: [f32; 4],
    fog_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Section {
    /// First element: an index when indexed, a vertex otherwise.
    first: u32,
    count: u32,
}

#[derive(Clone, Copy)]
struct DrawCall {
    pipeline: usize,
    cloud: bool,
    section: Section,
    uniforms: Draw,
}

/// [`Section`]s of the one-time sky vertex buffer, in `RenderGlobal.renderSky` order.
struct SkySections {
    dome: Section,
    void: Section,
    sunrise: Section,
    sun: Section,
    moon: Section,
    stars: Section,
}

/// `RenderGlobal`'s `glSkyList` / `glSkyList2` / sunrise fan / celestial quads / `starGLCallList`
/// in camera-relative model space. Their vertices never change, so they are built and uploaded
/// once; per-frame motion comes from [`Draw`].
fn sky_geometry(indexed: bool) -> (Mesh, SkySections) {
    let mut mesh = Mesh::new(indexed);
    let dome_start = mesh.start();
    for x in (-384..=384).step_by(64) {
        for z in (-384..=384).step_by(64) {
            mesh.quad([
                Vertex::new([x as f32, 16., z as f32], [0.; 2], [1.; 2]),
                Vertex::new([(x + 64) as f32, 16., z as f32], [0.; 2], [1.; 2]),
                Vertex::new([(x + 64) as f32, 16., (z + 64) as f32], [0.; 2], [1.; 2]),
                Vertex::new([x as f32, 16., (z + 64) as f32], [0.; 2], [1.; 2]),
            ]);
        }
    }
    let dome = mesh.section(dome_start);
    // `renderSky`'s void plane; the `63 - camera y` offset is the draw translation.
    let void_start = mesh.start();
    mesh.quad([
        Vertex::new([-384., 0., -384.], [0.; 2], [1.; 2]),
        Vertex::new([448., 0., -384.], [0.; 2], [1.; 2]),
        Vertex::new([448., 0., 448.], [0.; 2], [1.; 2]),
        Vertex::new([-384., 0., 448.], [0.; 2], [1.; 2]),
    ]);
    let void = mesh.section(void_start);
    // `calcSunriseSunsetColors` fan. `afloat[3]` scales the rim radius (draw transform) and the
    // centre alpha (draw colour).
    let sunrise_start = mesh.start();
    for i in 0..16 {
        let a = i as f32 * std::f32::consts::TAU / 16.;
        let b = (i + 1) as f32 * std::f32::consts::TAU / 16.;
        let points = [
            [0., 100., 0.],
            [a.sin() * 120., a.cos() * 120., -a.cos() * 40.],
            [b.sin() * 120., b.cos() * 120., -b.cos() * 40.],
        ];
        mesh.triangle(std::array::from_fn(|j| {
            Vertex::new(points[j], [0.; 2], [1., if j == 0 { 1. } else { 0. }])
        }));
    }
    let sunrise = mesh.section(sunrise_start);
    let sun_start = mesh.start();
    mesh.quad([
        Vertex::new([-30., 100., -30.], [0., 0.], [1.; 2]),
        Vertex::new([30., 100., -30.], [1., 0.], [1.; 2]),
        Vertex::new([30., 100., 30.], [1., 1.], [1.; 2]),
        Vertex::new([-30., 100., 30.], [0., 1.], [1.; 2]),
    ]);
    let sun = mesh.section(sun_start);
    // `moon_phases.png`; the phase offset is the draw's uv translation.
    let moon_start = mesh.start();
    mesh.quad([
        Vertex::new([-20., -100., 20.], [0.25, 0.5], [1.; 2]),
        Vertex::new([20., -100., 20.], [0., 0.5], [1.; 2]),
        Vertex::new([20., -100., -20.], [0., 0.], [1.; 2]),
        Vertex::new([-20., -100., -20.], [0.25, 0.], [1.; 2]),
    ]);
    let moon = mesh.section(moon_start);
    let stars_start = mesh.start();
    for &quad in star_quads() {
        mesh.quad(std::array::from_fn(|i| Vertex::new(quad[i], [0.; 2], [1.; 2])));
    }
    let stars = mesh.section(stars_start);
    (mesh, SkySections { dome, void, sunrise, sun, moon, stars })
}

/// `RenderGlobal.renderClouds` / `renderCloudsFancy` geometry in cell units. The moving sub-cell
/// offset (`fx`/`fz`), the scroll (`dx`/`dz`, `u`/`w`) and the cloud height are draw uniforms,
/// so the vertices only change when the visible face set does.
fn cloud_geometry(indexed: bool, fancy: bool, top: bool, bottom: bool) -> Mesh {
    let mut mesh = Mesh::new(indexed);
    if !fancy {
        for x in (-256..256).step_by(32) {
            for z in (-256..256).step_by(32) {
                let (x, z) = (x as f32, z as f32);
                mesh.quad([
                    Vertex::new([x, 0., z + 32.], [x / 2048., (z + 32.) / 2048.], [1.; 2]),
                    Vertex::new([x + 32., 0., z + 32.], [(x + 32.) / 2048., (z + 32.) / 2048.], [1.; 2]),
                    Vertex::new([x + 32., 0., z], [(x + 32.) / 2048., z / 2048.], [1.; 2]),
                    Vertex::new([x, 0., z], [x / 2048., z / 2048.], [1.; 2]),
                ]);
            }
        }
        return mesh;
    }
    let bottom_y = 4. - 0.0009765625;
    let face = |mesh: &mut Mesh, points: [[f32; 3]; 4], uv: [[f32; 2]; 4], shade: f32| {
        mesh.quad(std::array::from_fn(|i| {
            Vertex::new(
                [points[i][0] * 12., points[i][1], points[i][2] * 12.],
                uv[i],
                [shade, 1.],
            )
        }));
    };
    for j in -3..=4 {
        for k in -3..=4 {
            let (x, z) = ((j * 8) as f32, (k * 8) as f32);
            let uv = [
                [x / 256., (z + 8.) / 256.],
                [(x + 8.) / 256., (z + 8.) / 256.],
                [(x + 8.) / 256., z / 256.],
                [x / 256., z / 256.],
            ];
            if top {
                face(&mut mesh, [[x, 0., z + 8.], [x + 8., 0., z + 8.], [x + 8., 0., z], [x, 0., z]], uv, 0.7);
            }
            if bottom {
                face(
                    &mut mesh,
                    [[x, bottom_y, z + 8.], [x + 8., bottom_y, z + 8.], [x + 8., bottom_y, z], [x, bottom_y, z]],
                    uv,
                    1.,
                );
            }
            for n in 0..8 {
                let n = n as f32;
                if j > -1 {
                    let a = x + n;
                    let t = (x + n + 0.5) / 256.;
                    face(
                        &mut mesh,
                        [[a, 0., z + 8.], [a, 4., z + 8.], [a, 4., z], [a, 0., z]],
                        [[t, (z + 8.) / 256.], [t, (z + 8.) / 256.], [t, z / 256.], [t, z / 256.]],
                        0.9,
                    );
                }
                if j <= 1 {
                    let a = x + n + 1. - 0.0009765625;
                    let t = (x + n + 0.5) / 256.;
                    face(
                        &mut mesh,
                        [[a, 0., z + 8.], [a, 4., z + 8.], [a, 4., z], [a, 0., z]],
                        [[t, (z + 8.) / 256.], [t, (z + 8.) / 256.], [t, z / 256.], [t, z / 256.]],
                        0.9,
                    );
                }
                if k > -1 {
                    let a = z + n;
                    let t = (z + n + 0.5) / 256.;
                    face(
                        &mut mesh,
                        [[x, 4., a], [x + 8., 4., a], [x + 8., 0., a], [x, 0., a]],
                        [[x / 256., t], [(x + 8.) / 256., t], [(x + 8.) / 256., t], [x / 256., t]],
                        0.8,
                    );
                }
                if k <= 1 {
                    let a = z + n + 1. - 0.0009765625;
                    let t = (z + n + 0.5) / 256.;
                    face(
                        &mut mesh,
                        [[x, 4., a], [x + 8., 4., a], [x + 8., 0., a], [x, 0., a]],
                        [[x / 256., t], [(x + 8.) / 256., t], [(x + 8.) / 256., t], [x / 256., t]],
                        0.8,
                    );
                }
            }
        }
    }
    mesh
}

/// Small indexed/expanded mesh builder shared by the static sky and the cloud cache.
struct Mesh {
    indexed: bool,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    elements: u32,
}

impl Mesh {
    fn new(indexed: bool) -> Self {
        Self { indexed, vertices: Vec::new(), indices: Vec::new(), elements: 0 }
    }
    fn start(&self) -> u32 {
        self.elements
    }
    fn quad(&mut self, vertices: [Vertex; 4]) {
        if self.indexed {
            let base = self.vertices.len() as u32;
            self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            self.vertices.extend_from_slice(&vertices);
        } else {
            self.vertices.extend([vertices[0], vertices[1], vertices[2], vertices[0], vertices[2], vertices[3]]);
        }
        self.elements += 6;
    }
    fn triangle(&mut self, vertices: [Vertex; 3]) {
        if self.indexed {
            let base = self.vertices.len() as u32;
            self.indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
        self.vertices.extend_from_slice(&vertices);
        self.elements += 3;
    }
    fn section(&self, start: u32) -> Section {
        Section { first: start, count: self.elements - start }
    }
}

/// The per-frame draw list of `RenderGlobal.renderSky` and `renderClouds` in submission order.
/// Colours, transforms and fog used to be baked into the vertices; they are now the uniforms of
/// each draw, which leaves the static geometry untouched between frames.
fn frame_draws(
    cam: &Camera,
    aspect: f32,
    distance: f32,
    sky: &Sky,
    sections: &SkySections,
    cloud: Section,
) -> ([Option<DrawCall>; MAX_DRAWS], usize) {
    let vp = cam.view_proj(aspect, distance);
    let fog_color = [sky.fog[0], sky.fog[1], sky.fog[2], 1.];
    let draw = |transform: Mat4, color: [f32; 4], fog: [f32; 4], translate: [f32; 4], uv: [f32; 4]| Draw {
        transform: transform.to_cols_array_2d(),
        color,
        fog,
        translate,
        uv_off: uv,
        fog_color,
    };
    let mut calls: [Option<DrawCall>; MAX_DRAWS] = [None; MAX_DRAWS];
    let mut count = 0;
    let mut push = |pipeline: usize, cloud: bool, section: Section, uniforms: Draw| {
        calls[count] = Some(DrawCall { pipeline, cloud, section, uniforms });
        count += 1;
    };
    let sky_fog = [0., distance, sky.fog_density, 1.];
    push(0, false, sections.dome, draw(vp, [sky.sky[0], sky.sky[1], sky.sky[2], 1.], sky_fog, [0.; 4], [0., 0., 0., 0.]));
    let bottom = [sky.sky[0] * 0.2 + 0.04, sky.sky[1] * 0.2 + 0.04, sky.sky[2] * 0.6 + 0.1];
    push(
        0,
        false,
        sections.void,
        draw(vp, [bottom[0], bottom[1], bottom[2], 1.], sky_fog, [0., 63. - cam.pos.y, 0., 0.], [0., 0., 0., 0.]),
    );
    if let Some(c) = sky.sunrise {
        let transform = vp
            * Mat4::from_rotation_x(90f32.to_radians())
            * Mat4::from_rotation_z(if mc_core::math::sin(sky.angle * std::f32::consts::TAU) < 0. {
                std::f32::consts::PI
            } else {
                0.
            })
            * Mat4::from_rotation_z(90f32.to_radians())
            * Mat4::from_scale(Vec3::new(1., 1., c[3]));
        push(0, false, sections.sunrise, draw(transform, c, [0.; 4], [0.; 4], [0., 0., 0., 0.]));
    }
    let celestial = vp * Mat4::from_rotation_y(-90f32.to_radians()) * Mat4::from_rotation_x(sky.angle * std::f32::consts::TAU);
    let visibility = [1., 1., 1., sky.celestial_visibility];
    push(1, false, sections.sun, draw(celestial, visibility, [0.; 4], [0.; 4], [0., 0., 1., 0.]));
    let (u, w) = ((sky.moon % 4) as f32 / 4., (sky.moon / 4 % 2) as f32 / 2.);
    push(1, false, sections.moon, draw(celestial, visibility, [0.; 4], [0.; 4], [u, w, 2., 0.]));
    if sky.stars > 0. {
        push(1, false, sections.stars, draw(celestial, [sky.stars; 4], [0.; 4], [0.; 4], [0., 0., 0., 0.]));
    }
    if sky.clouds {
        let height = 128. - cam.pos.y + 0.33;
        let (uv, translate) = if sky.fancy_clouds {
            let dx = ((cam.pos.x as f64 + sky.ticks as f64 * 0.029999999329447746) / 12.).rem_euclid(2048.);
            let dz = (cam.pos.z as f64 / 12. + 0.33000001311302185).rem_euclid(2048.);
            let (fx, fz) = ((dx - dx.floor()) as f32, (dz - dz.floor()) as f32);
            ([dx.floor() as f32 / 256., dz.floor() as f32 / 256., 3., 0.], [-fx * 12., height, -fz * 12., 0.])
        } else {
            let dx = (cam.pos.x as f64 + sky.ticks as f64 * 0.029999999329447746).rem_euclid(2048.) as f32 / 2048.;
            let dz = (cam.pos.z as f64).rem_euclid(2048.) as f32 / 2048.;
            ([dx, dz, 3., 0.], [0., height, 0., 0.])
        };
        let uniforms = draw(vp, [sky.cloud[0], sky.cloud[1], sky.cloud[2], 0.8], [distance * 0.75, distance, sky.fog_density, 1.], translate, uv);
        // `renderCloudsFancy` draws every face twice: once with the colour mask off to seed
        // depth, then the colour pass, so overlapping faces only blend the nearest one.
        push(2, true, cloud, uniforms);
        push(3, true, cloud, uniforms);
    }
    drop(push);
    (calls, count)
}

pub struct SkyRenderer {
    pipelines: Vec<wgpu::RenderPipeline>,
    bg: wgpu::BindGroup,
    sky_vertices: wgpu::Buffer,
    sky_indices: wgpu::Buffer,
    sections: SkySections,
    cloud_vertices: wgpu::Buffer,
    cloud_indices: wgpu::Buffer,
    cloud_topology: Option<u8>,
    cloud_elements: u32,
    uniforms: wgpu::Buffer,
    uniform_slots: Vec<u8>,
    uniform_stride: usize,
    indexed: bool,
}
impl SkyRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, assets: &AssetSource) -> Self {
        Self::with_indexing(device, queue, format, assets, std::env::var_os("RUSTMC_NO_SKY_INDEXING").is_none())
    }
    fn with_indexing(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        assets: &AssetSource,
        indexed: bool,
    ) -> Self {
        let align = (device.limits().min_uniform_buffer_offset_alignment as usize).max(16);
        let stride = std::mem::size_of::<Draw>().div_ceil(align) * align;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky textures"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                tex_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Draw>() as u64),
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let load = |path: &str| {
            let img = image::load_from_memory(&assets.read(path).expect(path)).unwrap().to_rgba8();
            device
                .create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: Some(path),
                        size: wgpu::Extent3d {
                            width: img.width(),
                            height: img.height(),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &img,
                )
                .create_view(&Default::default())
        };
        let sun = load("textures/environment/sun.png");
        let moon = load("textures/environment/moon_phases.png");
        let cloud = load("textures/environment/clouds.png");
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky draws"),
            size: (MAX_DRAWS * stride) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&sun),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&moon),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&cloud),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Draw>() as u64),
                    }),
                },
            ],
        });
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("sky shader"),source:wgpu::ShaderSource::Wgsl(format!(r#"
const SRGB:bool={};
@group(0) @binding(0) var samp:sampler;
@group(0) @binding(1) var sun:texture_2d<f32>;
@group(0) @binding(2) var moon:texture_2d<f32>;
@group(0) @binding(3) var clouds:texture_2d<f32>;
struct Draw {{transform:mat4x4<f32>,color:vec4<f32>,fog:vec4<f32>,translate:vec4<f32>,uv_off:vec4<f32>,fog_color:vec4<f32>}};
@group(0) @binding(4) var<uniform> draw:Draw;
struct V {{@builtin(position) pos:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>,@location(2) fog:f32}};
@vertex fn vs(@location(0) p:vec3<f32>,@location(1) uv:vec2<f32>,@location(2) tint:vec2<f32>)->V {{var v:V;let p3=p+draw.translate.xyz;var clip=draw.transform*vec4<f32>(p3,1.0);if(draw.uv_off.z<3.0){{clip.z=clip.w;}}v.pos=clip;v.uv=uv+draw.uv_off.xy;v.color=vec4<f32>(draw.color.rgb*tint.x,draw.color.a*tint.y);let d=length(p3);var f=0.0;if(draw.fog.w>0.5){{if(draw.fog.z!=0.0){{let dd=d*draw.fog.z;f=1.0-exp(-dd);}}else{{f=clamp((d-draw.fog.x)/(draw.fog.y-draw.fog.x),0.0,1.0);}}}}v.fog=f;return v;}}
@fragment fn fs(v:V)->@location(0) vec4<f32> {{var c=v.color;if(draw.uv_off.z==1.0){{c=c*textureSample(sun,samp,v.uv);}}if(draw.uv_off.z==2.0){{c=c*textureSample(moon,samp,v.uv);}}if(draw.uv_off.z==3.0){{c=c*textureSample(clouds,samp,v.uv);if(c.a<=0.1){{discard;}}}}c=vec4<f32>(mix(c.rgb,draw.fog_color.rgb,clamp(v.fog,0.,1.)),c.a);if(SRGB){{c=vec4<f32>(select(c.rgb/12.92,pow((c.rgb+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c.rgb>vec3<f32>(0.04045)),c.a);}}return c;}}
"#,format.is_srgb()).into())});
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipelines = (0..4)
            .map(|kind| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("sky/cloud"),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x2],
                        }],
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: kind == 2,
                        depth_compare: wgpu::CompareFunction::LessEqual,
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(if kind == 1 {
                                wgpu::BlendState {
                                    color: wgpu::BlendComponent {
                                        src_factor: wgpu::BlendFactor::SrcAlpha,
                                        dst_factor: wgpu::BlendFactor::One,
                                        operation: wgpu::BlendOperation::Add,
                                    },
                                    alpha: wgpu::BlendComponent::REPLACE,
                                }
                            } else {
                                wgpu::BlendState::ALPHA_BLENDING
                            }),
                            write_mask: if kind == 2 {
                                wgpu::ColorWrites::empty()
                            } else {
                                wgpu::ColorWrites::ALL
                            },
                        })],
                    }),
                    multiview: None,
                    cache: None,
                })
            })
            .collect();
        // Static sky, stars and void plane: built and uploaded once.
        let (sky, sections) = sky_geometry(indexed);
        let sky_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sky/cloud vertices"),
            contents: bytemuck::cast_slice(&sky.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let placeholder = [0u8; 4];
        let sky_indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sky/cloud indices"),
            contents: if indexed { bytemuck::cast_slice(&sky.indices) } else { &placeholder },
            usage: wgpu::BufferUsages::INDEX,
        });
        // `renderCloudsFancy`'s face set changes with the camera height band; sized for the
        // densest grid so a rebuild only rewrites the occupied prefix.
        let cloud_vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cloud vertices"),
            size: (MAX_CLOUD_QUADS * 6 * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cloud_indices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cloud indices"),
            size: (MAX_CLOUD_QUADS * 6 * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipelines,
            bg,
            sky_vertices,
            sky_indices,
            sections,
            cloud_vertices,
            cloud_indices,
            cloud_topology: None,
            cloud_elements: 0,
            uniforms,
            uniform_slots: vec![0; MAX_DRAWS * stride],
            uniform_stride: stride,
            indexed,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        cam: &Camera,
        aspect: f32,
        distance: f32,
        sky: &Sky,
    ) {
        let height = 128. - cam.pos.y + 0.33;
        // Only the camera height band changes which `renderCloudsFancy` faces exist; the scroll
        // offset, sub-cell translation and cloud height are per-draw uniforms.
        let topology = sky.fancy_clouds as u8
            | ((sky.fancy_clouds && height > -5.) as u8) << 1
            | ((sky.fancy_clouds && height <= 5.) as u8) << 2;
        if sky.clouds && self.cloud_topology != Some(topology) {
            let mesh = cloud_geometry(self.indexed, sky.fancy_clouds, height > -5., height <= 5.);
            if self.indexed {
                queue.write_buffer(&self.cloud_indices, 0, bytemuck::cast_slice(&mesh.indices));
            }
            queue.write_buffer(&self.cloud_vertices, 0, bytemuck::cast_slice(&mesh.vertices));
            self.cloud_topology = Some(topology);
            self.cloud_elements = mesh.elements;
        }
        let (calls, count) = frame_draws(
            cam,
            aspect,
            distance,
            sky,
            &self.sections,
            Section { first: 0, count: self.cloud_elements },
        );
        for (slot, call) in self.uniform_slots.chunks_exact_mut(self.uniform_stride).zip(calls[..count].iter()) {
            slot[..std::mem::size_of::<Draw>()].copy_from_slice(bytemuck::bytes_of(&call.unwrap().uniforms));
        }
        queue.write_buffer(&self.uniforms, 0, &self.uniform_slots[..count * self.uniform_stride]);
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sky and clouds"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_vertex_buffer(0, self.sky_vertices.slice(..));
        if self.indexed {
            pass.set_index_buffer(self.sky_indices.slice(..), wgpu::IndexFormat::Uint32);
        }
        let mut cloud_buffer = false;
        for (i, call) in calls[..count].iter().enumerate() {
            let call = call.unwrap();
            if call.cloud != cloud_buffer {
                cloud_buffer = call.cloud;
                let (vertices, indices) = if cloud_buffer {
                    (&self.cloud_vertices, &self.cloud_indices)
                } else {
                    (&self.sky_vertices, &self.sky_indices)
                };
                pass.set_vertex_buffer(0, vertices.slice(..));
                if self.indexed {
                    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                }
            }
            pass.set_pipeline(&self.pipelines[call.pipeline]);
            pass.set_bind_group(0, &self.bg, &[(i * self.uniform_stride) as u32]);
            let range = call.section.first..call.section.first + call.section.count;
            if self.indexed {
                pass.draw_indexed(range, 0, 0..1);
            } else {
                pass.draw(range, 0..1);
            }
        }
    }
}
/// Static star field generated from RenderGlobal's seeded random sequence.
fn star_quads() -> &'static Vec<[[f32; 3]; 4]> {
    static STARS: std::sync::OnceLock<Vec<[[f32; 3]; 4]>> = std::sync::OnceLock::new();
    STARS.get_or_init(|| {
        let mut random = mc_core::random::JavaRandom::new(10842);
        let mut stars = Vec::new();
        for _ in 0..1500 {
            let mut direction = [0.; 3];
            for d in &mut direction { *d = (random.next_float() * 2. - 1.) as f64; }
            let size = (0.15_f32 + random.next_float() * 0.1) as f64;
            let length = direction.iter().map(|d| d * d).sum::<f64>();
            if !(0.01..1.).contains(&length) { continue; }
            let scale = length.sqrt().recip();
            direction.iter_mut().for_each(|d| *d *= scale);
            let [x, y, z] = direction;
            let azimuth = x.atan2(z);
            let inclination = (x * x + z * z).sqrt().atan2(y);
            let roll = random.next_double() * std::f64::consts::TAU;
            let (sa, ca) = azimuth.sin_cos();
            let (si, ci) = inclination.sin_cos();
            let (sr, cr) = roll.sin_cos();
            let mut quad = [[0.; 3]; 4];
            for (j, point) in quad.iter_mut().enumerate() {
                let a = ((j & 2) as i32 - 1) as f64 * size;
                let b = (((j + 1) & 2) as i32 - 1) as f64 * size;
                let u = a * cr - b * sr;
                let v = b * cr + a * sr;
                let w = -u * ci;
                *point = [(x * 100. + w * sa - v * ca) as f32, (y * 100. + u * si) as f32, (z * 100. + v * sa + w * ca) as f32];
            }
            stars.push(quad);
        }
        stars
    })
}
fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    struct Resolved {
        clip: [f32; 4],
        uv: [f32; 2],
        color: [f32; 4],
        fog: f32,
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol * (1. + b.abs())
    }

    fn compare(actual: &[Resolved], expected: &[Resolved], tol: f32, label: &str) {
        assert_eq!(actual.len(), expected.len(), "{label}: vertex count");
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            for k in 0..4 {
                assert!(close(a.clip[k], e.clip[k], tol), "{label} vertex {i} clip[{k}]: {} vs {}", a.clip[k], e.clip[k]);
            }
            for k in 0..2 {
                assert!(close(a.uv[k], e.uv[k], tol), "{label} vertex {i} uv[{k}]: {} vs {}", a.uv[k], e.uv[k]);
            }
            for k in 0..4 {
                assert!(close(a.color[k], e.color[k], tol), "{label} vertex {i} color[{k}]: {} vs {}", a.color[k], e.color[k]);
            }
            assert!(close(a.fog, e.fog, tol), "{label} vertex {i} fog: {} vs {}", a.fog, e.fog);
        }
    }

    /// Evaluates the static model-space vertex plus its draw uniforms exactly like the shader.
    fn resolve(mesh: &Mesh, call: &DrawCall) -> Vec<Resolved> {
        let d = &call.uniforms;
        let first = call.section.first as usize;
        let count = call.section.count as usize;
        let vertices: Vec<Vertex> = if mesh.indexed {
            mesh.indices[first..first + count].iter().map(|&i| mesh.vertices[i as usize]).collect()
        } else {
            mesh.vertices[first..first + count].to_vec()
        };
        let m = Mat4::from_cols_array_2d(&d.transform);
        vertices
            .into_iter()
            .map(|v| {
                let p = Vec3::from(v.pos) + Vec3::new(d.translate[0], d.translate[1], d.translate[2]);
                let mut clip = (m * p.extend(1.)).to_array();
                if d.uv_off[2] < 3. {
                    clip[2] = clip[3];
                }
                let len = p.length();
                let fog = if d.fog[3] > 0.5 {
                    if d.fog[2] != 0. {
                        let dd = len * d.fog[2];
                        1. - (-dd).exp()
                    } else {
                        ((len - d.fog[0]) / (d.fog[1] - d.fog[0])).clamp(0., 1.)
                    }
                } else {
                    0.
                };
                Resolved {
                    clip,
                    uv: [v.uv[0] + d.uv_off[0], v.uv[1] + d.uv_off[1]],
                    color: [
                        d.color[0] * v.tint[0],
                        d.color[1] * v.tint[0],
                        d.color[2] * v.tint[0],
                        d.color[3] * v.tint[1],
                    ],
                    fog,
                }
            })
            .collect()
    }

    fn expand(mesh: &Mesh) -> Vec<Vertex> {
        if mesh.indexed {
            mesh.indices.iter().map(|&i| mesh.vertices[i as usize]).collect()
        } else {
            mesh.vertices.clone()
        }
    }

    /// The pre-optimisation `clouds` push loop, kept as the parity reference.
    fn legacy_clouds(out: &mut Vec<Resolved>, vp: Mat4, pos: Vec3, sky: &Sky, distance: f32) {
        let height = 128. - pos.y + 0.33;
        let push = |out: &mut Vec<Resolved>, p: [[f32; 3]; 4], uv: [[f32; 2]; 4], color: [f32; 4], fog: Option<(f32, f32, f32)>| {
            for i in [0, 1, 2, 0, 2, 3] {
                let q = Vec3::from(p[i]);
                let f = fog.map_or(0., |(start, end, density)| {
                    if density != 0. {
                        let d = q.length() * density;
                        1. - (-d).exp()
                    } else {
                        ((q.length() - start) / (end - start)).clamp(0., 1.)
                    }
                });
                let clip = (vp * q.extend(1.)).to_array();
                out.push(Resolved { clip, uv: uv[i], color, fog: f });
            }
        };
        if !sky.fancy_clouds {
            let dx = (pos.x as f64 + sky.ticks as f64 * 0.029999999329447746).rem_euclid(2048.) as f32 / 2048.;
            let dz = (pos.z as f64).rem_euclid(2048.) as f32 / 2048.;
            for x in (-256..256).step_by(32) {
                for z in (-256..256).step_by(32) {
                    let (x, z) = (x as f32, z as f32);
                    push(
                        out,
                        [[x, height, z + 32.], [x + 32., height, z + 32.], [x + 32., height, z], [x, height, z]],
                        [
                            [x / 2048. + dx, (z + 32.) / 2048. + dz],
                            [(x + 32.) / 2048. + dx, (z + 32.) / 2048. + dz],
                            [(x + 32.) / 2048. + dx, z / 2048. + dz],
                            [x / 2048. + dx, z / 2048. + dz],
                        ],
                        [sky.cloud[0], sky.cloud[1], sky.cloud[2], 0.8],
                        Some((distance * 0.75, distance, sky.fog_density)),
                    );
                }
            }
            return;
        }
        let dx = ((pos.x as f64 + sky.ticks as f64 * 0.029999999329447746) / 12.).rem_euclid(2048.);
        let dz = (pos.z as f64 / 12. + 0.33000001311302185).rem_euclid(2048.);
        let (u, w) = (dx.floor() as f32 / 256., dz.floor() as f32 / 256.);
        let (fx, fz) = ((dx - dx.floor()) as f32, (dz - dz.floor()) as f32);
        let face = |out: &mut Vec<Resolved>, points: [[f32; 3]; 4], uv: [[f32; 2]; 4], shade: f32| {
            let p = points.map(|p| [p[0] * 12., p[1], p[2] * 12.]);
            push(
                out,
                p,
                uv,
                [sky.cloud[0] * shade, sky.cloud[1] * shade, sky.cloud[2] * shade, 0.8],
                Some((distance * 0.75, distance, sky.fog_density)),
            );
        };
        for j in -3..=4 {
            for k in -3..=4 {
                let (x, z) = ((j * 8) as f32, (k * 8) as f32);
                let (px, pz) = (x - fx, z - fz);
                let uv = [
                    [x / 256. + u, (z + 8.) / 256. + w],
                    [(x + 8.) / 256. + u, (z + 8.) / 256. + w],
                    [(x + 8.) / 256. + u, z / 256. + w],
                    [x / 256. + u, z / 256. + w],
                ];
                if height > -5. {
                    face(
                        out,
                        [[px, height, pz + 8.], [px + 8., height, pz + 8.], [px + 8., height, pz], [px, height, pz]],
                        uv,
                        0.7,
                    );
                }
                if height <= 5. {
                    let y = height + 4. - 0.0009765625;
                    face(out, [[px, y, pz + 8.], [px + 8., y, pz + 8.], [px + 8., y, pz], [px, y, pz]], uv, 1.);
                }
                for n in 0..8 {
                    let n = n as f32;
                    if j > -1 {
                        let a = px + n;
                        let t = (x + n + 0.5) / 256. + u;
                        face(
                            out,
                            [[a, height, pz + 8.], [a, height + 4., pz + 8.], [a, height + 4., pz], [a, height, pz]],
                            [
                                [t, (z + 8.) / 256. + w],
                                [t, (z + 8.) / 256. + w],
                                [t, z / 256. + w],
                                [t, z / 256. + w],
                            ],
                            0.9,
                        );
                    }
                    if j <= 1 {
                        let a = px + n + 1. - 0.0009765625;
                        let t = (x + n + 0.5) / 256. + u;
                        face(
                            out,
                            [[a, height, pz + 8.], [a, height + 4., pz + 8.], [a, height + 4., pz], [a, height, pz]],
                            [
                                [t, (z + 8.) / 256. + w],
                                [t, (z + 8.) / 256. + w],
                                [t, z / 256. + w],
                                [t, z / 256. + w],
                            ],
                            0.9,
                        );
                    }
                    if k > -1 {
                        let a = pz + n;
                        let t = (z + n + 0.5) / 256. + w;
                        face(
                            out,
                            [[px, height + 4., a], [px + 8., height + 4., a], [px + 8., height, a], [px, height, a]],
                            [
                                [x / 256. + u, t],
                                [(x + 8.) / 256. + u, t],
                                [(x + 8.) / 256. + u, t],
                                [x / 256. + u, t],
                            ],
                            0.8,
                        );
                    }
                    if k <= 1 {
                        let a = pz + n + 1. - 0.0009765625;
                        let t = (z + n + 0.5) / 256. + w;
                        face(
                            out,
                            [[px, height + 4., a], [px + 8., height + 4., a], [px + 8., height, a], [px, height, a]],
                            [
                                [x / 256. + u, t],
                                [(x + 8.) / 256. + u, t],
                                [(x + 8.) / 256. + u, t],
                                [x / 256. + u, t],
                            ],
                            0.8,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn indexed_geometry_expands_to_the_same_vertices() {
        let (indexed, sections) = sky_geometry(true);
        let (expanded, _) = sky_geometry(false);
        assert_eq!(expand(&indexed), expanded.vertices);
        assert_eq!(sections.dome.count, 169 * 6);
        assert_eq!(sections.sunrise.count, 16 * 3);
        for fancy in [false, true] {
            for top in [false, true] {
                for bottom in [false, true] {
                    let indexed = cloud_geometry(true, fancy, top, bottom);
                    let expanded = cloud_geometry(false, fancy, top, bottom);
                    assert_eq!(expand(&indexed), expanded.vertices, "fancy={fancy} top={top} bottom={bottom}");
                    assert!(indexed.vertices.len() <= MAX_CLOUD_QUADS * 4);
                    assert!(expanded.vertices.len() <= MAX_CLOUD_QUADS * 6);
                }
            }
        }
    }

    #[test]
    fn cloud_geometry_matches_legacy_builder() {
        let (_, sections) = sky_geometry(true);
        for (time, x, y, z, fancy) in [
            (6000, 12.3, 80.2, -47.9, true),
            (18000, -300.7, 130.5, 500.1, true),
            (23000, 0.5, 140., 0.5, true),
            (0, 0.5, 60., 0.5, true),
            (6000, 7.25, 80., -3.5, false),
            (18000, -1024.3, 120., 2047.9, false),
        ] {
            let camera = Camera { pos: Vec3::new(x, y, z), yaw: 37., pitch: -20., fov_deg: 70., bob: Mat4::IDENTITY };
            let mut sky = Sky::overworld(time, 0.5, 0.8, 8);
            sky.fancy_clouds = fancy;
            let height = 128. - camera.pos.y + 0.33;
            let mesh = cloud_geometry(true, fancy, height > -5., height <= 5.);
            let (calls, n) = frame_draws(&camera, 1.6, 128., &sky, &sections, Section { first: 0, count: mesh.elements });
            let call = calls[..n].iter().flatten().find(|c| c.cloud).unwrap();
            let actual = resolve(&mesh, call);
            let mut expected = Vec::new();
            legacy_clouds(&mut expected, camera.view_proj(1.6, 128.), camera.pos, &sky, 128.);
            compare(&actual, &expected, 1e-4, "clouds");
        }
    }

    #[test]
    fn draw_order_and_constants_follow_render_sky() {
        let (_, sections) = sky_geometry(true);
        let camera = Camera { pos: Vec3::new(0., 80., 0.), yaw: 0., pitch: 0., fov_deg: 70., bob: Mat4::IDENTITY };
        let distance = 128.;
        let cloud = Section { first: 0, count: 6 };
        for time in [0, 6000, 18000, 23000] {
            let sky = Sky::overworld(time, 0.5, 0.8, 8);
            let (calls, n) = frame_draws(&camera, 1., distance, &sky, &sections, cloud);
            let calls: Vec<_> = calls[..n].iter().flatten().collect();
            let mut expected = vec![0, 0];
            if sky.sunrise.is_some() {
                expected.push(0);
            }
            expected.extend([1, 1]);
            if sky.stars > 0. {
                expected.push(1);
            }
            if sky.clouds {
                expected.extend([2, 3]);
            }
            assert_eq!(calls.iter().map(|c| c.pipeline).collect::<Vec<_>>(), expected, "time={time}");
        }

        let sky = Sky::overworld(23000, 0.5, 0.8, 8);
        assert!(sky.sunrise.is_some() && sky.stars > 0.);
        let (calls, n) = frame_draws(&camera, 1., distance, &sky, &sections, cloud);
        let calls: Vec<_> = calls[..n].iter().flatten().collect();
        assert_eq!(calls.len(), MAX_DRAWS);
        let vp = camera.view_proj(1., distance);
        let d = &calls[0].uniforms;
        assert_eq!((calls[0].section.first, calls[0].section.count), (sections.dome.first, sections.dome.count));
        assert_eq!(d.transform, vp.to_cols_array_2d());
        assert_eq!(d.color, [sky.sky[0], sky.sky[1], sky.sky[2], 1.]);
        assert_eq!(d.fog, [0., distance, sky.fog_density, 1.]);
        assert_eq!(d.translate, [0.; 4]);
        let dome = resolve(&sky_geometry(true).0, calls[0]);
        let mut clip = (vp * Vec3::new(-384., 16., -384.).extend(1.)).to_array();
        clip[2] = clip[3];
        compare(&dome[..1], &[Resolved { clip, uv: [0.; 2], color: d.color, fog: dome[0].fog }], 1e-6, "dome");
        let bottom = [sky.sky[0] * 0.2 + 0.04, sky.sky[1] * 0.2 + 0.04, sky.sky[2] * 0.6 + 0.1];
        assert_eq!(calls[1].uniforms.color, [bottom[0], bottom[1], bottom[2], 1.]);
        assert_eq!(calls[1].uniforms.translate, [0., 63. - camera.pos.y, 0., 0.]);
        let c = sky.sunrise.unwrap();
        assert_eq!(calls[2].uniforms.color, c);
        assert_eq!(calls[2].uniforms.uv_off[2], 0.);
        let sunrise = resolve(&sky_geometry(true).0, calls[2]);
        assert_eq!(sunrise[0].color, [c[0], c[1], c[2], c[3]]);
        assert_eq!(sunrise[1].color, [c[0], c[1], c[2], 0.]);
        // The fan's rim radius is still `afloat[3] * 40`; the scale now lives in the transform.
        let m = vp
            * Mat4::from_rotation_x(90f32.to_radians())
            * Mat4::from_rotation_z(if mc_core::math::sin(sky.angle * std::f32::consts::TAU) < 0. {
                std::f32::consts::PI
            } else {
                0.
            })
            * Mat4::from_rotation_z(90f32.to_radians());
        let mut rim = (m * Vec3::new(0., 120., -40. * c[3]).extend(1.)).to_array();
        rim[2] = rim[3];
        compare(&sunrise[1..2], &[Resolved { clip: rim, uv: [0.; 2], color: [c[0], c[1], c[2], 0.], fog: 0. }], 1e-4, "sunrise rim");
        let celestial = vp * Mat4::from_rotation_y(-90f32.to_radians()) * Mat4::from_rotation_x(sky.angle * std::f32::consts::TAU);
        assert_eq!(calls[3].uniforms.transform, celestial.to_cols_array_2d());
        assert_eq!(calls[3].uniforms.uv_off, [0., 0., 1., 0.]);
        let (u, w) = ((sky.moon % 4) as f32 / 4., (sky.moon / 4 % 2) as f32 / 2.);
        assert_eq!(calls[4].uniforms.uv_off, [u, w, 2., 0.]);
        assert_eq!(calls[5].uniforms.color, [sky.stars; 4]);
        assert_eq!(calls[5].uniforms.uv_off[2], 0.);
        assert_eq!(calls[6].uniforms.color, [sky.cloud[0], sky.cloud[1], sky.cloud[2], 0.8]);
        assert!(calls[6].cloud && calls[7].cloud);
        assert_eq!(calls[6].uniforms.transform, calls[7].uniforms.transform);
    }

    #[test]
    fn star_vertices_match_java() {
        let expected: Vec<[i32; 3]> = serde_json::from_str(include_str!("../tests/data/stars.json")).unwrap();
        let actual: Vec<_> = star_quads().iter().flatten().collect();
        assert_eq!(actual.len(), expected.len());
        for (index, (a, e)) in actual.iter().zip(expected).enumerate() {
            for axis in 0..3 {
                assert_eq!(a[axis].to_bits(), e[axis] as u32, "vertex {index} axis {axis}");
            }
        }
    }

    #[test]
    fn indexed_sky_matches_legacy_pixels_headlessly() {
        let Some(jar) = crate::gpu_test_jar() else { return };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = crate::headless_adapter(&instance) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let mut assets = AssetSource::new(); assets.push(&jar).unwrap();
        let mut indexed = SkyRenderer::with_indexing(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &assets, true);
        let mut expanded = SkyRenderer::with_indexing(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &assets, false);
        let texture = |format, usage| device.create_texture(&wgpu::TextureDescriptor {
            label: None, size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format, usage, view_formats: &[],
        });
        let color = texture(wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let depth = texture(wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let view = color.create_view(&Default::default()); let depth_view = depth.create_view(&Default::default());
        let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 64 * 64 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let render = |renderer: &mut SkyRenderer, camera: &Camera, sky: &Sky| {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                    timestamp_writes: None, occlusion_query_set: None });
            }
            renderer.encode(&queue, &mut encoder, &view, &depth_view, camera, 1., 128., sky);
            encoder.copy_texture_to_buffer(color.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(64) } },
                wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 });
            queue.submit(Some(encoder.finish()));
            let slice = read.slice(..); let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(wgpu::Maintain::Wait); rx.recv().unwrap().unwrap();
            let pixels = slice.get_mapped_range().to_vec(); read.unmap(); pixels
        };
        let mut saw_sunrise = false;
        for (time, height, fancy, clouds) in [(6000, 80., true, true), (18000, 80., true, true),
            (23000, 128., true, true), (0, 134., true, true), (18000, 122., true, true),
            (6000, 80., false, true), (18000, 80., true, false)] {
            let camera = Camera { pos: Vec3::new(0., height, 0.), yaw: 0., pitch: -35., fov_deg: 70., bob: Mat4::IDENTITY };
            let mut sky = Sky::overworld(time, 0.5, 0.8, 8);
            sky.fancy_clouds = fancy; sky.clouds = clouds;
            saw_sunrise |= sky.sunrise.is_some();
            let old = render(&mut expanded, &camera, &sky);
            let new = render(&mut indexed, &camera, &sky);
            assert_eq!(old, new, "time={time} height={height} fancy={fancy} clouds={clouds}");
        }
        assert!(saw_sunrise);
    }
}