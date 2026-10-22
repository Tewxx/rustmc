//! The local player's body (`RenderPlayer` + `LayerHeldItem`), first-person hand and held item
//! (`ItemRenderer.renderItemInFirstPerson`) and the block selection box.
use crate::entity_model::{lights_in, standard_lighting, ModelQuad, PlayerModel, Pose};
use crate::items::{ItemModel, TransformType};
use crate::Camera;
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3, Vec4};
use mc_assets::AssetSource;
use mc_core::Aabb;
use std::sync::Arc;

/// An item stack's render data: its model and `getColorFromItemStack` per tint index.
#[derive(Clone)]
pub struct HeldItem {
    pub model: Arc<ItemModel>,
    pub tints: [u32; 4],
    /// `Block.getRenderType() == 2` (chests): LayerHeldItem's extra transform.
    pub entity_block: bool,
}

#[derive(Default)]
pub struct AvatarFrame {
    /// Interpolated feet position.
    pub pos: Vec3,
    pub body_yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    /// `ticksExisted + partialTicks`
    pub age: f32,
    /// `getSwingProgress(partialTicks)`
    pub swing: f32,
    pub sneaking: bool,
    pub perspective: u8,
    pub hide_hand: bool,
    pub outline: Option<Aabb>,
    pub held: Option<HeldItem>,
    /// Lightmap color at the entity (`getBrightnessForRender`).
    pub light: [f32; 3],
    /// First person: `1 - equippedProgress` interpolated.
    pub equip: f32,
    pub yaw: f32,
    pub arm_pitch: f32,
    pub arm_yaw: f32,
    /// Hurt-camera and view-bobbing transform applied to the hand pass.
    pub bob: Mat4,
    pub hand_light: [f32; 3],
    /// `ItemRenderer.itemToRender` (lags behind the selected slot during the equip animation).
    pub hand_item: Option<HeldItem>,
    pub slim: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    clip: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
    kind: f32,
}

pub struct AvatarRenderer {
    pipeline: wgpu::RenderPipeline,
    vertices: [wgpu::Buffer; 2],
    bind_group: wgpu::BindGroup,
    steve: PlayerModel,
    alex: PlayerModel,
}

const SKIN: f32 = 0.;
const ATLAS: f32 = 1.;
const LINE: f32 = 3.;

