use crate::{sky::Sky, Camera};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use mc_assets::AssetSource;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    clip: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
    kind: f32,
    fog: f32,
}
pub struct SkyRenderer {
    pipelines: Vec<wgpu::RenderPipeline>,
    bg: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    fog: wgpu::Buffer,
}
impl SkyRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, assets: &AssetSource) -> Self {
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
        let fog = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky fog"),
            size: 16,
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
                    resource: fog.as_entire_binding(),
                },
            ],
        });
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("sky shader"),source:wgpu::ShaderSource::Wgsl(format!(r#"
const SRGB:bool={};
@group(0) @binding(0) var samp:sampler;
@group(0) @binding(1) var sun:texture_2d<f32>;
@group(0) @binding(2) var moon:texture_2d<f32>;
@group(0) @binding(3) var clouds:texture_2d<f32>;
@group(0) @binding(4) var<uniform> fog_color:vec4<f32>;
struct V {{@builtin(position) pos:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) color:vec4<f32>,@location(2) @interpolate(flat) kind:u32,@location(3) fog:f32}};
@vertex fn vs(@location(0) pos:vec4<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) kind:f32,@location(4) fog:f32)->V {{var v:V;v.pos=pos;if(kind<3.){{v.pos.z=v.pos.w;}}v.uv=uv;v.color=color;v.kind=u32(kind);v.fog=fog;return v;}}
@fragment fn fs(v:V)->@location(0) vec4<f32> {{var c=v.color;if(v.kind==1u){{c*=textureSample(sun,samp,v.uv);}}if(v.kind==2u){{c*=textureSample(moon,samp,v.uv);}}if(v.kind==3u){{c*=textureSample(clouds,samp,v.uv);if(c.a<=0.1){{discard;}}}}c=vec4<f32>(mix(c.rgb,fog_color.rgb,clamp(v.fog,0.,1.)),c.a);if(SRGB){{c=vec4<f32>(select(c.rgb/12.92,pow((c.rgb+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c.rgb>vec3<f32>(0.04045)),c.a);}}return c;}}
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
                            attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4,3=>Float32,4=>Float32],
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
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky/cloud vertices"),
            size: 100000 * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipelines,
            bg,
            vertices,
            fog,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        cam: &Camera,
        aspect: f32,
        distance: f32,
        sky: &Sky,
    ) {
        let vp = cam.view_proj(aspect, distance);
        let mut v = Vec::new();
        let color = |rgb: [f32; 3], a: f32| [rgb[0], rgb[1], rgb[2], a];
        for x in (-384..=384).step_by(64) {
            for z in (-384..=384).step_by(64) {
                push(
                    &mut v,
                    vp,
                    [
                        [x as f32, 16., z as f32],
                        [(x + 64) as f32, 16., z as f32],
                        [(x + 64) as f32, 16., (z + 64) as f32],
                        [x as f32, 16., (z + 64) as f32],
                    ],
                    [[0.; 2]; 4],
                    color(sky.sky, 1.),
                    0.,
                    Some((0., distance)),
                );
            }
        }
        let bottom = [sky.sky[0] * 0.2 + 0.04, sky.sky[1] * 0.2 + 0.04, sky.sky[2] * 0.6 + 0.1];
        let y = 63. - cam.pos.y;
        push(
            &mut v,
            vp,
            [[-384., y, -384.], [448., y, -384.], [448., y, 448.], [-384., y, 448.]],
            [[0.; 2]; 4],
            color(bottom, 1.),
            0.,
            Some((0., distance)),
        );
        if let Some(c) = sky.sunrise {
            let m = vp
                * Mat4::from_rotation_x(90f32.to_radians())
                * Mat4::from_rotation_z(if mc_core::math::sin(sky.angle * std::f32::consts::TAU) < 0. {
                    std::f32::consts::PI
                } else {
                    0.
                })
                * Mat4::from_rotation_z(90f32.to_radians());
            for i in 0..16 {
                let a = i as f32 * std::f32::consts::TAU / 16.;
                let b = (i + 1) as f32 * std::f32::consts::TAU / 16.;
                let points = [
                    [0., 100., 0.],
                    [a.sin() * 120., a.cos() * 120., -a.cos() * 40. * c[3]],
                    [b.sin() * 120., b.cos() * 120., -b.cos() * 40. * c[3]],
                ];
                for j in 0..3 {
                    v.push(Vertex {
                        clip: (m * Vec3::from(points[j]).extend(1.)).to_array(),
                        uv: [0.; 2],
                        color: [c[0], c[1], c[2], if j == 0 { c[3] } else { 0. }],
                        kind: 0.,
                        fog: 0.,
                    });
                }
            }
        }
        let flat_end = v.len() as u32;
        let celestial = vp * Mat4::from_rotation_y(-90f32.to_radians()) * Mat4::from_rotation_x(sky.angle * std::f32::consts::TAU);
        push(
            &mut v,
            celestial,
            [[-30., 100., -30.], [30., 100., -30.], [30., 100., 30.], [-30., 100., 30.]],
            [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            [1., 1., 1., sky.celestial_visibility],
            1.,
            None,
        );
        let (u, w) = ((sky.moon % 4) as f32 / 4., (sky.moon / 4 % 2) as f32 / 2.);
        push(
            &mut v,
            celestial,
            [[-20., -100., 20.], [20., -100., 20.], [20., -100., -20.], [-20., -100., -20.]],
            [[u + 0.25, w + 0.5], [u, w + 0.5], [u, w], [u + 0.25, w]],
            [1., 1., 1., sky.celestial_visibility],
            2.,
            None,
        );
        if sky.stars > 0. {
            for &quad in star_quads() {
                push(&mut v, celestial, quad, [[0.; 2]; 4], [sky.stars; 4], 0., None);
            }
        }
        let celestial_end = v.len() as u32;
        if sky.clouds {
            clouds(&mut v, vp, cam.pos, sky, distance);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&v));
        queue.write_buffer(&self.fog, 0, bytemuck::cast_slice(&[sky.fog[0], sky.fog[1], sky.fog[2], 1.]));
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
        pass.set_bind_group(0, &self.bg, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (pipeline, range) in [
            (0, 0..flat_end),
            (1, flat_end..celestial_end),
            (2, celestial_end..v.len() as u32),
            (3, celestial_end..v.len() as u32),
        ] {
            pass.set_pipeline(&self.pipelines[pipeline]);
            pass.draw(range, 0..1);
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
fn push(v: &mut Vec<Vertex>, m: Mat4, pos: [[f32; 3]; 4], uv: [[f32; 2]; 4], color: [f32; 4], kind: f32, fog: Option<(f32, f32)>) {
    for i in [0, 1, 2, 0, 2, 3] {
        let p = Vec3::from(pos[i]);
        let f = fog.map_or(0., |(start, end)| ((p.length() - start) / (end - start)).clamp(0., 1.));
        v.push(Vertex {
            clip: (m * p.extend(1.)).to_array(),
            uv: uv[i],
            color,
            kind,
            fog: f,
        });
    }
}
fn clouds(v: &mut Vec<Vertex>, vp: Mat4, pos: Vec3, sky: &Sky, distance: f32) {
    let height = 128. - pos.y + 0.33;
    if !sky.fancy_clouds {
        let dx = (pos.x as f64 + sky.ticks as f64 * 0.029999999329447746).rem_euclid(2048.) as f32 / 2048.;
        let dz = (pos.z as f64).rem_euclid(2048.) as f32 / 2048.;
        for x in (-256..256).step_by(32) {
            for z in (-256..256).step_by(32) {
                let x = x as f32;
                let z = z as f32;
                push(
                    v,
                    vp,
                    [
                        [x, height, z + 32.],
                        [x + 32., height, z + 32.],
                        [x + 32., height, z],
                        [x, height, z],
                    ],
                    [
                        [x / 2048. + dx, (z + 32.) / 2048. + dz],
                        [(x + 32.) / 2048. + dx, (z + 32.) / 2048. + dz],
                        [(x + 32.) / 2048. + dx, z / 2048. + dz],
                        [x / 2048. + dx, z / 2048. + dz],
                    ],
                    [sky.cloud[0], sky.cloud[1], sky.cloud[2], 0.8],
                    3.,
                    Some((distance * 0.75, distance)),
                );
            }
        }
        return;
    }
    let dx = ((pos.x as f64 + sky.ticks as f64 * 0.029999999329447746) / 12.).rem_euclid(2048.);
    let dz = (pos.z as f64 / 12. + 0.33000001311302185).rem_euclid(2048.);
    let (u, w) = (dx.floor() as f32 / 256., dz.floor() as f32 / 256.);
    let (fx, fz) = ((dx - dx.floor()) as f32, (dz - dz.floor()) as f32);
    let mut face = |points: [[f32; 3]; 4], uv: [[f32; 2]; 4], shade: f32| {
        let p = points.map(|p| [p[0] * 12., p[1], p[2] * 12.]);
        push(
            v,
            vp,
            p,
            uv,
            [sky.cloud[0] * shade, sky.cloud[1] * shade, sky.cloud[2] * shade, 0.8],
            3.,
            Some((distance * 0.75, distance)),
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
                    [
                        [px, height, pz + 8.],
                        [px + 8., height, pz + 8.],
                        [px + 8., height, pz],
                        [px, height, pz],
                    ],
                    uv,
                    0.7,
                );
            }
            if height <= 5. {
                let y = height + 4. - 0.0009765625;
                face([[px, y, pz + 8.], [px + 8., y, pz + 8.], [px + 8., y, pz], [px, y, pz]], uv, 1.);
            }
            for n in 0..8 {
                let n = n as f32;
                if j > -1 {
                    let a = px + n;
                    let t = (x + n + 0.5) / 256. + u;
                    face(
                        [
                            [a, height, pz + 8.],
                            [a, height + 4., pz + 8.],
                            [a, height + 4., pz],
                            [a, height, pz],
                        ],
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
                        [
                            [a, height, pz + 8.],
                            [a, height + 4., pz + 8.],
                            [a, height + 4., pz],
                            [a, height, pz],
                        ],
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
                        [
                            [px, height + 4., a],
                            [px + 8., height + 4., a],
                            [px + 8., height, a],
                            [px, height, a],
                        ],
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
                        [
                            [px, height + 4., a],
                            [px + 8., height + 4., a],
                            [px + 8., height, a],
                            [px, height, a],
                        ],
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

#[cfg(test)]
mod star_tests {
    use super::*;

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
}
