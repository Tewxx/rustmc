//! The local player's body (`RenderPlayer` + `LayerHeldItem`), first-person hand and held item
//! (`ItemRenderer.renderItemInFirstPerson`) and the block selection box.
use crate::entity_model::{lights_in, standard_lighting, ModelQuad, PlayerModel, Pose};
use crate::items::{ItemModel, TransformType};
use crate::mobs::{MobFrame, MobModels, Tex};
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
    /// `ItemStack.hasEffect`: `RenderItem.renderEffect` after the model.
    pub glint: bool,
}

/// An `EntityItem` to draw this frame.
pub struct DroppedItem {
    /// Interpolated position.
    pub pos: Vec3,
    /// `getAge() + partialTicks`
    pub age: f32,
    pub hover_start: f32,
    pub count: u8,
    pub item: HeldItem,
    pub light: [f32; 3],
}

/// One face of the block being broken, retextured with the destroy stage (`BreakingFour`).
pub struct DamageQuad {
    pub pos: [Vec3; 4],
    pub uv: [[f32; 2]; 4],
}

/// One worn armor piece (`LayerBipedArmor`).
#[derive(Clone, Copy, Debug)]
pub struct ArmorPiece {
    /// `ArmorMaterial` ordinal: 0 leather, 1 chain, 2 iron, 3 gold, 4 diamond.
    pub material: u8,
    /// `ItemArmor.getColor` (leather only).
    pub color: u32,
    /// `isItemEnchanted`
    pub glint: bool,
}

/// A block-model quad in world space (TNT, falling blocks, item frames). UVs are already in
/// block-atlas space and the colour includes the per-face light and entity brightness.
#[derive(Clone, Copy, Debug)]
pub struct WorldQuad {
    pub pos: [Vec3; 4],
    pub uv: [[f32; 2]; 4],
    pub color: [f32; 3],
}

/// `RenderSnowball` / `RenderPotion`: the item model as a GROUND-transform billboard.
#[derive(Clone)]
pub struct Billboard {
    pub pos: Vec3,
    pub item: HeldItem,
    pub light: [f32; 3],
}

/// An item with its own model matrix in camera-relative space (item-frame contents).
#[derive(Clone)]
pub struct FixedItem {
    pub m: Mat4,
    pub item: HeldItem,
    pub light: [f32; 3],
}

/// Which atlas a [`ParticleQuad`] samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleTex {
    /// `particles.png` cells (`EffectRenderer` layer 0).
    Misc,
    /// `textures/entity/explosion.png` frames (`EntityLargeExplodeFX`).
    Explosion,
    /// The block/item atlas (breaking particles, barrier), UVs already in atlas space.
    Block,
}

/// One `EntityFX.renderParticle` quad: world-space corners, source-space UVs, tint and
/// lightmap colour. The client sorts these back-to-front before uploading.
#[derive(Clone, Copy, Debug)]
pub struct ParticleQuad {
    pub pos: [Vec3; 4],
    /// `Misc`/`Explosion` UVs are 0–1 of their own texture; `Block` UVs are atlas space.
    pub uv: [[f32; 2]; 4],
    pub color: [f32; 4],
    pub light: [f32; 3],
    pub tex: ParticleTex,
}

/// An item being used (`getItemInUseCount() > 0`).
#[derive(Clone, Copy, Debug)]
pub struct UseAnim {
    /// `EnumAction` ordinal: 0 none, 1 eat, 2 drink, 3 block, 4 bow.
    pub action: u8,
    /// `getItemInUseCount() - partialTicks + 1`
    pub left: f32,
    /// `getMaxItemUseDuration()`
    pub max: f32,
}

#[derive(Default)]
pub struct AvatarFrame {
    pub damage: Vec<DamageQuad>,
    pub dropped: Vec<DroppedItem>,
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
    /// Cell in the downloaded-skin atlas ([`AvatarRenderer::upload_skin`]); `None` = default skin.
    pub skin: Option<u32>,
    /// Other players (only the body fields are used).
    pub others: Vec<AvatarFrame>,
    /// `GuiInventory.drawEntityOnScreen` for the local player.
    pub gui_player: Option<GuiPlayer>,
    /// The item in use: first-person use animations, third-person block/bow poses.
    pub use_anim: Option<UseAnim>,
    /// `armorInventory` (0 boots .. 3 helmet).
    pub armor: [Option<ArmorPiece>; 4],
    /// Living mobs in view.
    pub mobs: Vec<MobFrame>,
    /// Tile entities (`TileEntityRendererDispatcher`); drawn with the world, after the entities.
    pub tiles: Vec<crate::tile::TileFrame>,
    /// Non-living entities drawn from the entity atlas (arrows, XP orbs, paintings, stands).
    pub objects: Vec<crate::objects::ObjectFrame>,
    /// Item-model billboards (`RenderSnowball`).
    pub billboards: Vec<Billboard>,
    /// Item-frame contents.
    pub fixed_items: Vec<FixedItem>,
    /// Block-model quads in world space (TNT, falling blocks, item frames).
    pub world_quads: Vec<WorldQuad>,
    /// `EffectRenderer.renderParticles` billboards.
    pub particles: Vec<ParticleQuad>,
}

