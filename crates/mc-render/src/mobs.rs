//! Mob models (`ModelCreeper` .. `ModelRabbit` and the `ModelBiped` family) posed and drawn
//! the way `RendererLivingEntity.doRender` does it: `rotateCorpse`, `scale(-1, -1, 1)`,
//! `preRenderCallback`, `setLivingAnimations` + `setRotationAngles`, the main model, then the
//! renderer's layers.
use crate::avatar::{ArmorPiece, HeldItem};
use crate::entity_model::{ModelQuad, Part, PlayerModel, Pose};
use glam::{Mat4, Vec3};
use mc_core::math::{cos, sin, sqrt_float};
use std::f32::consts::PI;

/// What the renderer needs to know about one mob beyond the common living-entity state.
#[derive(Clone, Copy, Debug)]
pub enum Mob {
    Zombie { villager: bool },
    PigZombie,
    Giant,
    Skeleton { wither: bool },
    /// `getCreeperFlashIntensity(partialTicks)`
    Creeper { flash: f32 },
    Spider { cave: bool },
    Enderman { carrying: bool, screaming: bool },
    Pig { saddle: bool },
    Cow { mooshroom: bool },
    Sheep { color: u8, sheared: bool },
    /// `RenderChicken.handleRotationFloat`
    Chicken { wing: f32 },
    /// `getTailRotation()`
    Wolf { tail: f32, sitting: bool, angry: bool, tamed: bool, collar: u8 },
    /// Size and interpolated `squishFactor`.
    Slime { size: f32, squish: f32 },
    MagmaCube { size: f32, squish: f32 },
    Villager { profession: i32 },
    Witch { holding: bool },
    /// Interpolated `squidPitch`, `squidYaw` and tentacle angle.
    Squid { pitch: f32, yaw: f32, tentacle: f32 },
    Bat { hanging: bool },
    Silverfish,
    Endermite,
    Blaze,
    Ghast { shooting: bool },
    SnowMan,
    /// `getAttackTimer() - partialTicks` (0 when idle) and `getHoldRoseTick()`.
    IronGolem { attack: f32, rose: i32 },
    /// `getTameSkin()`, and the `ModelOcelot` pose: 0 sneaking, 1 standing, 2 sprinting, 3 sitting.
    Ocelot { skin: u8, state: u8, tamed: bool },
    Rabbit { kind: u8 },
}

/// One mob to draw this frame.
#[derive(Clone)]
pub struct MobFrame {
    pub mob: Mob,
    pub id: i32,
    /// Interpolated feet position.
    pub pos: Vec3,
    pub body_yaw: f32,
    pub head_yaw: f32,
    pub pitch: f32,
    /// `limbSwing - limbSwingAmount * (1 - partialTicks)`
    pub limb_swing: f32,
    pub limb_amount: f32,
    /// `ticksExisted + partialTicks`
    pub age: f32,
    pub ticks: i32,
    pub swing: f32,
    pub child: bool,
    /// `hurtTime > 0`
    pub hurt: bool,
    /// `deathTime + partialTicks` while dying, else 0.
    pub death: f32,
    pub light: [f32; 3],
    /// Lightmap color at full block and sky light (`61680`): the eye layers.
    pub bright: [f32; 3],
    /// `LayerHeldItem` (bipeds only).
    pub held: Option<HeldItem>,
    /// `getEquipmentInSlot(1..4)` (0 boots .. 3 helmet).
    pub armor: [Option<ArmorPiece>; 4],
}

/// The texture a layer samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tex {
    /// `textures/entity/<path>.png` in the mob atlas.
    Mob(&'static str),
    /// Armor atlas cell (as `LayerBipedArmor` lays them out).
    Armor(u32),
    /// A downloaded player skin's cell in the skin atlas (player skulls).
    Skin(u32),
}

/// How one model pass is drawn.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub tex: Tex,
    pub tint: [f32; 3],
    /// `setBrightness`'s combiner: `rgb * w + texel * (1 - w)` as `[r, g, b, w]`.
    pub overlay: Option<[f32; 4]>,
    /// `blendFunc(ONE, ONE)` at full brightness (spider and enderman eyes).
    pub additive: bool,
    /// `GlStateManager.enableNormalize` for this pass (`LayerSlimeGel`).
    pub normalize: bool,
    /// `LayerArmorBase.renderGlint` over this pass's geometry.
    pub glint: bool,
}

impl Layer {
    fn tex(path: &'static str) -> Self {
        Self { tex: Tex::Mob(path), tint: [1.; 3], overlay: None, additive: false, normalize: false, glint: false }
    }
}

/// Every texture a mob layer can name, for the atlas.
pub const TEXTURES: &[&str] = &[
    "zombie/zombie",
    "zombie/zombie_villager",
    "zombie_pigman",
    "skeleton/skeleton",
    "skeleton/wither_skeleton",
    "creeper/creeper",
    "spider/spider",
    "spider/cave_spider",
    "spider_eyes",
    "enderman/enderman",
    "enderman/enderman_eyes",
    "pig/pig",
    "pig/pig_saddle",
    "cow/cow",
    "cow/mooshroom",
    "sheep/sheep",
    "sheep/sheep_fur",
    "chicken",
    "wolf/wolf",
    "wolf/wolf_tame",
    "wolf/wolf_angry",
    "wolf/wolf_collar",
    "slime/slime",
    "slime/magmacube",
    "villager/villager",
    "villager/farmer",
    "villager/librarian",
    "villager/priest",
    "villager/smith",
    "villager/butcher",
    "witch",
    "squid",
    "bat",
    "silverfish",
    "endermite",
    "blaze",
    "ghast/ghast",
    "ghast/ghast_shooting",
    "snowman",
    "iron_golem",
    "cat/ocelot",
    "cat/black",
    "cat/red",
    "cat/siamese",
    "rabbit/brown",
    "rabbit/white",
    "rabbit/black",
    "rabbit/white_splotched",
    "rabbit/gold",
    "rabbit/salt",
    "rabbit/caerbannog",
];

/// `EntitySheep.getDyeRgb` by dye metadata.
pub const DYE_RGB: [[f32; 3]; 16] = [
    [1.0, 1.0, 1.0],
    [0.85, 0.5, 0.2],
    [0.7, 0.3, 0.85],
    [0.4, 0.6, 0.85],
    [0.9, 0.9, 0.2],
    [0.5, 0.8, 0.1],
    [0.95, 0.5, 0.65],
    [0.3, 0.3, 0.3],
    [0.6, 0.6, 0.6],
    [0.3, 0.5, 0.6],
    [0.5, 0.25, 0.7],
    [0.2, 0.3, 0.7],
    [0.4, 0.3, 0.2],
    [0.4, 0.5, 0.2],
    [0.6, 0.2, 0.2],
    [0.1, 0.1, 0.1],
];

const T32: [f32; 2] = [64., 32.];
const T64: [f32; 2] = [64., 64.];

fn b(t: [f32; 2], uv: [f32; 2], from: [f32; 3], size: [i32; 3], d: f32) -> Part {
    Part::new().add_box(t, uv, from, size, d, false)
}

fn bm(t: [f32; 2], uv: [f32; 2], from: [f32; 3], size: [i32; 3], d: f32) -> Part {
    Part::new().add_box(t, uv, from, size, d, true)
}

/// A `ModelBase` as its parts in render order; the first `head` parts take the baby head
/// transform (`scale`, then `translate(0, y, z) * 1/16`), the rest `scale(0.5) .
/// translate(0, 24/16, 0)`.
#[derive(Clone)]
pub struct Model {
    pub parts: Vec<Part>,
    head: usize,
    child: Option<(f32, f32, f32)>,
}

impl Model {
    fn new(parts: Vec<Part>, head: usize, child: Option<(f32, f32, f32)>) -> Self {
        Self { parts, head, child }
    }

    fn render(&self, m: Mat4, child: bool, l: &Layer, out: &mut dyn FnMut(&Layer, Mat4, &ModelQuad)) {
        let (hm, bm) = match (child, self.child) {
            (true, Some((s, y, z))) => (
                m * Mat4::from_scale(Vec3::splat(s)) * Mat4::from_translation(Vec3::new(0., y, z) / 16.),
                m * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::new(0., 24. / 16., 0.)),
            ),
            _ => (m, m),
        };
        for (i, p) in self.parts.iter().enumerate() {
            p.render(if i < self.head { hm } else { bm }, &mut |pm, q| out(l, pm, q));
        }
    }
}

/// `ModelBiped.render`: the baby head at 3/4 scale; the headwear goes with the body.
fn render_biped(b: &PlayerModel, m: Mat4, child: bool, l: &Layer, out: &mut dyn FnMut(&Layer, Mat4, &ModelQuad)) {
    let mut emit = |p: &Part, m: Mat4| p.render(m, &mut |pm, q| out(l, pm, q));
    if child {
        emit(&b.head, m * Mat4::from_scale(Vec3::splat(0.75)) * Mat4::from_translation(Vec3::new(0., 1., 0.)));
        let bm = m * Mat4::from_scale(Vec3::splat(0.5)) * Mat4::from_translation(Vec3::new(0., 1.5, 0.));
        for p in [&b.body, &b.right_arm, &b.left_arm, &b.right_leg, &b.left_leg, &b.headwear] {
            emit(p, bm);
        }
    } else {
        for p in [&b.head, &b.body, &b.right_arm, &b.left_arm, &b.right_leg, &b.left_leg, &b.headwear] {
            emit(p, m);
        }
    }
}

