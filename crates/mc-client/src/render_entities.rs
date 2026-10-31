//! Per-frame entity render data.

use crate::*;

/// The entity vectors one frame's world pass draws. [`App::collect_frame_entities`] fills them in
/// one scan; `App` keeps the struct and [`Self::recycle`]s the previous frame's vectors back into
/// it, so the steady state does not allocate a fresh `Vec` per entity kind every frame.
#[derive(Default)]
pub(crate) struct FrameEntities {
    pub(crate) mobs: Vec<mc_render::mobs::MobFrame>,
    pub(crate) objects: Vec<mc_render::objects::ObjectFrame>,
    /// Other players (`RenderPlayer`).
    pub(crate) others: Vec<mc_render::avatar::AvatarFrame>,
    pub(crate) billboards: Vec<mc_render::avatar::Billboard>,
    pub(crate) fixed_items: Vec<mc_render::avatar::FixedItem>,
    /// `RenderTNTPrimed`/`RenderFallingBlock`/`RenderFireball` quads followed by the item-frame
    /// model quads (the order the two separate passes produced).
    pub(crate) world_quads: Vec<mc_render::avatar::WorldQuad>,
    /// [`Self::world_quads`]' item-frame half, appended after the entity scan so the entity-block
    /// quads stay in front of it.
    pub(crate) item_frame_quads: Vec<mc_render::avatar::WorldQuad>,
    pub(crate) dropped: Vec<mc_render::avatar::DroppedItem>,
    /// `damage_quads` then `break_anim_quads`.
    pub(crate) damage: Vec<mc_render::avatar::DamageQuad>,
}

impl FrameEntities {
    /// Empties every vector without releasing its allocation.
    pub(crate) fn clear(&mut self) {
        self.mobs.clear();
        self.objects.clear();
        self.others.clear();
        self.billboards.clear();
        self.fixed_items.clear();
        self.world_quads.clear();
        self.item_frame_quads.clear();
        self.dropped.clear();
        self.damage.clear();
    }

    /// Takes the vectors out of the frame the renderer has finished with. `AvatarFrame` is
    /// replaced every frame, so without this its allocations would be dropped and the next frame
    /// would grow the vectors from empty.
    pub(crate) fn recycle(&mut self, frame: mc_render::avatar::AvatarFrame) {
        let mc_render::avatar::AvatarFrame { mobs, objects, others, billboards, fixed_items, world_quads, dropped, damage, .. } = frame;
        self.mobs = mobs;
        self.objects = objects;
        self.others = others;
        self.billboards = billboards;
        self.fixed_items = fixed_items;
        self.world_quads = world_quads;
        self.dropped = dropped;
        self.damage = damage;
        self.clear();
    }

    /// Moves the item-frame quads behind the entity-block quads and parks their (now empty)
    /// vector on `retained`, the `App`-owned scratch: unlike the other fields it is not part of
    /// `AvatarFrame`, so it would otherwise be dropped with this struct every frame.
    pub(crate) fn append_item_frames(&mut self, retained: &mut FrameEntities) {
        self.world_quads.append(&mut self.item_frame_quads);
        std::mem::swap(&mut retained.item_frame_quads, &mut self.item_frame_quads);
    }
}

/// `Entity.getEntityBoundingBox().getAverageEdgeLength()`.
fn average_edge(width: f64, height: f64) -> f64 {
    (width * 2. + height) / 3.
}

/// `Entity.isInRangeToRenderDist` (`renderDistanceWeight` 1.0) with the squared distance:
/// `Render.shouldRender`'s first test, run before any frame value for the entity is built. The
/// comparison is the old passes' negated `>=`, so the cull keeps exactly the entities it used to.
fn in_render_range(p: mc_core::Vec3, cam: Vec3, width: f64, height: f64) -> bool {
    let range = average_edge(width, height) * 64.;
    let d2 = (p.x - cam.x as f64).powi(2) + (p.y - cam.y as f64).powi(2) + (p.z - cam.z as f64).powi(2);
    !(d2 >= range * range)
}