/// Physical-pixel placement of an inventory player preview.
#[derive(Clone, Copy, Debug)]
pub struct GuiPlayer {
    pub x: f32,
    pub y: f32,
    /// `scale` × GUI scale.
    pub scale: f32,
    /// `posX - mouseX`, `posY - 50 - mouseY` in GUI units.
    pub mouse_x: f32,
    pub mouse_y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    clip: [f32; 4],
    uv: [f32; 2],
    color: [f32; 4],
    kind: f32,
    /// `setBrightness` combiner color (already lightmapped) and its weight.
    overlay: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleInstance {
    pos: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    color: [f32; 4],
    kind: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EntityInstance {
    pos: [[f32; 3]; 4], uv: [[f32; 2]; 4], mvp: [[f32; 4]; 4],
    color: [f32; 4], kind: f32, overlay: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParticleGlobals { vp: [[f32; 4]; 4], camera: [f32; 4] }

pub struct AvatarRenderer {
    pipeline: wgpu::RenderPipeline,
    particle_pipeline: wgpu::RenderPipeline,
    entity_pipeline: wgpu::RenderPipeline,
    entity_instances: wgpu::Buffer,
    entity_capacity: usize,
    use_entity_instancing: bool,
    particle_globals: wgpu::Buffer,
    particle_bind_group: wgpu::BindGroup,
    particle_instances: wgpu::Buffer,
    particle_capacity: usize,
    use_particle_instancing: bool,
    particle_scratch: Vec<ParticleInstance>,
    entity_scratch: Vec<EntityInstance>,
    damage_pipeline: wgpu::RenderPipeline,
    vertices: [wgpu::Buffer; 3],
    bind_group: wgpu::BindGroup,
    steve: PlayerModel,
    alex: PlayerModel,
    armor_model: PlayerModel,
    leggings_model: PlayerModel,
    skins: wgpu::Texture,
    glint_pipeline: wgpu::RenderPipeline,
    mobs: MobModels,
    /// `TileEntityChestRenderer`'s `simpleChest` / `largeChest`.
    chests: crate::tile::ChestModels,
    /// Armour-stand parts, posed per frame.
    stands: crate::objects::ArmorStandModel,
    /// Mob texture rectangles in the mob atlas: (u, v, width, height), normalized.
    mob_atlas: std::collections::HashMap<&'static str, [f32; 4]>,
    /// `blendFunc(ONE, ONE)` eye layers, drawn after the opaque geometry.
    additive_pipeline: wgpu::RenderPipeline,
    additive: Vec<Vertex>,
    /// Glint passes collected while building a frame; drawn last with `GL_EQUAL` depth.
    glint: Vec<Vertex>,
}

pub const SKIN_ATLAS: u32 = 2048;

const SKIN: f32 = 0.;
const ATLAS: f32 = 1.;
const LINE: f32 = 3.;
const ARMOR: f32 = 6.;
const GLINT: f32 = 7.;
/// Armor sheets stacked vertically: `{material}_layer_{1,2}`, then the leather overlays.
const ARMOR_CELLS: u32 = 12;
const MOB: f32 = 8.;
/// Vertices per buffer (a frame's world pass, hand pass and inventory preview).
const MAX_VERTS: usize = 131072;

fn make_particle_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some("particle instances"),
        size: count as u64 * std::mem::size_of::<ParticleInstance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

fn record_entity_run(verts: &mut Vec<Vertex>, entities: &mut Vec<EntityInstance>, runs: &mut Vec<(usize, std::ops::Range<usize>)>, vertex_start: usize, instance_start: usize, max: usize) {
    if entities.len() > max {
        let fallback = entities.drain(instance_start..).flat_map(|instance| {
            let mvp = Mat4::from_cols_array_2d(&instance.mvp);
            [0, 1, 2, 0, 2, 3].map(|i| Vertex { clip: (mvp * Vec3::from_array(instance.pos[i]).extend(1.)).to_array(),
                uv: instance.uv[i], color: instance.color, kind: instance.kind, overlay: instance.overlay })
        });
        verts.splice(vertex_start..vertex_start, fallback);
    } else if entities.len() > instance_start {
        if let Some((_, range)) = runs.last_mut().filter(|(at, range)| *at == vertex_start && range.end == instance_start) {
            range.end = entities.len();
        } else { runs.push((vertex_start, instance_start..entities.len())); }
    }
}

fn make_entity_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some("entity instances"),
        size: count as u64 * std::mem::size_of::<EntityInstance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

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
                texture_entry(4),
                texture_entry(5),
                texture_entry(6),
                texture_entry(7),
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
@group(0) @binding(4) var skins:texture_2d<f32>;
@group(0) @binding(5) var armor:texture_2d<f32>;
@group(0) @binding(6) var glint:texture_2d<f32>;
@group(0) @binding(7) var mobs:texture_2d<f32>;
struct ParticleGlobals {{ vp:mat4x4<f32>, camera:vec4<f32> }};
@group(1) @binding(0) var<uniform> particle_globals:ParticleGlobals;
struct V {{ @builtin(position) pos:vec4<f32>, @location(0) uv:vec2<f32>, @location(1) color:vec4<f32>, @location(2) @interpolate(flat) kind:u32, @location(3) overlay:vec4<f32> }};
@vertex fn vs(@location(0) p:vec4<f32>,@location(1) uv:vec2<f32>,@location(2) color:vec4<f32>,@location(3) kind:f32,@location(4) overlay:vec4<f32>)->V {{
 var o:V;o.pos=p;o.uv=uv;o.color=color;o.kind=u32(kind);o.overlay=overlay;return o;
}}
@vertex fn vs_particle(@builtin(vertex_index) vertex:u32,
 @location(0) p0:vec3<f32>, @location(1) p1:vec3<f32>, @location(2) p2:vec3<f32>, @location(3) p3:vec3<f32>,
 @location(4) u0:vec2<f32>, @location(5) u1:vec2<f32>, @location(6) u2:vec2<f32>, @location(7) u3:vec2<f32>,
 @location(8) color:vec4<f32>, @location(9) kind:f32)->V {{
 var p=p0; var uv=u0;
 if(vertex==1u) {{p=p1;uv=u1;}} else if(vertex==2u || vertex==4u) {{p=p2;uv=u2;}} else if(vertex==5u) {{p=p3;uv=u3;}}
 var o:V; o.pos=particle_globals.vp*vec4<f32>(p-particle_globals.camera.xyz,1.0);
 o.uv=uv; o.color=color; o.kind=u32(kind); o.overlay=vec4<f32>(0.0); return o;
}}
@vertex fn vs_entity(@builtin(vertex_index) vertex:u32,
 @location(0) p0:vec3<f32>, @location(1) p1:vec3<f32>, @location(2) p2:vec3<f32>, @location(3) p3:vec3<f32>,
 @location(4) u0:vec2<f32>, @location(5) u1:vec2<f32>, @location(6) u2:vec2<f32>, @location(7) u3:vec2<f32>,
 @location(8) m0:vec4<f32>, @location(9) m1:vec4<f32>, @location(10) m2:vec4<f32>, @location(11) m3:vec4<f32>,
 @location(12) color:vec4<f32>, @location(13) kind:f32, @location(14) overlay:vec4<f32>)->V {{
 var p=p0; var uv=u0;
 if(vertex==1u) {{p=p1;uv=u1;}} else if(vertex==2u || vertex==4u) {{p=p2;uv=u2;}} else if(vertex==5u) {{p=p3;uv=u3;}}
 var o:V; o.pos=mat4x4<f32>(m0,m1,m2,m3)*vec4<f32>(p,1.0);
 o.uv=uv; o.color=color; o.kind=u32(kind); o.overlay=overlay; return o;
}}
@fragment fn fs(v:V)->@location(0) vec4<f32> {{
 var c=v.color;
 if(v.kind==0u){{c*=textureSampleLevel(skin,samp,v.uv,0.0);}}
 if(v.kind==1u){{c*=textureSampleLevel(atlas,samp,v.uv,0.0);}}
 if(v.kind==2u){{c*=textureSampleLevel(alex,samp,v.uv,0.0);}}
 if(v.kind==5u){{c*=textureSampleLevel(skins,samp,v.uv,0.0);}}
 if(v.kind==6u){{c*=textureSampleLevel(armor,samp,v.uv,0.0);}}
 if(v.kind==7u){{c*=textureSampleLevel(glint,samp,fract(v.uv),0.0);}}
 if(v.kind==8u){{c*=textureSampleLevel(mobs,samp,v.uv,0.0);}}
 if(v.kind==9u){{c*=textureSampleLevel(atlas,samp,v.uv,0.0);}}
 if(v.kind==10u){{c*=textureSampleLevel(mobs,samp,v.uv,0.0);}}
 c=vec4<f32>(mix(c.rgb,v.overlay.rgb,v.overlay.a),c.a);
 if(v.kind==4u){{c*=textureSampleLevel(atlas,samp,v.uv,0.0);c=vec4<f32>(min(c.rgb*2.0,vec3<f32>(1.0)),c.a);}}
 if(v.kind!=3u && v.kind!=10u && c.a<=0.1){{discard;}}
 if(v.kind==10u && c.a<=0.003921569){{discard;}}
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
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4,3=>Float32,4=>Float32x4],
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
        let particle_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle globals"), entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None,
            }],
        });
        let particle_globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle globals"), size: std::mem::size_of::<ParticleGlobals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
        });
        let particle_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle globals"), layout: &particle_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: particle_globals.as_entire_binding() }],
        });
        let particle_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particle pipeline"), bind_group_layouts: &[&layout, &particle_layout], push_constant_ranges: &[],
        });
        let particle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("instanced particles"), layout: Some(&particle_pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_particle"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<ParticleInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x3,4=>Float32x2,5=>Float32x2,6=>Float32x2,7=>Float32x2,8=>Float32x4,9=>Float32],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None, cache: None,
        });
        let entity_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("instanced entities"), layout: Some(&pl),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_entity"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<EntityInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x3,4=>Float32x2,5=>Float32x2,6=>Float32x2,7=>Float32x2,8=>Float32x4,9=>Float32x4,10=>Float32x4,11=>Float32x4,12=>Float32x4,13=>Float32,14=>Float32x4],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual, stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None, cache: None,
        });
        let entity_capacity = 1024;
        let entity_instances = make_entity_buffer(device, entity_capacity);
        let particle_capacity = 1024;
        let particle_instances = make_particle_buffer(device, particle_capacity);
        // drawBlockDamageTexture: blend (DST_COLOR, SRC_COLOR), polygon offset -3, no depth writes.
        let damage_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("block damage"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4,3=>Float32,4=>Float32x4],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: -3, slope_scale: -3.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview: None,
            cache: None,
        });
        // LayerArmorBase.renderGlint: blend (SRC_COLOR, ONE), depthFunc(GL_EQUAL), no depth writes.
        let glint_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("armor glint"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4,3=>Float32,4=>Float32x4],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Equal,
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
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Src,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview: None,
            cache: None,
        });
        // LayerSpiderEyes / LayerEndermanEyes: blendFunc(ONE, ONE), alpha test off.
        let additive_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mob eyes"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32x4,3=>Float32,4=>Float32x4],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
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
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::COLOR,
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
        let skins = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("skin atlas"),
            size: wgpu::Extent3d { width: SKIN_ATLAS, height: SKIN_ATLAS, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let skins_view = skins.create_view(&Default::default());
        let mut armor = image::RgbaImage::new(64, 32 * ARMOR_CELLS);
        for (cell, name) in ["leather", "chainmail", "iron", "gold", "diamond"]
            .iter()
            .flat_map(|m| [format!("{m}_layer_1"), format!("{m}_layer_2")])
            .chain(["leather_layer_1_overlay".to_string(), "leather_layer_2_overlay".to_string()])
            .enumerate()
        {
            let img = load(&format!("textures/models/armor/{name}.png"));
            let img = image::imageops::resize(&img, 64, 32, image::imageops::FilterType::Nearest);
            image::imageops::replace(&mut armor, &img, 0, cell as i64 * 32);
        }
        let armor_view = texture(device, queue, &armor).create_view(&Default::default());
        let glint_view = texture(device, queue, &load("textures/misc/enchanted_item_glint.png")).create_view(&Default::default());
        let (mob_img, mob_atlas) = mob_atlas(&load);
        let mobs_view = texture(device, queue, &mob_img).create_view(&Default::default());
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
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&skins_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&armor_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&glint_view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&mobs_view) },
            ],
        });
        let vertices = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("avatar vertices"),
                size: MAX_VERTS as u64 * std::mem::size_of::<Vertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Self {
            pipeline,
            particle_pipeline,
            entity_pipeline, entity_instances, entity_capacity,
            use_entity_instancing: std::env::var_os("RUSTMC_NO_ENTITY_INSTANCING").is_none(),
            particle_globals,
            particle_bind_group,
            particle_instances,
            particle_capacity,
            use_particle_instancing: std::env::var_os("RUSTMC_NO_PARTICLE_INSTANCING").is_none(),
            particle_scratch: Vec::new(), entity_scratch: Vec::new(),
            damage_pipeline,
            vertices,
            bind_group,
            steve: PlayerModel::new(false),
            alex: PlayerModel::new(true),
            armor_model: PlayerModel::biped(1.0),
            leggings_model: PlayerModel::biped(0.5),
            skins,
            glint_pipeline,
            glint: Vec::new(),
            mobs: MobModels::new(),
            chests: crate::tile::ChestModels::default(),
            stands: crate::objects::ArmorStandModel::new(),
            mob_atlas,
            additive_pipeline,
            additive: Vec::new(),
        }
    }

    /// Stores a 64×64 RGBA skin in atlas cell `cell` (32×32 cells).
    pub fn upload_skin(&self, queue: &wgpu::Queue, cell: u32, rgba: &[u8]) {
        let per = SKIN_ATLAS / 64;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.skins,
                mip_level: 0,
                origin: wgpu::Origin3d { x: cell % per * 64, y: cell / per * 64, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) },
            wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
        );
    }

    /// `hand_stage` false: body + selection box (world pass); true: first-person hand after a depth clear.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
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
        let mut damage = Vec::new();
        let mut particles = std::mem::take(&mut self.particle_scratch);
        let mut entities = std::mem::take(&mut self.entity_scratch);
        particles.clear(); entities.clear();
        let mut entity_runs: Vec<(usize, std::ops::Range<usize>)> = Vec::new();
        let max_entities = (device.limits().max_buffer_size / std::mem::size_of::<EntityInstance>() as u64).min(u32::MAX as u64) as usize;
        let max_instances = (device.limits().max_buffer_size / std::mem::size_of::<ParticleInstance>() as u64).min(u32::MAX as u64) as usize;
        let use_particle_instancing = self.use_particle_instancing && frame.particles.len() <= max_instances;
        let mut particle_split = 0;
        let world_vp = cam.view_proj(aspect, far);
        if !hand_stage {
            for player in (frame.perspective != 0).then_some(frame).into_iter().chain(&frame.others) {
                let vertex_start = verts.len();
                let instance_start = entities.len();
                let use_instances = self.use_entity_instancing && !player.armor.iter().flatten().any(|a| a.glint);
                self.body(&mut verts, &mut entities, use_instances, player, world_vp, cam.pos);
                record_entity_run(&mut verts, &mut entities, &mut entity_runs, vertex_start, instance_start, max_entities);
            }
            for m in &frame.mobs {
                let vertex_start = verts.len();
                let instance_start = entities.len();
                let use_instances = self.use_entity_instancing && !m.armor.iter().flatten().any(|a| a.glint);
                self.mob(&mut verts, &mut entities, use_instances, m, world_vp, cam.pos);
                record_entity_run(&mut verts, &mut entities, &mut entity_runs, vertex_start, instance_start, max_entities);
            }
            for o in &frame.objects {
                let vertex_start = verts.len(); let instance_start = entities.len();
                self.object(&mut verts, &mut entities, self.use_entity_instancing, o, world_vp, cam);
                record_entity_run(&mut verts, &mut entities, &mut entity_runs, vertex_start, instance_start, max_entities);
            }
            for t in &frame.tiles {
                let vertex_start = verts.len(); let instance_start = entities.len();
                self.tile(&mut verts, &mut entities, self.use_entity_instancing, t, world_vp, cam.pos);
                record_entity_run(&mut verts, &mut entities, &mut entity_runs, vertex_start, instance_start, max_entities);
            }
            for b in &frame.billboards {
                // RenderSnowball: translate, scale 0.5, rotate(-playerViewY, Y),
                // rotate(playerViewX, X), then the item's GROUND transform.
                let m = Mat4::from_translation(b.pos - cam.pos)
                    * Mat4::from_scale(Vec3::splat(0.5))
                    * Mat4::from_rotation_y((-cam.yaw).to_radians())
                    * Mat4::from_rotation_x(cam.pitch.to_radians());
                render_item(&mut verts, &mut self.glint, world_vp, m, &b.item, TransformType::Ground, lights_in(Mat3::IDENTITY), b.light);
            }
            for f in &frame.fixed_items {
                render_item(&mut verts, &mut self.glint, world_vp, f.m, &f.item, TransformType::Fixed, lights_in(Mat3::IDENTITY), f.light);
            }
            for q in &frame.world_quads {
                let clip = q.pos.map(|p| world_vp * (p - cam.pos).extend(1.));
                for i in [0, 1, 2, 0, 2, 3] {
                    verts.push(Vertex { clip: clip[i].to_array(), uv: q.uv[i], color: [q.color[0], q.color[1], q.color[2], 1.], kind: 9., overlay: [0.; 4] });
                }
            }
            particle_split = verts.len();
            for q in &frame.particles {
                let (kind, uv) = match q.tex {
                    ParticleTex::Block => (9., q.uv),
                    ParticleTex::Misc | ParticleTex::Explosion => {
                        let key = if q.tex == ParticleTex::Misc { "particles" } else { "explosion" };
                        let r = self.mob_atlas.get(key).copied().unwrap_or([0., 0., 0., 0.]);
                        (10., q.uv.map(|u| [r[0] + u[0] * r[2], r[1] + u[1] * r[3]]))
                    }
                };
                let color = [q.color[0] * q.light[0], q.color[1] * q.light[1], q.color[2] * q.light[2], q.color[3]];
                if use_particle_instancing {
                    particles.push(ParticleInstance { pos: q.pos.map(|p| p.to_array()), uv, color, kind });
                } else {
                    let clip = q.pos.map(|p| world_vp * (p - cam.pos).extend(1.));
                    for i in [0, 1, 2, 0, 2, 3] {
                        verts.push(Vertex { clip: clip[i].to_array(), uv: uv[i], color, kind, overlay: [0.; 4] });
                    }
                }
            }
            for d in &frame.dropped {
                dropped_item(&mut verts, &mut self.glint, world_vp, cam.pos, d);
            }
            for q in &frame.damage {
                for i in [0, 1, 2, 0, 2, 3] {
                    let clip = world_vp * (q.pos[i] - cam.pos).extend(1.);
                    damage.push(Vertex { clip: clip.to_array(), uv: q.uv[i], color: [1., 1., 1., 0.5], kind: 4., overlay: [0.; 4] });
                }
            }
            if let Some(bb) = frame.outline {
                outline(&mut verts, world_vp, cam.pos, bb.expand(0.002, 0.002, 0.002), height * aspect, height);
            }
        } else if frame.perspective == 0 && !frame.hide_hand {
            self.hand(&mut verts, frame, aspect, far);
        }
        // The crack overlay draws right after the terrain, before entities.
        let split = damage.len() as u32;
        particle_split += split as usize;
        damage.extend(verts);
        let additive_start = damage.len() as u32;
        damage.append(&mut self.additive);
        let glint_start = damage.len() as u32;
        damage.append(&mut self.glint);
        let verts = damage;
        if verts.is_empty() && particles.is_empty() && entities.is_empty() {
            self.particle_scratch = particles; self.entity_scratch = entities;
            return;
        }
        if !entities.is_empty() {
            if entities.len() > self.entity_capacity {
                self.entity_capacity = entities.len().next_power_of_two().min(max_entities);
                self.entity_instances = make_entity_buffer(device, self.entity_capacity);
            }
            queue.write_buffer(&self.entity_instances, 0, bytemuck::cast_slice(&entities));
        }
        if !particles.is_empty() {
            if particles.len() > self.particle_capacity {
                self.particle_capacity = particles.len().next_power_of_two().min(max_instances);
                self.particle_instances = make_particle_buffer(device, self.particle_capacity);
            }
            queue.write_buffer(&self.particle_instances, 0, bytemuck::cast_slice(&particles));
            queue.write_buffer(&self.particle_globals, 0, bytemuck::bytes_of(&ParticleGlobals {
                vp: world_vp.to_cols_array_2d(), camera: cam.pos.extend(0.).to_array(),
            }));
        }
        let particle_count = particles.len();
        self.particle_scratch = particles; self.entity_scratch = entities;
        let buf = &self.vertices[hand_stage as usize];
        if !verts.is_empty() { queue.write_buffer(buf, 0, bytemuck::cast_slice(&verts[..verts.len().min(MAX_VERTS)])); }
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
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        let end = verts.len().min(MAX_VERTS) as u32;
        if split > 0 {
            pass.set_pipeline(&self.damage_pipeline);
            pass.draw(0..split.min(end), 0..1);
        }
        pass.set_pipeline(&self.pipeline);
        let mut start = split.min(end);
        for (vertex_start, instances) in &entity_runs {
            if split as usize + vertex_start > end as usize { break; }
            let at = (split as usize + vertex_start).min(end as usize) as u32;
            pass.draw(start..at, 0..1);
            pass.set_pipeline(&self.entity_pipeline);
            pass.set_vertex_buffer(0, self.entity_instances.slice(..));
            pass.draw(0..6, instances.start as u32..instances.end as u32);
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, buf.slice(..));
            start = at;
        }
        if particle_count == 0 {
            pass.draw(start..additive_start.min(end), 0..1);
        } else {
            let at = (particle_split as u32).min(additive_start).min(end);
            pass.draw(start..at, 0..1);
            pass.set_pipeline(&self.particle_pipeline);
            pass.set_bind_group(1, &self.particle_bind_group, &[]);
            pass.set_vertex_buffer(0, self.particle_instances.slice(..));
            pass.draw(0..6, 0..particle_count as u32);
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(at..additive_start.min(end), 0..1);
        }
        if additive_start < glint_start.min(end) {
            pass.set_pipeline(&self.additive_pipeline);
            pass.draw(additive_start..glint_start.min(end), 0..1);
        }
        if glint_start < end {
            pass.set_pipeline(&self.glint_pipeline);
            pass.draw(glint_start..end, 0..1);
        }
    }

    /// `GuiInventory.drawEntityOnScreen`: the player in the inventory, drawn over the GUI
    /// with its own depth clear.
    #[allow(clippy::too_many_arguments)]
    pub fn encode_gui(&mut self, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, depth: &wgpu::TextureView, w: f32, h: f32, frame: &AvatarFrame) {
        let Some(g) = frame.gui_player else { return };
        let ortho = Mat4::from_cols(
            Vec4::new(2. / w, 0., 0., 0.),
            Vec4::new(0., -2. / h, 0., 0.),
            Vec4::new(0., 0., -1. / 2000., 0.),
            Vec4::new(-1., 1., 0.5, 1.),
        );
        let yaw = (g.mouse_x / 40.).atan() * 20.;
        let pitch = -(g.mouse_y / 40.).atan() * 20.;
        let mut f = AvatarFrame {
            body_yaw: yaw,
            head_yaw: (g.mouse_x / 40.).atan() * 40.,
            pitch,
            age: frame.age,
            sneaking: frame.sneaking,
            held: frame.held.clone(),
            light: [1.; 3],
            slim: frame.slim,
            skin: frame.skin,
            limb_swing: frame.limb_swing,
            limb_amount: frame.limb_amount,
            swing: frame.swing,
            use_anim: frame.use_anim,
            armor: frame.armor,
            ..Default::default()
        };
        f.head_yaw = f.head_yaw.clamp(f.body_yaw - 75., f.body_yaw + 75.);
        self.pose(&f);
        let base = Mat4::from_translation(Vec3::new(g.x, g.y, 50.))
            * Mat4::from_scale(Vec3::new(-g.scale, g.scale, g.scale))
            * Mat4::from_rotation_z(180f32.to_radians());
        let lights = lights_in(Mat3::from_mat4(Mat4::from_scale(Vec3::new(-1., 1., 1.)) * Mat4::from_rotation_z(180f32.to_radians()) * Mat4::from_rotation_y(135f32.to_radians())));
        let m = base * Mat4::from_rotation_x(pitch.to_radians());
        let mut verts = Vec::new();
        self.body_at(&mut verts, &mut Vec::new(), false, &f, ortho, m, lights);
        if verts.is_empty() {
            return;
        }
        let glint_start = verts.len() as u32;
        verts.append(&mut self.glint);
        let buf = &self.vertices[2];
        queue.write_buffer(buf, 0, bytemuck::cast_slice(&verts[..verts.len().min(MAX_VERTS)]));
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gui player"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        let end = verts.len().min(MAX_VERTS) as u32;
        pass.set_pipeline(&self.pipeline);
        pass.draw(0..glint_start.min(end), 0..1);
        if glint_start < end {
            pass.set_pipeline(&self.glint_pipeline);
            pass.draw(glint_start..end, 0..1);
        }
    }

    /// `RendererLivingEntity.doRender` for the local player, plus `LayerHeldItem`.
    fn body(&mut self, out: &mut Vec<Vertex>, instances: &mut Vec<EntityInstance>, use_instances: bool, f: &AvatarFrame, vp: Mat4, cam: Vec3) {
        self.pose(f);
        // World-space model matrix (the view is applied separately so normals stay in world space).
        self.body_at(out, instances, use_instances, f, vp, Mat4::from_translation(f.pos - cam), lights_in(Mat3::IDENTITY));
    }

    /// `RenderPlayer.setModelVisibilities` + `setRotationAngles` on the skin model.
    fn pose(&mut self, f: &AvatarFrame) {
        let model = if f.slim { &mut self.alex } else { &mut self.steve };
        model.swing = f.swing;
        model.sneak = f.sneaking;
        // RenderPlayer.setModelVisibilities
        let action = f.use_anim.map_or(0, |u| u.action);
        model.held_right = if f.held.is_none() {
            0
        } else if action == 3 {
            3
        } else {
            1
        };
        model.aimed_bow = f.held.is_some() && action == 4;
        model.set_angles(Pose {
            limb_swing: f.limb_swing,
            limb_amount: f.limb_amount.min(1.),
            age: f.age,
            head_yaw: f.head_yaw - f.body_yaw,
            head_pitch: f.pitch,
        });
    }

    fn body_at(&mut self, out: &mut Vec<Vertex>, instances: &mut Vec<EntityInstance>, use_instances: bool, f: &AvatarFrame, vp: Mat4, at: Mat4, lights: [Vec3; 2]) {
        let model = if f.slim { &mut self.alex } else { &mut self.steve };
        let m = at
            * Mat4::from_rotation_y((180. - f.body_yaw).to_radians())
            * Mat4::from_scale(Vec3::new(-1., -1., 1.))
            * Mat4::from_scale(Vec3::splat(0.9375))
            * Mat4::from_translation(Vec3::new(0., -1.5078125, 0.));
        let kind = if f.slim { 2. } else { SKIN };
        let light = f.light;
        let start = out.len();
        let instance_start = instances.len();
        model.render(m, &mut |pm, q| player_quad(out, instances, use_instances, vp, pm, q, kind, lights, light, [1.; 3]));
        retarget_skin(&mut out[start..], f.skin);
        if let Some(cell) = f.skin {
            let per = (SKIN_ATLAS / 64) as f32;
            let (cx, cy) = ((cell % (SKIN_ATLAS / 64)) as f32, (cell / (SKIN_ATLAS / 64)) as f32);
            for instance in &mut instances[instance_start..] {
                instance.kind = 5.;
                instance.uv = instance.uv.map(|[u,v]| [(cx + u) / per, (cy + v) / per]);
            }
        }
        // LayerBipedArmor (before LayerHeldItem): helmet, chestplate, leggings, boots.
        let (swing, sneak, held_right, aimed_bow, pose) = (model.swing, model.sneak, model.held_right, model.aimed_bow, model.pose);
        for slot in [4, 3, 2, 1] {
            let Some(a) = f.armor[slot - 1] else { continue };
            let am = if slot == 2 { &mut self.leggings_model } else { &mut self.armor_model };
            am.swing = swing;
            am.sneak = sneak;
            am.held_right = held_right;
            am.aimed_bow = aimed_bow;
            am.set_angles(pose);
            am.show_armor_slot(slot);
            let layer = (slot == 2) as u32;
            let mut pass = |cell: u32, tint: [f32; 3]| {
                let start = out.len();
                let instance_start = instances.len();
                am.render(m, &mut |pm, q| player_quad(out, instances, use_instances, vp, pm, q, ARMOR, lights, light, tint));
                for v in &mut out[start..] {
                    v.uv[1] = (cell as f32 + v.uv[1]) / ARMOR_CELLS as f32;
                }
                for instance in &mut instances[instance_start..] {
                    for uv in &mut instance.uv { uv[1] = (cell as f32 + uv[1]) / ARMOR_CELLS as f32; }
                }
            };
            if a.material == 0 {
                let c = a.color;
                pass(layer, [((c >> 16) & 255) as f32 / 255., ((c >> 8) & 255) as f32 / 255., (c & 255) as f32 / 255.]);
                pass(10 + layer, [1.; 3]);
            } else {
                pass(a.material as u32 * 2 + layer, [1.; 3]);
            }
            if a.glint {
                let glint = &mut self.glint;
                am.render(m, &mut |pm, q| armor_glint(glint, vp * pm, q, f.age, light));
            }
        }
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
            render_item(out, &mut self.glint, vp, im, item, TransformType::ThirdPerson, lights, light);
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
                // transformFirstPersonItem
                let transform = |m: Mat4, swing: f32| {
                    let a = sin(swing * swing * PI);
                    let b = sin(sqrt(swing) * PI);
                    m * Mat4::from_translation(Vec3::new(0.56, -0.52, -0.71999997))
                        * Mat4::from_translation(Vec3::new(0., equip * -0.6, 0.))
                        * Mat4::from_rotation_y(45f32.to_radians())
                        * Mat4::from_rotation_y((a * -20.).to_radians())
                        * Mat4::from_rotation_z((b * -20.).to_radians())
                        * Mat4::from_rotation_x((b * -80.).to_radians())
                        * Mat4::from_scale(Vec3::splat(0.4))
                };
                match f.use_anim {
                    Some(u) if u.action == 1 || u.action == 2 => {
                        // performDrinking
                        let f0 = u.left;
                        let f1 = f0 / u.max;
                        let mut f2 = (mc_core::math::cos(f0 / 4.0 * PI) * 0.1).abs();
                        if f1 >= 0.8 {
                            f2 = 0.0;
                        }
                        m *= Mat4::from_translation(Vec3::new(0., f2, 0.));
                        let f3 = 1.0 - (f1 as f64).powf(27.0) as f32;
                        m = m
                            * Mat4::from_translation(Vec3::new(f3 * 0.6, f3 * -0.5, f3 * 0.0))
                            * Mat4::from_rotation_y((f3 * 90.0).to_radians())
                            * Mat4::from_rotation_x((f3 * 10.0).to_radians())
                            * Mat4::from_rotation_z((f3 * 30.0).to_radians());
                        m = transform(m, 0.);
                    }
                    Some(u) if u.action == 3 => {
                        // doBlockTransformations
                        m = transform(m, 0.)
                            * Mat4::from_translation(Vec3::new(-0.5, 0.2, 0.0))
                            * Mat4::from_rotation_y(30f32.to_radians())
                            * Mat4::from_rotation_x((-80f32).to_radians())
                            * Mat4::from_rotation_y(60f32.to_radians());
                    }
                    Some(u) if u.action == 4 => {
                        // doBowTransformations
                        m = transform(m, 0.)
                            * Mat4::from_rotation_z((-18f32).to_radians())
                            * Mat4::from_rotation_y((-12f32).to_radians())
                            * Mat4::from_rotation_x((-8f32).to_radians())
                            * Mat4::from_translation(Vec3::new(-0.9, 0.2, 0.0));
                        let f0 = u.max - u.left;
                        let mut f1 = f0 / 20.0;
                        f1 = (f1 * f1 + f1 * 2.0) / 3.0;
                        if f1 > 1.0 {
                            f1 = 1.0;
                        }
                        if f1 > 0.1 {
                            let f4 = sin((f0 - 0.1) * 1.3) * (f1 - 0.1);
                            m *= Mat4::from_translation(Vec3::new(f4 * 0.0, f4 * 0.01, f4 * 0.0));
                        }
                        m = m * Mat4::from_translation(Vec3::new(f1 * 0.0, f1 * 0.0, f1 * 0.1)) * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0 + f1 * 0.2));
                    }
                    Some(_) => m = transform(m, 0.),
                    None => {
                        // doItemUsedTransformations
                        m *= Mat4::from_translation(Vec3::new(
                            -0.4 * sin(sqrt(swing) * PI),
                            0.2 * sin(sqrt(swing) * PI * 2.),
                            -0.2 * sin(swing * PI),
                        ));
                        m = transform(m, swing);
                    }
                }
                render_item(out, &mut self.glint, proj, m, item, TransformType::FirstPerson, lights, light);
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
                let start = out.len();
                model.render_right_arm(m, &mut |pm, q| model_quad(out, proj, pm, q, kind, lights, light, [1.; 3], false));
                retarget_skin(&mut out[start..], f.skin);
            }
        }
    }
}