impl AvatarRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, assets: &AssetSource, atlas: wgpu::TextureView) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("avatar textures"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("avatar"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    r#"
const SRGB:bool={};
@group(0) @binding(0) var samp:sampler;
@group(0) @binding(1) var skin:texture_2d<f32>;
@group(0) @binding(2) var atlas:texture_2d<f32>;
@group(0) @binding(3) var alex:texture_2d<f32>;
struct V {{ @builtin(position) pos:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) color:vec4<f32>, @location(2) @interpolate(flat) kind:u32 }};
@vertex fn vs(@location(0) p:vec4<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) kind:f32)->V {{
 var o:V;o.pos=p;o.uv=uv;o.color=color;o.kind=u32(kind);return o;
}}
@fragment fn fs(v:V)->@location(0) vec4<f32> {{
 var c=v.color;
 if(v.kind==0u){{c*=textureSampleLevel(skin,samp,v.uv,0.0);}}
 if(v.kind==1u){{c*=textureSampleLevel(atlas,samp,v.uv,0.0);}}
 if(v.kind==2u){{c*=textureSampleLevel(alex,samp,v.uv,0.0);}}
 if(v.kind!=3u && c.a<=0.1){{discard;}}
 if(SRGB){{c=vec4<f32>(select(c.rgb/12.92,pow((c.rgb+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c.rgb>vec3<f32>(0.04045)),c.a);}}
 return c;
}}
"#,
                    format.is_srgb()
                )
                .into(),
            ),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("avatar"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("avatar"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4,3=>Float32],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let load = |path: &str| {
            assets
                .read(path)
                .and_then(|b| image::load_from_memory(&b).ok())
                .map(|i| i.to_rgba8())
                .unwrap_or_else(|| image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 255, 255])))
        };
        let steve = load("textures/entity/steve.png");
        let alex = load("textures/entity/alex.png");
        let skin = texture(device, queue, &steve).create_view(&Default::default());
        let alex_view = texture(device, queue, &alex).create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("avatar textures"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&skin) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&atlas) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&alex_view) },
            ],
        });
        let vertices = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("avatar vertices"),
                size: 65536 * std::mem::size_of::<Vertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Self { pipeline, vertices, bind_group, steve: PlayerModel::new(false), alex: PlayerModel::new(true) }
    }

    /// `hand_stage` false: body + selection box (world pass); true: first-person hand after a depth clear.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        cam: &Camera,
        aspect: f32,
        height: f32,
        far: f32,
        frame: &AvatarFrame,
        _assets: &AssetSource,
        hand_stage: bool,
    ) {
        let mut verts = Vec::new();
        let world_vp = cam.view_proj(aspect, far);
        if !hand_stage {
            if frame.perspective != 0 {
                self.body(&mut verts, frame, world_vp, cam.pos);
            }
            if let Some(bb) = frame.outline {
                outline(&mut verts, world_vp, cam.pos, bb.expand(0.002, 0.002, 0.002), height * aspect, height);
            }
        } else if frame.perspective == 0 && !frame.hide_hand {
            self.hand(&mut verts, frame, aspect, far);
        }
        if verts.is_empty() {
            return;
        }
        let buf = &self.vertices[hand_stage as usize];
        queue.write_buffer(buf, 0, bytemuck::cast_slice(&verts[..verts.len().min(65536)]));
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if hand_stage { "first person" } else { "player" }),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: if hand_stage { wgpu::LoadOp::Clear(1.) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..verts.len().min(65536) as u32, 0..1);
    }

    /// `RendererLivingEntity.doRender` for the local player, plus `LayerHeldItem`.
    fn body(&mut self, out: &mut Vec<Vertex>, f: &AvatarFrame, vp: Mat4, cam: Vec3) {
        let model = if f.slim { &mut self.alex } else { &mut self.steve };
        model.swing = f.swing;
        model.sneak = f.sneaking;
        model.held_right = if f.held.is_some() { 1 } else { 0 };
        model.set_angles(Pose {
            limb_swing: f.limb_swing,
            limb_amount: f.limb_amount.min(1.),
            age: f.age,
            head_yaw: f.head_yaw - f.body_yaw,
            head_pitch: f.pitch,
        });
        // World-space model matrix (the view is applied separately so normals stay in world space).
        let m = Mat4::from_translation(f.pos - cam)
            * Mat4::from_rotation_y((180. - f.body_yaw).to_radians())
            * Mat4::from_scale(Vec3::new(-1., -1., 1.))
            * Mat4::from_scale(Vec3::splat(0.9375))
            * Mat4::from_translation(Vec3::new(0., -1.5078125, 0.));
        let lights = lights_in(Mat3::IDENTITY);
        let kind = if f.slim { 2. } else { SKIN };
        let light = f.light;
        model.render(m, &mut |pm, q| model_quad(out, vp, pm, q, kind, lights, light, [1.; 3], false));
        if let Some(item) = &f.held {
            let mut im = m * model.right_arm.post_render() * Mat4::from_translation(Vec3::new(-0.0625, 0.4375, 0.0625));
            if item.entity_block {
                im = im
                    * Mat4::from_translation(Vec3::new(0., 0.1875, -0.3125))
                    * Mat4::from_rotation_x(20f32.to_radians())
                    * Mat4::from_rotation_y(45f32.to_radians())
                    * Mat4::from_scale(Vec3::new(-0.375, -0.375, 0.375));
            }
            if f.sneaking {
                im *= Mat4::from_translation(Vec3::new(0., 0.203125, 0.));
            }
            render_item(out, vp, im, item, TransformType::ThirdPerson, lights, light);
        }
    }

    /// `EntityRenderer.renderHand` → `ItemRenderer.renderItemInFirstPerson`.
    fn hand(&mut self, out: &mut Vec<Vertex>, f: &AvatarFrame, aspect: f32, far: f32) {
        use mc_core::math::{sin, sqrt_float as sqrt};
        use std::f32::consts::PI;
        let proj = Mat4::perspective_rh(70f32.to_radians(), aspect, 0.05, far * 2.);
        let equip = f.equip;
        let swing = f.swing;
        // rotateArroundXAndY: the light directions follow the camera's pitch/yaw.
        let lights = lights_in(Mat3::from_mat4(f.bob) * Mat3::from_rotation_x(f.pitch.to_radians()) * Mat3::from_rotation_y(f.yaw.to_radians()));
        // rotateWithPlayerRotations
        let mut m = f.bob
            * Mat4::from_rotation_x(((f.pitch - f.arm_pitch) * 0.1).to_radians())
            * Mat4::from_rotation_y(((f.yaw - f.arm_yaw) * 0.1).to_radians());
        let light = f.hand_light;
        match &f.hand_item {
            Some(item) => {
                // doItemUsedTransformations + transformFirstPersonItem
                m *= Mat4::from_translation(Vec3::new(
                    -0.4 * sin(sqrt(swing) * PI),
                    0.2 * sin(sqrt(swing) * PI * 2.),
                    -0.2 * sin(swing * PI),
                ));
                m *= Mat4::from_translation(Vec3::new(0.56, -0.52, -0.71999997))
                    * Mat4::from_translation(Vec3::new(0., equip * -0.6, 0.))
                    * Mat4::from_rotation_y(45f32.to_radians());
                let a = sin(swing * swing * PI);
                let b = sin(sqrt(swing) * PI);
                m = m
                    * Mat4::from_rotation_y((a * -20.).to_radians())
                    * Mat4::from_rotation_z((b * -20.).to_radians())
                    * Mat4::from_rotation_x((b * -80.).to_radians())
                    * Mat4::from_scale(Vec3::splat(0.4));
                render_item(out, proj, m, item, TransformType::FirstPerson, lights, light);
            }
            None => {
                // renderPlayerArm
                m *= Mat4::from_translation(Vec3::new(
                    -0.3 * sin(sqrt(swing) * PI),
                    0.4 * sin(sqrt(swing) * PI * 2.),
                    -0.4 * sin(swing * PI),
                ));
                m *= Mat4::from_translation(Vec3::new(0.64000005, -0.6, -0.71999997))
                    * Mat4::from_translation(Vec3::new(0., equip * -0.6, 0.))
                    * Mat4::from_rotation_y(45f32.to_radians());
                let f3 = sin(swing * swing * PI);
                let f4 = sin(sqrt(swing) * PI);
                m = m
                    * Mat4::from_rotation_y((f4 * 70.).to_radians())
                    * Mat4::from_rotation_z((f3 * -20.).to_radians())
                    * Mat4::from_translation(Vec3::new(-1., 3.6, 3.5))
                    * Mat4::from_rotation_z(120f32.to_radians())
                    * Mat4::from_rotation_x(200f32.to_radians())
                    * Mat4::from_rotation_y((-135f32).to_radians())
                    * Mat4::from_translation(Vec3::new(5.6, 0., 0.));
                let model = if f.slim { &mut self.alex } else { &mut self.steve };
                let kind = if f.slim { 2. } else { SKIN };
                model.render_right_arm(m, &mut |pm, q| model_quad(out, proj, pm, q, kind, lights, light, [1.; 3], false));
            }
        }
    }
}

