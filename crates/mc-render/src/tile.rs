//! Tile-entity renderers that draw from the shared entity atlas (1.8.9).
//!
//! Ported so far: `TileEntityChestRenderer` with `ModelChest` / `ModelLargeChest`. The client
//! owns the per-tile animation state (`TileEntityChest.update`) and hands it to the renderer in a
//! [`TileFrame`], exactly like `TileEntityRendererDispatcher` does.
//!
//! The dispatcher sets the lightmap from `World.getCombinedLight(pos, 0)` and leaves
//! `RenderHelper.enableStandardItemLighting()` active from `RenderGlobal.renderEntities`, so a
//! tile's faces are shaded by the standard two-light model times that position's lightmap.
use crate::entity_model::{ModelQuad, Part};
use crate::mobs::{Layer, Tex};
use glam::{Mat4, Vec3};

/// Atlas keys this module samples (added to the entity atlas at startup).
pub const TEXTURES: &[&str] = &[
    "sign",
    "chest/normal",
    "chest/trapped",
    "chest/ender",
    "chest/christmas",
    "chest/normal_double",
    "chest/trapped_double",
    "chest/ender_double",
    "chest/christmas_double",
];

/// `TileEntityChestRenderer`'s texture choice for a chest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestSkin {
    Normal,
    Trapped,
    Ender,
    /// `isChristmas`: December 24th-26th.
    Christmas,
}

impl ChestSkin {
    /// The atlas key for this skin; `large` selects the `_double` sheet.
    pub fn key(self, large: bool) -> &'static str {
        match (self, large) {
            (ChestSkin::Normal, false) => "chest/normal",
            (ChestSkin::Normal, true) => "chest/normal_double",
            (ChestSkin::Trapped, false) => "chest/trapped",
            (ChestSkin::Trapped, true) => "chest/trapped_double",
            (ChestSkin::Ender, false) => "chest/ender",
            (ChestSkin::Ender, true) => "chest/ender_double",
            (ChestSkin::Christmas, false) => "chest/christmas",
            (ChestSkin::Christmas, true) => "chest/christmas_double",
        }
    }
}

/// `ModelChest` / `ModelLargeChest`. Both sheets are 64 or 128 wide by 64 high.
#[derive(Clone, Debug)]
pub struct ChestModel {
    lid: Part,
    knob: Part,
    below: Part,
}

impl ChestModel {
    pub fn new(large: bool) -> Self {
        let t = if large { [128., 64.] } else { [64., 64.] };
        let (width, knob_x) = if large { (30, 16.) } else { (14, 8.) };
        Self {
            lid: Part::new().add_box(t, [0., 0.], [0., -5., -14.], [width, 5, 14], 0., false).at(1., 7., 15.),
            knob: Part::new().add_box(t, [0., 0.], [-1., -2., -15.], [2, 4, 1], 0., false).at(knob_x, 7., 15.),
            below: Part::new().add_box(t, [0., 19.], [0., 0., 0.], [width, 10, 14], 0., false).at(1., 6., 1.),
        }
    }

    /// `ModelChest.renderAll`: the knob follows the lid, then lid, knob, base.
    fn render_all(&self, lid_rot_x: f32, out: &mut impl FnMut(Mat4, &ModelQuad)) {
        let lid = Part { rot: Vec3::new(lid_rot_x, 0., 0.), ..self.lid.clone() };
        let knob = Part { rot: Vec3::new(lid_rot_x, 0., 0.), ..self.knob.clone() };
        let mut emit = |m: Mat4, q: &ModelQuad| out(m, q);
        lid.render(Mat4::IDENTITY, &mut emit);
        knob.render(Mat4::IDENTITY, &mut emit);
        self.below.render(Mat4::IDENTITY, &mut emit);
    }
}

/// The renderer's two models (`simpleChest` / `largeChest`), built once.
#[derive(Clone, Debug)]
pub struct ChestModels {
    simple: ChestModel,
    large: ChestModel,
    sign_board: Part,
    sign_stick: Part,
}