/// Points skin-textured vertices at a downloaded skin's cell in the skin atlas.
impl AvatarRenderer {
    /// `RendererLivingEntity.doRender` for a mob, then `LayerHeldItem`.
    fn mob(&mut self, out: &mut Vec<Vertex>, instances: &mut Vec<EntityInstance>, use_instances: bool, f: &MobFrame, vp: Mat4, cam: Vec3) {
        let base = Mat4::from_translation(f.pos - cam);
        let lights = lights_in(Mat3::IDENTITY);
        let (atlas, glint, additive) = (&self.mob_atlas, &mut self.glint, &mut self.additive);
        let held = self.mobs.render(f, base, &mut |l, pm, q| {
            let light = if l.additive { f.bright } else { f.light };
            if l.glint {
                armor_glint(glint, vp * pm, q, f.age, light);
                return;
            }
            let (kind, r) = match l.tex {
                Tex::Mob(t) => (MOB, atlas.get(t).copied().unwrap_or([0., 0., 0., 0.])),
                Tex::Armor(cell) => (ARMOR, [0., cell as f32 / ARMOR_CELLS as f32, 1., 1. / ARMOR_CELLS as f32]),
            };
            let n = Mat3::from_mat4(pm).inverse().transpose() * q.normal;
            let shade = standard_lighting(n, lights);
            let color = [l.tint[0] * shade * light[0], l.tint[1] * shade * light[1], l.tint[2] * shade * light[2], 1.];
            let overlay = l.overlay.map_or([0.; 4], |o| [o[0] * light[0], o[1] * light[1], o[2] * light[2], o[3]]);
            let mvp = vp * pm;
            if use_instances && !l.additive {
                instances.push(EntityInstance { pos: q.pos.map(|p| p.to_array()),
                    uv: q.uv.map(|[u,v]| [r[0] + u * r[2], r[1] + v * r[3]]),
                    mvp: mvp.to_cols_array_2d(), color, kind, overlay });
                return;
            }
            let target = if l.additive { &mut *additive } else { &mut *out };
            let clip = q.pos.map(|p| (mvp * p.extend(1.)).to_array());
            for i in [0, 1, 2, 0, 2, 3] {
                let [u, v] = q.uv[i];
                target.push(Vertex { clip: clip[i], uv: [r[0] + u * r[2], r[1] + v * r[3]], color, kind, overlay });
            }
        });
        if let (Some(item), Some(m)) = (&f.held, held) {
            render_item(out, &mut self.glint, vp, m, item, TransformType::ThirdPerson, lights, f.light);
        }
    }

