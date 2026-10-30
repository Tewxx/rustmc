//! wgpu renderer: frustum-culled sections packed into shared GPU arenas.

use crate::gui_gpu::GuiRenderer;
use crate::mesh::{ChunkMesh, PackedVertex, Vertex};
use crate::models::BlockModels;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use mc_assets::AssetSource;
use mc_gui::batch::GuiBatch;
use mc_gui::Scaled;
use std::collections::{BTreeMap, HashMap};
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

/// First-fit range allocator with coalescing; offsets and lengths are in elements.
struct FreeList {
    free: BTreeMap<u32, u32>,
}

impl FreeList {
    fn new(cap: u32) -> Self {
        Self { free: BTreeMap::from([(0, cap)]) }
    }

    fn alloc(&mut self, n: u32) -> Option<u32> {
        let (&off, &len) = self.free.iter().find(|&(_, &len)| len >= n)?;
        self.free.remove(&off);
        if len > n {
            self.free.insert(off + n, len - n);
        }
        Some(off)
    }

    fn release(&mut self, mut off: u32, mut n: u32) {
        if let Some((&po, &pl)) = self.free.range(..off).next_back() {
            if po + pl == off {
                self.free.remove(&po);
                off = po;
                n += pl;
            }
        }
        if let Some(&nl) = self.free.get(&(off + n)) {
            self.free.remove(&(off + n));
            n += nl;
        }
        self.free.insert(off, n);
    }
}

/// One large vertex + index buffer pair that sections are sub-allocated from.
struct ArenaPage {
    packed: bool,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    vfree: FreeList,
    ifree: FreeList,
}

const CHUNK_ATTRIBUTES: [wgpu::VertexAttribute; 4] = [
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint16x4, offset: 0, shader_location: 0 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 8, shader_location: 1 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4, offset: 16, shader_location: 2 },
    wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x2, offset: 6, shader_location: 3 },
];

const FULL_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4, 3 => Unorm8x4];

const PAGE_VERTS: u32 = 1 << 20;
const PAGE_INDICES: u32 = PAGE_VERTS / 2 * 3;
const INITIAL_SLOTS: usize = 4096;

struct GpuChunk {
    page: u32,
    /// Vertex / index element offsets inside the page; solid indices first, then sorted translucent ones.
    v_off: u32,
    v_count: u32,
    i_off: u32,
    index_count: u32,
    solid_count: u32,
    transparent: Vec<([u32; 6], Vec3)>,
    /// Camera position the translucent quads were last sorted/uploaded for (NaN = never).
    sorted_at: Vec3,
    key: (i32, i32, i32),
    min: Vec3,
    max: Vec3,
}