/// `ModelZombie.setRotationAngles` after the biped's: arms held out.
fn zombie_arms(b: &mut PlayerModel, swing: f32, age: f32) {
    let f = sin(swing * PI);
    let f1 = sin((1. - (1. - swing) * (1. - swing)) * PI);
    b.right_arm.rot.z = 0.;
    b.left_arm.rot.z = 0.;
    b.right_arm.rot.y = -(0.1 - f * 0.6);
    b.left_arm.rot.y = 0.1 - f * 0.6;
    b.right_arm.rot.x = -(PI / 2.);
    b.left_arm.rot.x = -(PI / 2.);
    b.right_arm.rot.x -= f * 1.2 - f1 * 0.4;
    b.left_arm.rot.x -= f * 1.2 - f1 * 0.4;
    b.right_arm.rot.z += cos(age * 0.09) * 0.05 + 0.05;
    b.left_arm.rot.z -= cos(age * 0.09) * 0.05 + 0.05;
    b.right_arm.rot.x += sin(age * 0.067) * 0.05;
    b.left_arm.rot.x -= sin(age * 0.067) * 0.05;
}

fn pose_biped(b: &mut PlayerModel, f: &MobFrame, limb: f32, amount: f32) {
    b.swing = f.swing;
    b.sneak = false;
    b.held_right = 0;
    b.aimed_bow = false;
    b.set_angles(Pose { limb_swing: limb, limb_amount: amount, age: f.age, head_yaw: f.head_yaw - f.body_yaw, head_pitch: f.pitch });
}

/// `ModelZombieVillager`: the villager head (with nose) on a biped.
fn zombie_villager(delta: f32, armor: bool) -> PlayerModel {
    let t = if armor { T32 } else { T64 };
    let mut m = PlayerModel::biped_with(delta, 0., t);
    m.head = if armor {
        b(t, [0., 0.], [-4., -10., -4.], [8, 8, 8], delta)
    } else {
        b(t, [0., 32.], [-4., -10., -4.], [8, 10, 8], delta).add_box(t, [24., 32.], [-1., -3., -6.], [2, 4, 2], delta, false)
    };
    m
}

/// `ModelSkeleton`: thin limbs.
fn skeleton() -> PlayerModel {
    let mut m = PlayerModel::biped_with(0., 0., T32);
    m.right_arm = b(T32, [40., 16.], [-1., -2., -1.], [2, 12, 2], 0.).at(-5., 2., 0.);
    m.left_arm = bm(T32, [40., 16.], [-1., -2., -1.], [2, 12, 2], 0.).at(5., 2., 0.);
    m.right_leg = b(T32, [0., 16.], [-1., 0., -1.], [2, 12, 2], 0.).at(-2., 12., 0.);
    m.left_leg = bm(T32, [0., 16.], [-1., 0., -1.], [2, 12, 2], 0.).at(2., 12., 0.);
    m
}

/// `ModelEnderman(0)`
fn enderman() -> PlayerModel {
    let y = -14.;
    let mut m = PlayerModel::biped_with(0., y, T32);
    m.headwear = b(T32, [0., 16.], [-4., -8., -4.], [8, 8, 8], -0.5).at(0., y, 0.);
    m.body = b(T32, [32., 16.], [-4., 0., -2.], [8, 12, 4], 0.).at(0., y, 0.);
    m.right_arm = b(T32, [56., 0.], [-1., -2., -1.], [2, 30, 2], 0.).at(-3., 2. + y, 0.);
    m.left_arm = bm(T32, [56., 0.], [-1., -2., -1.], [2, 30, 2], 0.).at(5., 2. + y, 0.);
    m.right_leg = b(T32, [56., 0.], [-1., 0., -1.], [2, 30, 2], 0.).at(-2., 12. + y, 0.);
    m.left_leg = bm(T32, [56., 0.], [-1., 0., -1.], [2, 30, 2], 0.).at(2., 12. + y, 0.);
    m
}

/// `ModelEnderman.setRotationAngles` after the biped's.
fn enderman_angles(m: &mut PlayerModel, carrying: bool, attacking: bool) {
    let y = -14.;
    m.head.show = true;
    m.body.rot.x = 0.;
    m.body.pivot.y = y;
    m.body.pivot.z = -0.;
    for p in [&mut m.right_arm, &mut m.left_arm, &mut m.right_leg, &mut m.left_leg] {
        p.rot.x = (p.rot.x as f64 * 0.5) as f32;
        p.rot.x = p.rot.x.clamp(-0.4, 0.4);
    }
    if carrying {
        m.right_arm.rot.x = -0.5;
        m.left_arm.rot.x = -0.5;
        m.right_arm.rot.z = 0.05;
        m.left_arm.rot.z = -0.05;
    }
    m.right_arm.pivot.z = 0.;
    m.left_arm.pivot.z = 0.;
    m.right_leg.pivot.z = 0.;
    m.left_leg.pivot.z = 0.;
    m.right_leg.pivot.y = 9. + y;
    m.left_leg.pivot.y = 9. + y;
    m.head.pivot.z = -0.;
    m.head.pivot.y = y + 1.;
    m.headwear.copy_angles(&m.head);
    if attacking {
        m.head.pivot.y -= 5.;
    }
}

/// `ModelQuadruped(legHeight, delta)`: head, body, four legs.
fn quadruped(h: i32, d: f32) -> Vec<Part> {
    let hf = h as f32;
    let leg = |x: f32, z: f32| b(T32, [0., 16.], [-2., 0., -2.], [4, h, 4], d).at(x, 24. - hf, z);
    vec![
        b(T32, [0., 0.], [-4., -4., -8.], [8, 8, 8], d).at(0., 18. - hf, -6.),
        b(T32, [28., 8.], [-5., -10., -7.], [10, 16, 8], d).at(0., 17. - hf, 2.),
        leg(-3., 7.),
        leg(3., 7.),
        leg(-3., -5.),
        leg(3., -5.),
    ]
}

fn pig(d: f32) -> Model {
    let mut p = quadruped(6, d);
    p[0] = p[0].clone().add_box(T32, [16., 16.], [-2., 0., -9.], [4, 3, 1], d, false);
    Model::new(p, 1, Some((1., 4., 4.)))
}

fn cow() -> Model {
    let mut p = quadruped(12, 0.);
    p[0] = b(T32, [0., 0.], [-4., -4., -6.], [8, 8, 6], 0.)
        .add_box(T32, [22., 0.], [-5., -5., -4.], [1, 3, 1], 0., false)
        .add_box(T32, [22., 0.], [4., -5., -4.], [1, 3, 1], 0., false)
        .at(0., 4., -8.);
    p[1] = b(T32, [18., 4.], [-6., -10., -7.], [12, 18, 10], 0.).add_box(T32, [52., 0.], [-2., 2., -8.], [4, 6, 1], 0., false).at(0., 5., 2.);
    p[2].pivot.x -= 1.;
    p[3].pivot.x += 1.;
    p[4].pivot.x -= 1.;
    p[5].pivot.x += 1.;
    p[4].pivot.z -= 1.;
    p[5].pivot.z -= 1.;
    Model::new(p, 1, Some((1., 8., 6.)))
}

/// `ModelSheep2` (the body) and `ModelSheep1` (the wool).
fn sheep(wool: bool) -> Model {
    let mut p = quadruped(12, 0.);
    if wool {
        p[0] = b(T32, [0., 0.], [-3., -4., -4.], [6, 6, 6], 0.6).at(0., 6., -8.);
        p[1] = b(T32, [28., 8.], [-4., -10., -7.], [8, 16, 6], 1.75).at(0., 5., 2.);
        for (i, (x, z)) in [(-3., 7.), (3., 7.), (-3., -5.), (3., -5.)].into_iter().enumerate() {
            p[2 + i] = b(T32, [0., 16.], [-2., 0., -2.], [4, 6, 4], 0.5).at(x, 12., z);
        }
    } else {
        p[0] = b(T32, [0., 0.], [-3., -4., -6.], [6, 6, 8], 0.).at(0., 6., -8.);
        p[1] = b(T32, [28., 8.], [-4., -10., -7.], [8, 16, 6], 0.).at(0., 5., 2.);
    }
    Model::new(p, 1, Some((1., 8., 4.)))
}

/// `ModelQuadruped.setRotationAngles`
fn quadruped_angles(p: &mut [Part], limb: f32, amount: f32, yaw: f32, pitch: f32) {
    p[0].rot.x = pitch / (180. / PI);
    p[0].rot.y = yaw / (180. / PI);
    p[1].rot.x = PI / 2.;
    p[2].rot.x = cos(limb * 0.6662) * 1.4 * amount;
    p[3].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
    p[4].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
    p[5].rot.x = cos(limb * 0.6662) * 1.4 * amount;
}

