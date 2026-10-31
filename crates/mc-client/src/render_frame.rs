//! View setup, HUD/GUI building and frame drawing.

use crate::*;

/// One projected name tag: its clip-space `w`, screen position, the text's byte range in
/// [`TAGS`]' text buffer and whether it is the faint sneaking label.
struct NameTag {
    w: f32,
    x: f32,
    y: f32,
    text: std::ops::Range<usize>,
    faint: bool,
}

/// The text of one name tag, before it is written into the shared buffer
/// (`EntityLivingBase.getDisplayName().getFormattedText()`).
enum TagText<'a> {
    /// `ScorePlayerTeam.formatPlayerName`: prefix + name + suffix.
    Team(&'a str),
    /// A custom name, or the entity's localized default name.
    Plain(&'a str),
}

thread_local! {
    /// `App::name_tags`' scratch, kept across frames so the steady state allocates nothing:
    /// the projected tags and the text they share. Name text is only ever borrowed from the
    /// entity/scoreboard or copied here, exactly as one `String` per label before.
    static TAGS: std::cell::RefCell<(Vec<NameTag>, String)> = const { std::cell::RefCell::new((Vec::new(), String::new())) };
}

/// `I18n.format(key)` without the `String` clone: the key itself when missing, like vanilla.
fn lang_name<'a>(lang: &'a mc_gui::lang::Lang, key: &'a str) -> &'a str {
    lang.table().get(key).map_or(key, |s| s.as_str())
}

/// Whether a label whose box is `half_w`×`half_h` GUI pixels around `(x, y)` can still touch a
/// `w`×`h` viewport. Used to drop a name tag before its text is built.
fn label_in_view(x: f32, y: f32, half_w: f32, half_h: f32, w: f32, h: f32) -> bool {
    x + half_w >= 0. && x - half_w <= w && y + half_h >= 0. && y - half_h <= h
}

/// Whether `Hud::render` draws its scoreboard/tab-list branch (`!hide_gui || chat_open`) with the
/// HUD visible at all. Building the views outside this gate is per-frame list sorting and
/// formatting that is then thrown away (F1 with no chat open).
fn text_panels_visible(hud_visible: bool, hide_gui: bool, chat_open: bool) -> bool {
    hud_visible && (!hide_gui || chat_open)
}

impl App {
    /// Lightmap color at the player's eyes (`getBrightnessForRender`).
    pub(crate) fn light_at_eyes(&self) -> [f32; 3] {
        let e = &self.entity;
        let (x, y, z) = (e.pos.x.floor() as i32, (e.pos.y + e.eye_height() as f64).floor() as i32, e.pos.z.floor() as i32);
        let (sky, block) = self.world.brightness_for_render(BlockPos::new(x, y, z));
        self.lightmap_color(sky as u8, block as u8)
    }

    pub(crate) fn lightmap_color(&self, sky: u8, block: u8) -> [f32; 3] {
        self.lightmap_units(sky as f32 * 16.0, block as f32 * 16.0)
    }

    /// A lightmap sample from packed-light units (`0..240` each, see
    /// [`mc_render::lightmap::sample`]).
    pub(crate) fn lightmap_units(&self, sky: f32, block: f32) -> [f32; 3] {
        mc_render::lightmap::sample(&self.lightmap, sky, block)
    }

    /// `ActiveRenderInfo.getBlockAtEntityViewpoint`: the block at the camera viewpoint. A point
    /// above a liquid's rendered surface (`BlockLiquid.getLiquidHeightPercent`) sees the block
    /// above the liquid instead.
    pub(crate) fn block_at_entity_viewpoint(&self, p: Vec3) -> &'static mc_data::Block {
        let (x, y, z) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        let state = self.world.state_at(x, y, z);
        let block = state.block();
        if !block.material.props().liquid {
            return block;
        }
        let meta = state.meta() as i32;
        let meta = if meta >= 8 { 0 } else { meta };
        let f = (meta + 1) as f32 / 9. - 0.11111111;
        if p.y as f64 >= ((y + 1) as f32 - f) as f64 {
            self.world.state_at(x, y + 1, z).block()
        } else {
            block
        }
    }

    /// `RenderItem.renderItemModelForEntity`: a drawn bow shows its pulling model.
    pub(crate) fn held_item_using(&self, stack: &mc_gui::ItemStack, use_count: i32) -> Option<mc_render::avatar::HeldItem> {
        let def = &self.catalogue.items[stack.item];
        if def.id == 261 && use_count > 0 {
            let i = 72000 - use_count;
            let name = if i >= 18 {
                "bow_pulling_2"
            } else if i > 13 {
                "bow_pulling_1"
            } else {
                "bow_pulling_0"
            };
            if let Some(model) = self.models.items.by_name(name) {
                let tints = std::array::from_fn(|i| mc_render::items::item_color(def.id, def.meta, i as i32, &self.models.colors));
                let glint = tooltip::has_effect(&self.catalogue, stack);
                return Some(mc_render::avatar::HeldItem { model: model.clone(), tints, entity_block: false, glint });
            }
        }
        self.held_item(stack)
    }

    /// `RenderItem.renderItem(stack, model)` inputs: the model for the stack's damage value,
    /// `getColorFromItemStack` tints and `hasEffect`.
    pub(crate) fn held_item(&self, stack: &mc_gui::ItemStack) -> Option<mc_render::avatar::HeldItem> {
        let def = self.catalogue.render_def(stack);
        let model = self.models.items.get(def.id, def.meta)?.clone();
        let meta = if def.id == 373 { stack.damage as u16 } else { def.meta };
        let dye = tooltip::armor_dye(&self.catalogue, stack);
        let tints = std::array::from_fn(|i| match dye {
            Some(c) if i == 0 => c,
            _ => mc_render::items::item_color(def.id, meta, i as i32, &self.models.colors),
        });
        let glint = tooltip::has_effect(&self.catalogue, stack);
        Some(mc_render::avatar::HeldItem { model, tints, entity_block: matches!(def.block, 54 | 130 | 146), glint })
    }

    /// `EntityRenderer.hurtCameraEffect`: death roll, then the hurt shake.
    pub(crate) fn hurt_camera(&self, pt: f32) -> glam::Mat4 {
        let e = &self.entity;
        let mut m = glam::Mat4::IDENTITY;
        if self.player.health <= 0. && self.net.is_some() {
            let f1 = e.death_time as f32 + pt;
            m *= glam::Mat4::from_rotation_z((40.0 - 8000.0 / (f1 + 200.0)).to_radians());
        }
        let f = e.hurt_time as f32 - pt;
        if f < 0. || e.max_hurt_time == 0 {
            return m;
        }
        let f = f / e.max_hurt_time as f32;
        let f = mc_core::math::sin(f * f * f * f * std::f32::consts::PI);
        let yaw = e.attacked_at_yaw;
        m * glam::Mat4::from_rotation_y((-yaw).to_radians()) * glam::Mat4::from_rotation_z((-f * 14.0).to_radians()) * glam::Mat4::from_rotation_y(yaw.to_radians())
    }

    /// `EntityRenderer.setupViewBobbing`
    pub(crate) fn view_bobbing(&self, pt: f32) -> glam::Mat4 {
        let e = &self.entity;
        if !self.ui.settings.view_bobbing {
            return glam::Mat4::IDENTITY;
        }
        let f = e.distance_walked - e.prev_distance_walked;
        let f1 = -(e.distance_walked + f * pt);
        let f2 = e.prev_camera_yaw + (e.camera_yaw - e.prev_camera_yaw) * pt;
        let f3 = e.prev_camera_pitch + (e.camera_pitch - e.prev_camera_pitch) * pt;
        let pi = std::f32::consts::PI;
        glam::Mat4::from_translation(Vec3::new(
            mc_core::math::sin(f1 * pi) * f2 * 0.5,
            -(mc_core::math::cos(f1 * pi) * f2).abs(),
            0.,
        )) * glam::Mat4::from_rotation_z((mc_core::math::sin(f1 * pi) * f2 * 3.).to_radians())
            * glam::Mat4::from_rotation_x(((mc_core::math::cos(f1 * pi - 0.2) * f2).abs() * 5.).to_radians())
            * glam::Mat4::from_rotation_x(f3.to_radians())
    }

    pub(crate) fn sync_view(&mut self) {
        let _z = crate::bench::zone(crate::bench::Phase::Entities);
        // The renderer drops last frame's `AvatarFrame` when this one replaces it; take its
        // vectors back first so every entity collection below reuses the same allocations.
        if let Some(old) = self.renderer.as_mut().and_then(|r| r.avatar_frame.take()) {
            self.frame_entities.recycle(old);
        }
        let scripted_camera = self.bench.as_ref().map(|_| (self.camera.pos, self.camera.yaw, self.camera.pitch, self.camera.fov_deg, self.camera.bob));
        // Only the multiplayer edit box reads this; copying the name every frame is a needless
        // `String` allocation when it has not changed.
        if self.ui.player_name != self.name {
            self.ui.player_name = self.name.clone();
        }
        let pt = self.tick_accum;
        let mut pos = self.entity.interpolated(pt);
        let mut eye = self.eye(pt);
        let e = &self.entity;
        let mut yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * pt;
        let mut pitch = e.prev_pitch + (e.pitch - e.prev_pitch) * pt;
        let mut bob = self.view_bobbing(pt);
        // `Minecraft.setRenderViewEntity` (S43PacketCamera): the camera follows the entity.
        let camera_view = self.camera_entity.and_then(|id| {
            self.remote.map.get(&id).map(|r| {
                let p = r.interpolated(pt);
                let child = r.is_child();
                let eye_h = match r.kind {
                    entities::Kind::Player => 1.62,
                    entities::Kind::Object(78) => r.height as f32 * if child { 0.5 } else { 0.9 },
                    _ => r.height as f32 * 0.85,
                };
                (
                    p,
                    mc_core::Vec3::new(p.x, p.y + eye_h as f64, p.z),
                    r.prev_yaw + (r.yaw - r.prev_yaw) * pt,
                    r.prev_pitch + (r.pitch - r.prev_pitch) * pt,
                )
            })
        });
        if let Some((p, cam_eye, cam_yaw, cam_pitch)) = camera_view {
            pos = Vec3::new(p.x as f32, p.y as f32, p.z as f32);
            eye = Vec3::new(cam_eye.x as f32, cam_eye.y as f32, cam_eye.z as f32);
            yaw = cam_yaw;
            pitch = cam_pitch;
            bob = glam::Mat4::IDENTITY;
        }
        let mut fov = self.ui.settings.fov * (e.prev_fov_modifier_hand + (e.fov_modifier_hand - e.prev_fov_modifier_hand) * pt);
        self.camera.yaw = yaw;
        self.camera.pitch = pitch;
        self.camera.pos = eye;
        self.camera.bob = self.hurt_camera(pt) * bob * glam::Mat4::from_translation(Vec3::new(0., 0., -0.1));
        self.update_target(pt);
        if self.camera_entity.is_none() && self.entity.perspective != 0 {
            if self.entity.perspective == 2 {
                self.camera.yaw += 180.;
                self.camera.pitch = -self.camera.pitch;
            }
            self.camera.bob = bob;
            let back = -self.camera.look_dir();
            let hit = player::target(
                &self.world,
                &mut self.geometry,
                mc_core::Vec3::new(eye.x as f64, eye.y as f64, eye.z as f64),
                back,
                4.,
            );
            let distance = hit.map_or(4., |h| {
                ((Vec3::new(h.hit.x as f32, h.hit.y as f32, h.hit.z as f32) - eye).length() - 0.15).max(0.)
            });
            self.camera.pos = eye + back * distance;
        }
        // `ActiveRenderInfo.getBlockAtEntityViewpoint` tests the camera, including the third-person
        // offset, not the entity's eye. `EntityRenderer.getFOVModifier` narrows the view in water.
        let view_block = self.block_at_entity_viewpoint(self.camera.pos);
        if view_block.material == mc_data::Material::Water {
            fov = fov * 60. / 70.;
        }
        self.camera.fov_deg = fov;
        if let Some((pos, yaw, pitch, fov, bob)) = scripted_camera {
            (self.camera.pos, self.camera.yaw, self.camera.pitch, self.camera.fov_deg, self.camera.bob) = (pos, yaw, pitch, fov, bob);
        }
        self.player.survival = !self.entity.creative;
        self.player.armor = self.total_armor_value();
        self.player.hurt_resistant_time = self.entity.hurt_resistant_time;
        let fx = &self.entity.effects;
        self.player.regeneration = fx.has(10);
        self.player.poison = fx.has(19);
        self.player.wither = fx.has(20);
        self.player.hunger = fx.has(17);
        self.player.eye_in_water = self.is_inside_of_material(mc_data::Material::Water);
        self.ui.selected_slot = self.player.selected_slot;
        let use_count = if self.item_in_use.is_some() { self.item_in_use_count } else { 0 };
        let current = self.ui.inventory[self.player.selected_slot];
        let held = current.and_then(|s| self.held_item_using(&s, use_count));
        // itemToRender keeps the stack it last equipped; ours keeps its index and count.
        let hand_item = self.entity.item_to_render.and_then(|(i, c)| {
            let s = current.filter(|s| s.item == i).unwrap_or_else(|| self.catalogue.stack(i, c));
            self.held_item_using(&s, use_count)
        });
        let outline = if self.ui.settings.hide_gui {
            None
        } else {
            self.target.map(|t| {
                self.geometry
                    .selected_box(self.world.state_at(t.pos.x, t.pos.y, t.pos.z), &self.world, t.pos)
            })
        };
        let block_pos = BlockPos::new(
            self.entity.pos.x.floor() as i32,
            self.entity.pos.y.floor() as i32,
            self.entity.pos.z.floor() as i32,
        );
        let biome = mc_data::biome::Biome::by_id(self.world.biome_id(block_pos)).unwrap_or(mc_data::biome::Biome::by_id(1).unwrap());
        let mut sky = mc_render::sky::Sky::overworld(self.info.time, pt, biome.temperature_at(block_pos), self.ui.settings.render_distance);
        let (rain, thunder) = (self.weather.rain_strength(pt), self.weather.thunder_strength(pt));
        sky.apply_weather(rain, thunder);
        // `EntityRenderer.updateFogColor`: the water/lava branch replaces the weather-darkened
        // colour after it, and `EntityRenderer.setupFog` switches the same cases to GL_EXP.
        // `EnchantmentHelper.getRespiration`: the best level over `EntityPlayer.getInventory()`,
        // which is the armor slots only.
        let respiration = self
            .ui
            .armor
            .iter()
            .map(|s| tooltip::enchantment_level(s.as_ref(), 5))
            .max()
            .unwrap_or(0);
        sky.apply_liquid(view_block.material, respiration, self.entity.effects.has(13));
        // EntityRenderer.updateLightmap (getSunBrightness(1.0)).
        let sun_sky = mc_render::sky::Sky::overworld(self.info.time, 1.0, 0.0, 2);
        // `WorldProviderHell.calculateCelestialAngle` is always 0.5 and the End's is 0.0, so
        // both dimensions have a constant 1.0 sun brightness; only the overworld follows time.
        let sun = if self.forge_dimension == 0 {
            lightmap::sun_brightness_weather(sun_sky.angle, self.weather.rain_strength(1.0), self.weather.thunder_strength(1.0))
        } else {
            1.0
        };
        // `Potion.nightVision` at the player's remaining duration.
        let night_vision = self
            .entity
            .effects
            .get(mc_entity::effects::NIGHT_VISION)
            .map(|e| lightmap::night_vision_brightness(e.duration, pt));
        self.lightmap = lightmap::build(sun, self.torch_flicker_x, self.ui.settings.gamma, self.forge_dimension, night_vision, !self.lightning.is_empty());
        if let Some(r) = &mut self.renderer {
            r.set_lightmap(&self.lightmap);
        }
        sky.ticks = self.ui.ticks;
        // GameSettings.shouldRenderClouds: off below 4 chunks; 2 = fancy.
        let clouds = if self.ui.settings.render_distance >= 4 { self.ui.settings.clouds } else { 0 };
        sky.clouds = clouds != 0;
        sky.fancy_clouds = clouds == 2;
        let light = self.light_at_eyes();
        // One entity scan for every world-pass entity vector (players, mobs, objects,
        // billboards, item frames, entity blocks, dropped items, break overlays).
        let entity_frames = self.collect_frame_entities(pt);
        let e = &self.entity;
        let frame = mc_render::avatar::AvatarFrame {
            pos,
            body_yaw: e.prev_render_yaw_offset + (e.render_yaw_offset - e.prev_render_yaw_offset) * pt,
            head_yaw: e.prev_rotation_yaw_head + (e.rotation_yaw_head - e.prev_rotation_yaw_head) * pt,
            pitch,
            limb_swing: e.limb_swing - e.limb_swing_amount * (1. - pt),
            limb_amount: (e.prev_limb_swing_amount + (e.limb_swing_amount - e.prev_limb_swing_amount) * pt).min(1.),
            age: e.ticks_existed as f32 + pt,
            swing: e.swing_progress(pt),
            sneaking: e.is_sneaking(),
            perspective: self.entity.perspective,
            hide_hand: self.ui.settings.hide_gui || self.entity.dead || self.camera_entity.is_some(),
            outline,
            held,
            light,
            equip: 1. - (self.entity.prev_equipped_progress + (self.entity.equipped_progress - self.entity.prev_equipped_progress) * pt),
            yaw,
            arm_pitch: e.prev_render_arm_pitch + (e.render_arm_pitch - e.prev_render_arm_pitch) * pt,
            arm_yaw: e.prev_render_arm_yaw + (e.render_arm_yaw - e.prev_render_arm_yaw) * pt,
            // `EntityRenderer.renderHand`: `hurtCameraEffect` then `setupViewBobbing`.
            bob: self.hurt_camera(pt) * bob,
            hand_light: light,
            hand_item,
            slim: self.net.as_ref().is_some_and(|n| self.skins.loaded.get(&n.uuid).map_or(skins::default_slim(n.uuid), |s| s.1)),
            skin: self.net.as_ref().and_then(|n| self.skins.loaded.get(&n.uuid)).map(|s| s.0),
            dropped: entity_frames.dropped,
            damage: entity_frames.damage,
            others: entity_frames.others,
            mobs: entity_frames.mobs,
            tiles: {
                let table = &self.lightmap;
                let color = |sky: u8, block: u8| {
                    let i = (sky as usize * 16 + block as usize) * 4;
                    [table[i] as f32 / 255., table[i + 1] as f32 / 255., table[i + 2] as f32 / 255.]
                };
                let mut frames = self.tiles.frames(&self.world, pt, self.camera.pos.to_array(), self.christmas, &color, &self.skins);
                for tile in &mut frames {
                    if let mc_render::tile::TileFrame::Banner { age, .. } = tile { *age = self.info.total_time as f32 + pt; }
                    let mc_render::tile::TileFrame::Sign { pos, glyphs, .. } = tile else { continue };
                    let p = mc_core::BlockPos::new(pos.x as i32, pos.y as i32, pos.z as i32);
                    self.tiles.append_sign_glyphs(p, glyphs, &self.world, &self.signs, &mut self.ui.font, &self.ui.lang, self.ui.settings.chat_colors);
                }
                frames
            },
            objects: entity_frames.objects,
            billboards: entity_frames.billboards,
            fixed_items: entity_frames.fixed_items,
            world_quads: entity_frames.world_quads,
            particles: self.particle_quads(pt),
            armor: std::array::from_fn(|i| self.ui.armor[3 - i].and_then(|s| tooltip::armor_piece(&self.catalogue, &s))),
            head_skull: self.ui.armor[0].and_then(|s| self.skull_head(&s)),
            use_anim: self.item_in_use.filter(|_| self.item_in_use_count > 0).map(|s| {
                let (a, max) = item_use::action(self.catalogue.items[s.item].id);
                mc_render::avatar::UseAnim { action: a.index(), left: self.item_in_use_count as f32 - pt + 1., max: max as f32 }
            }),
            gui_player: self.screen.as_ref().and_then(|s| s.entity_preview()).map(|(x, y, scale)| {
                let f = self.scaled().factor as f32;
                let (mx, my) = (self.mouse_px.0 as f32 / f, self.mouse_px.1 as f32 / f);
                mc_render::avatar::GuiPlayer { x: x as f32 * f, y: y as f32 * f, scale: scale as f32 * f, mouse_x: x as f32 - mx, mouse_y: (y - 50) as f32 - my }
            }),
        };
        if let Some(r) = &mut self.renderer {
            r.sky = sky;
            r.avatar_frame = Some(frame);
        }
        // `Minecraft.runGameLoop`: `mcSoundHandler.setListener(thePlayer, renderPartialTicks)`.
        self.update_sound_listener();
    }
    pub(crate) fn debug_info(&self) -> DebugInfo {
        let r = self.renderer.as_ref();
        let p = self.entity.pos;
        let (bx, by, bz) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        DebugInfo {
            fps: self.fps,
            chunk_updates: 0,
            chunks_drawn: r.map_or(0, |r| r.drawn_chunks),
            chunks_total: self.world.chunks.len() * 16,
            render_distance: self.ui.settings.render_distance,
            pos: [p.x, p.y, p.z],
            yaw: self.entity.yaw,
            pitch: self.entity.pitch,
            light: self.world.light(bx, by, bz),
            display: r.map_or((0, 0), |r| (r.config.width, r.config.height)),
            gpu: r.map_or(String::new(), |r| r.adapter_info.name.clone()),
            backend: r.map_or(String::new(), |r| format!("{:?}", r.adapter_info.backend)),
            vsync: self.ui.settings.vsync,
            limit_framerate: self.ui.settings.limit_framerate,
        }
    }

    /// `GuiIngame.updateTick`, including the held item's name highlight.
    pub(crate) fn hud_tick(&mut self) {
        self.hud.tick();
        if !self.in_game {
            return;
        }
        let cur = self.ui.inventory[self.player.selected_slot];
        match cur {
            None => self.hud.highlight_ticks = 0,
            Some(s) => {
                let cat = &self.catalogue;
                let same = self.highlight_stack.is_some_and(|h| {
                    cat.items[h.item].id == cat.items[s.item].id && tooltip::tags_equal(&h, &s) && (tooltip::damageable(cat, &s) || h.damage == s.damage)
                });
                if same {
                    if self.hud.highlight_ticks > 0 {
                        self.hud.highlight_ticks -= 1;
                    }
                } else {
                    self.hud.highlight_ticks = 40;
                }
                self.hud.highlight_name = tooltip::highlight_name(cat, &self.ui.lang, &s);
            }
        }
        self.highlight_stack = cur;
    }

    /// Our profile name as the scoreboard knows it, without the clone the sidebar build used to
    /// do every frame (`playerInfoMap.get(uuid).getName()`).
    pub(crate) fn profile_name(&self) -> &str {
        self.net.as_ref().and_then(|n| self.scoreboard.players.get(&n.uuid)).map_or("Player", |p| p.name.as_str())
    }

    /// Builds this frame's GUI batch.
    pub(crate) fn build_gui(&mut self) -> Scaled {
        self.sync_pack_ui();
        let sr = self.scaled();
        let (mx, my) = self.mouse_gui();
        self.batch.clear();
        let hud_visible = self.in_game && self.screen.as_ref().is_none_or(|s| s.shows_hud());
        // Name tags belong to the world pass, under every overlay.
        if hud_visible && !self.ui.settings.hide_gui {
            self.name_tags(sr);
        }
        // RenderWorldLastEvent: the end of renderWorldPass, still under every overlay.
        if hud_visible {
            self.forge_world_last(sr);
        }
        if hud_visible {
            // GuiNewChat.getChatOpen: `currentScreen instanceof GuiChat`.
            let chat_open = self.screen.as_ref().is_some_and(|s| s.is_chat());
            // F3's text is only drawn by `hud.render` when `show_debug`, so skip building it
            // (`DebugInfo::default()` has empty strings and no work).
            let debug = if self.ui.settings.show_debug { self.debug_info() } else { DebugInfo::default() };
            // `Hud::render` draws the sidebar and tab list under `!hide_gui || chat_open`; build
            // them only when that holds (and the list only while its key is held).
            if text_panels_visible(hud_visible, self.ui.settings.hide_gui, chat_open) {
                self.hud.sidebar = self.scoreboard.sidebar(self.profile_name());
                // The list shows while the key is held, unless this is a lone integrated-server
                // player with no list objective.
                let lone = self.net.is_none() && self.scoreboard.players.len() <= 1 && self.scoreboard.display[0].is_none();
                self.hud.tab_list = ((self.bound_down("key.playerlist") || self.force_tab) && !lone).then(|| self.scoreboard.tab_list());
            } else {
                self.hud.sidebar = None;
                self.hud.tab_list = None;
            }
            let hud_cancelled = self.forge_hud_pre();
            let (inventory, catalogue) = (self.ui.inventory, &self.catalogue);
            let mut items = |ui: &mut mc_gui::Ui, b: &mut mc_gui::batch::GuiBatch| {
                for (slot, stack) in inventory.iter().enumerate().take(9) {
                    if let Some(stack) = stack {
                        catalogue.draw(ui, b, *stack, sr.width / 2 - 90 + slot as i32 * 20 + 2, sr.height - 19);
                    }
                }
            };
            if !hud_cancelled {
                self.hud.render(&mut self.ui, &mut self.batch, sr, &self.player, &debug, chat_open, &mut items);
            }
            self.forge_hud_post(sr);
        }
        #[cfg(feature = "wasm-mods")]
        if hud_visible && !self.ui.settings.hide_gui { self.mod_hud(sr, mx, my); }
        self.forge_frame_begin(self.tick_accum);
        if let Some(s) = &mut self.screen {
            s.draw(&mut self.ui, &mut self.batch, sr.width, sr.height, mx, my);
        }
        self.forge_frame_end(mx, my, self.tick_accum);
        if self.screen.as_ref().is_some_and(|s| s.is_chat()) {
            if let Some(hover) = self.chat_piece_under_mouse().and_then(|p| p.hover) {
                self.chat_hover(&hover, mx, my, sr);
            }
        }
        // `Minecraft.runGameLoop`: `guiAchievement.updateAchievementWindow()` after the screen.
        if self.in_game {
            self.hud.achievement.draw(&mut self.ui, &mut self.batch, sr);
        }
        sr
    }

    /// `GuiScreen.handleComponentHover`
    pub(crate) fn chat_hover(&mut self, hover: &mc_gui::component::HoverEvent, mx: i32, my: i32, sr: Scaled) {
        use mc_gui::component::HoverAction;
        let lines: Vec<String> = match hover.action {
            HoverAction::ShowText => hover.value.formatted().split('\n').map(str::to_string).collect(),
            HoverAction::ShowItem => match tooltip::from_snbt(&hover.value.unformatted(), &self.catalogue, &self.ui) {
                Some(lines) => lines,
                None => vec![format!("\u{a7}cInvalid Item!")],
            },
            HoverAction::ShowEntity => tooltip::entity_lines(&hover.value.unformatted(), self.ui.settings.advanced_item_tooltips).unwrap_or_else(|| vec!["\u{a7}cInvalid Entity!".to_string()]),
            HoverAction::ShowAchievement => vec![hover.value.unformatted()],
        };
        self.ui.draw_hovering_text(&mut self.batch, &lines, mx, my, sr.width, sr.height);
    }

    /// `RendererLivingEntity.renderName` / `renderLivingLabel`, projected into the GUI pass:
    /// a 0.25-alpha backing and the name at 1/60 × 1.6 blocks per text pixel, 0.5 above the
    /// head. Normal tags show through walls (vanilla's see-through pass); sneaking ones are faint.
    pub(crate) fn name_tags(&mut self, sr: Scaled) {
        let pt = self.tick_accum.clamp(0., 1.);
        let Some(win) = &self.window else { return };
        let size = win.inner_size();
        let aspect = size.width as f32 / size.height.max(1) as f32;
        let vp = self.camera.view_proj(aspect, self.ui.settings.render_distance as f32 * 16.);
        let tan = (self.camera.fov_deg.to_radians() / 2.).tan();
        let (w_f, h_f) = (sr.width_f as f32, sr.height_f as f32);
        let camera = self.camera.pos;
        let pointed = self.pointed_entity;
        // `entity.riddenByEntity == null`: one set instead of scanning `attached` per entity.
        let ridden: HashSet<i32> = self.attached.values().copied().collect();
        {
            let scoreboard = &self.scoreboard;
            let lang = &self.ui.lang;
            let remote = &self.remote;
            let me = self.profile_name();
            TAGS.with(|scratch| {
                let (tags, names) = &mut *scratch.borrow_mut();
                tags.clear();
                names.clear();
                let mut project = |rel: Vec3, label: TagText<'_>, faint: bool| {
                    let clip = vp * rel.extend(1.);
                    if clip.w <= 0.05 {
                        return;
                    }
                    let (nx, ny) = (clip.x / clip.w, clip.y / clip.w);
                    let (x, y) = ((nx * 0.5 + 0.5) * w_f, (0.5 - ny * 0.5) * h_f);
                    // `Render.renderLivingLabel`: drop a label whose box cannot touch the
                    // viewport before any of its text is built. `bound` only has to be an upper
                    // bound on the glyph count: one byte is at least one glyph, and one glyph
                    // advances at most 10 font pixels even in bold.
                    let scale = 0.026666668 * (h_f / 2.) / (tan * clip.w);
                    let bound = match &label {
                        TagText::Team(name) => name.len() + scoreboard.team_of(name).map_or(0, |t| t.prefix.len() + t.suffix.len()),
                        TagText::Plain(name) => name.len(),
                    };
                    let (half_w, half_h) = ((8. * bound as f32 + 1.) * scale + 2., 11. * scale + 2.);
                    if !label_in_view(x, y, half_w, half_h, w_f, h_f) {
                        return;
                    }
                    let start = names.len();
                    match label {
                        TagText::Team(name) => match scoreboard.team_of(name) {
                            // `ScorePlayerTeam.formatPlayerName`
                            Some(t) => {
                                names.push_str(&t.prefix);
                                names.push_str(name);
                                names.push_str(&t.suffix);
                            }
                            None => names.push_str(name),
                        },
                        TagText::Plain(name) => names.push_str(name),
                    }
                    tags.push(NameTag { w: clip.w, x, y, text: start..names.len(), faint });
                };
                for r in remote.map.values() {
                    // `canRenderName`: a ridden entity has no tag; the local player has none.
                    if !ridden.is_empty() && ridden.contains(&r.id) {
                        continue;
                    }
                    // The renderer's text, decided before any text exists so an off-screen or
                    // out-of-range entity costs no formatting at all.
                    let (height, sneaking, faint, name) = match r.kind {
                        entities::Kind::Player => {
                            if r.name.is_empty() || r.invisible() {
                                continue;
                            }
                            // A team's name tag visibility decides first.
                            if scoreboard.name_tag_visible(&r.name, me) == Some(false) {
                                continue;
                            }
                            (1.8, r.sneaking, r.sneaking, TagText::Team(r.name.as_str()))
                        }
                        // ArmorStandRenderer.canRenderName: `getAlwaysRenderNameTag()` (byte 3), even
                        // when the stand itself is invisible. Hypixel's holograms rely on this.
                        entities::Kind::Object(78) => {
                            if !r.name_visible() {
                                continue;
                            }
                            let name = r.custom_name().map_or_else(|| lang_name(lang, "entity.ArmorStand.name"), |n| n);
                            (r.height as f32, false, false, TagText::Plain(name))
                        }
                        entities::Kind::Mob(kind) => {
                            // RenderLiving.canRenderName: the always-render flag, or a custom name on
                            // the entity the crosshair points at.
                            if r.invisible() || !(r.name_visible() || (r.custom_name().is_some() && pointed == Some(r.id))) {
                                continue;
                            }
                            let name = r.custom_name().map_or_else(|| lang_name(lang, mob_lang_key(kind)), |n| n);
                            (r.height as f32, false, false, TagText::Plain(name))
                        }
                        _ => continue,
                    };
                    let p = r.interpolated(pt);
                    // `RendererLivingEntity.renderName`: y + height + 0.5, lowered by height/2 for a
                    // child; 64 blocks (32 when sneaking).
                    let child = r.is_child();
                    let y = p.y as f32 + height + 0.5 - if child { height / 2. } else { 0. };
                    let base = Vec3::new(p.x as f32, y, p.z as f32) - camera;
                    let d2 = base.length_squared();
                    let max = if sneaking { 32. } else { 64. };
                    if d2.sqrt() >= max {
                        continue;
                    }
                    if r.kind == entities::Kind::Player && !r.sneaking && d2 < 100. {
                        // RenderPlayer.renderOffsetLivingLabel: the below-name objective within 10
                        // blocks, the name raised by FONT_HEIGHT * 1.15 * 0.026666668 above it.
                        let mut rel = base;
                        if let Some(s) = scoreboard.below_name(&r.name) {
                            project(rel, TagText::Plain(s.as_str()), false);
                            rel.y += 9. * 1.15 * 0.026666668;
                        }
                        project(rel, name, faint);
                        continue;
                    }
                    project(base, name, faint);
                }
                // Far tags first so near ones draw on top.
                tags.sort_by(|a, b| b.w.total_cmp(&a.w));
            });
        }
        TAGS.with(|scratch| {
            let (tags, names) = &mut *scratch.borrow_mut();
            for tag in tags.iter() {
                let name = &names[tag.text.clone()];
                let scale = 0.026666668 * (h_f / 2.) / (tan * tag.w);
                self.batch.transform = Some((scale, [tag.x, tag.y]));
                let half = self.ui.font.width(name) / 2;
                self.batch.rect(-half - 1, -1, half + 1, 8, 0x40000000);
                let color = if tag.faint { 0x20FFFFFF } else { 0xFFFFFFFF };
                self.ui.font.draw(&mut self.batch, name, -half as f32, 0., color);
                self.batch.transform = None;
            }
        });
    }

    pub(crate) fn draw_frame(&mut self) {
        let sr = {
            let _z = crate::bench::zone(crate::bench::Phase::Gui);
            self.build_gui()
        };
        let _z = crate::bench::zone(crate::bench::Phase::World);
        let cam = self.in_game.then_some(&self.camera);
        let Some(r) = &mut self.renderer else { return };
        match r.render(cam, Some((&self.batch, sr)), &self.assets) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let (w, h) = (r.config.width, r.config.height);
                r.resize(w, h);
            }
            Err(e) => eprintln!("render error: {e}"),
        }
        crate::bench::record(crate::bench::Phase::Acquire, r.frame_timing.acquire);
        crate::bench::record(crate::bench::Phase::Submit, r.frame_timing.submit);
        crate::bench::record(crate::bench::Phase::Present, r.frame_timing.present);
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    /// `I18n.format(key)`: the key itself when the table has no entry.
    #[test]
    fn localized_name_matches_i18n_format() {
        let lang = mc_gui::lang::Lang::default();
        let key = "entity.ArmorStand.name";
        assert_eq!(lang_name(&lang, key), key);
        assert_eq!(lang_name(&lang, key), lang.get(key));
    }

    /// F1 hides the sidebar and tab list; an open chat screen draws them again (`Hud::render`'s
    /// `!hide_gui || chat_open`).
    #[test]
    fn text_panels_only_build_while_drawn() {
        assert!(text_panels_visible(true, false, false), "normal HUD");
        assert!(text_panels_visible(true, true, true), "GuiChat keeps the panels");
        assert!(!text_panels_visible(true, true, false), "F1 with no chat open");
        assert!(!text_panels_visible(false, false, false), "no in-game HUD");
    }

    /// A name tag is only dropped when its whole box is outside the viewport (`renderLivingLabel`
    /// draws nothing there), which is what lets `name_tags` skip building its text.
    #[test]
    fn labels_fully_outside_the_viewport_are_culled() {
        assert!(label_in_view(640., 360., 10., 10., 1280., 720.));
        assert!(label_in_view(-5., 360., 10., 10., 1280., 720.), "half over the left edge");
        assert!(label_in_view(640., 725., 10., 10., 1280., 720.), "half below the bottom edge");
        assert!(!label_in_view(-11., 360., 10., 10., 1280., 720.));
        assert!(!label_in_view(1291., 360., 10., 10., 1280., 720.));
        assert!(!label_in_view(640., 731., 10., 10., 1280., 720.));
        assert!(!label_in_view(640., -11., 10., 10., 1280., 720.));
    }
}