/// `DrawIndexedIndirectArgs` layout.
type DrawCmd = [u32; 5];

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
    full_pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    full_translucent_pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    chunk_layout: wgpu::BindGroupLayout,
    origins_buf: wgpu::Buffer,
    origins_bg: wgpu::BindGroup,
    origins: Vec<[f32; 4]>,
    pages: Vec<ArenaPage>,
    /// Slot-indexed sections (slot = instance index in the shader); `index` maps keys to slots.
    slots: Vec<Option<GpuChunk>>,
    free_slots: Vec<u32>,
    index: HashMap<(i32, i32, i32), u32>,
    indirect: wgpu::Buffer,
    indirect_cap: usize,
    use_indirect: bool,
    cmds: Vec<DrawCmd>,
    /// (frames, CPU time in encode + submit excluding drawable acquire/present); printed on drop with RUSTMC_RENDER_TIMING.
    timing: (u32, std::time::Duration),
    runs: Vec<(u32, u32, u32)>,
    depth: wgpu::TextureView,
    lightmap: wgpu::Texture,
    atlas: wgpu::Texture,
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
                    required_features: adapter.features() & (wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE),
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
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let (origins_buf, origins_bg) = make_origins(&device, &chunk_layout, INITIAL_SLOTS);
        let use_indirect = std::env::var_os("RUSTMC_NO_MDI").is_none()
            && device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE);
        let indirect = make_indirect(&device, 4096);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let full_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("full precision chunk shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader_full.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("chunk"),
            bind_group_layouts: &[&globals_layout, &chunk_layout],
            push_constant_ranges: &[],
        });
        let make_pipeline = |translucent: bool, packed: bool| {
            let shader = if packed { &shader } else { &full_shader };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("chunk"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: if packed { std::mem::size_of::<PackedVertex>() } else { std::mem::size_of::<Vertex>() } as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: if packed { &CHUNK_ATTRIBUTES } else { &FULL_ATTRIBUTES },
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
                    module: shader,
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

        let pipeline = make_pipeline(false, true);
        let translucent_pipeline = make_pipeline(true, true);
        let full_pipeline = make_pipeline(false, false);
        let full_translucent_pipeline = make_pipeline(true, false);
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
            full_pipeline,
            translucent_pipeline,
            full_translucent_pipeline,
            globals_buf,
            globals_bg,
            chunk_layout,
            depth,
            lightmap,
            atlas: atlas_tex,
            origins_buf,
            origins_bg,
            origins: vec![[0.0; 4]; INITIAL_SLOTS],
            pages: Vec::new(),
            slots: Vec::new(),
            free_slots: Vec::new(),
            index: HashMap::new(),
            indirect,
            indirect_cap: 4096,
            use_indirect,
            cmds: Vec::new(),
            timing: (0, std::time::Duration::ZERO),
            runs: Vec::new(),
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

    /// A pack icon (or any runtime image) as a GUI texture under `path`.
    pub fn register_gui_texture(&mut self, path: &str, w: u32, h: u32, rgba: &[u8]) {
        self.gui.register_rgba(&self.device, &self.queue, path, w, h, rgba);
    }

    /// Drops every uploaded chunk mesh. Used after a resource-pack reload, where all baked
    /// atlas UVs change and the world must be remeshed.
    pub fn clear_sections(&mut self) {
        self.pages.clear();
        self.slots.clear();
        self.free_slots.clear();
        self.index.clear();
    }

    fn remove_section(&mut self, key: (i32, i32, i32)) {
        if let Some(slot) = self.index.remove(&key) {
            let c = self.slots[slot as usize].take().unwrap();
            let p = &mut self.pages[c.page as usize];
            p.vfree.release(c.v_off, c.v_count);
            p.ifree.release(c.i_off, c.index_count);
            self.free_slots.push(slot);
        }
    }

    pub fn upload_section(&mut self, cx: i32, sy: i32, cz: i32, mesh: &ChunkMesh) {
        let key = (cx, sy, cz);
        self.remove_section(key);
        if mesh.indices.is_empty() {
            return;
        }
        let section_y = (sy * 16) as f32;
        let use_packed = mesh.vertices.iter().all(|v| PackedVertex::can_pack(v, section_y));
        let origin = [(cx * 16) as f32, if use_packed { section_y } else { 0.0 }, (cz * 16) as f32, 0.0];
        let packed: Vec<PackedVertex> = if use_packed { mesh.vertices.iter().map(|v| PackedVertex::pack(v, section_y)).collect() } else { Vec::new() };
        let mut solid = Vec::new();
        let mut transparent: Vec<([u32; 6], Vec3)> = Vec::new();
        for q in mesh.indices.chunks_exact(6) {
            if mesh.vertices[q[0] as usize].light[3] != 0 {
                let center = [q[0], q[1], q[2], q[5]]
                    .into_iter()
                    .map(|i| Vec3::from(mesh.vertices[i as usize].pos) - Vec3::new(0.0, section_y, 0.0))
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
        let (v_count, i_count) = (mesh.vertices.len() as u32, solid.len() as u32);
        // First page with room, else a new one (never reallocating: no copies).
        let found = self.pages.iter_mut().enumerate().find_map(|(pi, p)| {
            if p.packed != use_packed { return None; }
            let v = p.vfree.alloc(v_count)?;
            match p.ifree.alloc(i_count) {
                Some(i) => Some((pi, v, i)),
                None => {
                    p.vfree.release(v, v_count);
                    None
                }
            }
        });
        let (page, v_off, i_off) = found.unwrap_or_else(|| {
            let (vc, ic) = (PAGE_VERTS.max(v_count), PAGE_INDICES.max(i_count));
            let mut p = ArenaPage {
                packed: use_packed,
                vertices: self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("arena vertices"),
                    size: vc as u64 * if use_packed { std::mem::size_of::<PackedVertex>() } else { std::mem::size_of::<Vertex>() } as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                indices: self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("arena indices"),
                    size: ic as u64 * 4,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                vfree: FreeList::new(vc),
                ifree: FreeList::new(ic),
            };
            let (v, i) = (p.vfree.alloc(v_count).unwrap(), p.ifree.alloc(i_count).unwrap());
            self.pages.push(p);
            (self.pages.len() - 1, v, i)
        });
        let p = &self.pages[page];
        if use_packed {
            self.queue.write_buffer(&p.vertices, v_off as u64 * std::mem::size_of::<PackedVertex>() as u64, bytemuck::cast_slice(&packed));
        } else {
            self.queue.write_buffer(&p.vertices, v_off as u64 * std::mem::size_of::<Vertex>() as u64, bytemuck::cast_slice(&mesh.vertices));
        }
        self.queue.write_buffer(&p.indices, i_off as u64 * 4, bytemuck::cast_slice(&solid));

        let slot = self.free_slots.pop().unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() as u32 - 1
        });
        if slot as usize >= self.origins.len() {
            let cap = (self.origins.len() * 2).max(slot as usize + 1);
            self.origins.resize(cap, [0.0; 4]);
            let (buf, bg) = make_origins(&self.device, &self.chunk_layout, cap);
            self.queue.write_buffer(&buf, 0, bytemuck::cast_slice(&self.origins));
            self.origins_buf = buf;
            self.origins_bg = bg;
        }
        self.origins[slot as usize] = origin;
        self.queue.write_buffer(&self.origins_buf, slot as u64 * 16, bytemuck::cast_slice(&origin));

        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for v in &mesh.vertices {
            lo = lo.min(Vec3::from(v.pos));
            hi = hi.max(Vec3::from(v.pos));
        }
        let o = Vec3::new(origin[0], 0.0, origin[2]);
        self.slots[slot as usize] = Some(GpuChunk {
            page: page as u32,
            v_off,
            v_count,
            i_off,
            index_count: i_count,
            solid_count,
            transparent,
            sorted_at: Vec3::NAN,
            key,
            min: lo + o,
            max: hi + o,
        });
        self.index.insert(key, slot);
    }

    /// Grows the indirect buffer (contents are rewritten every frame, so no copy).
    fn ensure_indirect(&mut self, cmds: usize) {
        if cmds > self.indirect_cap {
            self.indirect_cap = cmds.next_power_of_two();
            self.indirect = make_indirect(&self.device, self.indirect_cap);
        }
    }

    /// Issues `self.cmds[start..start + n]` (all in arena page `page`); `ind_base` is where this
    /// frame's commands start in the indirect buffer.
    fn issue(&self, pass: &mut wgpu::RenderPass<'_>, page: u32, start: u32, n: u32, ind_base: u32, translucent: bool) {
        let p = &self.pages[page as usize];
        pass.set_pipeline(match (p.packed, translucent) {
            (true, false) => &self.pipeline, (true, true) => &self.translucent_pipeline,
            (false, false) => &self.full_pipeline, (false, true) => &self.full_translucent_pipeline,
        });
        pass.set_vertex_buffer(0, p.vertices.slice(..));
        pass.set_index_buffer(p.indices.slice(..), wgpu::IndexFormat::Uint32);
        if self.use_indirect {
            pass.multi_draw_indexed_indirect(&self.indirect, (ind_base + start) as u64 * 20, n);
        } else {
            for c in &self.cmds[start as usize..(start + n) as usize] {
                pass.draw_indexed(c[2]..c[2] + c[0], c[3] as i32, c[4]..c[4] + 1);
            }
        }
    }

    /// Draws the world (when `cam` is set) and then the GUI batch on top.
    pub fn render(
        &mut self,
        cam: Option<&Camera>,
        gui: Option<(&GuiBatch, Scaled)>,
        assets: &AssetSource,
    ) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let t0 = std::time::Instant::now();
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
            if let Some(frame) = &self.avatar_frame {
                self.avatar.encode_gui(&self.queue, &mut enc, &view, &self.depth, self.config.width as f32, self.config.height as f32, frame);
            }
        }
        self.queue.submit([enc.finish()]);
        self.timing.0 += 1;
        self.timing.1 += t0.elapsed();
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
            if let Some(frame) = &self.avatar_frame {
                self.avatar.encode_gui(&self.queue, &mut enc, &view, &depth, w as f32, h as f32, frame);
            }
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

        // Visible opaque draws, one batch per arena page (order is irrelevant under depth testing).
        self.cmds.clear();
        self.runs.clear();
        self.drawn_chunks = 0;
        for page in 0..self.pages.len() as u32 {
            let start = self.cmds.len() as u32;
            for (slot, c) in self.slots.iter().enumerate() {
                let Some(c) = c else { continue };
                if c.page != page || !aabb_visible(&planes, c.min - cam.pos, c.max - cam.pos) {
                    continue;
                }
                self.drawn_chunks += 1;
                if c.solid_count > 0 {
                    self.cmds.push([c.solid_count, 1, c.i_off, c.v_off, slot as u32]);
                }
            }
            let n = self.cmds.len() as u32 - start;
            if n > 0 {
                self.runs.push((page, start, n));
            }
        }
        self.ensure_indirect(self.slots.len() * 2);
        if self.use_indirect && !self.cmds.is_empty() {
            self.queue.write_buffer(&self.indirect, 0, bytemuck::cast_slice(&self.cmds));
        }

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
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(1, &self.origins_bg, &[]);
            for &(page, start, n) in &self.runs {
                self.issue(&mut pass, page, start, n, 0, false);
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
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, c)| {
                let c = c.as_ref()?;
                (!c.transparent.is_empty() && aabb_visible(&planes, c.min - cam.pos, c.max - cam.pos))
                    .then(|| (slot, ((c.min + c.max) * 0.5 - cam.pos).length_squared()))
            })
            .collect();
        translucent.sort_by(|a, b| b.1.total_cmp(&a.1));
        for &(slot, _) in &translucent {
            let c = self.slots[slot].as_mut().unwrap();
            // Vanilla re-sorts translucent quads only after the camera moved more than a block.
            if c.sorted_at.is_finite() && (cam.pos - c.sorted_at).length_squared() <= 1.0 {
                continue;
            }
            c.sorted_at = cam.pos;
            let relative = cam.pos - Vec3::new((c.key.0 * 16) as f32, (c.key.1 * 16) as f32, (c.key.2 * 16) as f32);
            c.transparent
                .sort_by(|a, b| (b.1 - relative).length_squared().total_cmp(&(a.1 - relative).length_squared()));
            let indices: Vec<u32> = c.transparent.iter().flat_map(|(q, _)| q.iter().copied()).collect();
            self.queue.write_buffer(
                &self.pages[c.page as usize].indices,
                (c.i_off + c.solid_count) as u64 * 4,
                bytemuck::cast_slice(&indices),
            );
        }
        if !translucent.is_empty() {
            // Back-to-front across the whole world: batch only consecutive sections of the same page.
            let base = self.slots.len() as u32;
            self.cmds.clear();
            self.runs.clear();
            for &(slot, _) in &translucent {
                let c = self.slots[slot].as_ref().unwrap();
                self.cmds.push([c.index_count - c.solid_count, 1, c.i_off + c.solid_count, c.v_off, slot as u32]);
                let i = self.cmds.len() as u32 - 1;
                match self.runs.last_mut() {
                    Some(r) if r.0 == c.page => r.2 += 1,
                    _ => self.runs.push((c.page, i, 1)),
                }
            }
            if self.use_indirect {
                self.queue.write_buffer(&self.indirect, base as u64 * 20, bytemuck::cast_slice(&self.cmds));
            }
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
            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(1, &self.origins_bg, &[]);
            for &(page, start, n) in &self.runs {
                self.issue(&mut pass, page, start, n, base, true);
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

fn make_origins(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, slots: usize) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("section origins"),
        size: slots as u64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("section origins"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
    });
    (buf, bg)
}