fn creeper() -> Model {
    let leg = |x: f32, z: f32| b(T32, [0., 16.], [-2., 0., -2.], [4, 6, 4], 0.).at(x, 18., z);
    Model::new(
        vec![
            b(T32, [0., 0.], [-4., -8., -4.], [8, 8, 8], 0.).at(0., 6., 0.),
            b(T32, [16., 16.], [-4., 0., -2.], [8, 12, 4], 0.).at(0., 6., 0.),
            leg(-2., 4.),
            leg(2., 4.),
            leg(-2., -4.),
            leg(2., -4.),
        ],
        0,
        None,
    )
}

fn spider() -> Model {
    let mut p = vec![
        b(T32, [32., 4.], [-4., -4., -8.], [8, 8, 8], 0.).at(0., 15., -3.),
        b(T32, [0., 0.], [-3., -3., -3.], [6, 6, 6], 0.).at(0., 15., 0.),
        b(T32, [0., 12.], [-5., -4., -6.], [10, 8, 12], 0.).at(0., 15., 9.),
    ];
    for i in 0..8 {
        let right = i % 2 == 0;
        let x = if right { -15. } else { -1. };
        p.push(b(T32, [18., 0.], [x, -1., -1.], [16, 2, 2], 0.).at(if right { -4. } else { 4. }, 15., 2. - (i / 2) as f32));
    }
    Model::new(p, 0, None)
}

fn chicken() -> Model {
    Model::new(
        vec![
            b(T32, [0., 0.], [-2., -6., -2.], [4, 6, 3], 0.).at(0., 15., -4.),
            b(T32, [14., 0.], [-2., -4., -4.], [4, 2, 2], 0.).at(0., 15., -4.),
            b(T32, [14., 4.], [-1., -2., -3.], [2, 2, 2], 0.).at(0., 15., -4.),
            b(T32, [0., 9.], [-3., -4., -3.], [6, 8, 6], 0.).at(0., 16., 0.),
            b(T32, [26., 0.], [-1., 0., -3.], [3, 5, 3], 0.).at(-2., 19., 1.),
            b(T32, [26., 0.], [-1., 0., -3.], [3, 5, 3], 0.).at(1., 19., 1.),
            b(T32, [24., 13.], [0., 0., -3.], [1, 4, 6], 0.).at(-4., 13., 0.),
            b(T32, [24., 13.], [-1., 0., -3.], [1, 4, 6], 0.).at(4., 13., 0.),
        ],
        3,
        Some((1., 5., 2.)),
    )
}

/// `ModelWolf`: head (renderWithRotation), body, legs, tail (renderWithRotation), mane.
fn wolf() -> Model {
    let leg = |x: f32, z: f32| b(T32, [0., 18.], [-1., 0., -1.], [2, 8, 2], 0.).at(x, 16., z);
    let mut head = b(T32, [0., 0.], [-3., -3., -2.], [6, 6, 4], 0.)
        .add_box(T32, [16., 14.], [-3., -5., 0.], [2, 2, 1], 0., false)
        .add_box(T32, [16., 14.], [1., -5., 0.], [2, 2, 1], 0., false)
        .add_box(T32, [0., 10.], [-1.5, 0., -5.], [3, 3, 4], 0., false)
        .at(-1., 13.5, -7.);
    head.yxz = true;
    let mut tail = b(T32, [9., 18.], [-1., 0., -1.], [2, 8, 2], 0.).at(-1., 12., 8.);
    tail.yxz = true;
    Model::new(
        vec![
            head,
            b(T32, [18., 14.], [-4., -2., -3.], [6, 9, 6], 0.).at(0., 14., 2.),
            leg(-2.5, 7.),
            leg(0.5, 7.),
            leg(-2.5, -4.),
            leg(0.5, -4.),
            tail,
            b(T32, [21., 0.], [-4., -3., -3.], [8, 6, 7], 0.).at(-1., 14., 2.),
        ],
        1,
        Some((1., 5., 2.)),
    )
}

/// `ModelSlime(16)` (body, eyes, mouth) or `ModelSlime(0)` (the outer gel).
fn slime(inner: bool) -> Model {
    let p = if inner {
        vec![
            b(T32, [0., 16.], [-3., 17., -3.], [6, 6, 6], 0.),
            b(T32, [32., 0.], [-3.25, 18., -3.5], [2, 2, 2], 0.),
            b(T32, [32., 4.], [1.25, 18., -3.5], [2, 2, 2], 0.),
            b(T32, [32., 8.], [0., 21., -3.5], [1, 1, 1], 0.),
        ]
    } else {
        vec![b(T32, [0., 0.], [-4., 16., -4.], [8, 8, 8], 0.)]
    };
    Model::new(p, 0, None)
}

/// `ModelMagmaCube`: the core, then eight slices.
fn magma_cube() -> Model {
    let mut p = vec![b(T32, [0., 16.], [-2., 18., -2.], [4, 4, 4], 0.)];
    for i in 0..8 {
        let uv = match i {
            2 => [24., 10.],
            3 => [24., 19.],
            _ => [0., i as f32],
        };
        p.push(b(T32, uv, [-4., (16 + i) as f32, -4.], [8, 1, 8], 0.));
    }
    Model::new(p, 0, None)
}

/// `ModelVillager(0, 0, 64, h)`: head (+ nose), body, legs, arms. `witch` adds the wart and hat.
fn villager(witch: bool) -> Model {
    let t = if witch { [64., 128.] } else { T64 };
    let mut nose = b(t, [24., 0.], [-1., -1., -6.], [2, 4, 2], 0.).at(0., -2., 0.);
    if witch {
        nose = nose.child(b(t, [0., 0.], [0., 3., -6.75], [1, 1, 1], -0.25).at(0., -2., 0.));
    }
    let mut head = b(t, [0., 0.], [-4., -10., -4.], [8, 10, 8], 0.).child(nose);
    if witch {
        let c3 = b(t, [0., 95.], [0., 0., 0.], [1, 2, 1], 0.25).at(1.75, -2., 2.).rot(-0.20943952, 0., 0.10471976);
        let c2 = b(t, [0., 87.], [0., 0., 0.], [4, 4, 4], 0.).at(1.75, -4., 2.).rot(-0.10471976, 0., 0.05235988).child(c3);
        let c1 = b(t, [0., 76.], [0., 0., 0.], [7, 4, 7], 0.).at(1.75, -4., 2.).rot(-0.05235988, 0., 0.02617994).child(c2);
        head = head.child(b(t, [0., 64.], [0., 0., 0.], [10, 2, 10], 0.).at(-5., -10.03125, -5.).child(c1));
    }
    Model::new(
        vec![
            head,
            b(t, [16., 20.], [-4., 0., -3.], [8, 12, 6], 0.).add_box(t, [0., 38.], [-4., 0., -3.], [8, 18, 6], 0.5, false),
            b(t, [0., 22.], [-2., 0., -2.], [4, 12, 4], 0.).at(-2., 12., 0.),
            bm(t, [0., 22.], [-2., 0., -2.], [4, 12, 4], 0.).at(2., 12., 0.),
            b(t, [44., 22.], [-8., -2., -2.], [4, 8, 4], 0.)
                .add_box(t, [44., 22.], [4., -2., -2.], [4, 8, 4], 0., false)
                .add_box(t, [40., 38.], [-4., 2., -2.], [8, 4, 4], 0., false)
                .at(0., 2., 0.),
        ],
        0,
        None,
    )
}

fn squid() -> Model {
    let mut p = vec![b(T32, [0., 0.], [-6., -8., -6.], [12, 16, 12], 0.).at(0., 8., 0.)];
    for j in 0..8 {
        let d0 = j as f64 * std::f64::consts::PI * 2. / 8.;
        let mut t = b(T32, [48., 0.], [-1., 0., -1.], [2, 18, 2], 0.).at(d0.cos() as f32 * 5., 15., d0.sin() as f32 * 5.);
        t.rot.y = (j as f64 * std::f64::consts::PI * -2. / 8. + std::f64::consts::PI / 2.) as f32;
        p.push(t);
    }
    Model::new(p, 0, None)
}

/// `ModelBat`: head (+ ears), body (+ wings, + outer wings).
fn bat() -> Model {
    let head = b(T64, [0., 0.], [-3., -3., -3.], [6, 6, 6], 0.)
        .child(b(T64, [24., 0.], [-4., -6., -2.], [3, 4, 1], 0.))
        .child(bm(T64, [24., 0.], [1., -6., -2.], [3, 4, 1], 0.));
    let right = b(T64, [42., 0.], [-12., 1., 1.5], [10, 16, 1], 0.).child(b(T64, [24., 16.], [-8., 1., 0.], [8, 12, 1], 0.).at(-12., 1., 1.5));
    let left = bm(T64, [42., 0.], [2., 1., 1.5], [10, 16, 1], 0.).child(bm(T64, [24., 16.], [0., 1., 0.], [8, 12, 1], 0.).at(12., 1., 1.5));
    let body = b(T64, [0., 16.], [-3., 4., -3.], [6, 12, 6], 0.).add_box(T64, [0., 34.], [-5., 16., 0.], [10, 6, 1], 0., false).child(right).child(left);
    Model::new(vec![head, body], 0, None)
}

