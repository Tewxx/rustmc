//! Draws an `mc_gui` batch: textured/flat quads in GUI units, two blend modes,
//! optional alpha test, plus the title-screen panorama.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use mc_assets::AssetSource;
use mc_gui::batch::{Blend, GuiBatch, Item, Special, ASCII_TEX, WHITE_TEXEL_UV};
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
    /// Persistent GUI geometry buffers; `encode` only re-creates one when a frame outgrows it.
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_quads: usize,
    indexed: bool,
    /// Reused interleaved vertex scratch, so the per-frame expansion does not allocate.
    scratch: Vec<Vertex>,
    /// [blend][alpha_test]
    pipelines: [[wgpu::RenderPipeline; 2]; 2],
    glint_pipeline: wgpu::RenderPipeline,
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

/// Paints the white texel that [`WHITE_TEXEL_UV`] addresses in the font sheet. For the vanilla
/// 16×16 cell grid this lands in cell 32 (space), which `FontRenderer` never samples, so no
/// glyph pixel changes; the sampler's fixed UV then returns exactly (255, 255, 255, 255).
fn paint_white_texel(img: &mut image::RgbaImage) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let x = ((WHITE_TEXEL_UV[0] * w as f32) as u32).min(w - 1);
    let y = ((WHITE_TEXEL_UV[1] * h as f32) as u32).min(h - 1);
    img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
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
    pub fn register_rgba(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, path: &str, w: u32, h: u32, rgba: &[u8]) {
        let t=texture_from_rgba(device,queue,w,h,rgba,path);
        self.textures.insert(path.into(),Self::bind(device,&self.tex_layout,&self.nearest,&self.linear,&t));
    }
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, assets: &AssetSource) -> Self {
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gui globals"),
            size: 32,
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
        let glint_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui glint"),
            bind_group_layouts: &[&globals_layout, &tex_layout, &tex_layout],
            push_constant_ranges: &[],
        });
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let glint_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gui glint"),
            layout: Some(&glint_layout),
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
                entry_point: Some("fs_glint"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: add,
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

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
            glint_pipeline,
            blur_x,
            blur_y,
            textures: HashMap::new(),
            vbuf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gui vertices"),
                size: 64 * 1024,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            scratch: Vec::new(),
            ibuf: quad_indices(device, 1024),
            index_quads: 1024,
            indexed: std::env::var_os("RUSTMC_NO_GUI_INDEXING").is_none(),
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

    /// Binds an existing texture view (the shared block atlas's level 0) under `path`.
    pub fn register_view(&mut self, device: &wgpu::Device, path: &str, view: &wgpu::TextureView) {
        let g = Self::bind_view(device, &self.tex_layout, &self.nearest, &self.linear, view);
        self.textures.insert(path.into(), g);
    }
    fn bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, nearest: &wgpu::Sampler, linear: &wgpu::Sampler, t: &wgpu::Texture) -> GuiTexture {
        Self::bind_view(device, layout, nearest, linear, &t.create_view(&Default::default()))
    }
    fn bind_view(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, nearest: &wgpu::Sampler, linear: &wgpu::Sampler, view: &wgpu::TextureView) -> GuiTexture {
        let mk = |s: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gui texture"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
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
        // Item animations are strips, not tall inventory icons. TextureMap uploads one
        // frame per sprite; rendering the entire PNG squeezes compasses/clocks into lines.
        let mut img=if path.starts_with("textures/items/") && img.height()>img.width() && assets.read(&format!("{path}.mcmeta")).is_some() {
            image::imageops::crop_imm(&img,0,0,img.width(),img.width()).to_image()
        } else {img};
        // `GuiBatch::rect` fills sample a white texel in the space glyph cell so they can share
        // one draw with the text on top of them. `FontRenderer` never samples U+0020, so the
        // patch cannot alter a glyph. The UV is fixed, so paint exactly the texel it addresses.
        if path == ASCII_TEX {
            paint_white_texel(&mut img);
        }
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
        // Minecraft.getSystemTime() for renderEffect's scrolling.
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let (g0, g1) = ((ms % 3000) as f32 / 3000.0 / 8.0, (ms % 4873) as f32 / 4873.0 / 8.0);
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[sr.width_f as f32, sr.height_f as f32, srgb as u32 as f32, 0.0, g0, g1, 0.0, 0.0]));
        const GLINT: &str = "textures/misc/enchanted_item_glint.png";
        if batch.items.iter().any(|i| matches!(i, Item::Draw(d) if d.blend == Blend::Glint)) {
            self.ensure_texture(device, queue, assets, GLINT);
        }

        let mut verts = std::mem::take(&mut self.scratch);
        verts.clear();
        for item in &batch.items {
            if let Item::Draw(d) = item {
                if let Some(t) = &d.texture { self.ensure_texture(device, queue, assets, t); }
                for q in d.vertices.chunks_exact(4) {
                    if self.indexed {
                        verts.extend(q.iter().map(|v| Vertex { pos: v.pos, uv: v.uv, color: v.color }));
                    } else {
                        for &i in &[0usize, 1, 2, 0, 2, 3] {
                            let v = q[i];
                            verts.push(Vertex { pos: v.pos, uv: v.uv, color: v.color });
                        }
                    }
                }
            }
        }
        let pano = batch.items.iter().find_map(|item| match item { Item::Special(Special::Panorama { ticks }) => Some(*ticks), _ => None });
        // The panorama's blit quad covers the whole screen (renderSkybox's final draw).
        let pano_quad_start = if self.indexed { verts.len() / 4 * 6 } else { verts.len() } as u32;
        let (w, h) = (sr.width_f as f32, sr.height_f as f32);
        let f = if w > h { 120.0 / w } else { 120.0 / h };
        let (fu, fv) = (w * f / 256.0, h * f / 256.0);
        let quad = [
            ([0.0, h], [0.5 - fu, 0.5 + fv]),
            ([w, h], [0.5 + fu, 0.5 + fv]),
            ([w, 0.0], [0.5 + fu, 0.5 - fv]),
            ([0.0, 0.0], [0.5 - fu, 0.5 - fv]),
        ];
        if pano.is_some() {
            let corners: &[usize] = if self.indexed { &[0, 1, 2, 3] } else { &[0, 1, 2, 0, 2, 3] };
            for &i in corners { verts.push(Vertex { pos: quad[i].0, uv: quad[i].1, color: [255; 4] }); }
        }
        if verts.is_empty() { self.scratch = verts; return; }
        let quads = verts.len() / 4;
        if self.indexed && quads > self.index_quads {
            self.index_quads = quads.next_power_of_two();
            self.ibuf = quad_indices(device, self.index_quads);
        }
        let bytes: &[u8] = bytemuck::cast_slice(&verts);
        if (self.vbuf.size() as usize) < bytes.len() {
            self.vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gui vertices"),
                size: (bytes.len().next_power_of_two().max(64 * 1024)) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.vbuf, 0, bytes);
        let vbuf = &self.vbuf;

        // Panorama offscreen pass first (it can't happen inside the screen pass).
        if let Some(ticks) = pano {
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
        if self.indexed { pass.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32); }
        // Pipeline ids: 0..=3 are `pipelines[invert][alpha_test]`, 4 is the glint pipeline.
        // Consecutive commands almost always differ in texture only, so remembering the last
        // pipeline and bind group removes a redundant set for each texture switch.
        let mut cur_pipe = usize::MAX;
        let mut cur_bind: Option<&wgpu::BindGroup> = None;
        let mut start = 0;
        for item in &batch.items {
            match item {
                Item::Draw(d) => {
                    let end = start + (d.vertices.len() / 4 * 6) as u32;
                    let t = d.texture.as_ref().map_or(&self.white, |p| &self.textures[p]);
                    let pipe = if d.blend == Blend::Glint {
                        (&self.glint_pipeline, 4)
                    } else {
                        let bi = (d.blend == Blend::Invert) as usize;
                        (&self.pipelines[bi][d.alpha_test as usize], bi * 2 + d.alpha_test as usize)
                    };
                    if pipe.1 != cur_pipe {
                        pass.set_pipeline(pipe.0);
                        cur_pipe = pipe.1;
                        cur_bind = None;
                    }
                    if d.blend == Blend::Glint {
                        pass.set_bind_group(2, &self.textures[GLINT].nearest, &[]);
                    }
                    let bind = if d.linear { &t.linear } else { &t.nearest };
                    if cur_bind.map_or(true, |b| !std::ptr::eq(b, bind)) {
                        pass.set_bind_group(1, bind, &[]);
                        cur_bind = Some(bind);
                    }
                    if self.indexed { pass.draw_indexed(start..end, 0, 0..1); }
                    else { pass.draw(start..end, 0..1); }
                    start = end;
                }
                Item::Special(Special::Panorama { .. }) => {
                    // Seven passes end in target [1].
                    let bind = &self.pano_target_bgs[1].linear;
                    if cur_pipe != 0 {
                        pass.set_pipeline(&self.pipelines[0][0]);
                        cur_pipe = 0;
                        cur_bind = None;
                    }
                    if cur_bind.map_or(true, |b| !std::ptr::eq(b, bind)) {
                        pass.set_bind_group(1, bind, &[]);
                        cur_bind = Some(bind);
                    }
                    if self.indexed { pass.draw_indexed(pano_quad_start..pano_quad_start + 6, 0, 0..1); }
                    else { pass.draw(pano_quad_start..pano_quad_start + 6, 0..1); }
                }
            }
        }
        drop(pass);
        self.scratch = verts;
    }
}

