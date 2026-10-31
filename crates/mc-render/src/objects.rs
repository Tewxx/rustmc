//! Non-living entity renderers that draw from the shared entity atlas: arrows, experience
//! orbs, paintings and armour stands. Ported from `RenderArrow`, `RenderXPOrb`,
//! `RenderPainting`, `ArmorStandRenderer` / `ModelArmorStand` (1.8.9).
//!
//! Billboards built from item models (`RenderSnowball`, `RenderPotion`), block models
//! (`RenderTNTPrimed`, `RenderFallingBlock`, `RenderItemFrame`) and the block-atlas quad of
//! `RenderFireball` are emitted by the client as `WorldQuad`/`Billboard` entries instead: their
//! textures live in the block/item atlas, not here.
use crate::entity_model::{ModelQuad, Part};
use crate::mobs::{Layer, Tex};
use glam::{Mat4, Vec3};

/// Atlas keys this module samples (added to the entity atlas at startup).
pub const TEXTURES: &[&str] = &["arrow", "experience_orb", "armorstand/wood"];

/// Extra atlas entries that do not live under `textures/entity/`: `(key, path)`.
pub const EXTRA_TEXTURES: &[(&str, &str)] = &[
    ("painting", "textures/painting/paintings_kristoffer_zetterstrand.png"),
    // `EffectRenderer.particleTextures` and `EntityLargeExplodeFX`'s own sheet.
    ("particles", "textures/particle/particles.png"),
    ("explosion", "textures/entity/explosion.png"),
];

/// `EntityPainting.EnumArt`: title, size in 16-pixel units, texture offset in the 256x256 sheet.
pub struct Art {
    pub title: &'static str,
    pub size: (i32, i32),
    pub offset: (i32, i32),
}

/// Declaration order is `EnumArt`'s ordinal order (the save/network index).
pub const ARTS: &[Art] = &[
    Art { title: "Kebab", size: (16, 16), offset: (0, 0) },
    Art { title: "Aztec", size: (16, 16), offset: (16, 0) },
    Art { title: "Alban", size: (16, 16), offset: (32, 0) },
    Art { title: "Aztec2", size: (16, 16), offset: (48, 0) },
    Art { title: "Bomb", size: (16, 16), offset: (64, 0) },
    Art { title: "Plant", size: (16, 16), offset: (80, 0) },
    Art { title: "Wasteland", size: (16, 16), offset: (96, 0) },
    Art { title: "Pool", size: (32, 16), offset: (0, 32) },
    Art { title: "Courbet", size: (32, 16), offset: (32, 32) },
    Art { title: "Sea", size: (32, 16), offset: (64, 32) },
    Art { title: "Sunset", size: (32, 16), offset: (96, 32) },
    Art { title: "Creebet", size: (32, 16), offset: (128, 32) },
    Art { title: "Wanderer", size: (16, 32), offset: (0, 64) },
    Art { title: "Graham", size: (16, 32), offset: (16, 64) },
    Art { title: "Match", size: (32, 32), offset: (0, 128) },
    Art { title: "Bust", size: (32, 32), offset: (32, 128) },
    Art { title: "Stage", size: (32, 32), offset: (64, 128) },
    Art { title: "Void", size: (32, 32), offset: (96, 128) },
    Art { title: "SkullAndRoses", size: (32, 32), offset: (128, 128) },
    Art { title: "Wither", size: (32, 32), offset: (160, 128) },
    Art { title: "Fighters", size: (64, 32), offset: (0, 96) },
    Art { title: "Pointer", size: (64, 64), offset: (0, 192) },
    Art { title: "Pigscene", size: (64, 64), offset: (64, 192) },
    Art { title: "BurningSkull", size: (64, 64), offset: (128, 192) },
    Art { title: "Skeleton", size: (64, 48), offset: (192, 64) },
    Art { title: "DonkeyKong", size: (64, 48), offset: (192, 112) },
];

