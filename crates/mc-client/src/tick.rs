//! Per-frame update and the in-game tick.

use crate::*;

impl App {
    /// `NetHandlerPlayClient`: a closed connection shows `GuiDisconnected` instead of leaving
    /// the player in a frozen world.
    pub(crate) fn poll_disconnect(&mut self) {
        let Some(reason) = self.net.as_ref().and_then(|n| n.closed.clone()) else { return };
        let text = chat::format(&reason, &self.ui.lang);
        self.leave_world();
        self.show_disconnected("disconnect.lost", text);
    }

    pub(crate) fn update(&mut self, dt: f32) {
        let _z = crate::bench::zone(crate::bench::Phase::Update);
        {
            let _z = crate::bench::zone(crate::bench::Phase::Net);
            self.process_net();
        }
        self.poll_connecting();
        self.poll_asset_download();
        self.poll_disconnect();
        self.flush_window();
        self.skins.request_wanted();
        for (uuid, cell, rgba) in self.skins.poll() {
            if let Some(r) = &mut self.renderer {
                r.upload_skin(cell, &rgba);
                r.register_gui_texture(&skins::gui_path(uuid), 64, 64, &rgba);
                skins::mark_gui_skin(uuid);
            }
        }
        self.sync_creative();
        if !self.ui.sounds.is_empty() {
            self.flush_sounds();
        }
        // `Minecraft.isGamePaused`: only the integrated server pauses; on a remote server the
        // world keeps ticking under the pause menu or the server would rubber-band the player.
        let paused = self.is_local() && self.screen.as_ref().is_some_and(|s| s.pauses());
        self.tick_accum += dt * 20.0;
        while self.tick_accum >= 1.0 {
            self.tick_accum -= 1.0;
            self.forge_run_client_tick(paused);
        }
        self.poll_meshes();
        self.ui.ticks = self.hud.update_counter as f32 + self.tick_accum;
        if self.in_game {
            self.sync_view();
        }
    }

    pub(crate) fn client_tick_body(&mut self, paused: bool) {
            self.forge_tick_begin();
            self.hud_tick();
            let tick_action = if let Some(sc) = &mut self.screen {
                sc.tick();
                sc.tick_action()
            } else {
                Action::None
            };
            if matches!(tick_action, Action::Respawn) {
                self.respawn();
            }
            // Minecraft.runTick fires ClientTickEvent and the screen's updateScreen in menus too (a mod's login screen
            // polls its sign-in progress there).
            // Minecraft.runTick: the music ticker runs in menus and in game.
            if let Some(s) = &mut self.sound {
                // `Minecraft.getAmbientMusicType`; `BossStatus` is not ported, so no dragon music.
                let kind = music::MusicType::ambient(self.in_game, self.forge_dimension, self.entity.creative, self.entity.allow_flying, false);
                for c in mc_sound::Category::ALL {
                    s.volumes.insert(c, self.ui.settings.sound_volume(c.key()));
                }
                self.music.update(s, kind);
                s.update_stream_volumes();
            }
            if let Some(r) = &self.renderer {
                let _z = crate::bench::zone(crate::bench::Phase::Animation);
                let uploads = self.models.atlas.tick();
                r.upload_atlas(&uploads);
            }
            if self.in_game && !paused {
                self.run_tick();
            }
            self.forge_tick_end();
    }