    /// Non-living entities: `RenderArrow`, `RenderXPOrb`, `RenderPainting`, `ArmorStandRenderer`.
    fn object(&mut self, out: &mut Vec<Vertex>, instances: &mut Vec<EntityInstance>, use_instances: bool, f: &crate::objects::ObjectFrame, vp: Mat4, cam: &Camera) {
        let lights = lights_in(Mat3::IDENTITY);
        let light = f.light();
        let (atlas, glint, additive) = (&self.mob_atlas, &mut self.glint, &mut self.additive);
        crate::objects::render(f, &mut self.stands, cam.yaw, cam.pitch, &mut |l, pm, q| {
            if l.glint {
                armor_glint(glint, vp * pm, q, 0., light);
                return;
            }
            let (kind, r) = match l.tex {
                Tex::Mob(t) => (MOB, atlas.get(t).copied().unwrap_or([0., 0., 0., 0.])),
                Tex::Armor(cell) => (ARMOR, [0., cell as f32 / ARMOR_CELLS as f32, 1., 1. / ARMOR_CELLS as f32]),
            };
            let n = Mat3::from_mat4(pm).inverse().transpose() * q.normal;
            let shade = standard_lighting(n, lights);
            let alpha = crate::objects::layer_alpha(l);
            let color = [l.tint[0] * shade * light[0], l.tint[1] * shade * light[1], l.tint[2] * shade * light[2], alpha];
            if use_instances && !l.additive {
                instances.push(EntityInstance { pos: q.pos.map(|p| ((pm * p.extend(1.)).truncate() - cam.pos).to_array()),
                    uv: q.uv.map(|[u,v]| [r[0] + u * r[2], r[1] + v * r[3]]), mvp: vp.to_cols_array_2d(),
                    color, kind, overlay: [0.; 4] });
                return;
            }
            let target = if l.additive { &mut *additive } else { &mut *out };
            let clip = q.pos.map(|p| vp * ((pm * p.extend(1.)).truncate() - cam.pos).extend(1.));
            for i in [0, 1, 2, 0, 2, 3] {
                let [u, v] = q.uv[i];
                target.push(Vertex { clip: clip[i].to_array(), uv: [r[0] + u * r[2], r[1] + v * r[3]], color, kind, overlay: [0.; 4] });
            }
        });
    }
    /// Tile entities: `TileEntityRendererDispatcher` with the standard item lighting active.
    fn tile(&mut self, out: &mut Vec<Vertex>, instances: &mut Vec<EntityInstance>, use_instances: bool, f: &crate::tile::TileFrame, vp: Mat4, cam: Vec3) {
        let lights = lights_in(Mat3::IDENTITY);
        let light = f.light();
        let atlas = &self.mob_atlas;
        crate::tile::render(f, &self.chests, &mut |l, pm, q| {
            let r = match l.tex {
                Tex::Mob(t) => atlas.get(t).copied().unwrap_or([0., 0., 0., 0.]),
                Tex::Armor(cell) => [0., cell as f32 / ARMOR_CELLS as f32, 1., 1. / ARMOR_CELLS as f32],
            };
            let n = Mat3::from_mat4(pm).inverse().transpose() * q.normal;
            let shade = standard_lighting(n, lights);
            let color = [l.tint[0] * shade * light[0], l.tint[1] * shade * light[1], l.tint[2] * shade * light[2], 1.];
            if use_instances {
                instances.push(EntityInstance { pos: q.pos.map(|p| ((pm * p.extend(1.)).truncate() - cam).to_array()),
                    uv: q.uv.map(|[u,v]| [r[0] + u * r[2], r[1] + v * r[3]]), mvp: vp.to_cols_array_2d(),
                    color, kind: MOB, overlay: [0.; 4] });
                return;
            }
            let clip = q.pos.map(|p| vp * ((pm * p.extend(1.)).truncate() - cam).extend(1.));
            for i in [0, 1, 2, 0, 2, 3] {
                let [u, v] = q.uv[i];
                out.push(Vertex { clip: clip[i].to_array(), uv: [r[0] + u * r[2], r[1] + v * r[3]], color, kind: MOB, overlay: [0.; 4] });
            }
        });
    }
}