const SILVERFISH_BOX: [[i32; 3]; 7] = [[3, 2, 2], [4, 3, 2], [6, 4, 3], [3, 3, 3], [2, 2, 3], [2, 1, 2], [1, 1, 2]];
const SILVERFISH_UV: [[f32; 2]; 7] = [[0., 0.], [0., 4.], [0., 9.], [0., 16.], [0., 22.], [11., 0.], [13., 4.]];
const ENDERMITE_BOX: [[i32; 3]; 4] = [[4, 3, 2], [6, 4, 5], [3, 3, 1], [1, 2, 1]];
const ENDERMITE_UV: [[f32; 2]; 4] = [[0., 0.], [0., 5.], [0., 14.], [0., 18.]];

/// The segment chain of `ModelSilverfish` / `ModelEnderMite`; returns the segments' z.
fn segments(boxes: &[[i32; 3]], uvs: &[[f32; 2]], p: &mut Vec<Part>) -> Vec<f32> {
    let mut f = -3.5;
    let mut zs = Vec::new();
    for (i, (s, uv)) in boxes.iter().zip(uvs).enumerate() {
        p.push(b(T32, *uv, [s[0] as f32 * -0.5, 0., s[2] as f32 * -0.5], *s, 0.).at(0., (24 - s[1]) as f32, f));
        zs.push(f);
        if i + 1 < boxes.len() {
            f += (s[2] + boxes[i + 1][2]) as f32 * 0.5;
        }
    }
    zs
}

fn silverfish() -> Model {
    let mut p = Vec::new();
    let z = segments(&SILVERFISH_BOX, &SILVERFISH_UV, &mut p);
    let s = SILVERFISH_BOX;
    p.push(b(T32, [20., 0.], [-5., 0., s[2][2] as f32 * -0.5], [10, 8, s[2][2]], 0.).at(0., 16., z[2]));
    p.push(b(T32, [20., 11.], [-3., 0., s[4][2] as f32 * -0.5], [6, 4, s[4][2]], 0.).at(0., 20., z[4]));
    p.push(b(T32, [20., 18.], [-3., 0., s[4][2] as f32 * -0.5], [6, 5, s[1][2]], 0.).at(0., 19., z[1]));
    Model::new(p, 0, None)
}

fn endermite() -> Model {
    let mut p = Vec::new();
    segments(&ENDERMITE_BOX, &ENDERMITE_UV, &mut p);
    Model::new(p, 0, None)
}

/// `ModelBlaze`: head, then twelve rods.
fn blaze() -> Model {
    let mut p = vec![b(T32, [0., 0.], [-4., -4., -4.], [8, 8, 8], 0.)];
    for _ in 0..12 {
        p.push(b(T32, [0., 16.], [0., 0., 0.], [2, 8, 2], 0.));
    }
    Model::new(p, 0, None)
}

/// `ModelGhast`: tentacle lengths come from `new Random(1660L)`.
fn ghast() -> Model {
    let mut p = vec![b(T32, [0., 0.], [-8., -8., -8.], [16, 16, 16], 0.).at(0., 8., 0.)];
    let mut r = mc_core::random::JavaRandom::new(1660);
    for j in 0..9 {
        let f = (((j % 3) as f32 - (j / 3 % 2) as f32 * 0.5 + 0.25) / 2. * 2. - 1.) * 5.;
        let f1 = ((j / 3) as f32 / 2. * 2. - 1.) * 5.;
        let k = r.next_int_bounded(7) + 8;
        p.push(b(T32, [0., 0.], [-1., 0., -1.], [2, k, 2], 0.).at(f, 15., f1));
    }
    Model::new(p, 0, None)
}

/// `ModelSnowMan`: body, bottom, head, hands.
fn snowman() -> Model {
    let hand = || b(T64, [32., 0.], [-1., 0., -1.], [12, 2, 2], -0.5).at(0., 6., 0.);
    Model::new(
        vec![
            b(T64, [0., 16.], [-5., -10., -5.], [10, 10, 10], -0.5).at(0., 13., 0.),
            b(T64, [0., 36.], [-6., -12., -6.], [12, 12, 12], -0.5).at(0., 24., 0.),
            b(T64, [0., 0.], [-4., -8., -4.], [8, 8, 8], -0.5).at(0., 4., 0.),
            hand(),
            hand(),
        ],
        0,
        None,
    )
}

/// `ModelIronGolem(0, -7)`: head, body, left leg, right leg, right arm, left arm.
fn iron_golem() -> Model {
    let t = [128., 128.];
    let y = -7.;
    Model::new(
        vec![
            b(t, [0., 0.], [-4., -12., -5.5], [8, 10, 8], 0.).add_box(t, [24., 0.], [-1., -5., -7.5], [2, 4, 2], 0., false).at(0., y, -2.),
            b(t, [0., 40.], [-9., -2., -6.], [18, 12, 11], 0.).add_box(t, [0., 70.], [-4.5, 10., -3.], [9, 5, 6], 0.5, false).at(0., y, 0.),
            b(t, [37., 0.], [-3.5, -3., -3.], [6, 16, 5], 0.).at(-4., 18. + y, 0.),
            bm(t, [60., 0.], [-3.5, -3., -3.], [6, 16, 5], 0.).at(5., 18. + y, 0.),
            b(t, [60., 21.], [-13., -2.5, -3.], [4, 30, 6], 0.).at(0., -7., 0.),
            b(t, [60., 58.], [9., -2.5, -3.], [4, 30, 6], 0.).at(0., -7., 0.),
        ],
        0,
        None,
    )
}

/// `ModelOcelot`: head, body, tails, back legs, front legs.
fn ocelot() -> Model {
    let tail = |uv: [f32; 2]| b(T32, uv, [-0.5, 0., 0.], [1, 8, 1], 0.);
    Model::new(
        vec![
            b(T32, [0., 0.], [-2.5, -2., -3.], [5, 4, 5], 0.)
                .add_box(T32, [0., 24.], [-1.5, 0., -4.], [3, 2, 2], 0., false)
                .add_box(T32, [0., 10.], [-2., -3., 0.], [1, 1, 2], 0., false)
                .add_box(T32, [6., 10.], [1., -3., 0.], [1, 1, 2], 0., false)
                .at(0., 15., -9.),
            b(T32, [20., 0.], [-2., 3., -8.], [4, 16, 6], 0.).at(0., 12., -10.),
            tail([0., 15.]).at(0., 15., 8.).rot(0.9, 0., 0.),
            tail([4., 15.]).at(0., 20., 14.),
            b(T32, [8., 13.], [-1., 0., 1.], [2, 6, 2], 0.).at(1.1, 18., 5.),
            b(T32, [8., 13.], [-1., 0., 1.], [2, 6, 2], 0.).at(-1.1, 18., 5.),
            b(T32, [40., 0.], [-1., 0., 0.], [2, 10, 2], 0.).at(1.2, 13.8, -5.),
            b(T32, [40., 0.], [-1., 0., 0.], [2, 10, 2], 0.).at(-1.2, 13.8, -5.),
        ],
        1,
        Some((0.75, 10., 4.)),
    )
}

/// `ModelRabbit`, head parts first: head, right ear, left ear, nose, then feet, thighs, body,
/// arms and tail. (`mirror` is set after each `addBox`, so nothing is mirrored.)
fn rabbit() -> Model {
    Model::new(
        vec![
            b(T32, [32., 0.], [-2.5, -4., -5.], [5, 4, 5], 0.).at(0., 16., -1.),
            b(T32, [52., 0.], [-2.5, -9., -1.], [2, 5, 1], 0.).at(0., 16., -1.).rot(0., -0.2617994, 0.),
            b(T32, [58., 0.], [0.5, -9., -1.], [2, 5, 1], 0.).at(0., 16., -1.).rot(0., 0.2617994, 0.),
            b(T32, [32., 9.], [-0.5, -2.5, -5.5], [1, 1, 1], 0.).at(0., 16., -1.),
            b(T32, [26., 24.], [-1., 5.5, -3.7], [2, 1, 7], 0.).at(3., 17.5, 3.7),
            b(T32, [8., 24.], [-1., 5.5, -3.7], [2, 1, 7], 0.).at(-3., 17.5, 3.7),
            b(T32, [30., 15.], [-1., 0., 0.], [2, 4, 5], 0.).at(3., 17.5, 3.7).rot(-0.34906584, 0., 0.),
            b(T32, [16., 15.], [-1., 0., 0.], [2, 4, 5], 0.).at(-3., 17.5, 3.7).rot(-0.34906584, 0., 0.),
            b(T32, [0., 0.], [-3., -2., -10.], [6, 5, 10], 0.).at(0., 19., 8.).rot(-0.34906584, 0., 0.),
            b(T32, [8., 15.], [-1., 0., -1.], [2, 7, 2], 0.).at(3., 17., -1.).rot(-0.17453292, 0., 0.),
            b(T32, [0., 15.], [-1., 0., -1.], [2, 7, 2], 0.).at(-3., 17., -1.).rot(-0.17453292, 0., 0.),
            b(T32, [52., 6.], [-1.5, -1.5, 0.], [3, 3, 2], 0.).at(0., 20., 7.).rot(-0.3490659, 0., 0.),
        ],
        4,
        Some((1., 5., 2.)),
    )
}