/// `RenderItem.renderItemModelForEntity` (+ `ItemRenderer.renderItem`'s 3D scale) for one transform type.
fn render_item(out: &mut Vec<Vertex>, proj: Mat4, m: Mat4, item: &HeldItem, t: TransformType, lights: [Vec3; 2], light: [f32; 3]) {
    let model = &item.model;
    let display = model.transform(t);
    // ItemRenderer.renderItem scales 3D items by 2, RenderItem.preTransform scales flat ones by 2.
    let m = m * Mat4::from_scale(Vec3::splat(2.)) * display * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::splat(-0.5));
    // isThereOneNegativeScale → glCullFace(FRONT)
    let flip = display.determinant() < 0.;
    let normal_m = Mat3::from_mat4(m).inverse().transpose();
    for q in &model.quads {
        let (nx, ny, nz) = q.face.offset();
        let n = normal_m * Vec3::new(nx as f32, ny as f32, nz as f32);
        let shade = standard_lighting(n, lights);
        let tint = if q.tint_index >= 0 { item.tints[(q.tint_index as usize).min(3)] } else { 0xFFFFFF };
        let c = [
            ((tint >> 16) & 255) as f32 / 255. * shade * light[0],
            ((tint >> 8) & 255) as f32 / 255. * shade * light[1],
            (tint & 255) as f32 / 255. * shade * light[2],
            1.,
        ];
        let clip = q.pos.map(|p| proj * m * Vec3::from(p).extend(1.));
        if is_back_face(&clip) != flip {
            continue;
        }
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(Vertex { clip: clip[i].to_array(), uv: q.uv[i], color: c, kind: ATLAS });
        }
    }
}