/// `LayerArmorBase.renderGlint` for one quad: two passes, lighting off, color
/// (0.5, 0.25, 0.8) * 0.76, texture matrix scale(1/3) . rotate(30 - 60i) .
/// translate(0, age * (0.001 + 0.003i) * 20).
fn armor_glint(glint: &mut Vec<Vertex>, mvp: Mat4, q: &ModelQuad, age: f32, light: [f32; 3]) {
    let color = [0.5 * 0.76 * light[0], 0.25 * 0.76 * light[1], 0.8 * 0.76 * light[2], 1.];
    let clip = q.pos.map(|p| (mvp * p.extend(1.)).to_array());
    for i in 0..2 {
        let (s, c) = (30. - i as f32 * 60.).to_radians().sin_cos();
        let d = age * (0.001 + i as f32 * 0.003) * 20.;
        for k in [0, 1, 2, 0, 2, 3] {
            let [u, v] = q.uv[k];
            let v = v + d;
            let uv = [(u * c - v * s) / 3., (u * s + v * c) / 3.];
            glint.push(Vertex { clip: clip[k], uv, color, kind: GLINT, overlay: [0.; 4] });
        }
    }
}

/// Shelf-packs every mob texture into one atlas.
#[allow(clippy::type_complexity)]
fn mob_atlas(load: &dyn Fn(&str) -> image::RgbaImage) -> (image::RgbaImage, std::collections::HashMap<&'static str, [f32; 4]>) {
    let mut imgs: Vec<(&'static str, image::RgbaImage)> = crate::mobs::TEXTURES
        .iter()
        .chain(crate::objects::TEXTURES.iter())
        .chain(crate::tile::TEXTURES.iter())
        .map(|t| (*t, load(&format!("textures/entity/{t}.png"))))
        .collect();
    // Object-atlas images outside `textures/entity/` (the painting sheet).
    imgs.extend(crate::objects::EXTRA_TEXTURES.iter().map(|(k, p)| (*k, load(p))));
    imgs.sort_by_key(|(_, i)| std::cmp::Reverse((i.height(), i.width())));
    let width = 1024u32.max(imgs.iter().map(|(_, i)| i.width()).max().unwrap_or(0));
    let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
    let mut placed = Vec::new();
    for (name, img) in &imgs {
        if x + img.width() > width {
            x = 0;
            y += row + 2;
            row = 0;
        }
        placed.push((*name, x, y));
        x += img.width() + 2;
        row = row.max(img.height());
    }
    let height = (y + row).max(1);
    let mut atlas = image::RgbaImage::new(width, height);
    let mut rects = std::collections::HashMap::new();
    for ((name, px, py), (_, img)) in placed.into_iter().zip(&imgs) {
        image::imageops::replace(&mut atlas, img, px as i64, py as i64);
        rects.insert(name, [px as f32 / width as f32, py as f32 / height as f32, img.width() as f32 / width as f32, img.height() as f32 / height as f32]);
    }
    (atlas, rects)
}

fn retarget_skin(verts: &mut [Vertex], skin: Option<u32>) {
    let Some(cell) = skin else { return };
    let per = (SKIN_ATLAS / 64) as f32;
    let (cx, cy) = ((cell % (SKIN_ATLAS / 64)) as f32, (cell / (SKIN_ATLAS / 64)) as f32);
    for v in verts {
        v.kind = 5.;
        v.uv = [(cx + v.uv[0]) / per, (cy + v.uv[1]) / per];
    }
}

/// `RenderItem.renderItemModelForEntity` (+ `ItemRenderer.renderItem`'s 3D scale) for one transform type.
/// `RenderItem.renderEffect`: the model again twice in color -8372020, texture matrix
/// scale(8) . translate(+-f, 0) . rotate(-50 / 10) over the atlas UVs, (SRC_COLOR, ONE) blended.
fn item_glint(glint: &mut Vec<Vertex>, clip: &[Vec4; 4], uv: &[[f32; 2]; 4], light: [f32; 3]) {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let f = (ms % 3000) as f32 / 3000. / 8.;
    let f1 = (ms % 4873) as f32 / 4873. / 8.;
    let color = [128. / 255. * light[0], 64. / 255. * light[1], 204. / 255. * light[2], 1.];
    for (off, deg) in [(f, -50f32), (-f1, 10.)] {
        let (s, c) = deg.to_radians().sin_cos();
        for i in [0, 1, 2, 0, 2, 3] {
            let [u, v] = uv[i];
            glint.push(Vertex { clip: clip[i].to_array(), uv: [8. * (c * u - s * v + off), 8. * (s * u + c * v)], color, kind: GLINT, overlay: [0.; 4] });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_item(out: &mut Vec<Vertex>, glint: &mut Vec<Vertex>, proj: Mat4, m: Mat4, item: &HeldItem, t: TransformType, lights: [Vec3; 2], light: [f32; 3]) {
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
            out.push(Vertex { clip: clip[i].to_array(), uv: q.uv[i], color: c, kind: ATLAS, overlay: [0.; 4] });
        }
        if item.glint {
            item_glint(glint, &clip, &q.uv, light);
        }
    }
}

/// `RenderEntityItem.doRender`
fn dropped_item(out: &mut Vec<Vertex>, glint: &mut Vec<Vertex>, vp: Mat4, cam: Vec3, d: &DroppedItem) {
    let model = &d.item.model;
    let ground = model.transform(TransformType::Ground);
    let scale = Vec3::new(ground.x_axis.truncate().length(), ground.y_axis.truncate().length(), ground.z_axis.truncate().length());
    let copies = match d.count {
        49.. => 5,
        33.. => 4,
        17.. => 3,
        2.. => 2,
        _ => 1,
    };
    let bob = mc_core::math::sin(d.age / 10. + d.hover_start) * 0.1 + 0.1;
    let mut m = Mat4::from_translation(d.pos - cam + Vec3::new(0., bob + 0.25 * scale.y, 0.))
        * Mat4::from_rotation_y((d.age / 20. + d.hover_start) * (180. / std::f32::consts::PI) * std::f32::consts::PI / 180.);
    if !model.gui3d {
        m *= Mat4::from_translation(Vec3::new(0., 0., -0.046875 * (copies - 1) as f32 * 0.5));
    }
    let mut rand = mc_core::random::JavaRandom::new(187);
    let lights = lights_in(Mat3::IDENTITY);
    for j in 0..copies {
        if model.gui3d {
            let mut im = m;
            if j > 0 {
                let mut r = || (rand.next_float() * 2. - 1.) * 0.15;
                im *= Mat4::from_translation(Vec3::new(r(), r(), r()));
            }
            im *= Mat4::from_scale(Vec3::splat(0.5)) * ground;
            entity_item(out, glint, vp, im, &d.item, lights, d.light);
        } else {
            entity_item(out, glint, vp, m * ground, &d.item, lights, d.light);
            m *= Mat4::from_translation(Vec3::new(0., 0., 0.046875 * scale.z));
        }
    }
}

/// `RenderItem.renderItem(stack, model)`: scale 0.5, centre, draw (both faces of flat items).
#[allow(clippy::too_many_arguments)]
fn entity_item(out: &mut Vec<Vertex>, glint: &mut Vec<Vertex>, vp: Mat4, m: Mat4, item: &HeldItem, lights: [Vec3; 2], light: [f32; 3]) {
    let m = m * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::splat(-0.5));
    let normal_m = Mat3::from_mat4(m).inverse().transpose();
    for q in &item.model.quads {
        let (nx, ny, nz) = q.face.offset();
        let shade = standard_lighting(normal_m * Vec3::new(nx as f32, ny as f32, nz as f32), lights);
        let tint = if q.tint_index >= 0 { item.tints[(q.tint_index as usize).min(3)] } else { 0xFFFFFF };
        let c = [
            ((tint >> 16) & 255) as f32 / 255. * shade * light[0],
            ((tint >> 8) & 255) as f32 / 255. * shade * light[1],
            (tint & 255) as f32 / 255. * shade * light[2],
            1.,
        ];
        let clip = q.pos.map(|p| vp * m * Vec3::from(p).extend(1.));
        if is_back_face(&clip) {
            continue;
        }
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(Vertex { clip: clip[i].to_array(), uv: q.uv[i], color: c, kind: ATLAS, overlay: [0.; 4] });
        }
        if item.glint {
            item_glint(glint, &clip, &q.uv, light);
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
#[allow(clippy::too_many_arguments)]
fn player_quad(out: &mut Vec<Vertex>, instances: &mut Vec<EntityInstance>, use_instances: bool, proj: Mat4, m: Mat4, q: &ModelQuad, kind: f32, lights: [Vec3; 2], light: [f32; 3], tint: [f32; 3]) {
    if !use_instances { model_quad(out, proj, m, q, kind, lights, light, tint, false); return; }
    let n = Mat3::from_mat4(m).inverse().transpose() * q.normal;
    let shade = standard_lighting(n, lights);
    let color = [tint[0] * shade * light[0], tint[1] * shade * light[1], tint[2] * shade * light[2], 1.];
    instances.push(EntityInstance { pos: q.pos.map(|p| p.to_array()), uv: q.uv,
        mvp: (proj * m).to_cols_array_2d(), color, kind, overlay: [0.; 4] });
}

fn model_quad(out: &mut Vec<Vertex>, proj: Mat4, m: Mat4, q: &ModelQuad, kind: f32, lights: [Vec3; 2], light: [f32; 3], tint: [f32; 3], _cull: bool) {
    let n = Mat3::from_mat4(m).inverse().transpose() * q.normal;
    let shade = standard_lighting(n, lights);
    let c = [tint[0] * shade * light[0], tint[1] * shade * light[1], tint[2] * shade * light[2], 1.];
    let mvp = proj * m;
    let clip = q.pos.map(|p| (mvp * p.extend(1.)).to_array());
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(Vertex { clip: clip[i], uv: q.uv[i], color: c, kind, overlay: [0.; 4] });
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
            out.push(Vertex { clip: quad[i].to_array(), uv: [0.; 2], color: [0., 0., 0., 0.4], kind: LINE, overlay: [0.; 4] });
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

#[cfg(test)]
mod particle_instance_tests {
    use super::*;

    #[test]
    fn particle_instance_layout_is_compact() {
        assert_eq!(std::mem::size_of::<ParticleInstance>(), 100);
        assert!(std::mem::size_of::<ParticleInstance>() < 6 * std::mem::size_of::<Vertex>());
    }

    /// CPU-side preparation/encoding only; excludes the time waiting for the GPU and presentation.
    #[test]
    #[ignore = "manual headless particle benchmark"]
    fn particle_headless_bench() {
        let jar = mc_testutil::jar().expect("benchmark needs the vanilla jar");
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("headless GPU");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let mut assets = AssetSource::new(); assets.push(&jar).unwrap();
        let atlas = texture(&device, &queue, &image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255])));
        let mut renderer = AvatarRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &assets, atlas.create_view(&Default::default()));
        let make_texture = |format, usage| device.create_texture(&wgpu::TextureDescriptor { label: None,
            size: wgpu::Extent3d { width: 128, height: 128, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
        let color = make_texture(wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let depth = make_texture(wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
        let camera = Camera { pos: Vec3::ZERO, yaw: 0., pitch: 0., fov_deg: 70., bob: Mat4::IDENTITY };
        let particles = (0..4000).map(|i| {
            let x = (i % 40) as f32 * 0.05 - 1.; let y = (i / 40 % 40) as f32 * 0.05 - 1.; let z = 3. + (i % 13) as f32 * 0.03;
            ParticleQuad { pos: [Vec3::new(x,y,z), Vec3::new(x,y+0.04,z), Vec3::new(x+0.04,y+0.04,z), Vec3::new(x+0.04,y,z)],
                uv: [[0.,1.],[0.,0.],[1.,0.],[1.,1.]], color: [0.8,0.5,0.3,0.8], light: [0.9;3], tex: ParticleTex::Block }
        }).collect();
        let frame = AvatarFrame { particles, ..Default::default() };
        for enabled in [false, true] {
            renderer.use_particle_instancing = enabled;
            let mut sum = std::time::Duration::ZERO; let mut max = std::time::Duration::ZERO;
            for iteration in 0..31 {
                let mut enc = device.create_command_encoder(&Default::default());
                {
                    let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &color, resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                        timestamp_writes: None, occlusion_query_set: None });
                }
                let t = std::time::Instant::now();
                renderer.encode(&device, &queue, &mut enc, &color, &depth, &camera, 1., 128., 128., &frame, &assets, false);
                let elapsed = t.elapsed();
                queue.submit(Some(enc.finish())); device.poll(wgpu::Maintain::Wait);
                if iteration > 0 { sum += elapsed; max = max.max(elapsed); }
            }
            eprintln!("particles {}: 4000 quads, mean encode {:.3} ms, max {:.3} ms, {} upload bytes/frame (CPU only)",
                if enabled { "instanced" } else { "legacy" }, sum.as_secs_f64()*1000./30., max.as_secs_f64()*1000.,
                if enabled { 4000 * std::mem::size_of::<ParticleInstance>() } else { 4000 * 6 * std::mem::size_of::<Vertex>() });
        }
        let mob = MobFrame { mob: crate::mobs::Mob::Zombie { villager: false }, id: 1,
            pos: Vec3::new(0., -1., 4.), body_yaw: 15., head_yaw: 10., pitch: 5., limb_swing: 1., limb_amount: 0.5,
            age: 3., ticks: 3, swing: 0., child: false, hurt: true, death: 0., light: [1.;3], bright: [1.;3],
            held: None, armor: [None;4] };
        let frame = AvatarFrame { mobs: vec![mob; 200], ..Default::default() };
        for (enabled, reuse) in [(false, true), (true, false), (true, true)] {
            renderer.use_entity_instancing = enabled;
            let mut sum = std::time::Duration::ZERO;
            for iteration in 0..31 {
                let mut enc = device.create_command_encoder(&Default::default());
                if !reuse { renderer.entity_scratch = Vec::new(); }
                let t = std::time::Instant::now();
                renderer.encode(&device, &queue, &mut enc, &color, &depth, &camera, 1., 128., 128., &frame, &assets, false);
                let elapsed = t.elapsed();
                queue.submit(Some(enc.finish())); device.poll(wgpu::Maintain::Wait);
                if iteration > 0 { sum += elapsed; }
            }
            eprintln!("mobs {}: 200 zombies, mean CPU encode {:.3} ms", if !enabled { "legacy" } else if reuse { "instanced/reused" } else { "instanced/fresh" }, sum.as_secs_f64()*1000./30.);
        }
    }

    #[test]
    fn instanced_geometry_matches_legacy_pixels_headlessly() {
        let Some(jar) = mc_testutil::jar() else { return };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
        let mut assets = AssetSource::new(); assets.push(&jar).unwrap();
        let atlas = texture(&device, &queue, &image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255])));
        let mut renderer = AvatarRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &assets, atlas.create_view(&Default::default()));
        let camera = Camera { pos: Vec3::new(0.15, 0.1, 0.2), yaw: 0., pitch: 0., fov_deg: 70., bob: Mat4::IDENTITY };
        let quad = |z, color, tex| ParticleQuad {
            pos: [Vec3::new(-0.8,-0.8,z), Vec3::new(-0.8,0.8,z), Vec3::new(0.8,0.8,z), Vec3::new(0.8,-0.8,z)],
            uv: [[0.,1.], [0.,0.], [1.,0.], [1.,1.]], color, light: [0.9, 0.8, 0.7], tex,
        };
        let mut frame = AvatarFrame { particles: vec![quad(4., [0.8,0.2,0.3,0.8], ParticleTex::Block),
            quad(3., [0.2,0.8,0.4,0.6], ParticleTex::Block), quad(2., [1.,1.,1.,0.7], ParticleTex::Misc)], ..Default::default() };
        frame.mobs.push(MobFrame { mob: crate::mobs::Mob::Zombie { villager: false }, id: 1,
            pos: Vec3::new(0., -1., 2.5), body_yaw: 15., head_yaw: 10., pitch: 5., limb_swing: 1., limb_amount: 0.5,
            age: 3., ticks: 3, swing: 0., child: false, hurt: true, death: 0., light: [1.;3], bright: [1.;3],
            held: None, armor: [None;4] });
        frame.world_quads.push(WorldQuad { pos: quad(5., [1.;4], ParticleTex::Block).pos,
            uv: [[0.,1.], [0.,0.], [1.,0.], [1.,1.]], color: [0.1,0.1,0.5] });
        frame.damage.push(DamageQuad { pos: quad(6., [1.;4], ParticleTex::Block).pos,
            uv: [[0.,1.], [0.,0.], [1.,0.], [1.,1.]] });
        frame.outline = Some(Aabb::new(-0.6, -0.6, 2.5, 0.6, 0.6, 3.5));
        let render = |renderer: &mut AvatarRenderer, frame: &AvatarFrame, instancing: bool| -> Vec<u8> {
            renderer.use_particle_instancing = instancing;
            renderer.use_entity_instancing = instancing;
            let desc = |format, usage| wgpu::TextureDescriptor { label: None,
                size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
                dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] };
            let color = device.create_texture(&desc(wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC));
            let depth = device.create_texture(&desc(wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT));
            let color_view = color.create_view(&Default::default()); let depth_view = depth.create_view(&Default::default());
            let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 64*64*4,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = device.create_command_encoder(&Default::default());
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &color_view, resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                    timestamp_writes: None, occlusion_query_set: None });
            }
            renderer.encode(&device, &queue, &mut enc, &color_view, &depth_view, &camera, 1., 64., 128., frame, &assets, false);
            enc.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture: &color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &read, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(64) } },
                wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 });
            queue.submit(Some(enc.finish()));
            let (tx, rx) = std::sync::mpsc::channel();
            read.slice(..).map_async(wgpu::MapMode::Read, move |r| { tx.send(r).unwrap(); });
            device.poll(wgpu::Maintain::Wait); rx.recv().unwrap().unwrap();
            let bytes = read.slice(..).get_mapped_range().to_vec(); read.unmap(); bytes
        };
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert!(old.chunks_exact(4).any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0));
        assert_eq!(old, new);
        frame.particles.clear(); frame.world_quads.clear(); frame.damage.clear(); frame.outline = None;
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert!(old.chunks_exact(4).any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0), "mob must be visible");
        assert_eq!(old, new);
        let mut spider = frame.mobs[0].clone();
        spider.mob = crate::mobs::Mob::Spider { cave: false }; spider.pos = Vec3::new(-0.5, -0.8, 3.);
        let mut enchanted = frame.mobs[0].clone();
        enchanted.pos.x = 0.5; enchanted.armor[3] = Some(ArmorPiece { material: 2, color: 0xffffff, glint: true });
        frame.mobs.extend([spider, enchanted, frame.mobs[0].clone()]);
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert_eq!(old, new);
        frame.mobs.clear();
        renderer.upload_skin(&queue, 0, &vec![255; 64 * 64 * 4]);
        let player = AvatarFrame { pos: Vec3::new(-0.3, -1., 2.5), body_yaw: 15., head_yaw: 20.,
            limb_swing: 1., limb_amount: 0.5, light: [1.;3], skin: Some(0), slim: true,
            armor: [None, None, Some(ArmorPiece { material: 0, color: 0xa04020, glint: false }), None], ..Default::default() };
        frame.others.push(player);
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert!(old.chunks_exact(4).any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0), "player must be visible");
        assert_eq!(old, new);
        let held_model = ItemModel { quads: vec![mc_assets::model::BakedQuad {
            pos: [[0.,0.,0.],[0.,1.,0.],[1.,1.,0.],[1.,0.,0.]], uv: [[0.,1.],[0.,0.],[1.,0.],[1.,1.]],
            face: mc_core::Facing::North, cullface: None, tint_index: -1, shade: 1. }],
            gui3d: false, builtin: false, transforms: Default::default() };
        frame.others[0].held = Some(HeldItem { model: Arc::new(held_model), tints: [0xffffff; 4], entity_block: false, glint: false });
        frame.mobs.push(MobFrame { mob: crate::mobs::Mob::Zombie { villager: false }, id: 2,
            pos: Vec3::new(0.2, -1., 3.), body_yaw: 0., head_yaw: 0., pitch: 0., limb_swing: 0., limb_amount: 0.,
            age: 0., ticks: 0, swing: 0., child: false, hurt: false, death: 0., light: [1.;3], bright: [1.;3],
            held: None, armor: [None;4] });
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert_eq!(old, new);
        frame.mobs.clear();
        frame.others[0].armor[2].as_mut().unwrap().glint = true;
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert_eq!(old, new);
        frame.others.clear();
        frame.objects.push(crate::objects::ObjectFrame::XpOrb { pos: Vec3::new(-0.2, 0., 2.), icon: 2, color: [0.8, 0.9, 0.2], light: [1.;3] });
        frame.objects.push(crate::objects::ObjectFrame::ArmorStand { pos: Vec3::new(0.3, -1., 3.), yaw: 30., head_yaw: 10., pitch: 5., flags: 0, light: [1.;3], body_rot: (5., 10., 15.) });
        frame.tiles.push(crate::tile::TileFrame::Chest { pos: Vec3::new(-0.5, -1., 3.), meta: 2, large: false,
            skin: crate::tile::ChestSkin::Normal, x_shift: false, z_shift: false, lid: 0.5, light: [1.;3] });
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert!(old.chunks_exact(4).any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0), "objects and chest must be visible");
        assert_eq!(old, new);
        frame.objects.clear(); frame.tiles.clear();
        frame.particles = (0..1500).map(|i| quad(2. + (i % 100) as f32 * 0.01, [0.2, 0.7, 0.4, 0.6], ParticleTex::Block)).collect();
        let old = render(&mut renderer, &frame, false);
        let new = render(&mut renderer, &frame, true);
        assert_eq!(old, new);
        assert!(renderer.particle_capacity >= 1500);
        assert!(renderer.particle_scratch.capacity() >= 1500);
        assert!(renderer.entity_scratch.capacity() > 0);
    }
}