/// The models, built once and re-posed for each mob.
pub struct MobModels {
    zombie: PlayerModel,
    zombie_villager: PlayerModel,
    skeleton: PlayerModel,
    enderman: PlayerModel,
    /// `ModelZombie(1.0 / 0.5, true)`: armor on zombies, pigmen and skeletons.
    armor: [PlayerModel; 2],
    /// `ModelZombieVillager(1.0 / 0.5, 0, true)`
    villager_armor: [PlayerModel; 2],
    creeper: Model,
    spider: Model,
    pig: Model,
    saddle: Model,
    cow: Model,
    sheep: Model,
    wool: Model,
    chicken: Model,
    wolf: Model,
    slime: Model,
    slime_gel: Model,
    magma_cube: Model,
    villager: Model,
    witch: Model,
    squid: Model,
    bat: Model,
    silverfish: Model,
    endermite: Model,
    blaze: Model,
    ghast: Model,
    snowman: Model,
    iron_golem: Model,
    ocelot: Model,
    rabbit: Model,
}

impl Default for MobModels {
    fn default() -> Self {
        Self::new()
    }
}

impl MobModels {
    pub fn new() -> Self {
        Self {
            zombie: PlayerModel::biped_with(0., 0., T64),
            zombie_villager: zombie_villager(0., false),
            skeleton: skeleton(),
            enderman: enderman(),
            armor: [PlayerModel::biped(1.), PlayerModel::biped(0.5)],
            villager_armor: [zombie_villager(1., true), zombie_villager(0.5, true)],
            creeper: creeper(),
            spider: spider(),
            pig: pig(0.),
            saddle: pig(0.5),
            cow: cow(),
            sheep: sheep(false),
            wool: sheep(true),
            chicken: chicken(),
            wolf: wolf(),
            slime: slime(true),
            slime_gel: slime(false),
            magma_cube: magma_cube(),
            villager: villager(false),
            witch: villager(true),
            squid: squid(),
            bat: bat(),
            silverfish: silverfish(),
            endermite: endermite(),
            blaze: blaze(),
            ghast: ghast(),
            snowman: snowman(),
            iron_golem: iron_golem(),
            ocelot: ocelot(),
            rabbit: rabbit(),
        }
    }

