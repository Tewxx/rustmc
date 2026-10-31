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
    /// x = fog start, y = fog end, z = 1 when the target is sRGB (shader must linearize),
    /// w = GL_EXP density from `EntityRenderer.setupFog` (0 keeps the linear start/end fog).
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
/// Per-frame constants are uploaded into a ring of slots, so a frame never overwrites constants
/// that a frame still in flight is reading. The stride is the uniform dynamic-offset alignment;
/// `Limits::default` asks for 256 and no backend reports more.
const GLOBALS_STRIDE: u64 = 256;
const GLOBALS_SIZE: u64 = std::mem::size_of::<Globals>() as u64;
/// Dynamic buffers cycle through this many copies; three covers the default frame latency of two
/// plus one, so a write never has to wait on the immediately preceding frames' reads.
const FRAMES_IN_FLIGHT: usize = 3;

struct GpuChunk {
    page: u32,
    /// Vertex / index element offsets inside the page; solid indices first, then sorted translucent ones.
    v_off: u32,
    v_count: u32,
    v_capacity: u32,
    i_off: u32,
    index_count: u32,
    i_capacity: u32,
    solid_count: u32,
    transparent: Vec<([u32; 6], Vec3)>,
    /// Camera position the translucent quads were last sorted/uploaded for (NaN = never).
    sorted_at: Vec3,
    key: (i32, i32, i32),
    /// Models that protrude beyond their section must not be hidden by section connectivity.
    cull_safe: bool,
    min: Vec3,
    max: Vec3,
}

type CaveResult = Option<((i32, i32, i32), i32, Arc<crate::visibility::VisibleSections>)>;

fn cave_hidden(cull_safe: bool, key: (i32, i32, i32), cave: &CaveResult) -> bool {
    cull_safe && cave.as_ref().is_some_and(|(start, radius, visible)| crate::visibility::culls(visible, *start, *radius, key))
}

/// `DrawIndexedIndirectArgs` layout.
type DrawCmd = [u32; 5];

/// Pipeline and arena page already bound in a pass. The four chunk pipelines share one layout, so
/// re-setting the same pipeline (or the same page's vertex/index buffers) between sections is
/// redundant and skipped.
#[derive(Default)]
struct PassState {
    pipeline: Option<(bool, bool)>,
    page: Option<u32>,
}

#[derive(Default)]
pub struct FrameTiming {
    pub acquire: std::time::Duration,
    pub submit: std::time::Duration,
    pub present: std::time::Duration,
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
    pub frame_timing: FrameTiming,
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
    /// Increments once per recorded frame; selects the constants slot and the indirect buffer.
    frame_index: u64,
    chunk_layout: wgpu::BindGroupLayout,
    origins_buf: wgpu::Buffer,
    origins_bg: wgpu::BindGroup,
    origins: Vec<[f32; 4]>,
    pages: Vec<ArenaPage>,
    cave_cull: crate::visibility::CaveCull,
    use_cave_cull: bool,
    /// Slot-indexed sections (slot = instance index in the shader); `index` maps keys to slots.
    slots: Vec<Option<GpuChunk>>,
    free_slots: Vec<u32>,
    index: HashMap<(i32, i32, i32), u32>,
    indirect: [wgpu::Buffer; FRAMES_IN_FLIGHT],
    indirect_cap: usize,
    use_indirect: bool,
    cmds: Vec<DrawCmd>,
    page_cmds: Vec<Vec<DrawCmd>>,
    translucent: Vec<(usize, f32)>,
    sorted_quads: Vec<(f32, usize, [u32; 6], Vec3)>,
    sorted_indices: Vec<u32>,
    uploads: wgpu::util::StagingBelt,
    upload_encoder: Option<wgpu::CommandEncoder>,
    pool_uploads: bool,
    /// (frames, CPU time in encode + submit excluding drawable acquire/present); printed on drop with RUSTMC_RENDER_TIMING.
    timing: (u32, std::time::Duration),
    runs: Vec<(u32, u32, u32)>,
    depth: wgpu::TextureView,
    lightmap: wgpu::Texture,
    lightmap_pixels: Option<[u8; 1024]>,
    atlas: wgpu::Texture,
    gui: GuiRenderer,
    uncapped_mode: wgpu::PresentMode,
    /// Whether the surface advertises Immediate or Mailbox at all (independent of the override).
    uncapped_supported: bool,
    warned_uncapped: bool,
    pub adapter_info: wgpu::AdapterInfo,
    pub render_distance_blocks: f32,
    pub drawn_chunks: usize,
}