impl Default for ChestModels {
    fn default() -> Self {
        Self { simple: ChestModel::new(false), large: ChestModel::new(true),
            sign_board: Part::new().add_box([64., 32.], [0., 0.], [-12., -14., -1.], [24, 12, 2], 0., false),
            sign_stick: Part::new().add_box([64., 32.], [0., 14.], [-1., -2., -1.], [2, 14, 2], 0., false) }
    }
}

/// One tile-entity draw.
#[derive(Clone, Debug)]
pub enum TileFrame {
    Sign { pos: Vec3, standing: bool, meta: u8, light: [f32; 3] },
    /// `TileEntityChestRenderer.renderTileEntityAt`
    Chest {
        /// The tile's block position in world space.
        pos: Vec3,
        /// `te.getBlockMetadata()`: `EnumFacing.getIndex()` of the horizontal facing (2-5).
        meta: u8,
        /// `adjacentChestXPos != null || adjacentChestZPos != null`
        large: bool,
        skin: ChestSkin,
        /// `i == 2 && te.adjacentChestXPos != null`
        x_shift: bool,
        /// `i == 5 && te.adjacentChestZPos != null`
        z_shift: bool,
        /// Interpolated `lidAngle` (0 closed, 1 open).
        lid: f32,
        light: [f32; 3],
    },
}

impl TileFrame {
    pub fn light(&self) -> [f32; 3] {
        match self {
            TileFrame::Chest { light, .. } | TileFrame::Sign { light, .. } => *light,
        }
    }
}