pub fn art_by_title(title: &str) -> Option<usize> {
    ARTS.iter().position(|a| a.title == title)
}

/// One non-living entity draw.
#[derive(Clone, Debug)]
pub enum ObjectFrame {
    /// `RenderArrow`. `shake` = `arrowShake - partialTicks`.
    Arrow { pos: Vec3, yaw: f32, pitch: f32, shake: f32, light: [f32; 3] },
    /// `RenderXPOrb`. `color` is `(sin(xpColor+pt)/2, sin(xpColor+pt+4.1887903)/2)` mapped to
    /// bytes by the caller.
    XpOrb { pos: Vec3, icon: u8, color: [f32; 3], light: [f32; 3] },
    /// `RenderPainting`: `pos` is the entity origin (hanging block corner + offsets).
    Painting { pos: Vec3, yaw: f32, art: usize, light: [f32; 3] },
    /// `ArmorStandRenderer`. The wooden stand, its body parts and the flags byte 10.
    ArmorStand {
        pos: Vec3,
        yaw: f32,
        head_yaw: f32,
        pitch: f32,
        flags: u8,
        light: [f32; 3],
        /// `getBodyRotation` (degrees, metadata 12).
        body_rot: (f32, f32, f32),
    },
}

impl ObjectFrame {
    pub fn light(&self) -> [f32; 3] {
        match self {
            ObjectFrame::Arrow { light, .. } | ObjectFrame::XpOrb { light, .. } | ObjectFrame::Painting { light, .. } | ObjectFrame::ArmorStand { light, .. } => *light,
        }
    }
}

/// Armour stand parts (texture `armorstand/wood.png`, 64×64).
pub struct ArmorStandModel {
    head: Part,
    body: Part,
    right_arm: Part,
    left_arm: Part,
    right_leg: Part,
    left_leg: Part,
    right_side: Part,
    left_side: Part,
    waist: Part,
    base: Part,
}

impl ArmorStandModel {
    pub fn new() -> Self {
        let t = [64., 64.];
        Self {
            head: Part::new().add_box(t, [0., 0.], [-1., -7., -1.], [2, 7, 2], 0., false),
            body: Part::new().add_box(t, [0., 26.], [-6., 0., -1.5], [12, 3, 3], 0., false),
            right_arm: Part::new().add_box(t, [24., 0.], [-2., -2., -1.], [2, 12, 2], 0., false).at(-5., 2., 0.),
            left_arm: Part::new().add_box(t, [32., 16.], [0., -2., -1.], [2, 12, 2], 0., true).at(5., 2., 0.),
            right_leg: Part::new().add_box(t, [8., 0.], [-1., 0., -1.], [2, 11, 2], 0., false).at(-1.9, 12., 0.),
            left_leg: Part::new().add_box(t, [40., 16.], [-1., 0., -1.], [2, 11, 2], 0., true).at(1.9, 12., 0.),
            right_side: Part::new().add_box(t, [16., 0.], [-3., 3., -1.], [2, 7, 2], 0., false),
            left_side: Part::new().add_box(t, [48., 16.], [1., 3., -1.], [2, 7, 2], 0., false),
            waist: Part::new().add_box(t, [0., 48.], [-4., 10., -1.], [8, 2, 2], 0., false),
            base: Part::new().add_box(t, [0., 32.], [-6., 11., -6.], [12, 1, 12], 0., false).at(0., 12., 0.),
        }
    }
}