    /// `RendererLivingEntity.doRender` from `renderLivingAt` on: `base` places the entity's feet.
    /// Emits every quad with the layer it belongs to; returns the held item's transform
    /// (`LayerHeldItem`, before the item's own third-person transform) for bipeds.
    pub fn render(&mut self, f: &MobFrame, base: Mat4, out: &mut dyn FnMut(&Layer, Mat4, &ModelQuad)) -> Option<Mat4> {
        let mob = f.mob;
        let child = f.child;
        // handleRotationFloat
        let age = match mob {
            Mob::Chicken { wing } => wing,
            Mob::Squid { tentacle, .. } => tentacle,
            Mob::Wolf { tail, .. } => tail,
            _ => f.age,
        };
        let mut limb = f.limb_swing;
        if child {
            limb *= 3.;
        }
        let amount = f.limb_amount.min(1.);
        let net_yaw = f.head_yaw - f.body_yaw;
        let pitch = f.pitch;

        // rotateCorpse
        let mut m = base;
        match mob {
            Mob::Squid { pitch, yaw, .. } => {
                m = m
                    * Mat4::from_translation(Vec3::new(0., 0.5, 0.))
                    * Mat4::from_rotation_y((180. - f.body_yaw).to_radians())
                    * Mat4::from_rotation_x(pitch.to_radians())
                    * Mat4::from_rotation_y(yaw.to_radians())
                    * Mat4::from_translation(Vec3::new(0., -1.2, 0.));
            }
            _ => {
                if let Mob::Bat { hanging } = mob {
                    let y = if hanging { -0.1 } else { cos(f.age * 0.3) * 0.1 };
                    m *= Mat4::from_translation(Vec3::new(0., y, 0.));
                }
                m *= Mat4::from_rotation_y((180. - f.body_yaw).to_radians());
                if f.death > 0. {
                    let d = sqrt_float((f.death - 1.) / 20. * 1.6).min(1.);
                    let max = match mob {
                        Mob::Spider { .. } | Mob::Silverfish | Mob::Endermite => 180.,
                        _ => 90.,
                    };
                    m *= Mat4::from_rotation_z((d * max).to_radians());
                }
                if let Mob::IronGolem { .. } = mob {
                    if f.limb_amount >= 0.01 {
                        let l = f.limb_swing + 6.;
                        let w = ((l % 13.) - 6.5).abs() - 3.25;
                        m *= Mat4::from_rotation_z((6.5 * w / 3.25).to_radians());
                    }
                }
            }
        }
        m *= Mat4::from_scale(Vec3::new(-1., -1., 1.));
        // preRenderCallback
        let scale = |s: Vec3| Mat4::from_scale(s);
        match mob {
            Mob::Giant => m *= scale(Vec3::splat(6.)),
            Mob::Skeleton { wither: true } => m *= scale(Vec3::splat(1.2)),
            Mob::Spider { cave: true } => m *= scale(Vec3::splat(0.7)),
            Mob::Bat { .. } => m *= scale(Vec3::splat(0.35)),
            Mob::Villager { .. } => m *= scale(Vec3::splat(if child { (0.9375f64 * 0.5) as f32 } else { 0.9375 })),
            Mob::Witch { .. } => m *= scale(Vec3::splat(0.9375)),
            Mob::Ghast { .. } => m *= scale(Vec3::splat(4.5)),
            Mob::Ocelot { tamed: true, .. } => m *= scale(Vec3::splat(0.8)),
            Mob::Slime { size, squish } | Mob::MagmaCube { size, squish } => {
                let f1 = squish / (size * 0.5 + 1.);
                let f2 = 1. / (f1 + 1.);
                m *= scale(Vec3::new(f2 * size, 1. / f2 * size, f2 * size));
            }
            Mob::Creeper { flash } => {
                let f1 = 1. + sin(flash * 100.) * flash * 0.01;
                let mut g = flash.clamp(0., 1.);
                g *= g;
                g *= g;
                let f2 = (1. + g * 0.4) * f1;
                let f3 = (1. + g * 0.1) / f1;
                m *= scale(Vec3::new(f2, f3, f2));
            }
            _ => {}
        }
        m *= Mat4::from_translation(Vec3::new(0., -1.5078125, 0.));

        // setBrightness: hurt red wins over a color multiplier (the creeper's white flash).
        let overlay = if f.hurt || f.death > 0. {
            Some([1., 0., 0., 0.3])
        } else if let Mob::Creeper { flash } = mob {
            ((flash * 10.) as i32 % 2 != 0).then(|| {
                let i = ((flash * 0.2 * 255.) as i32).clamp(0, 255);
                [1., 1., 1., 1. - i as f32 / 255.]
            })
        } else {
            None
        };
        let main = |path: &'static str| Layer { overlay, ..Layer::tex(path) };

        match mob {
            Mob::Zombie { .. } | Mob::PigZombie | Mob::Giant | Mob::Skeleton { .. } => {
                let villager = matches!(mob, Mob::Zombie { villager: true });
                let (model, tex) = match mob {
                    Mob::Zombie { villager: true } => (&mut self.zombie_villager, "zombie/zombie_villager"),
                    Mob::PigZombie => (&mut self.zombie, "zombie_pigman"),
                    Mob::Skeleton { wither } => (&mut self.skeleton, if wither { "skeleton/wither_skeleton" } else { "skeleton/skeleton" }),
                    _ => (&mut self.zombie, "zombie/zombie"),
                };
                pose_biped(model, f, limb, amount);
                zombie_arms(model, f.swing, f.age);
                render_biped(model, m, child, &main(tex), out);
                let arm = model.right_arm.post_render();
                // LayerBipedArmor / LayerVillagerArmor
                let models = if villager { &mut self.villager_armor } else { &mut self.armor };
                for slot in [4, 3, 2, 1] {
                    let Some(a) = f.armor[slot - 1] else { continue };
                    let am = &mut models[(slot == 2) as usize];
                    pose_biped(am, f, limb, amount);
                    zombie_arms(am, f.swing, f.age);
                    am.show_armor_slot(slot);
                    let layer = (slot == 2) as u32;
                    let armor = |cell: u32, tint: [f32; 3]| Layer { tex: Tex::Armor(cell), tint, overlay: None, additive: false, normalize: false, glint: false };
                    let last = if a.material == 0 {
                        let c = a.color;
                        let tint = [((c >> 16) & 255) as f32 / 255., ((c >> 8) & 255) as f32 / 255., (c & 255) as f32 / 255.];
                        render_biped(am, m, child, &armor(layer, tint), out);
                        armor(10 + layer, [1.; 3])
                    } else {
                        armor(a.material as u32 * 2 + layer, [1.; 3])
                    };
                    render_biped(am, m, child, &last, out);
                    if a.glint {
                        render_biped(am, m, child, &Layer { glint: true, ..last }, out);
                    }
                }
                if f.held.is_none() {
                    return None;
                }
                let mut hm = m;
                if child {
                    hm = hm * Mat4::from_translation(Vec3::new(0., 0.625, 0.)) * Mat4::from_rotation_x(20f32.to_radians()) * Mat4::from_scale(Vec3::splat(0.5));
                }
                return Some(hm * arm * Mat4::from_translation(Vec3::new(-0.0625, 0.4375, 0.0625)));
            }
            Mob::Enderman { carrying, screaming } => {
                let e = &mut self.enderman;
                pose_biped(e, f, limb, amount);
                enderman_angles(e, carrying, screaming);
                render_biped(e, m, child, &main("enderman/enderman"), out);
                render_biped(e, m, child, &Layer { additive: true, ..Layer::tex("enderman/enderman_eyes") }, out);
            }
            Mob::Creeper { .. } => {
                let p = &mut self.creeper.parts;
                p[0].rot.y = net_yaw / (180. / PI);
                p[0].rot.x = pitch / (180. / PI);
                p[2].rot.x = cos(limb * 0.6662) * 1.4 * amount;
                p[3].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
                p[4].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
                p[5].rot.x = cos(limb * 0.6662) * 1.4 * amount;
                self.creeper.render(m, child, &main("creeper/creeper"), out);
            }
            Mob::Spider { cave } => {
                spider_angles(&mut self.spider.parts, limb, amount, net_yaw, pitch);
                self.spider.render(m, child, &main(if cave { "spider/cave_spider" } else { "spider/spider" }), out);
                self.spider.render(m, child, &Layer { additive: true, ..Layer::tex("spider_eyes") }, out);
            }
            Mob::Pig { saddle } => {
                quadruped_angles(&mut self.pig.parts, limb, amount, net_yaw, pitch);
                self.pig.render(m, child, &main("pig/pig"), out);
                if saddle {
                    quadruped_angles(&mut self.saddle.parts, limb, amount, net_yaw, pitch);
                    self.saddle.render(m, child, &Layer::tex("pig/pig_saddle"), out);
                }
            }
            Mob::Cow { mooshroom } => {
                quadruped_angles(&mut self.cow.parts, limb, amount, net_yaw, pitch);
                self.cow.render(m, child, &main(if mooshroom { "cow/mooshroom" } else { "cow/cow" }), out);
            }
            Mob::Sheep { color, sheared } => {
                quadruped_angles(&mut self.sheep.parts, limb, amount, net_yaw, pitch);
                self.sheep.render(m, child, &main("sheep/sheep"), out);
                if !sheared {
                    // LayerSheepWool combines textures, so it takes the hurt tint too.
                    quadruped_angles(&mut self.wool.parts, limb, amount, net_yaw, pitch);
                    self.wool.render(m, child, &Layer { tint: DYE_RGB[color as usize & 15], ..main("sheep/sheep_fur") }, out);
                }
            }
            Mob::Chicken { .. } => {
                let p = &mut self.chicken.parts;
                for i in 0..3 {
                    p[i].rot.x = pitch / (180. / PI);
                    p[i].rot.y = net_yaw / (180. / PI);
                }
                p[3].rot.x = PI / 2.;
                p[4].rot.x = cos(limb * 0.6662) * 1.4 * amount;
                p[5].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
                p[6].rot.z = age;
                p[7].rot.z = -age;
                self.chicken.render(m, child, &main("chicken"), out);
            }
            Mob::Wolf { sitting, angry, tamed, collar, .. } => {
                wolf_angles(&mut self.wolf.parts, limb, amount, age, net_yaw, pitch, sitting, angry);
                let tex = if tamed {
                    "wolf/wolf_tame"
                } else if angry {
                    "wolf/wolf_angry"
                } else {
                    "wolf/wolf"
                };
                self.wolf.render(m, child, &main(tex), out);
                if tamed {
                    self.wolf.render(m, child, &Layer { tint: DYE_RGB[collar as usize & 15], ..main("wolf/wolf_collar") }, out);
                }
            }
            Mob::Slime { .. } => {
                self.slime.render(m, child, &main("slime/slime"), out);
                // LayerSlimeGel: the translucent outer cube, blended (SRC_ALPHA, 1 - SRC_ALPHA).
                // The layer enables `GL_NORMALIZE`, which the main pass does not.
                self.slime_gel.render(m, child, &Layer { normalize: true, ..main("slime/slime") }, out);
            }
            Mob::MagmaCube { squish, .. } => {
                let s = squish.max(0.);
                for i in 0..8 {
                    self.magma_cube.parts[1 + i].pivot.y = -((4 - i as i32) as f32) * s * 1.7;
                }
                self.magma_cube.render(m, child, &main("slime/magmacube"), out);
            }
            Mob::Villager { profession } => {
                villager_angles(&mut self.villager.parts, limb, amount, net_yaw, pitch);
                let tex = match profession {
                    0 => "villager/farmer",
                    1 => "villager/librarian",
                    2 => "villager/priest",
                    3 => "villager/smith",
                    4 => "villager/butcher",
                    _ => "villager/villager",
                };
                // ModelVillager has no baby layout; RenderVillager just scales it down.
                self.villager.render(m, false, &main(tex), out);
            }
            Mob::Witch { holding } => {
                let p = &mut self.witch.parts;
                villager_angles(p, limb, amount, net_yaw, pitch);
                let nose = &mut p[0].children[0];
                nose.offset = Vec3::ZERO;
                let k = 0.01 * (f.id % 10) as f32;
                nose.rot.x = sin(f.ticks as f32 * k) * 4.5 * PI / 180.;
                nose.rot.y = 0.;
                nose.rot.z = cos(f.ticks as f32 * k) * 2.5 * PI / 180.;
                if holding {
                    nose.rot.x = -0.9;
                    nose.offset.z = -0.09375;
                    nose.offset.y = 0.1875;
                }
                self.witch.render(m, false, &main("witch"), out);
            }
            Mob::Squid { .. } => {
                for t in &mut self.squid.parts[1..] {
                    t.rot.x = age;
                }
                self.squid.render(m, child, &main("squid"), out);
            }
            Mob::Bat { hanging } => {
                bat_angles(&mut self.bat.parts, age, net_yaw, pitch, hanging);
                self.bat.render(m, child, &main("bat"), out);
            }
            Mob::Silverfish => {
                let p = &mut self.silverfish.parts;
                for i in 0..7 {
                    let a = age * 0.9 + i as f32 * 0.15 * PI;
                    p[i].rot.y = cos(a) * PI * 0.05 * (1 + (i as i32 - 2).abs()) as f32;
                    p[i].pivot.x = sin(a) * PI * 0.2 * (i as i32 - 2).abs() as f32;
                }
                p[7].rot.y = p[2].rot.y;
                p[8].rot.y = p[4].rot.y;
                p[8].pivot.x = p[4].pivot.x;
                p[9].rot.y = p[1].rot.y;
                p[9].pivot.x = p[1].pivot.x;
                self.silverfish.render(m, child, &main("silverfish"), out);
            }
            Mob::Endermite => {
                for (i, s) in self.endermite.parts.iter_mut().enumerate() {
                    let a = age * 0.9 + i as f32 * 0.15 * PI;
                    s.rot.y = cos(a) * PI * 0.01 * (1 + (i as i32 - 2).abs()) as f32;
                    s.pivot.x = sin(a) * PI * 0.1 * (i as i32 - 2).abs() as f32;
                }
                self.endermite.render(m, child, &main("endermite"), out);
            }
            Mob::Blaze => {
                let p = &mut self.blaze.parts;
                let mut a = age * PI * -0.1;
                for i in 0..4 {
                    let s = &mut p[1 + i];
                    s.pivot = Vec3::new(cos(a) * 9., -2. + cos(((i * 2) as f32 + age) * 0.25), sin(a) * 9.);
                    a += 1.;
                }
                a = PI / 4. + age * PI * 0.03;
                for j in 4..8 {
                    let s = &mut p[1 + j];
                    s.pivot = Vec3::new(cos(a) * 7., 2. + cos(((j * 2) as f32 + age) * 0.25), sin(a) * 7.);
                    a += 1.;
                }
                a = 0.47123894 + age * PI * -0.05;
                for k in 8..12 {
                    let s = &mut p[1 + k];
                    s.pivot = Vec3::new(cos(a) * 5., 11. + cos((k as f32 * 1.5 + age) * 0.5), sin(a) * 5.);
                    a += 1.;
                }
                p[0].rot.y = net_yaw / (180. / PI);
                p[0].rot.x = pitch / (180. / PI);
                self.blaze.render(m, child, &main("blaze"), out);
            }
            Mob::Ghast { shooting } => {
                for (i, t) in self.ghast.parts[1..].iter_mut().enumerate() {
                    t.rot.x = 0.2 * sin(age * 0.3 + i as f32) + 0.4;
                }
                let gm = m * Mat4::from_translation(Vec3::new(0., 0.6, 0.));
                self.ghast.render(gm, child, &main(if shooting { "ghast/ghast_shooting" } else { "ghast/ghast" }), out);
            }
            Mob::SnowMan => {
                let p = &mut self.snowman.parts;
                p[2].rot.y = net_yaw / (180. / PI);
                p[2].rot.x = pitch / (180. / PI);
                p[0].rot.y = net_yaw / (180. / PI) * 0.25;
                let (s, c) = (sin(p[0].rot.y), cos(p[0].rot.y));
                p[3].rot.z = 1.;
                p[4].rot.z = -1.;
                p[3].rot.y = p[0].rot.y;
                p[4].rot.y = PI + p[0].rot.y;
                p[3].pivot.x = c * 5.;
                p[3].pivot.z = -s * 5.;
                p[4].pivot.x = -c * 5.;
                p[4].pivot.z = s * 5.;
                self.snowman.render(m, child, &main("snowman"), out);
            }
            Mob::IronGolem { attack, rose } => {
                let p = &mut self.iron_golem.parts;
                let tri = |x: f32, y: f32| ((x % y - y * 0.5).abs() - y * 0.25) / (y * 0.25);
                if attack > 0. {
                    p[4].rot.x = -2. + 1.5 * tri(attack, 10.);
                    p[5].rot.x = -2. + 1.5 * tri(attack, 10.);
                } else if rose > 0 {
                    p[4].rot.x = -0.8 + 0.025 * tri(rose as f32, 70.);
                    p[5].rot.x = 0.;
                } else {
                    p[4].rot.x = (-0.2 + 1.5 * tri(limb, 13.)) * amount;
                    p[5].rot.x = (-0.2 - 1.5 * tri(limb, 13.)) * amount;
                }
                p[0].rot.y = net_yaw / (180. / PI);
                p[0].rot.x = pitch / (180. / PI);
                p[2].rot.x = -1.5 * tri(limb, 13.) * amount;
                p[3].rot.x = 1.5 * tri(limb, 13.) * amount;
                p[2].rot.y = 0.;
                p[3].rot.y = 0.;
                self.iron_golem.render(m, child, &main("iron_golem"), out);
            }
            Mob::Ocelot { skin, state, .. } => {
                ocelot_angles(&mut self.ocelot.parts, limb, amount, net_yaw, pitch, state);
                let tex = match skin {
                    1 => "cat/black",
                    2 => "cat/red",
                    3 => "cat/siamese",
                    _ => "cat/ocelot",
                };
                self.ocelot.render(m, child, &main(tex), out);
            }
            Mob::Rabbit { kind } => {
                let p = &mut self.rabbit.parts;
                let (px, py) = (pitch * 0.017453292, net_yaw * 0.017453292);
                for i in 0..4 {
                    p[i].rot.x = px;
                }
                p[0].rot.y = py;
                p[3].rot.y = py;
                p[1].rot.y = py - 0.2617994;
                p[2].rot.y = py + 0.2617994;
                // The hop (`func_175521_o`) rests at 0 between jumps.
                let hop = 0.;
                p[6].rot.x = (hop * 50. - 21.) * 0.017453292;
                p[7].rot.x = p[6].rot.x;
                p[4].rot.x = hop * 50. * 0.017453292;
                p[5].rot.x = p[4].rot.x;
                p[9].rot.x = (hop * -40. - 11.) * 0.017453292;
                p[10].rot.x = p[9].rot.x;
                let tex = match kind {
                    1 => "rabbit/white",
                    2 => "rabbit/black",
                    3 => "rabbit/white_splotched",
                    4 => "rabbit/gold",
                    5 => "rabbit/salt",
                    99 => "rabbit/caerbannog",
                    _ => "rabbit/brown",
                };
                self.rabbit.render(m, child, &main(tex), out);
            }
        }
        None
    }
}

