//! Draws an `mc_gui` batch: textured/flat quads in GUI units, two blend modes,
//! optional alpha test, plus the title-screen panorama.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use mc_assets::AssetSource;
use mc_gui::batch::{Blend, GuiBatch, Item, Special};
use mc_gui::Scaled;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PanoVertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

struct GuiTexture {
    nearest: wgpu::BindGroup,
    linear: wgpu::BindGroup,
}

/// GuiMainMenu.renderSkybox draws the panorama into a 256×256 viewport.
const PANO_SIZE: u32 = 256;

pub struct GuiRenderer {
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    /// [blend][alpha_test]
    pipelines: [[wgpu::RenderPipeline; 2]; 2],
    blur_x: wgpu::RenderPipeline,
    blur_y: wgpu::RenderPipeline,
    textures: HashMap<String, GuiTexture>,
    white: GuiTexture,
    pano_pipeline: wgpu::RenderPipeline,
    pano_globals: wgpu::Buffer,
    pano_globals_bg: wgpu::BindGroup,
    pano_faces: Vec<wgpu::BindGroup>,
    pano_vertices: wgpu::Buffer,
    /// Ping-pong targets: [0] gets the panorama, the blur passes alternate.
    pano_targets: [wgpu::TextureView; 2],
    pano_target_bgs: [GuiTexture; 2],
}

fn texture_from_rgba(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, rgba: &[u8], label: &str) -> wgpu::Texture {
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba,
    )
}