fn make_indirect(device: &wgpu::Device, cmds: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("draw commands"),
        size: cmds as u64 * 20,
        usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if std::env::var_os("RUSTMC_RENDER_TIMING").is_some() && self.timing.0 > 0 {
            let arena: u64 = self.pages.iter().map(|p| p.vertices.size() + p.indices.size()).sum();
            let used: u64 = self.slots.iter().flatten().map(|c| c.v_count as u64 * if self.pages[c.page as usize].packed { std::mem::size_of::<PackedVertex>() } else { std::mem::size_of::<Vertex>() } as u64 + c.index_count as u64 * 4).sum();
            eprintln!(
                "render cpu (encode+submit): {:.3} ms/frame over {} frames; sections {} pages {} arena {:.1} MiB used {:.1} MiB; multi_draw_indirect={}",
                self.timing.1.as_secs_f64() * 1e3 / self.timing.0 as f64,
                self.timing.0,
                self.index.len(),
                self.pages.len(),
                arena as f64 / 1048576.0,
                used as f64 / 1048576.0,
                self.use_indirect
            );
        }
    }
}

#[cfg(test)]
mod arena_tests {
    use super::*;

    #[test]
    fn free_list_reuses_and_coalesces_ranges() {
        let mut f = FreeList::new(12);
        let a = f.alloc(3).unwrap();
        let b = f.alloc(4).unwrap();
        let c = f.alloc(5).unwrap();
        assert_eq!((a, b, c), (0, 3, 7));
        assert_eq!(f.alloc(1), None);
        f.release(b, 4);
        f.release(a, 3);
        f.release(c, 5);
        assert_eq!(f.alloc(12), Some(0));
    }