impl Default for ArmorStandModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Emits one object frame. The callback receives the atlas layer, the model matrix and a quad
/// whose positions are in model units (the matrix includes the 1/16 scale where vanilla does).
/// `cam_yaw` / `cam_pitch` are `RenderManager.playerViewY/X` in degrees (billboards).
pub fn render(frame: &ObjectFrame, stands: &mut ArmorStandModel, cam_yaw: f32, cam_pitch: f32, out: &mut dyn FnMut(&Layer, Mat4, &ModelQuad)) {
    match frame {
        ObjectFrame::Arrow { pos, yaw, pitch, shake, .. } => {
            let mut m = Mat4::from_translation(*pos);
            // RenderArrow: rotate(yaw - 90, Y), rotate(pitch, Z), shake, rotate(45, X),
            // scale(0.05625), translate(-4, 0, 0).
            m *= Mat4::from_rotation_y((yaw - 90.).to_radians());
            m *= Mat4::from_rotation_z(pitch.to_radians());
            if *shake > 0. {
                m *= Mat4::from_rotation_z(-mc_core::math::sin(shake * 3.) * shake);
            }
            m *= Mat4::from_rotation_x(45f32.to_radians());
            m *= Mat4::from_scale(Vec3::splat(0.05625));
            m *= Mat4::from_translation(Vec3::new(-4., 0., 0.));
            let layer = Layer { tex: Tex::Mob("arrow"), tint: [1.; 3], overlay: None, additive: false, normalize: false, glint: false };
            // The two shaft faces at x=-7, then four fins every 90° about X. `RenderArrow`
            // passes `glNormal3f(+-0.05625, 0, 0)` / `(0, 0, 0.05625)` — the scale factor, not a
            // unit vector — and `GL_RESCALE_NORMAL` does not renormalise it, so arrows stay
            // almost flat-lit in vanilla.
            let f8 = 0.05625;
            let n = Vec3::new(f8, 0., 0.);
            let quad = |pos: [Vec3; 4], uv: [[f32; 2]; 4], n: Vec3| ModelQuad { pos, uv, normal: n };
            out(&layer, m, &quad(
                [Vec3::new(-7., -2., -2.), Vec3::new(-7., -2., 2.), Vec3::new(-7., 2., 2.), Vec3::new(-7., 2., -2.)],
                [[0., 0.15625], [0.15625, 0.15625], [0.15625, 0.3125], [0., 0.3125]],
                n,
            ));
            out(&layer, m, &quad(
                [Vec3::new(-7., 2., -2.), Vec3::new(-7., 2., 2.), Vec3::new(-7., -2., 2.), Vec3::new(-7., -2., -2.)],
                [[0., 0.15625], [0.15625, 0.15625], [0.15625, 0.3125], [0., 0.3125]],
                -n,
            ));
            let mut fm = m;
            for _ in 0..4 {
                fm *= Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
                out(
                    &layer,
                    fm,
                    &quad(
                        [Vec3::new(-8., -2., 0.), Vec3::new(8., -2., 0.), Vec3::new(8., 2., 0.), Vec3::new(-8., 2., 0.)],
                        [[0., 0.], [0.5, 0.], [0.5, 0.15625], [0., 0.15625]],
                        Vec3::new(0., 0., f8),
                    ),
                );
            }
        }
        ObjectFrame::XpOrb { pos, icon, color, .. } => {
            // RenderXPOrb: sprite index picks a 16×16 cell in the 64×64 sheet; the billboard
            // is rotated by the player's view angles and scaled 0.3.
            let i = *icon as i32;
            let (u0, v0) = ((i % 4 * 16) as f32 / 64., (i / 4 * 16) as f32 / 64.);
            let (u1, v1) = ((i % 4 * 16 + 16) as f32 / 64., (i / 4 * 16 + 16) as f32 / 64.);
            let layer = Layer { tex: Tex::Mob("experience_orb"), tint: *color, overlay: None, additive: false, normalize: false, glint: false };
            let m = Mat4::from_translation(*pos)
                * Mat4::from_rotation_y((180. - cam_yaw).to_radians())
                * Mat4::from_rotation_x((-cam_pitch).to_radians())
                * Mat4::from_scale(Vec3::splat(XP_SCALE));
            // Quad spans x -0.5..0.5, y -0.25..0.75 (f5 = 0.5, f6 = 0.25).
            out(
                &layer,
                m,
                &ModelQuad {
                    pos: [Vec3::new(-0.5, -0.25, 0.), Vec3::new(0.5, -0.25, 0.), Vec3::new(0.5, 0.75, 0.), Vec3::new(-0.5, 0.75, 0.)],
                    uv: [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
                    normal: Vec3::new(0., 1., 0.),
                },
            );
        }
        ObjectFrame::Painting { pos, yaw, art, .. } => {
            let a = &ARTS[(*art).min(ARTS.len() - 1)];
            let (w, h) = (a.size.0 as f32, a.size.1 as f32);
            let (tu, tv) = (a.offset.0 as f32, a.offset.1 as f32);
            let layer = Layer { tex: Tex::Mob("painting"), tint: [1.; 3], overlay: None, additive: false, normalize: false, glint: false };
            let mut m = Mat4::from_translation(*pos);
            m *= Mat4::from_rotation_y((180. - yaw).to_radians());
            m *= Mat4::from_scale(Vec3::splat(0.0625));
            let f = -w / 2.;
            let f1 = -h / 2.;
            let mut quad = |pos: [Vec3; 4], uv: [[f32; 2]; 4], n: Vec3| {
                out(&layer, m, &ModelQuad { pos, uv, normal: n });
            };
            for i in 0..(w as i32 / 16) {
                for j in 0..(h as i32 / 16) {
                    let f15 = f + ((i + 1) * 16) as f32;
                    let f16 = f + (i * 16) as f32;
                    let f17 = f1 + ((j + 1) * 16) as f32;
                    let f18 = f1 + (j * 16) as f32;
                    let f19 = (tu + w - (i * 16) as f32) / 256.;
                    let f20 = (tu + w - ((i + 1) * 16) as f32) / 256.;
                    let f21 = (tv + h - (j * 16) as f32) / 256.;
                    let f22 = (tv + h - ((j + 1) * 16) as f32) / 256.;
                    // Front (the artwork).
                    quad(
                        [Vec3::new(f15, f18, -0.5), Vec3::new(f16, f18, -0.5), Vec3::new(f16, f17, -0.5), Vec3::new(f15, f17, -0.5)],
                        [[f20, f21], [f19, f21], [f19, f22], [f20, f22]],
                        Vec3::new(0., 0., -1.),
                    );
                    // Back, top, bottom and sides use the generic canvas/edge parts of the sheet.
                    quad(
                        [Vec3::new(f15, f17, 0.5), Vec3::new(f16, f17, 0.5), Vec3::new(f16, f18, 0.5), Vec3::new(f15, f18, 0.5)],
                        [[0.75, 0.], [0.8125, 0.], [0.8125, 0.0625], [0.75, 0.0625]],
                        Vec3::new(0., 0., 1.),
                    );
                    quad(
                        [Vec3::new(f15, f17, -0.5), Vec3::new(f16, f17, -0.5), Vec3::new(f16, f17, 0.5), Vec3::new(f15, f17, 0.5)],
                        [[0.75, 0.001953125], [0.8125, 0.001953125], [0.8125, 0.001953125], [0.75, 0.001953125]],
                        Vec3::new(0., 1., 0.),
                    );
                    quad(
                        [Vec3::new(f15, f18, 0.5), Vec3::new(f16, f18, 0.5), Vec3::new(f16, f18, -0.5), Vec3::new(f15, f18, -0.5)],
                        [[0.75, 0.001953125], [0.8125, 0.001953125], [0.8125, 0.001953125], [0.75, 0.001953125]],
                        Vec3::new(0., -1., 0.),
                    );
                    quad(
                        [Vec3::new(f15, f17, 0.5), Vec3::new(f15, f18, 0.5), Vec3::new(f15, f18, -0.5), Vec3::new(f15, f17, -0.5)],
                        [[0.7519531, 0.], [0.7519531, 0.0625], [0.7519531, 0.0625], [0.7519531, 0.]],
                        Vec3::new(-1., 0., 0.),
                    );
                    quad(
                        [Vec3::new(f16, f17, -0.5), Vec3::new(f16, f18, -0.5), Vec3::new(f16, f18, 0.5), Vec3::new(f16, f17, 0.5)],
                        [[0.7519531, 0.], [0.7519531, 0.0625], [0.7519531, 0.0625], [0.7519531, 0.]],
                        Vec3::new(1., 0., 0.),
                    );
                }
            }
        }
        ObjectFrame::ArmorStand { pos, yaw, head_yaw, pitch, flags, body_rot, .. } => {
            let small = flags & 1 != 0;
            let show_arms = flags & 4 != 0;
            let no_base = flags & 8 != 0;
            let mut m = Mat4::from_translation(*pos);
            m *= Mat4::from_rotation_y((180. - yaw).to_radians());
            m *= Mat4::from_scale(Vec3::new(-1., -1., 1.));
            if small {
                // ModelBiped.render: scale 1/2 then translate 0, 24 * 0.0625.
                m *= Mat4::from_scale(Vec3::splat(0.5));
                m *= Mat4::from_translation(Vec3::new(0., 24. * 0.0625, 0.));
            }
            let layer = Layer { tex: Tex::Mob("armorstand/wood"), tint: [1.; 3], overlay: None, additive: false, normalize: false, glint: false };
            // ModelArmorStand.setRotationAngles: the biped parts keep the living pose (head
            // yaw/pitch only for an idle stand); the stand pieces take the body rotation and
            // the base takes the opposite of the entity yaw.
            let (bx, by, bz) = *body_rot;
            let (brx, bry, brz) = (bx.to_radians(), by.to_radians(), bz.to_radians());
            let net_head = head_yaw - yaw;
            stands.head.rot = Vec3::new(pitch.to_radians(), net_head.to_radians(), 0.);
            stands.body.rot = Vec3::ZERO;
            stands.right_arm.show = show_arms;
            stands.left_arm.show = show_arms;
            for p in [&mut stands.right_side, &mut stands.left_side, &mut stands.waist] {
                p.rot = Vec3::new(brx, bry, brz);
            }
            stands.base.show = !no_base;
            stands.base.rot = Vec3::new(0., -yaw.to_radians(), 0.);
            for p in [
                &stands.head,
                &stands.body,
                &stands.right_arm,
                &stands.left_arm,
                &stands.right_leg,
                &stands.left_leg,
                &stands.right_side,
                &stands.left_side,
                &stands.waist,
                &stands.base,
            ] {
                p.render(m, &mut |pm, q| out(&layer, pm, q));
            }
        }
    }
}

/// The XP orb's colour bytes: `RenderXPOrb` builds `(sin(f9), 1, sin(f9 + 4.1887903) / 10)`
/// scaled to 0..255 around 0.5 and 0.1.
pub fn xp_color(xp_color: f32) -> [f32; 3] {
    let f9 = xp_color / 2.;
    let l = (mc_core::math::sin(f9) + 1.) * 0.5;
    let j1 = (mc_core::math::sin(f9 + 4.1887903) + 1.) * 0.1;
    [l, 1., j1]
}

/// `EntityXPOrb.getTextureByXP` for an orb's value.
pub fn xp_texture(xp_value: i32) -> u8 {
    match xp_value {
        v if v >= 2477 => 10,
        v if v >= 1237 => 9,
        v if v >= 617 => 8,
        v if v >= 307 => 7,
        v if v >= 149 => 6,
        v if v >= 73 => 5,
        v if v >= 37 => 4,
        v if v >= 17 => 3,
        v if v >= 7 => 2,
        v if v >= 3 => 1,
        _ => 0,
    }
}

/// `RenderXPOrb`'s `0.3` scale (the quad spans 0..1 and is offset by `(0.5, 0.25)`).
pub const XP_SCALE: f32 = 0.3;
/// `RenderXPOrb` colour alpha (128/255).
pub const XP_ALPHA: f32 = 128. / 255.;

/// Vertex alpha for one layer. Only the XP orb's sprite is translucent
/// (`GlStateManager.color(..., 128)`); every other object pass is opaque.
pub fn layer_alpha(l: &Layer) -> f32 {
    if matches!(l.tex, Tex::Mob("experience_orb")) {
        XP_ALPHA
    } else {
        1.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat3;

    #[test]
    fn art_table_matches_enum_art() {
        // 26 artworks; titles are the S10 `title` strings and sizes the sheet extents.
        assert_eq!(ARTS.len(), 26);
        assert_eq!(art_by_title("Kebab"), Some(0));
        assert_eq!(art_by_title("Aztec"), Some(1));
        assert_eq!(art_by_title("DonkeyKong"), Some(25));
        assert_eq!(art_by_title("nope"), None);
        assert_eq!(ARTS[21].size, (64, 64));
        assert_eq!(ARTS[25].offset, (192, 112));
        // Every art lies inside the 256x256 sheet.
        for a in ARTS {
            assert!(a.offset.0 + a.size.0 <= 256 && a.offset.1 + a.size.1 <= 256, "{}", a.title);
            assert!(a.size.0 % 16 == 0 && a.size.1 % 16 == 0, "{}", a.title);
        }
    }

    #[test]
    fn xp_texture_thresholds_follow_get_texture_by_xp() {
        assert_eq!(xp_texture(0), 0);
        assert_eq!(xp_texture(2), 0);
        assert_eq!(xp_texture(3), 1);
        assert_eq!(xp_texture(6), 1);
        assert_eq!(xp_texture(7), 2);
        assert_eq!(xp_texture(16), 2);
        assert_eq!(xp_texture(17), 3);
        assert_eq!(xp_texture(37), 4);
        assert_eq!(xp_texture(73), 5);
        assert_eq!(xp_texture(2477), 10);
        assert_eq!(xp_texture(-5), 0);
    }

    #[test]
    fn xp_color_stays_in_range() {
        for i in 0..400 {
            let c = xp_color(i as f32);
            assert!(c[0] >= 0. && c[0] <= 1.);
            assert_eq!(c[1], 1.);
            assert!(c[2] >= 0. && c[2] <= 0.2);
        }
    }

    /// `RenderArrow` passes `glNormal3f(0.05625, 0, 0)` / `(0, 0, 0.05625)` — the model scale
    /// factor, not a unit vector — while `GL_RESCALE_NORMAL` only renormalises unit input, so
    /// vanilla arrows end up almost flat-lit (0.40–0.46 here, from the ambient term plus at most
    /// 0.6 * 0.05625 * |LIGHT0 + LIGHT1|). Normalising instead would be visibly brighter, so this
    /// locks in the quirk.
    #[test]
    fn arrow_normals_keep_vanillas_rescale_normal_quirk() {
        let mut stands = ArmorStandModel::new();
        let mut quads = Vec::new();
        render(
            &ObjectFrame::Arrow { pos: Vec3::ZERO, yaw: 0., pitch: 0., shake: 0., light: [1.; 3] },
            &mut stands,
            0.,
            0.,
            &mut |_, m, q| quads.push((m, *q)),
        );
        assert_eq!(quads.len(), 6);
        let lights = crate::entity_model::lights_in(Mat3::IDENTITY);
        let (mut lit_max, mut normalized_max) = (0f32, 0f32);
        for (m, q) in quads {
            let nm = Mat3::from_mat4(m).inverse().transpose();
            let lit = crate::entity_model::standard_lighting(crate::entity_model::gl_normal(nm, q.normal, false), lights);
            assert!((0.4..0.46).contains(&lit), "arrow shade {lit}");
            let normalized = crate::entity_model::standard_lighting(crate::entity_model::gl_normal(nm, q.normal, true), lights);
            lit_max = lit_max.max(lit);
            normalized_max = normalized_max.max(normalized);
        }
        // At least the faces turned towards a light would be visibly brighter if normalised.
        assert!(normalized_max > lit_max + 0.05, "{lit_max} vs {normalized_max}");
    }
}