fn spider_angles(p: &mut [Part], limb: f32, amount: f32, yaw: f32, pitch: f32) {
    p[0].rot.y = yaw / (180. / PI);
    p[0].rot.x = pitch / (180. / PI);
    let f = PI / 4.;
    let z = [-f, f, -f * 0.74, f * 0.74, -f * 0.74, f * 0.74, -f, f];
    let f2 = 0.3926991;
    let y = [f2 * 2., -f2 * 2., f2, -f2, -f2, f2, -f2 * 2., f2 * 2.];
    let w = limb * 0.6662;
    let swing = |o: f32| -(cos(w * 2. + o) * 0.4) * amount;
    let lift = |o: f32| (sin(w + o) * 0.4).abs() * amount;
    let ys = [swing(0.), swing(PI), swing(PI / 2.), swing(PI * 3. / 2.)];
    let zs = [lift(0.), lift(PI), lift(PI / 2.), lift(PI * 3. / 2.)];
    for i in 0..8 {
        let sign = if i % 2 == 0 { 1. } else { -1. };
        let leg = &mut p[3 + i];
        leg.rot.z = z[i] + sign * zs[i / 2];
        leg.rot.y = y[i] + sign * ys[i / 2];
    }
}

#[allow(clippy::too_many_arguments)]
fn wolf_angles(p: &mut [Part], limb: f32, amount: f32, tail: f32, yaw: f32, pitch: f32, sitting: bool, angry: bool) {
    // setLivingAnimations
    p[6].rot.y = if angry { 0. } else { cos(limb * 0.6662) * 1.4 * amount };
    if sitting {
        p[7].pivot = Vec3::new(-1., 16., -3.);
        p[7].rot.x = PI * 2. / 5.;
        p[7].rot.y = 0.;
        p[1].pivot = Vec3::new(0., 18., 0.);
        p[1].rot.x = PI / 4.;
        p[6].pivot = Vec3::new(-1., 21., 6.);
        p[2].pivot = Vec3::new(-2.5, 22., 2.);
        p[2].rot.x = PI * 3. / 2.;
        p[3].pivot = Vec3::new(0.5, 22., 2.);
        p[3].rot.x = PI * 3. / 2.;
        p[4].rot.x = 5.811947;
        p[4].pivot = Vec3::new(-2.49, 17., -4.);
        p[5].rot.x = 5.811947;
        p[5].pivot = Vec3::new(0.51, 17., -4.);
    } else {
        p[1].pivot = Vec3::new(0., 14., 2.);
        p[1].rot.x = PI / 2.;
        p[7].pivot = Vec3::new(-1., 14., -3.);
        p[7].rot.x = p[1].rot.x;
        p[6].pivot = Vec3::new(-1., 12., 8.);
        p[2].pivot = Vec3::new(-2.5, 16., 7.);
        p[3].pivot = Vec3::new(0.5, 16., 7.);
        p[4].pivot = Vec3::new(-2.5, 16., -4.);
        p[5].pivot = Vec3::new(0.5, 16., -4.);
        p[2].rot.x = cos(limb * 0.6662) * 1.4 * amount;
        p[3].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
        p[4].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount;
        p[5].rot.x = cos(limb * 0.6662) * 1.4 * amount;
    }
    // Interest tilt and shaking stay at rest.
    p[0].rot.z = 0.;
    p[7].rot.z = 0.;
    p[1].rot.z = 0.;
    p[6].rot.z = 0.;
    // setRotationAngles
    p[0].rot.x = pitch / (180. / PI);
    p[0].rot.y = yaw / (180. / PI);
    p[6].rot.x = tail;
}

fn villager_angles(p: &mut [Part], limb: f32, amount: f32, yaw: f32, pitch: f32) {
    p[0].rot.y = yaw / (180. / PI);
    p[0].rot.x = pitch / (180. / PI);
    p[4].pivot.y = 3.;
    p[4].pivot.z = -1.;
    p[4].rot.x = -0.75;
    p[2].rot.x = cos(limb * 0.6662) * 1.4 * amount * 0.5;
    p[3].rot.x = cos(limb * 0.6662 + PI) * 1.4 * amount * 0.5;
    p[2].rot.y = 0.;
    p[3].rot.y = 0.;
}

