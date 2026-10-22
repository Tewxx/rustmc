//! wgpu renderer: one atlas, one pipeline, one vertex/index buffer per chunk, frustum-culled.

use crate::gui_gpu::GuiRenderer;
use crate::mesh::{ChunkMesh, Vertex};
use crate::models::BlockModels;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use mc_assets::AssetSource;
use mc_gui::batch::GuiBatch;
use mc_gui::Scaled;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    fog_color: [f32; 4],
    /// x = fog start, y = fog end, z = 1 when the target is sRGB (shader must linearize).
    fog: [f32; 4],
}

struct GpuChunk {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    solid_count: u32,
    transparent: Vec<([u32; 6], Vec3)>,
    bind_group: wgpu::BindGroup,
    min: Vec3,
    max: Vec3,
}

pub struct Camera {
    pub pos: Vec3,
    /// Degrees, vanilla convention: yaw 0 looks south (+Z), pitch +90 looks down.
    pub yaw: f32,
    pub pitch: f32,
    pub fov_deg: f32,
    /// Hurt-camera shake and view bobbing, applied in eye space before the camera rotation.
    pub bob: Mat4,
}

impl Camera {
    /// `orientCamera`: rotate(pitch, X) then rotate(yaw + 180, Y), after `bob`.
    pub fn view(&self) -> Mat4 {
        self.bob * Mat4::from_rotation_x(self.pitch.to_radians()) * Mat4::from_rotation_y((self.yaw + 180.).to_radians())
    }

    pub fn proj(&self, aspect: f32, far: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_deg.to_radians(), aspect, 0.05, far)
    }

    /// Camera-relative view-projection with vanilla's `farPlaneDistance * sqrt(2)` far plane.
    pub fn view_proj(&self, aspect: f32, render_distance: f32) -> Mat4 {
        self.proj(aspect, render_distance * std::f32::consts::SQRT_2) * self.view()
    }

    pub fn look_dir(&self) -> Vec3 {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        Vec3::new(-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos())
    }
}

pub struct Renderer {
    pub sky: crate::sky::Sky,
    sky_renderer: crate::sky_gpu::SkyRenderer,
    pub avatar_frame: Option<crate::avatar::AvatarFrame>,
    avatar: crate::avatar::AvatarRenderer,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    chunk_layout: wgpu::BindGroupLayout,
    depth: wgpu::TextureView,
    lightmap: wgpu::Texture,
    atlas: wgpu::Texture,
    chunks: HashMap<(i32, i32, i32), GpuChunk>,
    gui: GuiRenderer,
    uncapped_mode: wgpu::PresentMode,
    pub adapter_info: wgpu::AdapterInfo,
    pub render_distance_blocks: f32,
    pub drawn_chunks: usize,
}