/// GL_CCW front faces, judged in normalized device coordinates.
fn is_back_face(clip: &[Vec4; 4]) -> bool {
    if clip.iter().any(|c| c.w <= 0.) {
        return false;
    }
    let p: Vec<glam::Vec2> = clip.iter().map(|c| glam::Vec2::new(c.x / c.w, c.y / c.w)).collect();
    let mut area = 0.;
    for i in 0..4 {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        area += a.x * b.y - b.x * a.y;
    }
    area < 0.
}

#[allow(clippy::too_many_arguments)]
fn model_quad(out: &mut Vec<Vertex>, proj: Mat4, m: Mat4, q: &ModelQuad, kind: f32, lights: [Vec3; 2], light: [f32; 3], tint: [f32; 3], _cull: bool) {
    let n = Mat3::from_mat4(m).inverse().transpose() * q.normal;
    let shade = standard_lighting(n, lights);
    let c = [tint[0] * shade * light[0], tint[1] * shade * light[1], tint[2] * shade * light[2], 1.];
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(Vertex { clip: (proj * m * q.pos[i].extend(1.)).to_array(), uv: q.uv[i], color: c, kind });
    }
}

/// `RenderGlobal.drawSelectionBoundingBox`: 12 black lines at 40% alpha, 2 px wide.
fn outline(out: &mut Vec<Vertex>, vp: Mat4, cam: Vec3, bb: Aabb, w: f32, h: f32) {
    let lo = Vec3::new(bb.min_x as f32, bb.min_y as f32, bb.min_z as f32) - cam;
    let hi = Vec3::new(bb.max_x as f32, bb.max_y as f32, bb.max_z as f32) - cam;
    let corner = |i: usize| Vec3::new(if i & 1 != 0 { hi.x } else { lo.x }, if i & 2 != 0 { hi.y } else { lo.y }, if i & 4 != 0 { hi.z } else { lo.z });
    let edges = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    for (a, b) in edges {
        let (mut pa, mut pb) = (vp * corner(a).extend(1.), vp * corner(b).extend(1.));
        // Clip against the near plane (z >= 0 in wgpu clip space) so lines behind the eye vanish.
        let near = |p: Vec4| p.z;
        if near(pa) < 0. && near(pb) < 0. {
            continue;
        }
        if near(pa) < 0. {
            let t = near(pa) / (near(pa) - near(pb));
            pa = pa + (pb - pa) * t;
        } else if near(pb) < 0. {
            let t = near(pb) / (near(pb) - near(pa));
            pb = pb + (pa - pb) * t;
        }
        let sa = glam::Vec2::new(pa.x / pa.w * w * 0.5, pa.y / pa.w * h * 0.5);
        let sb = glam::Vec2::new(pb.x / pb.w * w * 0.5, pb.y / pb.w * h * 0.5);
        let d = (sb - sa).normalize_or_zero();
        // 1 px either side of the line, in clip units.
        let off = glam::Vec2::new(-d.y, d.x);
        let ox = |p: Vec4, s: f32| Vec4::new(p.x + off.x * s * 2. / w * p.w, p.y + off.y * s * 2. / h * p.w, p.z, p.w);
        let quad = [ox(pa, -1.), ox(pb, -1.), ox(pb, 1.), ox(pa, 1.)];
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(Vertex { clip: quad[i].to_array(), uv: [0.; 2], color: [0., 0., 0., 0.4], kind: LINE });
        }
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

fn texture(device: &wgpu::Device, queue: &wgpu::Queue, img: &image::RgbaImage) -> wgpu::Texture {
    use wgpu::util::DeviceExt;
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("avatar texture"),
            size: wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        img,
    )
}