fn bat_angles(p: &mut [Part], age: f32, yaw: f32, pitch: f32, hanging: bool) {
    let (head, body) = p.split_at_mut(1);
    let (head, body) = (&mut head[0], &mut body[0]);
    if hanging {
        head.rot = Vec3::new(pitch / (180. / PI), PI - yaw / (180. / PI), PI);
        head.pivot = Vec3::new(0., -2., 0.);
        body.rot.x = PI;
        let (rw, lw) = body.children.split_at_mut(1);
        let (rw, lw) = (&mut rw[0], &mut lw[0]);
        rw.pivot = Vec3::new(-3., 0., 3.);
        lw.pivot = Vec3::new(3., 0., 3.);
        rw.rot.x = -0.15707964;
        rw.rot.y = -(PI * 2. / 5.);
        rw.children[0].rot.y = -1.7278761;
        lw.rot.x = rw.rot.x;
        lw.rot.y = -rw.rot.y;
        lw.children[0].rot.y = -rw.children[0].rot.y;
    } else {
        head.rot = Vec3::new(pitch / (180. / PI), yaw / (180. / PI), 0.);
        head.pivot = Vec3::ZERO;
        body.rot.x = PI / 4. + cos(age * 0.1) * 0.15;
        body.rot.y = 0.;
        let (rw, lw) = body.children.split_at_mut(1);
        let (rw, lw) = (&mut rw[0], &mut lw[0]);
        rw.pivot = Vec3::ZERO;
        lw.pivot = Vec3::ZERO;
        rw.rot.y = cos(age * 1.3) * PI * 0.25;
        lw.rot.y = -rw.rot.y;
        rw.children[0].rot.y = rw.rot.y * 0.5;
        lw.children[0].rot.y = -rw.rot.y * 0.5;
    }
}

fn ocelot_angles(p: &mut [Part], limb: f32, amount: f32, yaw: f32, pitch: f32, state: u8) {
    // setLivingAnimations
    p[1].pivot.y = 12.;
    p[1].pivot.z = -10.;
    p[0].pivot.y = 15.;
    p[0].pivot.z = -9.;
    p[2].pivot.y = 15.;
    p[2].pivot.z = 8.;
    p[3].pivot.y = 20.;
    p[3].pivot.z = 14.;
    for i in [6, 7] {
        p[i].pivot.y = 13.8;
        p[i].pivot.z = -5.;
    }
    for i in [4, 5] {
        p[i].pivot.y = 18.;
        p[i].pivot.z = 5.;
    }
    p[2].rot.x = 0.9;
    match state {
        0 => {
            p[1].pivot.y += 1.;
            p[0].pivot.y += 2.;
            p[2].pivot.y += 1.;
            p[3].pivot.y += -4.;
            p[3].pivot.z += 2.;
            p[2].rot.x = PI / 2.;
            p[3].rot.x = PI / 2.;
        }
        2 => {
            p[3].pivot.y = p[2].pivot.y;
            p[3].pivot.z += 2.;
            p[2].rot.x = PI / 2.;
            p[3].rot.x = PI / 2.;
        }
        3 => {
            p[1].rot.x = PI / 4.;
            p[1].pivot.y += -4.;
            p[1].pivot.z += 5.;
            p[0].pivot.y += -3.3;
            p[0].pivot.z += 1.;
            p[2].pivot.y += 8.;
            p[2].pivot.z += -2.;
            p[3].pivot.y += 2.;
            p[3].pivot.z += -0.8;
            p[2].rot.x = 1.7278761;
            p[3].rot.x = 2.670354;
            for i in [6, 7] {
                p[i].rot.x = -0.15707964;
                p[i].pivot.y = 15.8;
                p[i].pivot.z = -7.;
            }
            for i in [4, 5] {
                p[i].rot.x = -(PI / 2.);
                p[i].pivot.y = 21.;
                p[i].pivot.z = 1.;
            }
        }
        _ => {}
    }
    // setRotationAngles
    p[0].rot.x = pitch / (180. / PI);
    p[0].rot.y = yaw / (180. / PI);
    if state != 3 {
        p[1].rot.x = PI / 2.;
        let w = limb * 0.6662;
        if state == 2 {
            p[4].rot.x = cos(w) * amount;
            p[5].rot.x = cos(w + 0.3) * amount;
            p[6].rot.x = cos(w + PI + 0.3) * amount;
            p[7].rot.x = cos(w + PI) * amount;
            p[3].rot.x = 1.7278761 + (PI / 10.) * cos(limb) * amount;
        } else {
            p[4].rot.x = cos(w) * amount;
            p[5].rot.x = cos(w + PI) * amount;
            p[6].rot.x = cos(w + PI) * amount;
            p[7].rot.x = cos(w) * amount;
            let k = if state == 1 { PI / 4. } else { 0.47123894 };
            p[3].rot.x = 1.7278761 + k * cos(limb) * amount;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity_model::{gl_normal, lights_in, standard_lighting};
    use glam::Mat3;

    #[test]
    fn ghast_tentacles_follow_random_1660() {
        let g = ghast();
        assert_eq!(g.parts.len(), 10);
        // Each tentacle is a 2x k x 2 box with k in 8..=14.
        for t in &g.parts[1..] {
            let h = (t.quads[0].pos[2].y - t.quads[0].pos[0].y).abs();
            assert!((8. ..=14.).contains(&h), "{h}");
        }
    }

    #[test]
    fn every_mob_renders() {
        let mut m = MobModels::new();
        let mobs = [
            Mob::Zombie { villager: false },
            Mob::Zombie { villager: true },
            Mob::PigZombie,
            Mob::Skeleton { wither: true },
            Mob::Creeper { flash: 0.15 },
            Mob::Spider { cave: false },
            Mob::Enderman { carrying: true, screaming: true },
            Mob::Pig { saddle: true },
            Mob::Cow { mooshroom: false },
            Mob::Sheep { color: 14, sheared: false },
            Mob::Chicken { wing: 0.3 },
            Mob::Wolf { tail: 0.6, sitting: true, angry: false, tamed: true, collar: 14 },
            Mob::Slime { size: 2., squish: 0.1 },
            Mob::MagmaCube { size: 2., squish: 0.1 },
            Mob::Villager { profession: 2 },
            Mob::Witch { holding: true },
            Mob::Squid { pitch: 10., yaw: 0., tentacle: 0.3 },
            Mob::Bat { hanging: true },
            Mob::Silverfish,
            Mob::Endermite,
            Mob::Blaze,
            Mob::Ghast { shooting: false },
            Mob::SnowMan,
            Mob::IronGolem { attack: 0., rose: 0 },
            Mob::Ocelot { skin: 1, state: 3, tamed: true },
            Mob::Rabbit { kind: 0 },
        ];
        for mob in mobs {
            let f = MobFrame {
                mob,
                id: 7,
                pos: Vec3::ZERO,
                body_yaw: 0.,
                head_yaw: 10.,
                pitch: 0.,
                limb_swing: 1.,
                limb_amount: 0.5,
                age: 3.,
                ticks: 3,
                swing: 0.,
                child: false,
                hurt: true,
                death: 0.,
                light: [1.; 3],
                bright: [1.; 3],
                held: None,
                armor: [None; 4],
            };
            let mut n = 0;
            m.render(&f, Mat4::IDENTITY, &mut |l, pm, q| {
                if let Tex::Mob(t) = l.tex {
                    assert!(TEXTURES.contains(&t), "{t}");
                }
                assert!(pm.is_finite() && q.pos[0].is_finite());
                n += 1;
            });
            assert!(n >= 6, "{mob:?}: {n}");
        }
    }

    /// `RenderSlime.preRenderCallback` squashes the model non-uniformly. The renderer keeps
    /// `GL_RESCALE_NORMAL` (so the frozen normals stay un-normalised), while only
    /// `LayerSlimeGel` enables `GL_NORMALIZE`; the two passes therefore shade differently.
    #[test]
    fn squashed_slime_keeps_rescale_normal_but_the_gel_normalises() {
        let mut m = MobModels::new();
        let f = MobFrame {
            mob: Mob::Slime { size: 1., squish: 0.9 },
            id: 7,
            pos: Vec3::ZERO,
            body_yaw: 0.,
            head_yaw: 0.,
            pitch: 0.,
            limb_swing: 0.,
            limb_amount: 0.,
            age: 0.,
            ticks: 0,
            swing: 0.,
            child: false,
            hurt: false,
            death: 0.,
            light: [1.; 3],
            bright: [1.; 3],
            held: None,
            armor: [None; 4],
        };
        let lights = lights_in(Mat3::IDENTITY);
        let mut main = Vec::new();
        let mut gel = Vec::new();
        m.render(&f, Mat4::IDENTITY, &mut |l, pm, q| {
            let nm = Mat3::from_mat4(pm).inverse().transpose();
            let shade = standard_lighting(gl_normal(nm, q.normal, l.normalize), lights);
            if l.normalize { gel.push(shade) } else { main.push(shade) }
        });
        assert!(!main.is_empty() && !gel.is_empty());
        let worst = main.iter().map(|a| gel.iter().map(|b| (a - b).abs()).fold(0f32, f32::max)).fold(0f32, f32::max);
        assert!(worst > 0.05, "main and gel shades differ by at most {worst}");
        // The main pass is the un-normalised `GL_RESCALE_NORMAL` result, the gel the normalised one.
        for &s in &main {
            assert!((0.4..=1.).contains(&s), "{s}");
        }
        for &s in &gel {
            assert!((0.4..=1.).contains(&s), "{s}");
        }
    }
}