    /// The in-game part of `Minecraft.runTick`: input, then `updateEquippedItem`, then entities.
    pub(crate) fn run_tick(&mut self) {
        // PlayerControllerMP.updateController
        let sel = self.player.selected_slot as i32;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel);
        }
        if let Some(slot) = self.pending_selected_slot.take() {
            self.player.selected_slot = slot;
        }
        if std::mem::take(&mut self.pending_open_inventory) {
            use mc_protocol::packets::{ClientStatus, ClientStatusKind, EntityAction, EntityActionKind, PlayServerbound};
            if self.riding_horse() {
                let id = self.net.as_ref().unwrap().entity_id;
                self.send(PlayServerbound::EntityAction(EntityAction { entity_id: id, action: EntityActionKind::OpenInventory, aux: 0 }));
            } else {
                self.send(PlayServerbound::ClientStatus(ClientStatus { status: ClientStatusKind::OpenInventoryAchievement }));
            }
        }
        self.right_click_delay = self.right_click_delay.saturating_sub(1);
        self.left_click_counter = self.left_click_counter.saturating_sub(1);
        self.update_target(1.0);
        for whole in std::mem::take(&mut self.drop_presses) {
            // EntityPlayerSP.dropOneItem always sends C07; the server changes the inventory.
            if self.entity.game_type == level::GameType::Spectator { continue; }
            if self.net.is_some() {
                let status = if whole { mc_protocol::packets::DiggingStatus::DropAllItems } else { mc_protocol::packets::DiggingStatus::DropItem };
                self.send_dig(status, BlockPos::new(0, 0, 0), Facing::Down);
            } else {
                let slot = self.player.selected_slot;
                if let Some(s) = self.ui.inventory[slot] {
                    let n = if whole { s.count } else { 1 };
                    self.ui.inventory[slot] = (s.count > n).then_some(mc_gui::ItemStack { count: s.count - n, ..s });
                    self.throw_stack(mc_gui::ItemStack { count: n, ..s });
                }
            }
        }
        if self.screen.is_none() {
            let use_down = self.buttons.contains(&MouseButton::Right) && self.grabbed;
            if self.item_in_use.is_some() {
                // While using an item, presses are swallowed and releasing stops it.
                if !use_down {
                    self.on_stopped_using_item();
                }
                self.clicks.clear();
            } else {
                for b in std::mem::take(&mut self.clicks) {
                    self.interact(b);
                }
            }
            if use_down && self.right_click_delay == 0 && self.item_in_use.is_none() {
                self.interact(MouseButton::Right);
            }
            self.send_click_block(self.buttons.contains(&MouseButton::Left) && self.grabbed);
        } else {
            self.clicks.clear();
            if self.item_in_use.is_some() {
                // GuiScreen opening unpresses every key binding.
                self.on_stopped_using_item();
            }
        }
        for s in std::mem::take(&mut self.ui.drops) {
            self.throw_stack(s);
        }
        let current = self.ui.inventory[self.player.selected_slot].map(|s| (s.item, s.count));
        self.entity.update_equipped(current);
        let keys = player::Keys {
            forward: self.screen.is_none() && self.bound_down("key.forward"),
            back: self.screen.is_none() && self.bound_down("key.back"),
            left: self.screen.is_none() && self.bound_down("key.left"),
            right: self.screen.is_none() && self.bound_down("key.right"),
            jump: self.screen.is_none() && self.bound_down("key.jump"),
            sneak: self.screen.is_none() && self.bound_down("key.sneak"),
            sprint: self.screen.is_none() && self.bound_down("key.sprint"),
            using_item: self.item_in_use.is_some(),
        };
        // EntityPlayerSP.onUpdate only runs over a loaded chunk.
        let (px, pz) = (self.entity.pos.x.floor() as i32, self.entity.pos.z.floor() as i32);
        if self.is_local() || self.world.is_chunk_loaded(px >> 4, pz >> 4) {
            self.tick_item_in_use();
            let riding = self.net.as_ref().and_then(|n| self.attached.get(&n.entity_id)).copied();
            self.entity.riding = riding.is_some();
            self.entity.saddled_horse = riding.and_then(|id| self.remote.map.get(&id)).is_some_and(|e| {
                matches!(e.kind, entities::Kind::Mob(100)) && matches!(e.meta.get(&16), Some(mc_protocol::MetaValue::Int(flags)) if flags & 4 != 0)
            });
            if riding.is_some() { self.entity.motion = mc_core::Vec3::new(0., 0., 0.); }
            self.forge_player_update(|app| app.entity.tick(&app.world, &mut app.geometry, keys));
            let player = Vec3::new(self.entity.pos.x as f32, self.entity.pos.y as f32, self.entity.pos.z as f32);
            self.particles.tick(&self.world, &mut self.geometry, &mut self.rand, player, &self.remote.map, &self.models);
            // EntityLivingBase.heal from regeneration (the server corrects it with S06).
            // onDeathUpdate: deathTime stops at 20 when the dead player leaves the entity list.
            if self.player.health <= 0. && self.net.is_some() {
                self.entity.death_time = (self.entity.death_time + 1).min(20);
            } else {
                self.entity.death_time = 0;
            }
            let heal = std::mem::take(&mut self.entity.pending_heal);
            if heal > 0. && self.player.health > 0. {
                self.player.health = (self.player.health + heal).min(self.player.max_health);
            }
            if self.entity.abilities_dirty {
                // `EntityPlayer.onUpdate` sends C13 when `capabilities.isFlying` changes, so
                // creative/spectator servers accept the new movement state.
                self.entity.abilities_dirty = false;
                use mc_protocol::packets::{PlayServerbound as S, PlayerAbilitiesC};
                let e = &self.entity;
                let flags = e.disable_damage as u8 | (e.flying as u8) << 1 | (e.allow_flying as u8) << 2 | (e.creative as u8) << 3;
                self.send(S::PlayerAbilitiesC(PlayerAbilitiesC { flags, fly_speed: e.fly_speed, walk_speed: e.walk_speed }));
            }
            if let Some(jump) = self.entity.pending_horse_jump.take() {
                if let Some(n) = &mut self.net {
                    n.send(mc_protocol::packets::PlayServerbound::EntityAction(mc_protocol::packets::EntityAction { entity_id: n.entity_id, action: mc_protocol::packets::EntityActionKind::RidingJump, aux: jump }));
                }
            }
            if let Some(n) = &mut self.net {
                let e = &self.entity;
                if self.attached.contains_key(&n.entity_id) {
                    n.update_riding(e.yaw, e.pitch, e.on_ground, e.move_strafe_e, e.move_forward_e, e.jump_key, e.sneak_key);
                }
            }
            if self.net.as_ref().is_some_and(|n|!self.attached.contains_key(&n.entity_id)) {
                self.send_walking_update();
            }
            // Entity.updateRidden repositions the passenger after its onUpdate packet emission.
            if let Some(vehicle) = riding.and_then(|id| self.remote.map.get(&id)) {
                let (dx, dz, dy) = match vehicle.kind {
                    entities::Kind::Object(10) => (0.0, 0.0, 0.0),
                    entities::Kind::Object(1) => { let yaw = vehicle.yaw as f64 * std::f64::consts::PI / 180.0; (yaw.cos() * 0.4, yaw.sin() * 0.4, -0.3) },
                    _ => (0.0, 0.0, vehicle.height * 0.75),
                };
                self.entity.set_position(mc_core::Vec3::new(vehicle.pos.x + dx, vehicle.pos.y + dy - 0.35, vehicle.pos.z + dz));
            }
        }
        // `World.updateEntities`: tile entities tick after the world's entities.
        for s in self.tiles.update(&self.world, &mut self.rand) {
            self.play_sound(s.name, Some(s.pos), s.volume, s.pitch);
        }
        self.ui.effects = self
            .entity
            .effects
            .iter()
            .filter_map(|e| {
                let p = mc_entity::effects::potion(e.id)?;
                Some(mc_gui::EffectView { icon: p.icon, name: p.name, amplifier: e.amplifier, duration: e.duration_string() })
            })
            .collect();
        self.tick_items();
        for r in self.remote.map.values_mut().filter(|r| r.kind == entities::Kind::Mob(94)) {
            let (x, y, z) = (r.pos.x.floor() as i32, (r.pos.y + r.height * 0.5).floor() as i32, r.pos.z.floor() as i32);
            r.in_water = self.world.state_at(x, y, z).block().material == mc_data::Material::Water;
        }
        let mut players: Vec<(mc_core::Vec3, f64)> = self.remote.map.values().filter(|r| r.kind == entities::Kind::Player).map(|r| (r.pos, 1.62)).collect();
        if self.entity.game_type != level::GameType::Spectator {
            players.push((self.entity.pos, self.entity.eye_height() as f64));
        }
        self.remote.tick(&mut player::ObjectPhys { phys: player::PhysWorld { world: &self.world, geometry: &mut self.geometry }, players });
        // EntityOtherPlayerMP.onUpdate: EntityPlayer counts the use down, then the eating flag
        // starts or clears it.
        for r in self.remote.map.values_mut().filter(|r| r.kind == entities::Kind::Player) {
            if r.in_use {
                r.use_count -= 1;
            }
            if !r.in_use && r.eating {
                if let Some((i, _)) = r.held {
                    r.use_count = item_use::action(self.catalogue.items[i].id).1;
                    r.in_use = true;
                }
            } else if r.in_use && !r.eating {
                r.use_count = 0;
                r.in_use = false;
            }
        }
        self.flush_sounds();
        if self.is_local() && self.entity.pos.y < -64. {
            self.entity.teleport(self.spawn);
        }
        // EntityRenderer.updateTorchFlicker
        let (a, b, c, d) = (rand_f64(), rand_f64(), rand_f64(), rand_f64());
        self.torch_flicker_dx = (self.torch_flicker_dx as f64 + (a - b) * c * d) as f32;
        self.torch_flicker_dx = (self.torch_flicker_dx as f64 * 0.9) as f32;
        self.torch_flicker_x += self.torch_flicker_dx - self.torch_flicker_x;
        // WorldServer.tick: weather, then the clocks.
        if self.net.is_some() {
            // WorldClient: only the strengths follow the server's raining flags.
            self.weather.tick_remote(&self.info);
        } else if self.is_local() {
            self.weather.tick(&mut self.info, &mut self.rand);
        }
        #[cfg(feature = "singleplayer")]
        if self.net.is_none() {
            // WorldServer.tick: tickUpdates, then updateBlocks' random ticks (chunks within 7).
            let (cx, cz) = (px >> 4, pz >> 4);
            let chunks: Vec<(i32, i32)> = (-7..=7).flat_map(|x| (-7..=7).map(move |z| (cx + x, cz + z))).filter(|&(x, z)| self.world.is_chunk_loaded(x, z)).collect();
            let speed = self.info.rules.get_int("randomTickSpeed");
            self.with_level(|lv| {
                lv.tick_updates();
                lv.random_ticks(&chunks, speed);
            });
        }
        #[cfg(feature = "wasm-mods")]
        self.mod_tick();
        self.info.total_time += 1;
        if self.info.rules.get_bool("doDaylightCycle") {
            self.info.time += 1;
        }
    }

}