impl Renderer {
    pub async fn new(window: Arc<winit::window::Window>, models: &BlockModels, assets: &AssetSource) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
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
        let preferred_mode = std::env::var("RUSTMC_PRESENT_MODE").ok().and_then(|s| match s.as_str() {
            "immediate" => Some(wgpu::PresentMode::Immediate),
            "mailbox" => Some(wgpu::PresentMode::Mailbox),
            "fifo" => Some(wgpu::PresentMode::Fifo),
            _ => None,
        });
        // Mailbox reduces present overhead on Windows/Vulkan while retaining uncapped rendering.
        let uncapped_modes = if cfg!(target_os = "windows") && adapter.get_info().backend == wgpu::Backend::Vulkan {
            [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
        } else { [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox] };
        let present_mode = choose_present_mode(preferred_mode, &caps.present_modes, uncapped_modes);
        let uncapped_supported = uncapped_available(&caps.present_modes);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: std::env::var("RUSTMC_FRAME_LATENCY").ok().and_then(|s| s.parse::<u32>().ok())
                // Metal with latency 2 pins even Immediate presentation to 60 fps (measured: 60.0 vs 159 fps with 1).
                .unwrap_or(if cfg!(target_os = "macos") { 1 } else { 2 }).clamp(1, 3),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        // Unconditional: the effective present path is the first thing to check when the frame rate
        // is pinned to the monitor refresh.
        eprintln!(
            "render: backend={:?} adapter={} present_mode={:?} desired_maximum_frame_latency={} uncapped_supported={}",
            adapter.get_info().backend,
            adapter.get_info().name,
            config.present_mode,
            config.desired_maximum_frame_latency,
            uncapped_supported
        );

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
            &crate::lightmap::build(crate::lightmap::sun_brightness(0.0), 0.0, 0.0, 0, None, false),
        );
        // Vanilla's lightmap uses GL_LINEAR + clamp so AO-averaged values blend.
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: GLOBALS_STRIDE * FRAMES_IN_FLIGHT as u64,
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
                        has_dynamic_offset: true,
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
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }),
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
        let indirect = std::array::from_fn(|_| make_indirect(&device, 4096));

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
            frame_timing: FrameTiming::default(),
            sky: crate::sky::Sky::overworld(6000, 0., 0.8, 12),
            sky_renderer,
            avatar,
            avatar_frame: None,
            gui,
            uncapped_mode: present_mode,
            uncapped_supported,
            warned_uncapped: false,
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
            frame_index: 0,
            chunk_layout,
            depth,
            lightmap,
            lightmap_pixels: None,
            atlas: atlas_tex,
            origins_buf,
            origins_bg,
            origins: vec![[0.0; 4]; INITIAL_SLOTS],
            pages: Vec::new(),
            cave_cull: crate::visibility::CaveCull::default(),
            use_cave_cull: std::env::var_os("RUSTMC_NO_CAVE_CULL").is_none(),
            slots: Vec::new(),
            free_slots: Vec::new(),
            index: HashMap::new(),
            indirect,
            indirect_cap: 4096,
            use_indirect,
            cmds: Vec::new(),
            page_cmds: Vec::new(),
            translucent: Vec::new(),
            sorted_quads: Vec::new(),
            sorted_indices: Vec::new(),
            uploads: wgpu::util::StagingBelt::new(1024 * 1024),
            upload_encoder: None,
            pool_uploads: std::env::var_os("RUSTMC_NO_UPLOAD_POOL").is_none(),
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
        self.cave_cull.clear();
        self.slots.clear();
        self.free_slots.clear();
        self.index.clear();
    }

    fn remove_section(&mut self, key: (i32, i32, i32)) {
        if let Some(slot) = self.index.remove(&key) {
            let c = self.slots[slot as usize].take().unwrap();
            let p = &mut self.pages[c.page as usize];
            p.vfree.release(c.v_off, c.v_capacity);
            p.ifree.release(c.i_off, c.i_capacity);
            self.free_slots.push(slot);
        }
    }

    /// An in-flight rebuild must not use an obsolete occlusion graph.
    pub fn invalidate_section_visibility(&mut self, cx: i32, sy: i32, cz: i32) {
        self.cave_cull.update((cx, sy, cz), None);
    }

    pub fn upload_section(&mut self, cx: i32, sy: i32, cz: i32, mesh: &ChunkMesh) {
        let prepared = crate::mesh::PreparedSection::new(mesh, sy);
        self.upload_prepared_section(cx, sy, cz, mesh, prepared);
    }

    pub fn upload_prepared_section(&mut self, cx: i32, sy: i32, cz: i32, mesh: &ChunkMesh, prepared: crate::mesh::PreparedSection) {
        let key = (cx, sy, cz);
        self.cave_cull.update(key, mesh.visibility);
        if mesh.indices.is_empty() {
            self.remove_section(key);
            return;
        }
        let section_y = (sy * 16) as f32;
        let crate::mesh::PreparedSection { packed, use_packed, indices: solid, solid_count, transparent, min: lo, max: hi, cull_safe } = prepared;
        let origin = [(cx * 16) as f32, if use_packed { section_y } else { 0.0 }, (cz * 16) as f32, 0.0];
        let (v_count, i_count) = (mesh.vertices.len() as u32, solid.len() as u32);
        let reuse = self.index.get(&key).and_then(|&slot| {
            let c = self.slots[slot as usize].as_ref()?;
            (self.pages[c.page as usize].packed == use_packed && c.v_capacity >= v_count && c.i_capacity >= i_count)
                .then_some((slot, c.page as usize, c.v_off, c.i_off, c.v_capacity, c.i_capacity))
        });
        if reuse.is_none() { self.remove_section(key); }
        // First page with room, else a new one (never reallocating: no copies).
        let found = reuse.map(|(_, page, v, i, _, _)| (page, v, i)).or_else(|| self.pages.iter_mut().enumerate().find_map(|(pi, p)| {
            if p.packed != use_packed { return None; }
            let v = p.vfree.alloc(v_count)?;
            match p.ifree.alloc(i_count) {
                Some(i) => Some((pi, v, i)),
                None => {
                    p.vfree.release(v, v_count);
                    None
                }
            }
        }));
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
        let vertices = self.pages[page].vertices.clone();
        let indices = self.pages[page].indices.clone();
        if use_packed {
            self.upload_buffer(&vertices, v_off as u64 * std::mem::size_of::<PackedVertex>() as u64, bytemuck::cast_slice(&packed));
        } else {
            self.upload_buffer(&vertices, v_off as u64 * std::mem::size_of::<Vertex>() as u64, bytemuck::cast_slice(&mesh.vertices));
        }
        self.upload_buffer(&indices, i_off as u64 * 4, bytemuck::cast_slice(&solid));

        let slot = reuse.map(|r| r.0).unwrap_or_else(|| self.free_slots.pop().unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() as u32 - 1
        }));
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

        let o = Vec3::new(origin[0], 0.0, origin[2]);
        self.slots[slot as usize] = Some(GpuChunk {
            page: page as u32,
            v_off,
            v_count,
            v_capacity: reuse.map_or(v_count, |r| r.4),
            i_off,
            index_count: i_count,
            i_capacity: reuse.map_or(i_count, |r| r.5),
            solid_count,
            transparent,
            sorted_at: Vec3::NAN,
            key,
            cull_safe,
            min: lo + o,
            max: hi + o,
        });
        self.index.insert(key, slot);
    }

    /// Grows every indirect ring buffer at once (contents are rewritten every frame, so no copy).
    fn ensure_indirect(&mut self, cmds: usize) {
        let cap = grown_indirect_capacity(self.indirect_cap, cmds);
        if cap != self.indirect_cap {
            self.indirect_cap = cap;
            self.indirect = std::array::from_fn(|_| make_indirect(&self.device, cap));
        }
    }

    fn indirect_buffer(&self) -> &wgpu::Buffer {
        &self.indirect[ring_slot(self.frame_index, FRAMES_IN_FLIGHT)]
    }

    fn upload_buffer(&mut self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        if data.is_empty() { return; }
        if !self.pool_uploads { self.queue.write_buffer(buffer, offset, data); return; }
        let encoder = self.upload_encoder.get_or_insert_with(|| self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mesh uploads") }));
        self.uploads.write_buffer(encoder, buffer, offset, wgpu::BufferSize::new(data.len() as u64).unwrap(), &self.device).copy_from_slice(data);
    }

    fn finish_uploads(&mut self) -> Option<wgpu::CommandBuffer> {
        let encoder = self.upload_encoder.take()?;
        self.uploads.finish();
        Some(encoder.finish())
    }

    /// Issues `self.cmds[start..start + n]` (all in arena page `page`). `state` skips pipeline and
    /// vertex/index buffer calls that are already in effect, so a run continuing on the same page
    /// costs only the draw itself.
    fn issue(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut PassState, page: u32, start: u32, n: u32, translucent: bool) {
        let p = &self.pages[page as usize];
        let pipeline = (p.packed, translucent);
        if state.pipeline != Some(pipeline) {
            pass.set_pipeline(match pipeline {
                (true, false) => &self.pipeline, (true, true) => &self.translucent_pipeline,
                (false, false) => &self.full_pipeline, (false, true) => &self.full_translucent_pipeline,
            });
            state.pipeline = Some(pipeline);
        }
        if state.page != Some(page) {
            pass.set_vertex_buffer(0, p.vertices.slice(..));
            pass.set_index_buffer(p.indices.slice(..), wgpu::IndexFormat::Uint32);
            state.page = Some(page);
        }
        if self.use_indirect {
            pass.multi_draw_indexed_indirect(self.indirect_buffer(), start as u64 * 20, n);
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
        self.frame_timing = FrameTiming::default();
        let acquire = std::time::Instant::now();
        let frame = self.surface.get_current_texture();
        self.frame_timing.acquire = acquire.elapsed();
        let frame = frame?;
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
                self.avatar.encode_gui(&self.device, &self.queue, &mut enc, &view, &self.depth, self.config.width as f32, self.config.height as f32, frame);
            }
        }
        let commands = enc.finish();
        let submit = std::time::Instant::now();
        let uploads = self.finish_uploads();
        self.queue.submit(uploads.into_iter().chain(std::iter::once(commands)));
        self.uploads.recall();
        self.frame_timing.submit = submit.elapsed();
        self.timing.0 += 1;
        self.timing.1 += t0.elapsed();
        let present = std::time::Instant::now();
        frame.present();
        self.frame_timing.present = present.elapsed();
        Ok(())
    }

    /// Switches between vsync (Fifo) and uncapped presentation. Logs the effective present mode
    /// and warns once when the surface cannot present without vsync.
    pub fn set_vsync(&mut self, on: bool) {
        let mode = if on { wgpu::PresentMode::Fifo } else { self.uncapped_mode };
        if self.config.present_mode != mode {
            self.config.present_mode = mode;
            self.surface.configure(&self.device, &self.config);
            eprintln!(
                "render: vsync={on} present_mode={:?} desired_maximum_frame_latency={}",
                self.config.present_mode, self.config.desired_maximum_frame_latency
            );
        }
        if !on && !self.uncapped_supported && !self.warned_uncapped {
            self.warned_uncapped = true;
            eprintln!(
                "render: warning: vsync is off but the surface supports neither Immediate nor Mailbox; \
                 falling back to vsynced Fifo, so the frame rate stays capped at the display refresh"
            );
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
                self.avatar.encode_gui(&self.device, &self.queue, &mut enc, &view, &depth, w as f32, h as f32, frame);
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
        let uploads = self.finish_uploads();
        self.queue.submit(uploads.into_iter().chain(std::iter::once(enc.finish())));
        self.uploads.recall();
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
        self.frame_index = self.frame_index.wrapping_add(1);
        let frame_slot = ring_slot(self.frame_index, FRAMES_IN_FLIGHT);
        let globals_offset = (frame_slot as u64 * GLOBALS_STRIDE) as u32;
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
                self.sky.fog_density,
            ],
        };
        self.queue.write_buffer(&self.globals_buf, globals_offset as u64, bytemuck::bytes_of(&globals));
        let planes = frustum_planes(view_proj);
        let cave_visible = if self.use_cave_cull && cam.pos.is_finite() && self.render_distance_blocks.is_finite() {
            let start = ((cam.pos.x.floor() as i32) >> 4, (cam.pos.y.floor() as i32) >> 4, (cam.pos.z.floor() as i32) >> 4);
            let radius = ((self.render_distance_blocks * std::f32::consts::SQRT_2 / 16.0).ceil() as i32).saturating_add(2);
            self.cave_cull.visible(start, radius).map(|visible| (start, radius, visible))
        } else { None };

        // Keep page/slot order, including equal-depth ties, while testing visibility only once.
        self.cmds.clear();
        self.runs.clear();
        self.page_cmds.resize_with(self.pages.len(), Vec::new);
        for cmds in &mut self.page_cmds { cmds.clear(); }
        let mut translucent = std::mem::take(&mut self.translucent);
        translucent.clear();
        self.drawn_chunks = 0;
        for (slot, c) in self.slots.iter().enumerate() {
            let Some(c) = c else { continue };
            if cave_hidden(c.cull_safe, c.key, &cave_visible) || !aabb_visible(&planes, c.min - cam.pos, c.max - cam.pos) { continue; }
            self.drawn_chunks += 1;
            if c.solid_count > 0 {
                self.page_cmds[c.page as usize].push([c.solid_count, 1, c.i_off, c.v_off, slot as u32]);
            }
            if !c.transparent.is_empty() {
                translucent.push((slot, ((c.min + c.max) * 0.5 - cam.pos).length_squared()));
            }
        }
        for (page, cmds) in self.page_cmds.iter().enumerate() {
            if cmds.is_empty() { continue; }
            let start = self.cmds.len() as u32;
            self.cmds.extend_from_slice(cmds);
            self.runs.push((page as u32, start, cmds.len() as u32));
        }
        self.ensure_indirect(self.cmds.len() + translucent.len());

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
            pass.set_bind_group(0, &self.globals_bg, &[globals_offset]);
            pass.set_bind_group(1, &self.origins_bg, &[]);
            let mut state = PassState::default();
            for &(page, start, n) in &self.runs {
                self.issue(&mut pass, &mut state, page, start, n, false);
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
        translucent.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for &(slot, _) in &translucent {
            let c = self.slots[slot].as_mut().unwrap();
            // Vanilla re-sorts translucent quads only after the camera moved more than a block.
            if c.sorted_at.is_finite() && (cam.pos - c.sorted_at).length_squared() <= 1.0 {
                continue;
            }
            c.sorted_at = cam.pos;
            let relative = cam.pos - Vec3::new((c.key.0 * 16) as f32, (c.key.1 * 16) as f32, (c.key.2 * 16) as f32);
            self.sorted_quads.clear();
            self.sorted_quads.extend(c.transparent.iter().enumerate().map(|(i, &(q, center))| ((center - relative).length_squared(), i, q, center)));
            self.sorted_quads.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            self.sorted_indices.clear();
            for (dst, &(_, _, q, center)) in c.transparent.iter_mut().zip(&self.sorted_quads) {
                *dst = (q, center);
                self.sorted_indices.extend_from_slice(&q);
            }
            let buffer = self.pages[c.page as usize].indices.clone();
            let offset = (c.i_off + c.solid_count) as u64 * 4;
            let indices = std::mem::take(&mut self.sorted_indices);
            self.upload_buffer(&buffer, offset, bytemuck::cast_slice(&indices));
            self.sorted_indices = indices;
        }
        if !translucent.is_empty() {
            // Back-to-front across the whole world: batch only consecutive sections of the same page.
            // The translucent commands are appended to the opaque ones so both share one upload.
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
            pass.set_bind_group(0, &self.globals_bg, &[globals_offset]);
            pass.set_bind_group(1, &self.origins_bg, &[]);
            let mut state = PassState::default();
            for &(page, start, n) in &self.runs {
                self.issue(&mut pass, &mut state, page, start, n, true);
            }
        }
        // One upload per frame for the whole command list (opaque runs first, then back-to-front
        // translucent ones); it goes to this frame's ring slot so in-flight draws keep their data.
        if self.use_indirect && !self.cmds.is_empty() {
            let indirect = self.indirect_buffer().clone();
            self.queue.write_buffer(&indirect, 0, bytemuck::cast_slice(&self.cmds));
        }
        self.translucent = translucent;
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
    pub fn set_lightmap(&mut self, rgba: &[u8; 1024]) {
        if self.lightmap_pixels.as_ref() == Some(rgba) { return; }
        self.lightmap_pixels = Some(*rgba);
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

pub(crate) fn frustum_planes(m: Mat4) -> [Vec4; 5] {
    let r = |i| m.row(i);
    // Left, right, bottom, top, near (far is covered by fog distance).
    [r(3) + r(0), r(3) - r(0), r(3) + r(1), r(3) - r(1), r(2)]
}

pub(crate) fn sphere_visible(planes: &[Vec4; 5], center: Vec3, radius: f32) -> bool {
    if !center.is_finite() || !radius.is_finite() { return true; }
    planes.iter().all(|p| !p.is_finite() || p.truncate().dot(center) + p.w >= -(radius + 0.125) * p.truncate().length())
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

/// An explicit `RUSTMC_PRESENT_MODE` wins when the surface supports it, otherwise the first
/// uncapped mode the surface advertises (Mailbox on Windows/Vulkan), else vsynced Fifo.
fn choose_present_mode(preferred: Option<wgpu::PresentMode>, available: &[wgpu::PresentMode], uncapped: [wgpu::PresentMode; 2]) -> wgpu::PresentMode {
    preferred
        .filter(|m| available.contains(m))
        .or_else(|| uncapped.into_iter().find(|m| available.contains(m)))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

/// Whether the surface offers a way to present without electronic vsync at all.
fn uncapped_available(available: &[wgpu::PresentMode]) -> bool {
    available.contains(&wgpu::PresentMode::Immediate) || available.contains(&wgpu::PresentMode::Mailbox)
}

/// Slot of `frame` inside a ring of `ring` dynamic buffers.
fn ring_slot(frame: u64, ring: usize) -> usize {
    debug_assert!(ring > 0);
    (frame % ring as u64) as usize
}

/// Indirect command capacity grows to the next power of two and never shrinks; the ring buffers
/// are recreated (contents are rewritten every frame) only when a frame needs more commands.
fn grown_indirect_capacity(current: usize, required: usize) -> usize {
    if required > current { required.next_power_of_two().max(8) } else { current }
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
    fn cave_filter_keeps_unknown_out_of_range_and_protruding_geometry() {
        let visible = Arc::new(crate::visibility::VisibleSections::from_keys((0, 8, 0), 2, [(0, 8, 0)]));
        let cave = Some(((0, 8, 0), 2, visible));
        assert!(cave_hidden(true, (1, 8, 0), &cave));
        assert!(!cave_hidden(false, (1, 8, 0), &cave));
        assert!(!cave_hidden(true, (3, 8, 0), &cave));
        assert!(!cave_hidden(true, (1, 8, 0), &None));
    }

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
        let Some(adapter) = crate::headless_adapter(&instance) else { return };
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

    /// The per-frame constants ring binds one 112-byte window with a dynamic offset; make sure the
    /// layout, entry size and furthest offset validate on a real device.
    #[test]
    fn globals_dynamic_offset_binding_validates_on_headless_gpu() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = crate::headless_adapter(&instance) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals ring test"),
            size: GLOBALS_STRIDE * FRAMES_IN_FLIGHT as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals ring test"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals ring test"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(GLOBALS_SIZE),
                }),
            }],
        });
        for slot in 0..FRAMES_IN_FLIGHT as u64 {
            queue.write_buffer(&buf, slot * GLOBALS_STRIDE, &[0u8; GLOBALS_SIZE as usize]);
        }
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("globals ring test"),
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var<uniform> g: vec4<f32>;\n\
                 @vertex fn vs_main() -> @builtin(position) vec4<f32> { return g; }\n\
                 @fragment fn fs_main() -> @location(0) vec4<f32> { return g; }\n"
                    .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("globals ring test"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("globals ring test"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview: None,
            cache: None,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("globals ring test"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("globals ring test"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&pipeline);
            let last = ((FRAMES_IN_FLIGHT as u64 - 1) * GLOBALS_STRIDE) as u32;
            pass.set_bind_group(0, &bg, &[last]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
        device.poll(wgpu::Maintain::Wait);
    }

    #[test]
    fn present_mode_prefers_override_then_uncapped_then_fifo() {
        use wgpu::PresentMode::*;
        let all = [Fifo, Immediate, Mailbox];
        assert_eq!(choose_present_mode(Some(Immediate), &all, [Mailbox, Immediate]), Immediate);
        // An override the surface cannot honour is ignored, not forced.
        assert_eq!(choose_present_mode(Some(Mailbox), &[Fifo, Immediate], [Immediate, Mailbox]), Immediate);
        assert_eq!(choose_present_mode(None, &all, [Mailbox, Immediate]), Mailbox);
        // Windows/Vulkan uses Mailbox first; everything else Immediate first.
        assert_eq!(choose_present_mode(None, &[Fifo, Immediate, Mailbox], [Immediate, Mailbox]), Immediate);
        // No uncapped mode advertised: vsynced Fifo, and the warning path is taken.
        assert_eq!(choose_present_mode(None, &[Fifo], [Immediate, Mailbox]), Fifo);
        assert_eq!(choose_present_mode(Some(Immediate), &[Fifo], [Immediate, Mailbox]), Fifo);
        assert!(!uncapped_available(&[Fifo]));
        assert!(uncapped_available(&[Fifo, Mailbox]));
        assert!(uncapped_available(&[Fifo, Immediate]));
    }

    #[test]
    fn indirect_capacity_grows_by_powers_of_two_and_never_shrinks() {
        assert_eq!(grown_indirect_capacity(4096, 0), 4096);
        assert_eq!(grown_indirect_capacity(4096, 4096), 4096);
        assert_eq!(grown_indirect_capacity(4096, 4097), 8192);
        assert_eq!(grown_indirect_capacity(8, 9), 16);
        // A tiny first frame still leaves a usable buffer.
        assert_eq!(grown_indirect_capacity(0, 1), 8);
        assert_eq!(grown_indirect_capacity(0, 3), 8);
    }

    #[test]
    fn frame_ring_slots_cycle_and_globals_slots_are_aligned() {
        let slots: Vec<usize> = (0..7).map(|f| ring_slot(f, FRAMES_IN_FLIGHT)).collect();
        assert_eq!(slots, vec![0, 1, 2, 0, 1, 2, 0]);
        // A constants slot must fit its uniform and start on the alignment wgpu requires.
        assert!(GLOBALS_SIZE <= GLOBALS_STRIDE);
        assert_eq!(GLOBALS_STRIDE % 256, 0);
        let end = GLOBALS_STRIDE * (FRAMES_IN_FLIGHT as u64 - 1) + GLOBALS_SIZE;
        assert!(end <= GLOBALS_STRIDE * FRAMES_IN_FLIGHT as u64);
    }

}