/// `TileEntityChestRenderer.renderTileEntityAt`. `pos` is world space; the caller subtracts the
/// camera before projecting, matching `TileEntityRendererDispatcher.staticPlayer*`.
pub fn render(f: &TileFrame, models: &ChestModels, out: &mut dyn FnMut(&Layer, Mat4, &ModelQuad)) {
    match f {
        TileFrame::Sign { pos, standing, meta, .. } => {
            let mut m = Mat4::from_translation(*pos + Vec3::new(0.5, 0.5, 0.5));
            let rotation = if *standing { *meta as f32 * 360. / 16. } else { match meta { 2 => 180., 4 => 90., 5 => -90., _ => 0. } };
            m *= Mat4::from_rotation_y((-rotation).to_radians());
            if !standing { m *= Mat4::from_translation(Vec3::new(0., -0.3125, -0.4375)); }
            m *= Mat4::from_scale(Vec3::new(2. / 3., -2. / 3., -2. / 3.));
            let layer = Layer { tex: Tex::Mob("sign"), tint: [1.; 3], overlay: None, additive: false, glint: false };
            models.sign_board.render(m, &mut |pm, q| out(&layer, pm, q));
            if *standing { models.sign_stick.render(m, &mut |pm, q| out(&layer, pm, q)); }
        }
        TileFrame::Chest { pos, meta, large, skin, x_shift, z_shift, lid, .. } => {
            let model = if *large { &models.large } else { &models.simple };
            // GlStateManager.translate(x, y + 1, z + 1); scale(1, -1, -1);
            // translate(0.5, 0.5, 0.5); [double-chest shifts]; rotate(j, 0, 1, 0);
            // translate(-0.5, -0.5, -0.5).
            let j: f32 = match meta {
                2 => 180.,
                3 => 0.,
                4 => 90.,
                5 => -90.,
                _ => 0.,
            };
            let mut m = Mat4::from_translation(Vec3::new(pos.x, pos.y + 1., pos.z + 1.));
            m *= Mat4::from_scale(Vec3::new(1., -1., -1.));
            m *= Mat4::from_translation(Vec3::new(0.5, 0.5, 0.5));
            if *x_shift {
                m *= Mat4::from_translation(Vec3::new(1., 0., 0.));
            }
            if *z_shift {
                m *= Mat4::from_translation(Vec3::new(0., 0., -1.));
            }
            m *= Mat4::from_rotation_y(j.to_radians());
            m *= Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5));
            // f = 1 - f; f = 1 - f*f*f; rotateAngleX = -(f * PI / 2)
            let f = 1. - *lid;
            let f = 1. - f * f * f;
            let rot = -(f * std::f32::consts::PI / 2.);
            let layer = Layer { tex: Tex::Mob(skin.key(*large)), tint: [1.; 3], overlay: None, additive: false, glint: false };
            model.render_all(rot, &mut |pm, q| out(&layer, m * pm, q));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(lid: f32, large: bool) -> TileFrame {
        chest(lid, large, ChestSkin::Normal)
    }

    fn chest(lid: f32, large: bool, skin: ChestSkin) -> TileFrame {
        TileFrame::Chest {
            pos: Vec3::ZERO,
            meta: 3,
            large,
            skin,
            x_shift: false,
            z_shift: false,
            lid,
            light: [1.; 3],
        }
    }

    #[test]
    fn sign_models_follow_java_board_and_stick_visibility() {
        for (standing, expected) in [(true, 12), (false, 6)] {
            let frame = TileFrame::Sign { pos: Vec3::ZERO, standing, meta: 0, light: [1.;3] };
            let mut count = 0;
            render(&frame, &ChestModels::default(), &mut |layer, _, _| { assert!(matches!(layer.tex, Tex::Mob("sign"))); count += 1; });
            assert_eq!(count, expected);
        }
    }

    /// `ModelChest`'s three parts, in `renderAll` order: 6 quads each.
    #[test]
    fn chest_draws_three_boxes() {
        for large in [false, true] {
            let mut n = 0;
            render(&frame(0., large), &ChestModels::default(), &mut |_, _, _| n += 1);
            assert_eq!(n, 18, "large={large}");
        }
    }

    /// The lid is the first part rendered; opening it rotates about the hinge at model
    /// (1, 7, 15) by -90 degrees, so the top of the lid ends up well above the base's 0.875.
    #[test]
    fn lid_rotation_swings_up() {
        let top = |f: &TileFrame| {
            let mut ys = Vec::new();
            let mut i = 0;
            render(f, &ChestModels::default(), &mut |_, m, q| {
                if i < 6 {
                    for p in q.pos {
                        ys.push((m * p.extend(1.)).y);
                    }
                }
                i += 1;
            });
            ys.into_iter().fold(f32::MIN, f32::max)
        };
        let (closed, open) = (top(&frame(0., false)), top(&frame(1., false)));
        // Closed: the lid's outer face is at model y = 2/16 → world y = 0.875.
        assert!((closed - 0.875).abs() < 1e-5, "closed {closed}");
        // Open: the lid stands up behind the chest, its top edge at 23/16 = 1.4375.
        assert!((open - 1.4375).abs() < 1e-5, "open {open}");
    }

    #[test]
    fn double_chest_uses_the_wide_sheet() {
        let key = |f: &TileFrame| {
            let mut k = None;
            render(f, &ChestModels::default(), &mut |l, _, _| {
                if let Tex::Mob(t) = l.tex {
                    k = Some(t);
                }
            });
            k
        };
        assert_eq!(key(&chest(0., false, ChestSkin::Normal)), Some("chest/normal"));
        assert_eq!(key(&chest(0., true, ChestSkin::Normal)), Some("chest/normal_double"));
        assert_eq!(key(&chest(0., false, ChestSkin::Ender)), Some("chest/ender"));
        assert_eq!(key(&chest(0., false, ChestSkin::Trapped)), Some("chest/trapped"));
        assert_eq!(key(&chest(0., true, ChestSkin::Trapped)), Some("chest/trapped_double"));
        assert_eq!(key(&chest(0., false, ChestSkin::Christmas)), Some("chest/christmas"));
        assert_eq!(key(&chest(0., true, ChestSkin::Christmas)), Some("chest/christmas_double"));
    }
}