/// The horizontal 128-block cull `other_players` has always applied (`EntityOtherPlayerMP`),
/// kept as the old `>` so the accepted set is unchanged.
fn player_in_range(p: mc_core::Vec3, cam: Vec3) -> bool {
    let d2 = (p.x - cam.x as f64).powi(2) + (p.z - cam.z as f64).powi(2);
    !(d2 > 64.0 * 64.0 * 4.0)
}

/// `RenderTNTPrimed`/`RenderFallingBlock`: the block model's quads at `m`, tinted by `light`.
fn emit_block_quads(out: &mut Vec<mc_render::avatar::WorldQuad>, models: &BlockModels, state: mc_data::StateId, m: glam::Mat4, light: [f32; 3]) {
    let Some(entry) = models.get_state(state) else { return };
    let model = &entry.models[mc_assets::model::pick_weighted(&entry.weights, 0, 0, 0)].1;
    for q in &model.quads {
        let pos = q.pos.map(|p| m.transform_point3(Vec3::new(p[0], p[1], p[2])));
        out.push(mc_render::avatar::WorldQuad {
            pos,
            uv: q.uv,
            color: [q.shade * light[0], q.shade * light[1], q.shade * light[2]],
        });
    }
}

impl App {
    /// `Entity.getBrightnessForRender`: `World.getCombinedLight(pos(x, y + getEyeHeight(), z), 0)`
    /// as a lightmap colour. `eye` is `Entity.getEyeHeight` (`height * 0.85`, which
    /// `EntityArmorStand` overrides), not the entity's centre.
    pub(crate) fn entity_brightness(&self, pos: mc_core::Vec3, eye: f32) -> [f32; 3] {
        let p = BlockPos::new(pos.x.floor() as i32, (pos.y + eye as f64).floor() as i32, pos.z.floor() as i32);
        let (sky, block) = self.world.brightness_for_render(p);
        self.lightmap_color(sky as u8, block as u8)
    }

