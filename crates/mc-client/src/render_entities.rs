//! Per-frame entity render data.

use crate::*;

impl App {
    /// Every living mob in render range (`RenderManager.shouldRender`: within 64 blocks times
    /// the box's average edge), with what its renderer reads.
    pub(crate) fn mob_frames(&self, pt: f32) -> Vec<mc_render::mobs::MobFrame> {
        use mc_render::mobs::Mob;
        use std::f32::consts::PI;
        let cam = self.camera.pos;
        let bright = self.lightmap_color(15, 15);
        self.remote
            .map
            .values()
            .filter_map(|r| {
                let entities::Kind::Mob(kind) = r.kind else { return None };
                if r.invisible() {
                    return None;
                }
                let p = r.interpolated(pt);
                let range = (r.width * 2. + r.height) / 3. * 64.;
                let d2 = (p.x - cam.x as f64).powi(2) + (p.y - cam.y as f64).powi(2) + (p.z - cam.z as f64).powi(2);
                if d2 >= range * range {
                    return None;
                }
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
                let (sky, block) = self.world.light(p.x.floor() as i32, (p.y + r.height * 0.85).floor() as i32, p.z.floor() as i32);
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
                    light: self.lightmap_color(sky, block),
                    bright,
                    held: if biped { r.held_stack.and_then(|s| self.held_item_using(&s, 0)) } else { None },
                    armor: if biped { r.armor.map(|s| s.and_then(|s| tooltip::armor_piece(&self.catalogue, &s))) } else { [None; 4] },
                })
            })
            .collect()
    }

    /// Non-living entity frames: `RenderArrow`, `RenderXPOrb`, `ArmorStandRenderer`,
    /// `RenderPainting`. `RenderManager.shouldRender` range for each.
    pub(crate) fn object_frames(&self, pt: f32) -> Vec<mc_render::objects::ObjectFrame> {
        use mc_render::objects::ObjectFrame;
        let cam = self.camera.pos;
        let mut out = Vec::new();
        let range_ok = |p: mc_core::Vec3, w: f64, h: f64| {
            let range = (w * 2. + h) / 3. * 64.;
            (p.x - cam.x as f64).powi(2) + (p.y - cam.y as f64).powi(2) + (p.z - cam.z as f64).powi(2) < range * range
        };
        for r in self.remote.map.values() {
            let p = r.interpolated(pt);
            if !range_ok(p, r.width, r.height) {
                continue;
            }
            let (sky, block) = self.world.light(p.x.floor() as i32, (p.y + r.height * 0.5).floor() as i32, p.z.floor() as i32);
            let light = self.lightmap_color(sky, block);
            let pos = Vec3::new(p.x as f32, p.y as f32, p.z as f32);
            match r.kind {
                // RenderArrow (the client does not simulate the arrow's own collision, so the
                // shake timer is not reproduced).
                entities::Kind::Object(60) if !r.invisible() => {
                    out.push(ObjectFrame::Arrow { pos, yaw: r.yaw, pitch: r.pitch, shake: 0., light });
                }
                // ArmorStandRenderer: the wooden stand. Armour/held layers are not drawn yet.
                entities::Kind::Object(78) if !r.invisible() => {
                    let flags = r.meta_byte(10) as u8;
                    let body_rot = r.meta_rot(12).unwrap_or((0., 0., 0.));
                    out.push(ObjectFrame::ArmorStand { pos, yaw: r.yaw, head_yaw: r.head_yaw, pitch: r.pitch, flags, light, body_rot });
                }
                // EntityXPOrb.getTextureByXP: `xpValue` rode in the spawn packet.
                entities::Kind::XpOrb => {
                    out.push(ObjectFrame::XpOrb {
                        pos,
                        icon: mc_render::objects::xp_texture(r.data),
                        color: mc_render::objects::xp_color(r.ticks_existed as f32 + pt),
                        light,
                    });
                }
                _ => {}
            }
        }
        for p in &self.paintings {
            if !range_ok(p.pos, 1., 1.) {
                continue;
            }
            let (sky, block) = self.world.light(p.pos.x.floor() as i32, p.pos.y.floor() as i32, p.pos.z.floor() as i32);
            out.push(ObjectFrame::Painting { pos: Vec3::new(p.pos.x as f32, p.pos.y as f32, p.pos.z as f32), yaw: p.yaw, art: p.art, light: self.lightmap_color(sky, block) });
        }
        out
    }

    /// `RenderSnowball`/`RenderPotion` billboards: snowballs, eggs, ender pearls, eyes of
    /// ender, experience bottles, fireworks and splash potions.
    pub(crate) fn billboards(&self, pt: f32) -> Vec<mc_render::avatar::Billboard> {
        let mut out = Vec::new();
        for r in self.remote.map.values() {
            let entities::Kind::Object(kind) = r.kind else { continue };
            let item_id = match kind {
                61 => 332,  // snowball
                62 => 344,  // egg
                65 => 368,  // ender pearl
                72 => 381,  // eye of ender
                75 => 384,  // experience bottle
                76 => 401,  // fireworks
                73 => 373,  // splash potion
                _ => continue,
            };
            if r.invisible() {
                continue;
            }
            let p = r.interpolated(pt);
            let (sky, block) = self.world.light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let damage = if kind == 73 { r.data as i16 } else { 0 };
            let Some(stack) = self.catalogue.index_of(item_id, damage.max(0) as u16).map(|i| mc_gui::ItemStack::new(i, 1, damage)) else { continue };
            let Some(item) = self.held_item(&stack) else { continue };
            out.push(mc_render::avatar::Billboard { pos: Vec3::new(p.x as f32, p.y as f32, p.z as f32), item, light: self.lightmap_color(sky, block) });
        }
        out
    }

    /// `RenderItemFrame`: the frame model and the displayed stack with the FIXED transform.
    pub(crate) fn item_frames(&self, pt: f32) -> (Vec<mc_render::avatar::WorldQuad>, Vec<mc_render::avatar::FixedItem>) {
        use mc_render::avatar::FixedItem;
        let mut quads = Vec::new();
        let mut items = Vec::new();
        let Some(frame_model) = &self.models.item_frame else { return (quads, items) };
        for r in self.remote.map.values() {
            if r.kind != entities::Kind::Object(71) {
                continue;
            }
            let p = r.interpolated(pt);
            // EntityHanging placed the entity so that floor(pos) is the hanging block.
            let block = BlockPos::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let (sky, block_light) = self.world.light(block.x, block.y, block.z);
            let light = self.lightmap_color(sky, block_light);
            let base = glam::Mat4::from_translation(Vec3::new(block.x as f32 + 0.5, block.y as f32 + 0.5, block.z as f32 + 0.5))
                * glam::Mat4::from_rotation_y((180. - r.yaw).to_radians())
                * glam::Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5));
            push_model_quads(&mut quads, base, frame_model, light);
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
                    items.push(FixedItem { m, item, light });
                }
            }
        }
        (quads, items)
    }

    /// `RenderTNTPrimed` and `RenderFallingBlock`: block models in entity space.
    pub(crate) fn entity_blocks(&self, pt: f32) -> Vec<mc_render::avatar::WorldQuad> {
        use mc_render::avatar::WorldQuad;
        let mut out = Vec::new();
        fn emit(out: &mut Vec<WorldQuad>, models: &BlockModels, state: mc_data::StateId, m: glam::Mat4, light: [f32; 3]) {
            let Some(entry) = models.get_state(state) else { return };
            let model = &entry.models[mc_assets::model::pick_weighted(&entry.weights, 0, 0, 0)].1;
            for q in &model.quads {
                let pos = q.pos.map(|p| m.transform_point3(Vec3::new(p[0], p[1], p[2])));
                out.push(WorldQuad {
                    pos,
                    uv: q.uv,
                    color: [q.shade * light[0], q.shade * light[1], q.shade * light[2]],
                });
            }
        }
        for r in self.remote.map.values() {
            let p = r.interpolated(pt);
            let (sky, block_light) = self.world.light(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
            let light = self.lightmap_color(sky, block_light);
            match r.kind {
                // RenderTNTPrimed: translate to the block centre, swell near the end, then
                // R_y(90°) . T(-0.5, -0.5, 0.5) (renderModelBrightness's item orientation).
                entities::Kind::Object(50) => {
                    let fuse = r.fuse as f32 - pt + 1.;
                    let swell = if fuse < 10. { 1. + ((1. - fuse / 10.).clamp(0., 1.).powi(2)).powi(2) * 0.3 } else { 1. };
                    let Some(tnt) = mc_data::Block::by_id(46).and_then(|b| b.state_from_meta(0)) else { continue };
                    let base = glam::Mat4::from_translation(Vec3::new(p.x as f32, p.y as f32 + 0.5, p.z as f32))
                        * glam::Mat4::from_scale(Vec3::splat(swell))
                        * glam::Mat4::from_translation(Vec3::new(-0.5, -0.5, 0.5))
                        * glam::Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
                    emit(&mut out, &self.models, tnt, base, light);
                }
                // RenderFallingBlock: the model at the fractional position inside its block.
                entities::Kind::Object(70) => {
                    let state = mc_data::StateId::try_from_network(r.data & 0xFFFF).unwrap_or(mc_data::StateId::AIR);
                    if state == mc_data::StateId::AIR {
                        continue;
                    }
                    let fx = p.x - p.x.floor();
                    let fy = p.y - p.y.floor();
                    let fz = p.z - p.z.floor();
                    let m = glam::Mat4::from_translation(Vec3::new((fx - 0.5) as f32, fy as f32, (fz - 0.5) as f32));
                    emit(&mut out, &self.models, state, m, light);
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
                    out.push(WorldQuad { pos: corners.map(|c| m.transform_point3(c)), uv, color: light });
                }
                _ => {}
            }
        }
        out
    }

    /// `WorldClient.destroyBlockInWorldPartially`: the destroy-stage overlay for other
    /// players' blocks, on top of the local one.
    pub(crate) fn break_anim_quads(&self, pt: f32) -> Vec<mc_render::avatar::DamageQuad> {
        let mut out = Vec::new();
        let _ = pt;
        for (pos, (_, stage)) in &self.break_anims {
            let atlas = &self.models.atlas;
            let crack = atlas.sprite(&format!("blocks/destroy_stage_{}", (*stage).min(9)));
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
        out
    }

    pub(crate) fn other_players(&self, pt: f32) -> Vec<mc_render::avatar::AvatarFrame> {
        let cam = self.entity.interpolated(pt);
        self.remote
            .map
            .values()
            .filter(|r| r.kind == entities::Kind::Player)
            .filter(|r| !r.invisible())
            .filter_map(|r| {
                let p = r.interpolated(pt);
                let d2 = (p.x - cam.x as f64).powi(2) + (p.z - cam.z as f64).powi(2);
                if d2 > 64.0 * 64.0 * 4.0 {
                    return None;
                }
                let (sky, block) = self.world.light(p.x.floor() as i32, (p.y + 1.62).floor() as i32, p.z.floor() as i32);
                let lerp = |a: f32, b: f32| a + (b - a) * pt;
                let swing = {
                    let mut f = r.swing - r.prev_swing;
                    if f < 0. {
                        f += 1.;
                    }
                    r.prev_swing + f * pt
                };
                Some(mc_render::avatar::AvatarFrame {
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
                    light: self.lightmap_color(sky, block),
                    use_anim: r.held.filter(|_| r.use_count > 0).map(|(i, _)| {
                        let (a, max) = item_use::action(self.catalogue.items[i].id);
                        mc_render::avatar::UseAnim { action: a.index(), left: r.use_count as f32 - pt + 1., max: max as f32 }
                    }),
                    skin: r.uuid.and_then(|u| self.skins.loaded.get(&u)).map(|s| s.0),
                    armor: r.armor.map(|s| s.and_then(|s| tooltip::armor_piece(&self.catalogue, &s))),
                    slim: r.uuid.is_some_and(|u| self.skins.loaded.get(&u).map_or(skins::default_slim(u), |s| s.1)),
                    ..Default::default()
                })
            })
            .collect()
    }

}