fn quad_indices(device: &wgpu::Device, quads: usize) -> wgpu::Buffer {
    let indices: Vec<u32> = (0..quads as u32).flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i)).collect();
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gui quad indices"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Renders one batch into a 64×64 Rgba8Unorm target and reads it back.
    fn render(renderer: &mut GuiRenderer, device: &wgpu::Device, queue: &wgpu::Queue, assets: &AssetSource, indexed: bool, batch: &GuiBatch) -> Vec<u8> {
        renderer.indexed = indexed;
        let color = device.create_texture(&wgpu::TextureDescriptor { label: None,
            size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
        let view = color.create_view(&Default::default());
        let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 64 * 64 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None, color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None });
        renderer.encode(device, queue, &mut enc, &view, false, batch, Scaled::new(64, 64, 1), assets);
        enc.copy_texture_to_buffer(color.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &read,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(64) } }, color.size());
        queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..).map_async(wgpu::MapMode::Read, move |r| { tx.send(r).unwrap(); });
        device.poll(wgpu::Maintain::Wait); rx.recv().unwrap().unwrap();
        let pixels = read.slice(..).get_mapped_range().to_vec();
        read.unmap(); pixels
    }

    #[test]
    fn indexed_gui_matches_expanded_triangles_on_gpu() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = crate::headless_adapter(&instance) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let assets = AssetSource::new();
        let mut renderer = GuiRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let mut batch = GuiBatch::new();
        batch.gradient(0, 0, 64, 64, 0xFF123456, 0xFFABCDEF);
        batch.rect(4, 8, 37, 43, 0x8034AB56);
        batch.rect(2, 2, 12, 12, 0x0A234567);
        batch.textured_rect("missing-test-texture.png", 16, 12, 0, 0, 32, 32);
        batch.blend = Blend::Invert;
        batch.rect_f(10.25, 10.5, 30.75, 30.25, [255; 4]);
        batch.blend = Blend::Alpha;
        batch.transform = Some((0.75, [3.25, 4.5]));
        for i in 0..1100 { batch.rect(i % 60, i % 47, i % 60 + 2, i % 47 + 2, 0x40234567); }
        assert_eq!(render(&mut renderer, &device, &queue, &assets, false, &batch), render(&mut renderer, &device, &queue, &assets, true, &batch));
        batch.special(Special::Panorama { ticks: 123.5 });
        assert_eq!(render(&mut renderer, &device, &queue, &assets, false, &batch), render(&mut renderer, &device, &queue, &assets, true, &batch));
    }

    #[test]
    fn white_texel_is_painted_in_the_space_cell() {
        let mut img = image::RgbaImage::from_pixel(128, 128, image::Rgba([10, 20, 30, 40]));
        paint_white_texel(&mut img);
        assert_eq!(img.get_pixel(0, 16).0, [255, 255, 255, 255]);
        assert_eq!(img.get_pixel(0, 15).0, [10, 20, 30, 40]);
        assert_eq!(img.get_pixel(1, 16).0, [10, 20, 30, 40]);
        // Any other sheet size still paints the texel the fixed UV addresses.
        let mut tiny = image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 0, 0, 0]));
        paint_white_texel(&mut tiny);
        assert_eq!(tiny.get_pixel(0, 2).0, [255, 255, 255, 255]);
    }

    /// A fill that takes the white-texel path (so it merges with adjacent text) must produce the
    /// same pixels as the old untextured `drawRect` path.
    #[test]
    fn white_texel_fill_matches_untextured_fill() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = crate::headless_adapter(&instance) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let assets = AssetSource::new();
        let mut renderer = GuiRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &assets);
        let fill = [[4.0, 44.0], [44.0, 44.0], [44.0, 4.0], [4.0, 4.0]];
        let glyph_pos = [[16.0, 32.0], [16.0, 24.0], [24.0, 24.0], [24.0, 32.0]];
        let glyph_uv = [[8.0 / 128.0, 32.0 / 128.0], [8.0 / 128.0, 40.0 / 128.0], [16.0 / 128.0, 40.0 / 128.0], [16.0 / 128.0, 32.0 / 128.0]];
        // The pre-merge encoding: an untextured fill command, then the text command.
        let mut legacy = GuiBatch::new();
        legacy.quad(None, false, fill, [[0.0; 2]; 4], [[255, 40, 200, 230]; 4]);
        legacy.quad(Some(ASCII_TEX), true, glyph_pos, glyph_uv, [[255; 4]; 4]);
        // The merged encoding: the fill samples the font's white texel and joins the text command.
        let mut merged = GuiBatch::new();
        merged.rect(4, 4, 44, 44, 0xE6FF28C8);
        merged.quad(Some(ASCII_TEX), true, glyph_pos, glyph_uv, [[255; 4]; 4]);
        assert_eq!(merged.items.len(), 1, "fill and text must merge");
        assert_eq!(legacy.items.len(), 2);
        let a = render(&mut renderer, &device, &queue, &assets, false, &legacy);
        let b = render(&mut renderer, &device, &queue, &assets, false, &merged);
        assert_eq!(a, b, "a white-texel fill must render exactly like drawRect");
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