impl Renderer {
    pub async fn new(window: Arc<winit::window::Window>, models: &BlockModels, assets: &AssetSource) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window).expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter");
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("rustmc"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        // Legacy GL blends in raw color space. Prefer UNORM so alpha blending follows that
        // space as well; per-shader decoding on an sRGB surface only fixes opaque pixels.
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        // Uncapped FPS when the platform allows it.
        let present_mode = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
            .into_iter()
            .find(|m| caps.present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let atlas = &models.atlas;
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block atlas"),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: atlas.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Raw values: vanilla does all color math without sRGB decoding.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, data) in atlas.levels.iter().enumerate() {
            let (w, h) = (atlas.width >> level, atlas.height >> level);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &atlas_tex,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        // GL_NEAREST_MIPMAP_LINEAR / GL_NEAREST, like TextureUtil.setTextureBlurMipmap(false, true).
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_level0 = atlas_tex.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: 0,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let lightmap = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("lightmap"),
                size: wgpu::Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &crate::lightmap::build(crate::lightmap::sun_brightness(0.0), 0.0, 0.0),
        );
        // Vanilla's lightmap uses GL_LINEAR + clamp so AO-averaged values blend.
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_tex.create_view(&Default::default())),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&lightmap.create_view(&Default::default())),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&lightmap_sampler),
                },
            ],
        });
        let chunk_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("chunk"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chunk"),
            bind_group_layouts: &[&globals_layout, &chunk_layout],
            push_constant_ranges: &[],
        });
        let make_pipeline = |translucent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("chunk"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4, 3 => Unorm8x4],
                    }],
                },
                primitive: wgpu::PrimitiveState {
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: !translucent,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: translucent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };

        let pipeline = make_pipeline(false);
        let translucent_pipeline = make_pipeline(true);
        let depth = create_depth(&device, config.width, config.height);
        let mut gui = GuiRenderer::new(&device, &queue, format, assets);
        gui.register_view(&device, "rustmc:block_atlas", &atlas_level0);
        let avatar = crate::avatar::AvatarRenderer::new(&device, &queue, format, assets, atlas_level0);
        let sky_renderer = crate::sky_gpu::SkyRenderer::new(&device, &queue, format, assets);
        Self {
            sky: crate::sky::Sky::overworld(6000, 0., 0.8, 12),
            sky_renderer,
            avatar,
            avatar_frame: None,
            gui,
            uncapped_mode: present_mode,
            adapter_info: adapter.get_info(),
            surface,
            device,
            queue,
            config,
            pipeline,
            translucent_pipeline,
            globals_buf,
            globals_bg,
            chunk_layout,
            depth,
            lightmap,
            atlas: atlas_tex,
            chunks: HashMap::new(),
            render_distance_blocks: 128.0,
            drawn_chunks: 0,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = create_depth(&self.device, width, height);
    }

    /// Uploads one render section's mesh (`sy` = section index 0..16).
    /// A downloaded player skin into the avatar skin atlas.
    pub fn upload_skin(&self, cell: u32, rgba: &[u8]) {
        self.avatar.upload_skin(&self.queue, cell, rgba);
    }

    pub fn upload_section(&mut self, cx: i32, sy: i32, cz: i32, mesh: &ChunkMesh) {
        if mesh.indices.is_empty() {
            self.chunks.remove(&(cx, sy, cz));
            return;
        }
        let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mut solid = Vec::new();
        let mut transparent: Vec<([u32; 6], Vec3)> = Vec::new();
        for q in mesh.indices.chunks_exact(6) {
            if mesh.vertices[q[0] as usize].light[3] != 0 {
                let center = [q[0], q[1], q[2], q[5]]
                    .into_iter()
                    .map(|i| Vec3::from(mesh.vertices[i as usize].pos))
                    .sum::<Vec3>()
                    * 0.25;
                transparent.push((q.try_into().unwrap(), center));
            } else {
                solid.extend_from_slice(q);
            }
        }
        let solid_count = solid.len() as u32;
        for (q, _) in &transparent {
            solid.extend_from_slice(q);
        }
        let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk indices"),
            contents: bytemuck::cast_slice(&solid),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        });
        let origin = [(cx * 16) as f32, 0.0, (cz * 16) as f32, 0.0];
        let uniform = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk origin"),
            contents: bytemuck::cast_slice(&origin),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("chunk"),
            layout: &self.chunk_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for v in &mesh.vertices {
            lo = lo.min(Vec3::from(v.pos));
            hi = hi.max(Vec3::from(v.pos));
        }
        let o = Vec3::new(origin[0], 0.0, origin[2]);
        self.chunks.insert(
            (cx, sy, cz),
            GpuChunk {
                vertices,
                indices,
                index_count: mesh.indices.len() as u32,
                solid_count,
                transparent,
                bind_group,
                min: lo + o,
                max: hi + o,
            },
        );
    }

    /// Draws the world (when `cam` is set) and then the GUI batch on top.
    pub fn render(
        &mut self,
        cam: Option<&Camera>,
        gui: Option<(&GuiBatch, Scaled)>,
        assets: &AssetSource,
    ) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&Default::default());
        let aspect = self.config.width as f32 / self.config.height as f32;
        let mut enc = self.encode(cam, &view, None, aspect, self.config.height as f32, assets);
        if let (Some(cam), Some(frame)) = (cam, &self.avatar_frame) {
            self.avatar.encode(
                &self.device,
                &self.queue,
                &mut enc,
                &view,
                &self.depth,
                cam,
                aspect,
                self.config.height as f32,
                self.render_distance_blocks,
                frame,
                assets,
                true,
            );
        }
        if let Some((batch, sr)) = gui {
            let srgb = self.config.format.is_srgb();
            self.gui.encode(&self.device, &self.queue, &mut enc, &view, srgb, batch, sr, assets);
        }
        self.queue.submit([enc.finish()]);
        frame.present();
        Ok(())
    }

    /// Switches between vsync (Fifo) and uncapped presentation.
    pub fn set_vsync(&mut self, on: bool) {
        let mode = if on { wgpu::PresentMode::Fifo } else { self.uncapped_mode };
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Renders one frame off-screen and returns tightly packed RGBA8 pixels.
    pub fn capture(&mut self, cam: Option<&Camera>, gui: Option<(&GuiBatch, Scaled)>, assets: &AssetSource, w: u32, h: u32) -> Vec<u8> {
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = create_depth(&self.device, w, h);
        let view = target.create_view(&Default::default());
        let mut enc = self.encode(cam, &view, Some(&depth), w as f32 / h as f32, h as f32, assets);
        if let (Some(cam), Some(frame)) = (cam, &self.avatar_frame) {
            self.avatar.encode(
                &self.device,
                &self.queue,
                &mut enc,
                &view,
                &depth,
                cam,
                w as f32 / h as f32,
                h as f32,
                self.render_distance_blocks,
                frame,
                assets,
                true,
            );
        }
        if let Some((batch, sr)) = gui {
            let srgb = self.config.format.is_srgb();
            self.gui.encode(&self.device, &self.queue, &mut enc, &view, srgb, batch, sr, assets);
        }
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let data = buf.slice(..).get_mapped_range();
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let line = &data[(y * row) as usize..(y * row + w * 4) as usize];
            for px in line.chunks_exact(4) {
                if bgra {
                    out.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    out.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        out
    }

    /// Records the world pass. `depth` defaults to the window-sized depth buffer.
    fn encode(
        &mut self,
        cam: Option<&Camera>,
        view: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
        aspect: f32,
        height: f32,
        assets: &AssetSource,
    ) -> wgpu::CommandEncoder {
        let Some(cam) = cam else {
            // No world: clear to black for the GUI to draw over.
            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.drawn_chunks = 0;
            return enc;
        };
        let view_proj = cam.view_proj(aspect, self.render_distance_blocks);
        let srgb = self.config.format.is_srgb();
        let globals = Globals {
            view_proj: view_proj.to_cols_array_2d(),
            camera: [cam.pos.x, cam.pos.y, cam.pos.z, 0.0],
            fog_color: [self.sky.fog[0], self.sky.fog[1], self.sky.fog[2], 1.],
            fog: [
                self.render_distance_blocks * 0.75,
                self.render_distance_blocks,
                srgb as u32 as f32,
                0.0,
            ],
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
        let planes = frustum_planes(view_proj);

        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear({
                            // Clear colors are linear on sRGB targets; keep the raw vanilla value on screen.
                            let fog = [self.sky.fog[0], self.sky.fog[1], self.sky.fog[2], 1.];
                            let c = if srgb { srgb_to_linear(fog) } else { fog };
                            wgpu::Color {
                                r: c[0] as f64,
                                g: c[1] as f64,
                                b: c[2] as f64,
                                a: 1.0,
                            }
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth.unwrap_or(&self.depth),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.globals_bg, &[]);
            self.drawn_chunks = 0;
            for c in self.chunks.values() {
                if !aabb_visible(&planes, c.min - cam.pos, c.max - cam.pos) {
                    continue;
                }
                pass.set_bind_group(1, &c.bind_group, &[]);
                pass.set_vertex_buffer(0, c.vertices.slice(..));
                pass.set_index_buffer(c.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..c.solid_count, 0, 0..1);
                self.drawn_chunks += 1;
            }
        }
        self.sky_renderer.encode(
            &self.queue,
            &mut enc,
            view,
            depth.unwrap_or(&self.depth),
            cam,
            aspect,
            self.render_distance_blocks,
            &self.sky,
        );
        // Opaque player geometry must be underneath translucent world faces.
        if let Some(frame) = &self.avatar_frame {
            self.avatar.encode(
                &self.device,
                &self.queue,
                &mut enc,
                view,
                depth.unwrap_or(&self.depth),
                cam,
                aspect,
                height,
                self.render_distance_blocks,
                frame,
                assets,
                false,
            );
        }
        let mut translucent: Vec<_> = self
            .chunks
            .iter()
            .filter(|(_, c)| !c.transparent.is_empty() && aabb_visible(&planes, c.min - cam.pos, c.max - cam.pos))
            .map(|(&k, c)| (k, ((c.min + c.max) * 0.5 - cam.pos).length_squared()))
            .collect();
        translucent.sort_by(|a, b| b.1.total_cmp(&a.1));
        for &(key, _) in &translucent {
            let c = self.chunks.get_mut(&key).unwrap();
            let relative = cam.pos - Vec3::new((key.0 * 16) as f32, 0., (key.2 * 16) as f32);
            c.transparent
                .sort_by(|a, b| (b.1 - relative).length_squared().total_cmp(&(a.1 - relative).length_squared()));
            let indices: Vec<u32> = c.transparent.iter().flat_map(|(q, _)| q.iter().copied()).collect();
            self.queue
                .write_buffer(&c.indices, c.solid_count as u64 * 4, bytemuck::cast_slice(&indices));
        }
        if !translucent.is_empty() {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("translucent blocks"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth.unwrap_or(&self.depth),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.translucent_pipeline);
            pass.set_bind_group(0, &self.globals_bg, &[]);
            for (key, _) in &translucent {
                let c = &self.chunks[key];
                pass.set_bind_group(1, &c.bind_group, &[]);
                pass.set_vertex_buffer(0, c.vertices.slice(..));
                pass.set_index_buffer(c.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(c.solid_count..c.index_count, 0, 0..1);
            }
        }
        enc
    }
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// The clear color is specified in sRGB; shader output to an sRGB target must be linear.
impl Renderer {
    /// Applies `Atlas::tick` results (animated textures) to every mip level.
    pub fn upload_atlas(&self, uploads: &[mc_assets::atlas::Upload]) {
        for u in uploads {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: u.level,
                    origin: wgpu::Origin3d { x: u.x, y: u.y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &u.rgba,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(u.size * 4), rows_per_image: Some(u.size) },
                wgpu::Extent3d { width: u.size, height: u.size, depth_or_array_layers: 1 },
            );
        }
    }

    /// Replaces the 16×16 lightmap (see [`crate::lightmap::build`]).
    pub fn set_lightmap(&self, rgba: &[u8; 1024]) {
        self.queue.write_texture(
            self.lightmap.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64),
                rows_per_image: Some(16),
            },
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn srgb_to_linear(c: [f32; 4]) -> [f32; 4] {
    let f = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

fn frustum_planes(m: Mat4) -> [Vec4; 5] {
    let r = |i| m.row(i);
    // Left, right, bottom, top, near (far is covered by fog distance).
    [r(3) + r(0), r(3) - r(0), r(3) + r(1), r(3) - r(1), r(2)]
}

fn aabb_visible(planes: &[Vec4; 5], min: Vec3, max: Vec3) -> bool {
    planes.iter().all(|p| {
        let v = Vec3::new(
            if p.x >= 0.0 { max.x } else { min.x },
            if p.y >= 0.0 { max.y } else { min.y },
            if p.z >= 0.0 { max.z } else { min.z },
        );
        p.truncate().dot(v) + p.w >= 0.0
    })
}