    #[test]
    fn packed_vertex_preserves_uv_light_and_layout() {
        let v = crate::mesh::Vertex { pos: [1.5, 33.25, -0.5], uv: [0.12345678, 0.9876543], color: [30, 40, 50, 255], light: [112, 240, 128, 255] };
        let p = PackedVertex::pack(&v, 32.0);
        assert_eq!(std::mem::size_of::<PackedVertex>(), 20);
        assert_eq!(p.pos.map(|x| x as f32 / 2048.0 - 4.0), [1.5, 1.25, -0.5]);
        assert_eq!(p.uv, v.uv);
        assert_eq!(p.light, [112, 240]);
        assert_eq!(p.color, [30, 40, 50, 64]);
        assert!(PackedVertex::can_pack(&v, 32.0));
        let mut irregular = v;
        irregular.pos[0] = 1.0 / 3.0;
        assert!(!PackedVertex::can_pack(&irregular, 32.0));
        irregular.pos[0] = 28.0;
        assert!(!PackedVertex::can_pack(&irregular, 32.0));
    }

    #[test]
    fn chunk_pipeline_validates_on_headless_gpu() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            eprintln!("no headless adapter; GPU pipeline test skipped");
            return;
        };
        let (device, _) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        for (source, stride, attributes) in [
            (include_str!("shader.wgsl"), std::mem::size_of::<PackedVertex>(), &CHUNK_ATTRIBUTES),
            (include_str!("shader_full.wgsl"), std::mem::size_of::<Vertex>(), &FULL_ATTRIBUTES),
        ] {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chunk shader test"), source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("chunk pipeline test"), layout: None,
            vertex: wgpu::VertexState {
                module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: stride as u64,
                    step_mode: wgpu::VertexStepMode::Vertex, attributes }],
            },
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"),
                compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())] }),
            multiview: None, cache: None,
        });
        }
    }

}