impl GuiRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, assets: &AssetSource) -> Self {
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gui globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_layout = |vis| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("gui uniform"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let globals_layout = uniform_layout(wgpu::ShaderStages::VERTEX_FRAGMENT);
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gui globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gui shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gui.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui"),
            bind_group_layouts: &[&globals_layout, &tex_layout],
            push_constant_ranges: &[],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4],
        };
        let make = |blend: wgpu::BlendState, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("gui"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[vertex_layout.clone()],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview: None,
                cache: None,
            })
        };
        // tryBlendFuncSeparate(770, 771, 1, 0)
        let alpha = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add },
        };
        // tryBlendFuncSeparate(775, 769, 1, 0)
        let invert = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::OneMinusDst,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: alpha.alpha,
        };
        let pipelines = [[make(alpha, "fs_plain"), make(alpha, "fs_alpha_test")], [make(invert, "fs_plain"), make(invert, "fs_alpha_test")]];

        let white = Self::bind(device, &tex_layout, &nearest, &linear, &texture_from_rgba(device, queue, 1, 1, &[255; 4], "white"));

        // Panorama
        let pano_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("panorama shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pano.wgsl").into()),
        });
        let pano_globals_layout = uniform_layout(wgpu::ShaderStages::VERTEX);
        let pano_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("panorama globals"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pano_globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("panorama globals"),
            layout: &pano_globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: pano_globals.as_entire_binding() }],
        });
        let pano_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("panorama"),
            bind_group_layouts: &[&pano_globals_layout, &tex_layout],
            push_constant_ranges: &[],
        });
        let pano_format = wgpu::TextureFormat::Rgba8Unorm;
        let pano_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("panorama"),
            layout: Some(&pano_layout),
            vertex: wgpu::VertexState {
                module: &pano_shader,
                entry_point: Some("vs_pano"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<PanoVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &pano_shader,
                entry_point: Some("fs_pano"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: pano_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview: None,
            cache: None,
        });
        let pano_faces = (0..6)
            .map(|i| {
                let path = format!("textures/gui/title/background/panorama_{i}.png");
                let img = assets
                    .read(&path)
                    .and_then(|b| image::load_from_memory(&b).ok())
                    .map(|i| i.to_rgba8())
                    .unwrap_or_else(|| image::RgbaImage::from_pixel(1, 1, image::Rgba([40, 40, 40, 255])));
                let t = texture_from_rgba(device, queue, img.width(), img.height(), &img, "panorama face");
                Self::bind(device, &tex_layout, &linear, &linear, &t).linear
            })
            .collect();
        let pano_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("panorama cube"),
            contents: bytemuck::cast_slice(&pano_cube()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let pano_texs: [wgpu::Texture; 2] = std::array::from_fn(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("panorama target"),
                size: wgpu::Extent3d { width: PANO_SIZE, height: PANO_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: pano_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let pano_target_bgs = pano_texs.each_ref().map(|t| Self::bind(device, &tex_layout, &linear, &linear, t));
        let pano_targets = pano_texs.each_ref().map(|t| t.create_view(&Default::default()));
        let make_blur = |fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("panorama blur"),
                layout: Some(&pano_layout),
                vertex: wgpu::VertexState { module: &pano_shader, entry_point: Some("vs_blit"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &pano_shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: pano_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview: None,
                cache: None,
            })
        };
        let (blur_x, blur_y) = (make_blur("fs_blur_x"), make_blur("fs_blur_y"));

        Self {
            globals,
            globals_bg,
            tex_layout,
            nearest,
            linear,
            pipelines,
            blur_x,
            blur_y,
            textures: HashMap::new(),
            white,
            pano_pipeline,
            pano_globals,
            pano_globals_bg,
            pano_faces,
            pano_vertices,
            pano_targets,
            pano_target_bgs,
        }
    }

    fn bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, nearest: &wgpu::Sampler, linear: &wgpu::Sampler, t: &wgpu::Texture) -> GuiTexture {
        let view = t.create_view(&Default::default());
        let mk = |s: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gui texture"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(s) },
                ],
            })
        };
        GuiTexture { nearest: mk(nearest), linear: mk(linear) }
    }

    fn ensure_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetSource, path: &str) {
        if self.textures.contains_key(path) {
            return;
        }
        let img = assets
            .read(path)
            .and_then(|b| image::load_from_memory(&b).ok())
            .map(|i| i.to_rgba8())
            .unwrap_or_else(|| {
                image::RgbaImage::from_fn(16, 16, |x, y| {
                    if (x < 8) ^ (y < 8) { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([248, 0, 248, 255]) }
                })
            });
        let t = texture_from_rgba(device, queue, img.width(), img.height(), &img, path);
        let bound = Self::bind(device, &self.tex_layout, &self.nearest, &self.linear, &t);
        self.textures.insert(path.to_string(), bound);
    }

    /// Records the GUI pass on top of whatever `view` already holds.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        srgb: bool,
        batch: &GuiBatch,
        sr: Scaled,
        assets: &AssetSource,
    ) {
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[sr.width_f as f32, sr.height_f as f32, srgb as u32 as f32, 0.0]));

        // Flatten quads to triangles and remember each draw's range.
        enum Op {
            Draw { tex: Option<String>, blend: Blend, alpha_test: bool, linear: bool, range: std::ops::Range<u32> },
            Panorama { ticks: f32 },
        }
        let mut verts: Vec<Vertex> = Vec::new();
        let mut ops = Vec::new();
        for item in &batch.items {
            match item {
                Item::Draw(d) => {
                    if let Some(t) = &d.texture {
                        self.ensure_texture(device, queue, assets, t);
                    }
                    let start = verts.len() as u32;
                    for q in d.vertices.chunks_exact(4) {
                        for &i in &[0usize, 1, 2, 0, 2, 3] {
                            let v = q[i];
                            verts.push(Vertex { pos: v.pos, uv: v.uv, color: v.color });
                        }
                    }
                    ops.push(Op::Draw {
                        tex: d.texture.clone(),
                        blend: d.blend,
                        alpha_test: d.alpha_test,
                        linear: d.linear,
                        range: start..verts.len() as u32,
                    });
                }
                Item::Special(Special::Panorama { ticks }) => ops.push(Op::Panorama { ticks: *ticks }),
            }
        }
        // The panorama's blit quad covers the whole screen (renderSkybox's final draw).
        let pano_quad_start = verts.len() as u32;
        let (w, h) = (sr.width_f as f32, sr.height_f as f32);
        let f = if w > h { 120.0 / w } else { 120.0 / h };
        let (fu, fv) = (w * f / 256.0, h * f / 256.0);
        let quad = [
            ([0.0, h], [0.5 - fu, 0.5 + fv]),
            ([w, h], [0.5 + fu, 0.5 + fv]),
            ([w, 0.0], [0.5 + fu, 0.5 - fv]),
            ([0.0, 0.0], [0.5 - fu, 0.5 - fv]),
        ];
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            verts.push(Vertex { pos: quad[i].0, uv: quad[i].1, color: [255; 4] });
        }
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gui vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Panorama offscreen pass first (it can't happen inside the screen pass).
        if let Some(Op::Panorama { ticks }) = ops.iter().find(|o| matches!(o, Op::Panorama { .. })) {
            // GuiMainMenu.drawPanorama: pitch sin(t/400)*25+20, yaw -t*0.1, 120Â° FOV.
            let pitch = (ticks / 400.0).sin() * 25.0 + 20.0;
            let yaw = -ticks * 0.1;
            let proj = Mat4::perspective_rh(120f32.to_radians(), 1.0, 0.05, 10.0);
            let view_m = Mat4::from_rotation_x(pitch.to_radians()) * Mat4::from_rotation_y(yaw.to_radians());
            queue.write_buffer(&self.pano_globals, 0, bytemuck::cast_slice(&(proj * view_m).to_cols_array()));
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("panorama"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.pano_targets[0],
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pano_pipeline);
            pass.set_bind_group(0, &self.pano_globals_bg, &[]);
            pass.set_vertex_buffer(0, self.pano_vertices.slice(..));
            for (i, face) in self.pano_faces.iter().enumerate() {
                pass.set_bind_group(1, face, &[]);
                let s = i as u32 * 6;
                pass.draw(s..s + 6, 0..1);
            }
            drop(pass);
            // Seven rotateAndBlurSkybox calls: x, y, x, y, x, y, x in panorama space.
            for i in 0..7 {
                let (src, dst) = (i % 2, 1 - i % 2);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("panorama blur"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.pano_targets[dst],
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(if i % 2 == 0 { &self.blur_x } else { &self.blur_y });
                pass.set_bind_group(0, &self.pano_globals_bg, &[]);
                pass.set_bind_group(1, &self.pano_target_bgs[src].linear, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.globals_bg, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        for op in &ops {
            match op {
                Op::Draw { tex, blend, alpha_test, linear, range } => {
                    let t = tex.as_ref().map_or(&self.white, |p| &self.textures[p]);
                    let bi = (*blend == Blend::Invert) as usize;
                    pass.set_pipeline(&self.pipelines[bi][*alpha_test as usize]);
                    pass.set_bind_group(1, if *linear { &t.linear } else { &t.nearest }, &[]);
                    pass.draw(range.clone(), 0..1);
                }
                Op::Panorama { .. } => {
                    // Seven passes end in target [1].
                    pass.set_pipeline(&self.pipelines[0][0]);
                    pass.set_bind_group(1, &self.pano_target_bgs[1].linear, &[]);
                    pass.draw(pano_quad_start..pano_quad_start + 6, 0..1);
                }
            }
        }
    }
}

/// Six inward-facing faces: 0 front (âˆ’Z), 1 right, 2 back, 3 left, 4 up, 5 down,
/// each with the image's top edge toward +Y (or toward the front for up/down).
fn pano_cube() -> Vec<PanoVertex> {
    // Corners in the face's own frame: (right, up) in [-1, 1], at depth 1 along `fwd`.
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::NEG_Z, Vec3::X, Vec3::Y),
        (Vec3::X, Vec3::Z, Vec3::Y),
        (Vec3::Z, Vec3::NEG_X, Vec3::Y),
        (Vec3::NEG_X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
    ];
    let mut out = Vec::new();
    for (fwd, right, up) in faces {
        let corner = |r: f32, u: f32| PanoVertex { pos: (fwd + right * r + up * u).into(), uv: [(r + 1.0) / 2.0, (1.0 - u) / 2.0] };
        let (a, b, c, d) = (corner(-1.0, 1.0), corner(1.0, 1.0), corner(1.0, -1.0), corner(-1.0, -1.0));
        out.extend_from_slice(&[a, b, c, a, c, d]);
    }
    out
}
