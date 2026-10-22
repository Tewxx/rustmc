//! `ModelRenderer` / `ModelBox` / `TexturedQuad`: the box models every vanilla entity uses.
//!
//! Boxes are built exactly like `ModelBox` (corner order, texture rectangles, mirroring and the
//! cross-product normal), and parts render like `ModelRenderer.render`: translate to the pivot,
//! rotate Z, Y, X, draw at 1/16 scale.
use glam::{Mat3, Mat4, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct ModelQuad {
    pub pos: [Vec3; 4],
    pub uv: [[f32; 2]; 4],
    pub normal: Vec3,
}

#[derive(Clone, Debug, Default)]
pub struct Part {
    pub quads: Vec<ModelQuad>,
    pub pivot: Vec3,
    /// Radians (`rotateAngleX/Y/Z`).
    pub rot: Vec3,
    pub show: bool,
    pub children: Vec<Part>,
}

impl Part {
    pub fn new() -> Self {
        Self { show: true, ..Default::default() }
    }

    /// `ModelRenderer.addBox` with `ModelBox`'s texture layout. `tex` is the sheet size.
    #[allow(clippy::too_many_arguments)]
    pub fn add_box(mut self, tex: [f32; 2], uv: [f32; 2], from: [f32; 3], size: [i32; 3], delta: f32, mirror: bool) -> Self {
        let [dx, dy, dz] = size;
        let (mut x, mut y, mut z) = (from[0], from[1], from[2]);
        let mut f = x + dx as f32;
        let mut f1 = y + dy as f32;
        let mut f2 = z + dz as f32;
        x -= delta;
        y -= delta;
        z -= delta;
        f += delta;
        f1 += delta;
        f2 += delta;
        if mirror {
            std::mem::swap(&mut x, &mut f);
        }
        let v = [
            Vec3::new(x, y, z),
            Vec3::new(f, y, z),
            Vec3::new(f, f1, z),
            Vec3::new(x, f1, z),
            Vec3::new(x, y, f2),
            Vec3::new(f, y, f2),
            Vec3::new(f, f1, f2),
            Vec3::new(x, f1, f2),
        ];
        let (u, w) = (uv[0], uv[1]);
        let (dx, dy, dz) = (dx as f32, dy as f32, dz as f32);
        let faces: [([usize; 4], [f32; 4]); 6] = [
            ([5, 1, 2, 6], [u + dz + dx, w + dz, u + dz + dx + dz, w + dz + dy]),
            ([0, 4, 7, 3], [u, w + dz, u + dz, w + dz + dy]),
            ([5, 4, 0, 1], [u + dz, w, u + dz + dx, w + dz]),
            ([2, 3, 7, 6], [u + dz + dx, w + dz, u + dz + dx + dx, w]),
            ([1, 0, 3, 2], [u + dz, w + dz, u + dz + dx, w + dz + dy]),
            ([4, 5, 6, 7], [u + dz + dx + dz, w + dz, u + dz + dx + dz + dx, w + dz + dy]),
        ];
        for (idx, [u1, v1, u2, v2]) in faces {
            let mut pos = idx.map(|i| v[i]);
            let mut uvs = [
                [u2 / tex[0], v1 / tex[1]],
                [u1 / tex[0], v1 / tex[1]],
                [u1 / tex[0], v2 / tex[1]],
                [u2 / tex[0], v2 / tex[1]],
            ];
            if mirror {
                // TexturedQuad.flipFace reverses the vertex order.
                pos.reverse();
                uvs.reverse();
            }
            // TexturedQuad.draw: (v2 - v1) x (v0 - v1), normalized.
            let a = pos[0] - pos[1];
            let b = pos[2] - pos[1];
            let normal = b.cross(a).normalize_or_zero();
            self.quads.push(ModelQuad { pos, uv: uvs, normal });
        }
        self
    }

    pub fn at(mut self, x: f32, y: f32, z: f32) -> Self {
        self.pivot = Vec3::new(x, y, z);
        self
    }

    /// `ModelRenderer.render(scale)` with `scale` = 1/16; emits quads in the caller's space.
    pub fn render(&self, m: Mat4, out: &mut impl FnMut(Mat4, &ModelQuad)) {
        if !self.show {
            return;
        }
        let m = m * self.local();
        for q in &self.quads {
            out(m, q);
        }
        for c in &self.children {
            c.render(m, out);
        }
    }

    /// Pivot translation and Z, Y, X rotation in 1/16 units; geometry is then scaled by 1/16.
    pub fn local(&self) -> Mat4 {
        let mut m = Mat4::from_translation(self.pivot / 16.);
        if self.rot.z != 0. {
            m *= Mat4::from_rotation_z(self.rot.z);
        }
        if self.rot.y != 0. {
            m *= Mat4::from_rotation_y(self.rot.y);
        }
        if self.rot.x != 0. {
            m *= Mat4::from_rotation_x(self.rot.x);
        }
        m * Mat4::from_scale(Vec3::splat(1. / 16.))
    }

    /// `ModelRenderer.postRender`: the part's transform for attached objects (held items).
    pub fn post_render(&self) -> Mat4 {
        let mut m = Mat4::from_translation(self.pivot / 16.);
        if self.rot.z != 0. {
            m *= Mat4::from_rotation_z(self.rot.z);
        }
        if self.rot.y != 0. {
            m *= Mat4::from_rotation_y(self.rot.y);
        }
        if self.rot.x != 0. {
            m *= Mat4::from_rotation_x(self.rot.x);
        }
        m
    }

    /// `ModelBase.copyModelAngles`
    pub fn copy_angles(&mut self, from: &Part) {
        self.rot = from.rot;
        self.pivot = from.pivot;
    }
}

/// `RenderHelper.enableStandardItemLighting` directions.
pub const LIGHT0: Vec3 = Vec3::new(0.2, 1.0, -0.7);
pub const LIGHT1: Vec3 = Vec3::new(-0.2, 1.0, 0.7);

/// GL fixed-function result for those two lights: ambient 0.4, diffuse 0.6 each, clamped.
pub fn standard_lighting(normal: Vec3, lights: [Vec3; 2]) -> f32 {
    let n = normal.normalize_or_zero();
    (0.4 + 0.6 * n.dot(lights[0]).max(0.) + 0.6 * n.dot(lights[1]).max(0.)).min(1.)
}

/// Light directions after transforming by `m` (glLight positions use the current modelview).
pub fn lights_in(m: Mat3) -> [Vec3; 2] {
    [(m * LIGHT0.normalize()).normalize(), (m * LIGHT1.normalize()).normalize()]
}

/// `ModelPlayer` (64×64 skin, non-slim unless `slim`).
pub struct PlayerModel {
    pub head: Part,
    pub headwear: Part,
    pub body: Part,
    pub right_arm: Part,
    pub left_arm: Part,
    pub right_leg: Part,
    pub left_leg: Part,
    pub left_leg_wear: Part,
    pub right_leg_wear: Part,
    pub left_arm_wear: Part,
    pub right_arm_wear: Part,
    pub body_wear: Part,
    pub swing: f32,
    pub sneak: bool,
    /// `heldItemRight`: 0 empty, 1 item, 3 blocking.
    pub held_right: i32,
    pub aimed_bow: bool,
    /// The last `set_angles` input (armor layers re-pose their own models with it).
    pub pose: Pose,
}

/// Inputs to `ModelBiped.setRotationAngles`.
#[derive(Clone, Copy, Default)]
pub struct Pose {
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub age: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
}

impl PlayerModel {
    pub fn new(slim: bool) -> Self {
        let t = [64., 64.];
        let (left_arm, right_arm, left_arm_wear, right_arm_wear) = if slim {
            (
                Part::new().add_box(t, [32., 48.], [-1., -2., -2.], [3, 12, 4], 0., false).at(5., 2.5, 0.),
                Part::new().add_box(t, [40., 16.], [-2., -2., -2.], [3, 12, 4], 0., false).at(-5., 2.5, 0.),
                Part::new().add_box(t, [48., 48.], [-1., -2., -2.], [3, 12, 4], 0.25, false).at(5., 2.5, 0.),
                Part::new().add_box(t, [40., 32.], [-2., -2., -2.], [3, 12, 4], 0.25, false).at(-5., 2.5, 10.),
            )
        } else {
            (
                Part::new().add_box(t, [32., 48.], [-1., -2., -2.], [4, 12, 4], 0., false).at(5., 2., 0.),
                Part::new().add_box(t, [40., 16.], [-3., -2., -2.], [4, 12, 4], 0., false).at(-5., 2., 0.),
                Part::new().add_box(t, [48., 48.], [-1., -2., -2.], [4, 12, 4], 0.25, false).at(5., 2., 0.),
                Part::new().add_box(t, [40., 32.], [-3., -2., -2.], [4, 12, 4], 0.25, false).at(-5., 2., 10.),
            )
        };
        Self {
            head: Part::new().add_box(t, [0., 0.], [-4., -8., -4.], [8, 8, 8], 0., false),
            headwear: Part::new().add_box(t, [32., 0.], [-4., -8., -4.], [8, 8, 8], 0.5, false),
            body: Part::new().add_box(t, [16., 16.], [-4., 0., -2.], [8, 12, 4], 0., false),
            right_arm,
            left_arm,
            right_leg: Part::new().add_box(t, [0., 16.], [-2., 0., -2.], [4, 12, 4], 0., false).at(-1.9, 12., 0.),
            left_leg: Part::new().add_box(t, [16., 48.], [-2., 0., -2.], [4, 12, 4], 0., false).at(1.9, 12., 0.),
            left_leg_wear: Part::new().add_box(t, [0., 48.], [-2., 0., -2.], [4, 12, 4], 0.25, false).at(1.9, 12., 0.),
            right_leg_wear: Part::new().add_box(t, [0., 32.], [-2., 0., -2.], [4, 12, 4], 0.25, false).at(-1.9, 12., 0.),
            left_arm_wear,
            right_arm_wear,
            body_wear: Part::new().add_box(t, [16., 32.], [-4., 0., -2.], [8, 12, 4], 0.25, false),
            swing: 0.,
            sneak: false,
            held_right: 0,
            aimed_bow: false,
            pose: Pose::default(),
        }
    }

    /// `ModelBiped(delta, 0, 64, 32)`: `LayerBipedArmor`'s models (1.0, leggings 0.5). The
    /// overlay parts stay empty.
    pub fn biped(delta: f32) -> Self {
        let t = [64., 32.];
        Self {
            head: Part::new().add_box(t, [0., 0.], [-4., -8., -4.], [8, 8, 8], delta, false),
            headwear: Part::new().add_box(t, [32., 0.], [-4., -8., -4.], [8, 8, 8], delta + 0.5, false),
            body: Part::new().add_box(t, [16., 16.], [-4., 0., -2.], [8, 12, 4], delta, false),
            right_arm: Part::new().add_box(t, [40., 16.], [-3., -2., -2.], [4, 12, 4], delta, false).at(-5., 2., 0.),
            left_arm: Part::new().add_box(t, [40., 16.], [-1., -2., -2.], [4, 12, 4], delta, true).at(5., 2., 0.),
            right_leg: Part::new().add_box(t, [0., 16.], [-2., 0., -2.], [4, 12, 4], delta, false).at(-1.9, 12., 0.),
            left_leg: Part::new().add_box(t, [0., 16.], [-2., 0., -2.], [4, 12, 4], delta, true).at(1.9, 12., 0.),
            left_leg_wear: Part::new(),
            right_leg_wear: Part::new(),
            left_arm_wear: Part::new(),
            right_arm_wear: Part::new(),
            body_wear: Part::new(),
            swing: 0.,
            sneak: false,
            held_right: 0,
            aimed_bow: false,
            pose: Pose::default(),
        }
    }

    /// `LayerBipedArmor.setModelPartVisible` (1 boots, 2 leggings, 3 chestplate, 4 helmet).
    pub fn show_armor_slot(&mut self, slot: usize) {
        let s = |n: &[usize]| n.contains(&slot);
        self.head.show = s(&[4]);
        self.headwear.show = s(&[4]);
        self.body.show = s(&[2, 3]);
        self.right_arm.show = s(&[3]);
        self.left_arm.show = s(&[3]);
        self.right_leg.show = s(&[1, 2]);
        self.left_leg.show = s(&[1, 2]);
    }

    /// `ModelBiped.setRotationAngles` followed by `ModelPlayer`'s overlay copies.
    pub fn set_angles(&mut self, p: Pose) {
        use std::f32::consts::PI;
        use mc_core::math::{cos, sin, sqrt_float as sqrt};
        self.pose = p;
        self.head.rot.y = p.head_yaw / (180. / PI);
        self.head.rot.x = p.head_pitch / (180. / PI);
        self.right_arm.rot.x = cos(p.limb_swing * 0.6662 + PI) * 2. * p.limb_amount * 0.5;
        self.left_arm.rot.x = cos(p.limb_swing * 0.6662) * 2. * p.limb_amount * 0.5;
        self.right_arm.rot.z = 0.;
        self.left_arm.rot.z = 0.;
        self.right_leg.rot.x = cos(p.limb_swing * 0.6662) * 1.4 * p.limb_amount;
        self.left_leg.rot.x = cos(p.limb_swing * 0.6662 + PI) * 1.4 * p.limb_amount;
        self.right_leg.rot.y = 0.;
        self.left_leg.rot.y = 0.;
        self.right_arm.rot.y = 0.;
        self.right_arm.rot.z = 0.;
        match self.held_right {
            1 => self.right_arm.rot.x = self.right_arm.rot.x * 0.5 - (PI / 10.) * self.held_right as f32,
            3 => {
                self.right_arm.rot.x = self.right_arm.rot.x * 0.5 - (PI / 10.) * self.held_right as f32;
                self.right_arm.rot.y = -0.5235988;
            }
            _ => {}
        }
        self.left_arm.rot.y = 0.;
        if self.swing > -9990. {
            let f = self.swing;
            self.body.rot.y = sin(sqrt(f) * PI * 2.) * 0.2;
            self.right_arm.pivot.z = sin(self.body.rot.y) * 5.;
            self.right_arm.pivot.x = -cos(self.body.rot.y) * 5.;
            self.left_arm.pivot.z = -sin(self.body.rot.y) * 5.;
            self.left_arm.pivot.x = cos(self.body.rot.y) * 5.;
            self.right_arm.rot.y += self.body.rot.y;
            self.left_arm.rot.y += self.body.rot.y;
            self.left_arm.rot.x += self.body.rot.y;
            let mut f = 1. - self.swing;
            f *= f;
            f *= f;
            f = 1. - f;
            let f1 = sin(f * PI);
            let f2 = sin(self.swing * PI) * -(self.head.rot.x - 0.7) * 0.75;
            self.right_arm.rot.x = (self.right_arm.rot.x as f64 - (f1 as f64 * 1.2 + f2 as f64)) as f32;
            self.right_arm.rot.y += self.body.rot.y * 2.;
            self.right_arm.rot.z += sin(self.swing * PI) * -0.4;
        }
        if self.sneak {
            self.body.rot.x = 0.5;
            self.right_arm.rot.x += 0.4;
            self.left_arm.rot.x += 0.4;
            self.right_leg.pivot.z = 4.;
            self.left_leg.pivot.z = 4.;
            self.right_leg.pivot.y = 9.;
            self.left_leg.pivot.y = 9.;
            self.head.pivot.y = 1.;
        } else {
            self.body.rot.x = 0.;
            self.right_leg.pivot.z = 0.1;
            self.left_leg.pivot.z = 0.1;
            self.right_leg.pivot.y = 12.;
            self.left_leg.pivot.y = 12.;
            self.head.pivot.y = 0.;
        }
        self.right_arm.rot.z += cos(p.age * 0.09) * 0.05 + 0.05;
        self.left_arm.rot.z -= cos(p.age * 0.09) * 0.05 + 0.05;
        self.right_arm.rot.x += sin(p.age * 0.067) * 0.05;
        self.left_arm.rot.x -= sin(p.age * 0.067) * 0.05;
        if self.aimed_bow {
            self.right_arm.rot.z = 0.;
            self.left_arm.rot.z = 0.;
            self.right_arm.rot.y = -0.1 + self.head.rot.y;
            self.left_arm.rot.y = 0.1 + self.head.rot.y + 0.4;
            self.right_arm.rot.x = -(PI / 2.) + self.head.rot.x;
            self.left_arm.rot.x = -(PI / 2.) + self.head.rot.x;
            self.right_arm.rot.z += cos(p.age * 0.09) * 0.05 + 0.05;
            self.left_arm.rot.z -= cos(p.age * 0.09) * 0.05 + 0.05;
            self.right_arm.rot.x += sin(p.age * 0.067) * 0.05;
            self.left_arm.rot.x -= sin(p.age * 0.067) * 0.05;
        }
        let head = self.head.clone();
        self.headwear.copy_angles(&head);
        let (ll, rl, la, ra, b) = (self.left_leg.clone(), self.right_leg.clone(), self.left_arm.clone(), self.right_arm.clone(), self.body.clone());
        self.left_leg_wear.copy_angles(&ll);
        self.right_leg_wear.copy_angles(&rl);
        self.left_arm_wear.copy_angles(&la);
        self.right_arm_wear.copy_angles(&ra);
        self.body_wear.copy_angles(&b);
    }

    /// `ModelPlayer.render`: biped parts, then the overlay layer.
    pub fn render(&self, m: Mat4, out: &mut impl FnMut(Mat4, &ModelQuad)) {
        let m = if self.sneak { m * Mat4::from_translation(Vec3::new(0., 0.2, 0.)) } else { m };
        for p in [&self.head, &self.body, &self.right_arm, &self.left_arm, &self.right_leg, &self.left_leg, &self.headwear] {
            p.render(m, out);
        }
        for p in [&self.left_leg_wear, &self.right_leg_wear, &self.left_arm_wear, &self.right_arm_wear, &self.body_wear] {
            p.render(m, out);
        }
    }

    /// `RenderPlayer.renderRightArm` (first person, empty hand).
    pub fn render_right_arm(&mut self, m: Mat4, out: &mut impl FnMut(Mat4, &ModelQuad)) {
        self.swing = 0.;
        self.sneak = false;
        self.set_angles(Pose::default());
        self.right_arm.render(m, out);
        self.right_arm_wear.render(m, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn box_normals_point_outward() {
        let p = Part::new().add_box([64., 64.], [0., 0.], [-4., -8., -4.], [8, 8, 8], 0., false);
        let expected = [Vec3::X, -Vec3::X, -Vec3::Y, Vec3::Y, -Vec3::Z, Vec3::Z];
        for (q, e) in p.quads.iter().zip(expected) {
            assert!((q.normal - e).length() < 1e-5, "{:?} vs {e:?}", q.normal);
        }
        let m = Part::new().add_box([64., 64.], [0., 0.], [-4., -8., -4.], [8, 8, 8], 0., true);
        for (q, e) in m.quads.iter().zip(expected) {
            // Mirroring swaps the x extremes but keeps the face slots; flipFace re-orients them.
            let e = if e.x != 0. { -e } else { e };
            assert!((q.normal - e).length() < 1e-5, "mirror {:?} vs {e:?}", q.normal);
        }
    }
}