    /// One scan over `remote.map` for every entity the world pass draws.
    ///
    /// This replaces the old `mob_frames`/`object_frames`/`billboards`/`item_frames`/
    /// `entity_blocks`/`other_players` passes: each entity interpolates once, is distance-culled
    /// with its own `Render.shouldRender` range before anything is built, and looks up its light
    /// once. The kinds are disjoint, so each old vector receives the same entries in the same
    /// `HashMap` order; item-frame quads are appended after the entity-block quads.
    pub(crate) fn collect_frame_entities(&mut self, pt: f32) -> FrameEntities {
        let mut out = std::mem::take(&mut self.frame_entities);
        out.clear();
        let cam = self.camera.pos;
        // `EntityPlayerSP`'s interpolated position, the origin of the 128-block player cull.
        let player = self.entity.interpolated(pt);
        // `MobFrame.bright`: the full-bright overlay colour (`lightmap_color(15, 15)`).
        let bright = self.lightmap_color(15, 15);
        for r in self.remote.map.values() {
            let p = r.interpolated(pt);
            match r.kind {
                entities::Kind::Player => {
                    if !r.invisible() && player_in_range(p, player) {
                        out.others.push(self.other_player_frame(r, p, pt));
                    }
                }
                entities::Kind::Mob(kind) => {
                    if r.invisible() || !in_render_range(p, cam, r.width, r.height) {
                        continue;
                    }
                    if let Some(frame) = self.mob_frame(r, kind, p, pt, bright) {
                        out.mobs.push(frame);
                    }
                }
                entities::Kind::XpOrb => {
                    if in_render_range(p, cam, r.width, r.height) {
                        out.objects.push(self.xp_orb_frame(r, p, pt));
                    }
                }
                entities::Kind::Object(kind) => match kind {
                    // RenderArrow / ArmorStandRenderer.
                    60 | 78 => {
                        if !r.invisible() && in_render_range(p, cam, r.width, r.height) {
                            out.objects.push(if kind == 60 { self.arrow_frame(r, p) } else { self.armor_stand_frame(r, p) });
                        }
                    }
                    // RenderTNTPrimed / RenderFallingBlock / RenderFireball.
                    50 | 63 | 64 | 70 => self.push_entity_block(r, p, pt, &mut out.world_quads),
                    // RenderItemFrame; without the baked model there is nothing to draw.
                    71 => {
                        if let Some(model) = &self.models.item_frame {
                            self.push_item_frame(r, p, model, &mut out.item_frame_quads, &mut out.fixed_items);
                        }
                    }
                    // RenderSnowball / RenderPotion billboards.
                    61 | 62 | 65 | 72 | 73 | 75 | 76 => {
                        if !r.invisible() {
                            if let Some(b) = self.billboard(r, kind, p) {
                                out.billboards.push(b);
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
        for p in &self.paintings {
            if !in_render_range(p.pos, cam, 1., 1.) {
                continue;
            }
            // `EntityHanging.setSize(0.5F, 0.5F)`.
            let light = self.entity_brightness(p.pos, 0.5 * 0.85);
            out.objects.push(mc_render::objects::ObjectFrame::Painting {
                pos: Vec3::new(p.pos.x as f32, p.pos.y as f32, p.pos.z as f32),
                yaw: p.yaw,
                art: p.art,
                light,
            });
        }
        for it in &self.items {
            let p = it.interpolated(pt);
            // `EntityItem` is 0.25 tall, so `getEyeHeight` is `height * 0.85`.
            let light = self.entity_brightness(p, 0.25 * 0.85);
            let Some(item) = self.held_item(&mc_gui::ItemStack { tag: it.tag, ..mc_gui::ItemStack::new(it.stack.0, it.stack.1, it.damage) }) else { continue };
            out.dropped.push(mc_render::avatar::DroppedItem {
                pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
                age: it.age as f32 + pt,
                hover_start: it.hover_start,
                count: it.stack.1,
                item,
                light,
            });
        }
        out.damage.extend(self.damage_quads());
        self.push_break_anim_quads(pt, &mut out.damage);
        out.append_item_frames(&mut self.frame_entities);
        out
    }

    /// Every living mob in render range, with what its renderer reads. `p` was interpolated and
    /// range-culled by the caller; the light is sampled once, after the mob kind is known.
    fn mob_frame(&self, r: &entities::Remote, kind: u8, p: mc_core::Vec3, pt: f32, bright: [f32; 3]) -> Option<mc_render::mobs::MobFrame> {
        use mc_render::mobs::Mob;
        use std::f32::consts::PI;
        let lerp = |a: f32, b: f32| a + (b - a) * pt;
        let a = &r.anim;
        let flags = r.meta_byte(16);
        let mob = match kind {
            50 => Mob::Creeper { flash: (a.last_active as f32 + (a.since_ignited - a.last_active) as f32 * pt) / 28. },
            51 => Mob::Skeleton { wither: r.meta_byte(13) == 1 },
            52 | 59 => Mob::Spider { cave: kind == 59 },
            53 => Mob::Giant,
            54 => Mob::Zombie { villager: r.meta_byte(13) == 1 },
            55 => Mob::Slime { size: flags as f32, squish: lerp(a.prev_squish, a.squish) },
            56 => Mob::Ghast { shooting: flags != 0 },
            57 => Mob::PigZombie,
            58 => Mob::Enderman { carrying: r.meta_short(16) != 0, screaming: r.meta_byte(18) > 0 },
            60 => Mob::Silverfish,
            61 => Mob::Blaze,
            62 => Mob::MagmaCube { size: flags as f32, squish: lerp(a.prev_squish, a.squish) },
            65 => Mob::Bat { hanging: flags & 1 != 0 },
            66 => Mob::Witch { holding: r.held_stack.is_some() },
            67 => Mob::Endermite,
            90 => Mob::Pig { saddle: flags & 1 != 0 },
            91 => Mob::Sheep { color: (flags & 15) as u8, sheared: flags & 16 != 0 },
            92 | 96 => Mob::Cow { mooshroom: kind == 96 },
            93 => Mob::Chicken { wing: (mc_core::math::sin(lerp(a.prev_wing, a.wing)) + 1.) * lerp(a.prev_dest_pos, a.dest_pos) },
            94 => Mob::Squid { pitch: lerp(a.prev_squid_pitch, a.squid_pitch), yaw: lerp(a.prev_squid_yaw, a.squid_yaw), tentacle: lerp(a.last_tentacle, a.tentacle) },
            95 => {
                let (angry, tamed) = (flags & 2 != 0, flags & 4 != 0);
                // getTailRotation; the collar byte is a dye damage value.
                let tail = if angry {
                    1.5393804
                } else if tamed {
                    (0.55 - (20. - r.meta_float(18).unwrap_or(20.)) * 0.02) * PI
                } else {
                    PI / 5.
                };
                Mob::Wolf { tail, sitting: flags & 1 != 0, angry, tamed, collar: 15 - (r.meta_byte(20) & 15) as u8 }
            }
            97 => Mob::SnowMan,
            98 => {
                let state = if r.sneaking {
                    0
                } else if r.sprinting {
                    2
                } else if flags & 1 != 0 {
                    3
                } else {
                    1
                };
                Mob::Ocelot { skin: r.meta_byte(18) as u8, state, tamed: flags & 4 != 0 }
            }
            99 => Mob::IronGolem { attack: if a.attack_timer > 0 { a.attack_timer as f32 - pt } else { 0. }, rose: a.hold_rose },
            101 => Mob::Rabbit { kind: r.meta_byte(18) as u8 },
            120 => Mob::Villager { profession: r.meta_int(16) },
            _ => return None,
        };
        let biped = matches!(kind, 51 | 53 | 54 | 57);
        // `Entity.getEyeHeight` is `height * 0.85` for every living mob.
        let light = self.entity_brightness(p, r.height as f32 * 0.85);
        let swing = {
            let mut f = r.swing - r.prev_swing;
            if f < 0. {
                f += 1.;
            }
            r.prev_swing + f * pt
        };
        Some(mc_render::mobs::MobFrame {
            mob,
            id: r.id,
            pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
            body_yaw: lerp_angle(r.prev_body_yaw, r.body_yaw, pt),
            head_yaw: lerp_angle(r.prev_head_yaw, r.head_yaw, pt),
            pitch: lerp(r.prev_pitch, r.pitch),
            limb_swing: r.limb_swing - r.limb_amount * (1. - pt),
            limb_amount: lerp(r.prev_limb_amount, r.limb_amount),
            age: r.ticks_existed as f32 + pt,
            ticks: r.ticks_existed,
            swing,
            child: r.is_child(),
            hurt: r.hurt_time > 0,
            death: if a.death_time > 0 { a.death_time as f32 + pt } else { 0. },
            light,
            bright,
            held: if biped { r.held_stack.and_then(|s| self.held_item_using(&s, 0)) } else { None },
            armor: if biped { r.armor.map(|s| s.and_then(|s| tooltip::armor_piece(&self.catalogue, &s))) } else { [None; 4] },
        })
    }

    /// `RenderArrow` (the client does not simulate the arrow's own collision, so the shake timer
    /// is not reproduced).
    fn arrow_frame(&self, r: &entities::Remote, p: mc_core::Vec3) -> mc_render::objects::ObjectFrame {
        mc_render::objects::ObjectFrame::Arrow {
            pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
            yaw: r.yaw,
            pitch: r.pitch,
            shake: 0.,
            light: self.entity_brightness(p, (r.height * 0.85) as f32),
        }
    }

    /// ArmorStandRenderer: the wooden stand. Armour/held layers are not drawn yet.
    fn armor_stand_frame(&self, r: &entities::Remote, p: mc_core::Vec3) -> mc_render::objects::ObjectFrame {
        let flags = r.meta_byte(10) as u8;
        let body_rot = r.meta_rot(12).unwrap_or((0., 0., 0.));
        let eye = if flags & 1 != 0 { r.height * 0.5 } else { r.height * 0.9 };
        mc_render::objects::ObjectFrame::ArmorStand {
            pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
            yaw: r.yaw,
            head_yaw: r.head_yaw,
            pitch: r.pitch,
            flags,
            light: self.entity_brightness(p, eye as f32),
            body_rot,
        }
    }

    /// `EntityXPOrb.getTextureByXP`: `xpValue` rode in the spawn packet.
    fn xp_orb_frame(&self, r: &entities::Remote, p: mc_core::Vec3, pt: f32) -> mc_render::objects::ObjectFrame {
        // `EntityXPOrb.getBrightnessForRender` adds `0.5F * 15 * 16` units to the block-light
        // half (clamped to 240).
        let eye = r.height * 0.85;
        let bp = BlockPos::new(p.x.floor() as i32, (p.y + eye).floor() as i32, p.z.floor() as i32);
        let (sky, block) = self.world.brightness_for_render(bp);
        let light = self.lightmap_units(sky as f32 * 16.0, (block as f32 * 16.0 + 120.0).min(240.0));
        mc_render::objects::ObjectFrame::XpOrb {
            pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
            icon: mc_render::objects::xp_texture(r.data),
            color: mc_render::objects::xp_color(r.ticks_existed as f32 + pt),
            light,
        }
    }

    /// `LayerCustomHead`: a skull stack in the helmet slot.
    pub(crate) fn skull_head(&self, s: &mc_gui::ItemStack) -> Option<mc_render::avatar::SkullHead> {
        if self.catalogue.items[s.item].id != 397 {
            return None;
        }
        // renderSkull's switch: unknown types draw as a skeleton skull.
        let kind = if (0..=4).contains(&s.damage) { s.damage as u8 } else { 0 };
        let profile = (kind == 3).then(|| s.tag.and_then(|t| skins::item_skull_profile(&t.get()))).flatten();
        let (skin, alex) = self.skins.skull_skin(profile.as_ref());
        Some(mc_render::avatar::SkullHead { kind, skin, alex })
    }

    /// `RenderSnowball`/`RenderPotion` billboards: snowballs, eggs, ender pearls, eyes of
    /// ender, experience bottles, fireworks and splash potions.
    fn billboard(&self, r: &entities::Remote, kind: i8, p: mc_core::Vec3) -> Option<mc_render::avatar::Billboard> {
        let item_id = match kind {
            61 => 332,  // snowball
            62 => 344,  // egg
            65 => 368,  // ender pearl
            72 => 381,  // eye of ender
            75 => 384,  // experience bottle
            76 => 401,  // fireworks
            73 => 373,  // splash potion
            _ => return None,
        };
        let damage = if kind == 73 { r.data as i16 } else { 0 };
        let stack = self.catalogue.index_of(item_id, damage.max(0) as u16).map(|i| mc_gui::ItemStack::new(i, 1, damage))?;
        let item = self.held_item(&stack)?;
        let light = self.entity_brightness(p, r.height as f32 * 0.85);
        Some(mc_render::avatar::Billboard { pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32), item, light })
    }

    /// `RenderItemFrame`: the frame model and the displayed stack with the FIXED transform.
    fn push_item_frame(
        &self,
        r: &entities::Remote,
        p: mc_core::Vec3,
        frame_model: &mc_assets::model::BakedModel,
        quads: &mut Vec<mc_render::avatar::WorldQuad>,
        items: &mut Vec<mc_render::avatar::FixedItem>,
    ) {
        // EntityHanging placed the entity so that floor(pos) is the hanging block.
        let block = BlockPos::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        // `getBrightnessForRender` at the frame's eye height (`height * 0.85`, 0.5 tall).
        let light = self.entity_brightness(p, r.height as f32 * 0.85);
        let base = glam::Mat4::from_translation(Vec3::new(block.x as f32 + 0.5, block.y as f32 + 0.5, block.z as f32 + 0.5))
            * glam::Mat4::from_rotation_y((180. - r.yaw).to_radians())
            * glam::Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5));
        push_model_quads(quads, base, frame_model, light);
        let stack = match r.meta.get(&8) {
            Some(mc_protocol::MetaValue::Item(item)) => net::slot_stack(&self.catalogue, item),
            _ => None,
        };
        if let Some(stack) = stack {
            if let Some(item) = self.held_item(&stack) {
                let rot = r.meta_byte(9) as f32 * 45.;
                let m = base
                    * glam::Mat4::from_translation(Vec3::new(0., 0., 0.4375))
                    * glam::Mat4::from_rotation_z(rot.to_radians())
                    * glam::Mat4::from_scale(Vec3::splat(0.5));
                items.push(mc_render::avatar::FixedItem { m, item, light });
            }
        }
    }

    /// `RenderTNTPrimed`, `RenderFallingBlock` and `RenderFireball`: block models in entity space.
    fn push_entity_block(&self, r: &entities::Remote, p: mc_core::Vec3, pt: f32, out: &mut Vec<mc_render::avatar::WorldQuad>) {
        // `Entity.getBrightnessForRender`, only for the kinds that produce quads.
        let light = || self.entity_brightness(p, r.height as f32 * 0.85);
        match r.kind {
            // RenderTNTPrimed: translate to the block centre, swell near the end, then
            // R_y(90°) . T(-0.5, -0.5, 0.5) (renderModelBrightness's item orientation).
            entities::Kind::Object(50) => {
                let fuse = r.fuse as f32 - pt + 1.;
                let swell = if fuse < 10. { 1. + ((1. - fuse / 10.).clamp(0., 1.).powi(2)).powi(2) * 0.3 } else { 1. };
                let Some(tnt) = mc_data::Block::by_id(46).and_then(|b| b.state_from_meta(0)) else { return };
                let base = glam::Mat4::from_translation(Vec3::new(p.x as f32, p.y as f32 + 0.5, p.z as f32))
                    * glam::Mat4::from_scale(Vec3::splat(swell))
                    * glam::Mat4::from_translation(Vec3::new(-0.5, -0.5, 0.5))
                    * glam::Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
                emit_block_quads(out, &self.models, tnt, base, light());
            }
            // RenderFallingBlock: the model at the fractional position inside its block.
            entities::Kind::Object(70) => {
                let state = mc_data::StateId::try_from_network(r.data & 0xFFFF).unwrap_or(mc_data::StateId::AIR);
                if state == mc_data::StateId::AIR {
                    return;
                }
                let fx = p.x - p.x.floor();
                let fy = p.y - p.y.floor();
                let fz = p.z - p.z.floor();
                let m = glam::Mat4::from_translation(Vec3::new((fx - 0.5) as f32, fy as f32, (fz - 0.5) as f32));
                emit_block_quads(out, &self.models, state, m, light());
            }
            // RenderFireball: the fire-charge particle icon, camera-facing at 2.0 (large)
            // or 0.5 (small) scale, -0.5..0.5 by -0.25..0.75.
            entities::Kind::Object(63) | entities::Kind::Object(64) => {
                let scale = if matches!(r.kind, entities::Kind::Object(63)) { 2.0 } else { 0.5 };
                let s = self.models.atlas.sprite("items/fireball");
                let m = glam::Mat4::from_translation(Vec3::new(p.x as f32, p.y as f32, p.z as f32))
                    * glam::Mat4::from_scale(Vec3::splat(scale))
                    * glam::Mat4::from_rotation_y((180. - self.camera.yaw).to_radians())
                    * glam::Mat4::from_rotation_x((-self.camera.pitch).to_radians());
                let uv = [[s.min_u, s.max_v], [s.max_u, s.max_v], [s.max_u, s.min_v], [s.min_u, s.min_v]];
                let corners = [Vec3::new(-0.5, -0.25, 0.), Vec3::new(0.5, -0.25, 0.), Vec3::new(0.5, 0.75, 0.), Vec3::new(-0.5, 0.75, 0.)];
                out.push(mc_render::avatar::WorldQuad { pos: corners.map(|c| m.transform_point3(c)), uv, color: light() });
            }
            _ => {}
        }
    }

    /// `WorldClient.destroyBlockInWorldPartially`: the destroy-stage overlay for other
    /// players' blocks, on top of the local one. Appends to `out` so the frame can reuse it.
    pub(crate) fn push_break_anim_quads(&self, pt: f32, out: &mut Vec<mc_render::avatar::DamageQuad>) {
        let _ = pt;
        for (pos, (_, stage)) in &self.break_anims {
            let atlas = &self.models.atlas;
            // `destroy_stage_0..9` (`WorldClient.destroyBlockInWorldPartially`).
            const CRACK: [&str; 10] = [
                "blocks/destroy_stage_0", "blocks/destroy_stage_1", "blocks/destroy_stage_2", "blocks/destroy_stage_3",
                "blocks/destroy_stage_4", "blocks/destroy_stage_5", "blocks/destroy_stage_6", "blocks/destroy_stage_7",
                "blocks/destroy_stage_8", "blocks/destroy_stage_9",
            ];
            let crack = atlas.sprite(CRACK[(*stage).min(9) as usize]);
            let stored = self.world.state_at(pos.x, pos.y, pos.z);
            let actual = stored.actual_state(&self.world, *pos);
            let Some(entry) = self.models.get_state(actual) else { continue };
            if entry.models.is_empty() {
                continue;
            }
            let model = &entry.models[mc_assets::model::pick_weighted(&entry.weights, pos.x, pos.y, pos.z)].1;
            let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
            out.extend(model.quads.iter().filter_map(|q| {
                let c = [0.25f32, 0.75].map(|t| q.uv[0][0] * t + q.uv[2][0] * (1. - t));
                let own = atlas.sprite_at((c[0] + c[1]) / 2., (q.uv[0][1] + q.uv[2][1]) / 2.)?;
                let uv = q.uv.map(|[u, v]| {
                    let fu = (u - own.min_u) / (own.max_u - own.min_u);
                    let fv = (v - own.min_v) / (own.max_v - own.min_v);
                    [crack.min_u + fu * (crack.max_u - crack.min_u), crack.min_v + fv * (crack.max_v - crack.min_v)]
                });
                Some(mc_render::avatar::DamageQuad { pos: q.pos.map(|p| origin + Vec3::from(p)), uv })
            }));
        }
    }

    /// `RenderPlayer`'s body fields for another player. `p` was interpolated and the horizontal
    /// 128-block cull already passed.
    fn other_player_frame(&self, r: &entities::Remote, p: mc_core::Vec3, pt: f32) -> mc_render::avatar::AvatarFrame {
        // `EntityPlayer.getEyeHeight`: 1.62, minus 0.08 while sneaking.
        let light = self.entity_brightness(p, if r.sneaking { 1.62 - 0.08 } else { 1.62 });
        let lerp = |a: f32, b: f32| a + (b - a) * pt;
        let swing = {
            let mut f = r.swing - r.prev_swing;
            if f < 0. {
                f += 1.;
            }
            r.prev_swing + f * pt
        };
        mc_render::avatar::AvatarFrame {
            pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
            body_yaw: lerp(r.prev_body_yaw, r.body_yaw),
            head_yaw: lerp(r.prev_head_yaw, r.head_yaw),
            pitch: lerp(r.prev_pitch, r.pitch),
            limb_swing: r.limb_swing - r.limb_amount * (1. - pt),
            limb_amount: lerp(r.prev_limb_amount, r.limb_amount).min(1.),
            age: r.ticks_existed as f32 + pt,
            swing,
            sneaking: r.sneaking,
            perspective: 1,
            held: r.held_stack.and_then(|s| self.held_item_using(&s, r.use_count)),
            light,
            use_anim: r.held.filter(|_| r.use_count > 0).map(|(i, _)| {
                let (a, max) = item_use::action(self.catalogue.items[i].id);
                mc_render::avatar::UseAnim { action: a.index(), left: r.use_count as f32 - pt + 1., max: max as f32 }
            }),
            skin: r.uuid.and_then(|u| self.skins.loaded.get(&u)).map(|s| s.0),
            armor: r.armor.map(|s| s.and_then(|s| tooltip::armor_piece(&self.catalogue, &s))),
            head_skull: r.armor[3].and_then(|s| self.skull_head(&s)),
            slim: r.uuid.is_some_and(|u| self.skins.loaded.get(&u).map_or(skins::default_slim(u), |s| s.1)),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Entity.isInRangeToRenderDist`'s `distance < (averageEdge * 64)²` rule, which is what the
    /// merged scan culls with before it builds a frame.
    #[test]
    fn render_range_matches_vanilla_average_edge() {
        let cam = Vec3::new(0., 0., 0.);
        let p = |x: f64| mc_core::Vec3::new(x, 0., 0.);
        // A player-sized box (0.6 × 1.8) has an average edge of 1.0, so 64 blocks.
        assert!(in_render_range(p(63.9), cam, 0.6, 1.8));
        assert!(!in_render_range(p(64.1), cam, 0.6, 1.8));
        // A throwable (0.25 × 0.25) is 16 blocks.
        assert!(in_render_range(p(15.9), cam, 0.25, 0.25));
        assert!(!in_render_range(p(16.1), cam, 0.25, 0.25));
    }

    #[test]
    fn players_keep_the_128_block_horizontal_cull() {
        let cam = Vec3::new(0., 0., 0.);
        assert!(player_in_range(mc_core::Vec3::new(0., 100., 127.9), cam));
        assert!(!player_in_range(mc_core::Vec3::new(0., 0., 128.1), cam));
    }

    /// Recycling keeps the previous frame's vectors (and so their capacity) but starts empty.
    #[test]
    fn recycled_entity_buffers_keep_their_capacity() {
        let mut frame = mc_render::avatar::AvatarFrame::default();
        frame.mobs.reserve(8);
        frame.others.reserve(3);
        frame.dropped.reserve(5);
        let mobs_capacity = frame.mobs.capacity();
        let others_capacity = frame.others.capacity();
        let dropped_capacity = frame.dropped.capacity();
        let mut scratch = FrameEntities::default();
        scratch.recycle(frame);
        assert_eq!(scratch.mobs.capacity(), mobs_capacity);
        assert_eq!(scratch.others.capacity(), others_capacity);
        assert_eq!(scratch.dropped.capacity(), dropped_capacity);
        assert!(scratch.mobs.is_empty() && scratch.others.is_empty() && scratch.dropped.is_empty());
        assert!(scratch.world_quads.is_empty() && scratch.item_frame_quads.is_empty());
    }

    /// Item-frame quads are appended behind the entity-block quads, but their vector allocation
    /// stays on the `App`-owned scratch because `AvatarFrame` has no field for it.
    #[test]
    fn item_frame_quads_are_appended_and_their_buffer_parked() {
        let mut retained = FrameEntities::default();
        retained.item_frame_quads.reserve(4);
        let mut out = std::mem::take(&mut retained);
        out.item_frame_quads.push(mc_render::avatar::WorldQuad {
            pos: [Vec3::ZERO; 4],
            uv: [[0.; 2]; 4],
            color: [0.; 3],
        });
        let capacity = out.item_frame_quads.capacity();
        out.append_item_frames(&mut retained);
        assert_eq!(out.world_quads.len(), 1);
        assert!(out.item_frame_quads.is_empty());
        assert_eq!(retained.item_frame_quads.capacity(), capacity);
    }
}
