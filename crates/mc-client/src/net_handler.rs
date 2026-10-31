//! Applying server packets (`NetHandlerPlayClient`).

use crate::*;

/// 30,000,000 / 16
const MAX_CHUNK: i32 = 1_875_000;

impl App {
    /// `NetHandlerPlayClient`: applies everything the server sent since the last frame.
    pub(crate) fn process_net(&mut self) {
        let Some(n) = &mut self.net else { return };
        let incoming = n.poll();
        let mut edits = Vec::new();
        for inc in incoming {
            if self.net.as_ref().is_some_and(|n| n.closed.is_some()) { break; }
            // `NetworkManager.exceptionCaught` keeps malformed data from taking down the client.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match inc {
                net::Incoming::Packet(p) => self.handle_packet(*p, &mut edits),
                net::Incoming::Chunks(chunks) => {
                    for chunk in chunks {
                        match chunk {
                            net::ChunkUpdate::Full { chunk, remove_old } => {
                                if remove_old { self.world.remove(chunk.x, chunk.z); }
                                self.world.insert(chunk);
                            }
                            net::ChunkUpdate::Partial { x, z, wire } => {
                                let old = self.world.remove(x, z);
                                self.world.insert(net::fill_chunk(old, x, z, wire, false));
                            }
                        }
                    }
                }
                net::Incoming::Closed(reason) => {
                    if let Some(n) = &mut self.net {
                        if n.closed.is_none() {
                            n.closed = Some(reason);
                        }
                    }
                }
            }));
            if r.is_err() {
                if let Some(n) = &mut self.net {
                    if n.closed.is_none() {
                        n.closed = Some("Internal Exception: packet handler failed".into());
                    }
                    n.disconnect();
                }
            }
        }
        // `EntityLightningBolt.onUpdate`: remove after 4 client ticks (the client does not draw
        // the bolt for long).
        for l in &mut self.lightning {
            l.ticks += 1;
        }
        self.lightning.retain(|l| l.ticks < 4);
        self.poll_resource_pack();
        self.refresh_edits(&edits);
        // Sections of newly arrived or unloaded chunks.
        self.refresh_edits(&[]);
    }

    /// One `NetHandlerPlayClient.handleXxx`.
    pub(crate) fn handle_packet(&mut self, p: mc_protocol::packets::PlayClientbound, edits: &mut Vec<BlockPos>) {
        use mc_protocol::packets::{PlayClientbound as C, PlayServerbound as S, *};
        let Some(n) = &mut self.net else { return };
        match p {
                C::KeepAliveS(k) => n.send(S::KeepAliveC(KeepAliveC { key: k.key })),
                C::JoinGame(j) => {
                    self.forge_dimension = n.join_dimension(j.dimension);
                    n.entity_id = j.entity_id;
                    self.world.has_sky = n.join_dimension(j.dimension) == 0;
                    // `Minecraft.loadWorld`: a new world renders from the player again.
                    self.camera_entity = None;
                    self.particles.clear();
                    // A new WorldClient brings a fresh Scoreboard; the player info map stays.
                    let players = std::mem::take(&mut self.scoreboard.players);
                    self.scoreboard = scoreboard::Scoreboard { players, ..Default::default() };
                    self.player.hardcore = j.hardcore;
                    self.has_valid_health = false;
                    let view = self.ui.settings.render_distance as i8;
                    n.send_join_replies(&self.ui.settings.language, view, self.ui.settings.chat_visibility as i8, self.ui.settings.chat_colors, self.ui.settings.model_parts);
                    self.entity.set_game_type(level::GameType::by_id(j.game_mode as i32));
                    self.info.difficulty = j.difficulty;
                    self.player.max_health = 20.;
                    self.entity.effects.clear();
                    self.entity.attributes = Default::default();
                    self.sync_local_potion();
                    self.player.experience = 0.;
                    self.player.level = 0;
                }
                C::PlayerPosLookS(pl) => {
                    let (mut pos, mut yaw, mut pitch) = (self.entity.pos, self.entity.yaw, self.entity.pitch);
                    let mut prev = self.entity.prev_pos;
                    n.handle_pos_look(&pl, &mut pos, &mut self.entity.motion, &mut prev, &mut yaw, &mut pitch);
                    self.entity.set_position(pos);
                    self.entity.prev_pos = prev;
                    self.entity.yaw = yaw;
                    self.entity.pitch = pitch;
                    self.entity.prev_yaw = yaw;
                    self.entity.prev_pitch = pitch;
                }
                // Chunk coordinates beyond the world border (30M blocks) are never valid; the
                // mesher and light code do plain i32 arithmetic on neighbouring chunk coordinates.
                C::ChunkData(c) if c.x.abs() > MAX_CHUNK || c.z.abs() > MAX_CHUNK => {}
                C::ChunkData(c) => {
                    if c.full && c.bitmask == 0 {
                        self.unload_chunk(c.x, c.z);
                    } else if let Ok(w) = mc_protocol::chunk::decode(c.bitmask, &c.data, self.world.has_sky, c.full) {
                        let old = self.world.remove(c.x, c.z);
                        self.world.insert(net::fill_chunk(old, c.x, c.z, w, c.full));
                    }
                }
                C::MapChunkBulk(b) => {
                    for (x, z, mask, data) in b.chunks {
                        if x.abs() > MAX_CHUNK || z.abs() > MAX_CHUNK {
                            continue;
                        }
                        if let Ok(w) = mc_protocol::chunk::decode(mask, &data, b.sky_light, true) {
                            self.world.insert(net::fill_chunk(None, x, z, w, true));
                        }
                    }
                }
                C::BlockChange(b) => {
                    // S23 carries `Block.BLOCK_STATE_IDS` (`id << 4 | meta`), not `getStateId`.
                    let st = u16::try_from(b.state).ok().and_then(mc_data::StateId::try_from_raw).unwrap_or(mc_data::StateId::AIR);
                    if self.world.state_at(b.pos.x, b.pos.y, b.pos.z).block().id != st.block().id {
                        self.tiles.remove(b.pos); self.signs.remove(&b.pos);
                        self.tiles.placed(b.pos, st);
                    }
                    self.world.set_block_state(b.pos, st);
                    edits.push(b.pos);
                }
                C::MultiBlockChange(m) => {
                    for (p, state) in m.records {
                        let pos = BlockPos::new(m.chunk_x.wrapping_mul(16).wrapping_add((p >> 12) as i32), (p & 255) as i32, m.chunk_z.wrapping_mul(16).wrapping_add(((p >> 8) & 15) as i32));
                        let st = u16::try_from(state).ok().and_then(mc_data::StateId::try_from_raw).unwrap_or(mc_data::StateId::AIR);
                        if self.world.state_at(pos.x, pos.y, pos.z).block().id != st.block().id {
                            self.tiles.remove(pos); self.signs.remove(&pos);
                            self.tiles.placed(pos, st);
                        }
                        self.world.set_block_state(pos, st);
                        edits.push(pos);
                    }
                }
                C::TimeUpdate(t) => {
                    // WorldClient.setWorldTime: a negative time freezes the daylight cycle.
                    // Clamped: the tick adds 1 to both every tick and Java's wrap-around is never wanted.
                    const LIM: i64 = 1 << 62;
                    self.info.total_time = t.world_age.clamp(-LIM, LIM);
                    let neg = t.time_of_day < 0;
                    self.info.time = t.time_of_day.saturating_abs().min(LIM);
                    self.info.rules.set("doDaylightCycle", if neg { "false" } else { "true" });
                }
                C::ChatS(c) => {
                    let Some(json) = self.forge_chat(c.position as i8, &c.json) else { return };
                    let component = chat::parse(&json, &self.ui.lang);
                    if c.position == 2 {
                        // GuiIngame.setRecordPlaying(component, false) uses getUnformattedText.
                        self.hud.set_action_bar(component.unformatted());
                    } else {
                        self.hud.add_chat(component);
                    }
                }
                C::SpawnPosition(sp) => self.info.spawn = sp.pos,
                C::OpenWindow(o) => {
                    let title = chat::format(&o.title, &self.ui.lang);
                    let title = title.trim_end_matches("\u{a7}r").to_string();
                    let w = Arc::new(std::sync::Mutex::new(container::Window::new(o.window_id, &o.kind, title, o.slots)));
                    w.lock().unwrap().level = self.player.level;
                    self.open_window = Some(w.clone());
                    self.parents.clear();
                    // `handleOpenWindow`: "minecraft:villager" opens `GuiMerchant` instead of
                    // the generic container screen.
                    let screen: Box<dyn Screen> = if o.kind == "minecraft:villager" {
                        Box::new(special_screens::MerchantScreen::new(w, self.catalogue.clone()))
                    } else {
                        let mut screen = container::ContainerScreen::new(w, self.catalogue.clone());
                        screen.creative = self.entity.creative;
                        Box::new(screen)
                    };
                    self.open(screen, false);
                }
                C::CloseWindowS(_) => {
                    // handleCloseWindow: closeScreenAndDropStack without a packet.
                    self.open_window = None;
                    if self.screen.is_some() {
                        self.close_screen();
                    }
                }
                C::WindowProperty(p) => {
                    if let Some(w) = &self.open_window {
                        let mut w = w.lock().unwrap();
                        if w.id == p.window_id && (0..7).contains(&p.property) {
                            w.props[p.property as usize] = p.value;
                        }
                    }
                }
                C::ConfirmTransactionS(t) => {
                    let known = t.window_id == 0 || self.open_window.as_ref().is_some_and(|w| w.lock().unwrap().id == t.window_id);
                    if known && !t.accepted {
                        n.send(S::ConfirmTransactionC(ConfirmTransactionC { window_id: t.window_id, action: t.action, accepted: true }));
                    }
                }
                C::PlayerListItem(pl) => {
                    // handlePlayerListItem
                    for e in pl.entries {
                        let players = &mut self.scoreboard.players;
                        match pl.action {
                            PlayerListAction::AddPlayer => {
                                if let Some(t) = e.properties.iter().find(|p| p.name == "textures") {
                                    self.skins.request(e.uuid, &t.value);
                                }
                                self.player_names.insert(e.uuid, e.name.clone());
                                players.insert(
                                    e.uuid,
                                    scoreboard::PlayerInfo {
                                        name: e.name,
                                        game_mode: e.game_mode,
                                        ping: e.ping,
                                        display_name: e.display_name.map(|d| chat::format(&d, &self.ui.lang)),
                                    },
                                );
                            }
                            PlayerListAction::RemovePlayer => {
                                self.player_names.remove(&e.uuid);
                                players.remove(&e.uuid);
                            }
                            PlayerListAction::UpdateGameMode => {
                                if let Some(p) = players.get_mut(&e.uuid) {
                                    p.game_mode = e.game_mode;
                                }
                            }
                            PlayerListAction::UpdateLatency => {
                                if let Some(p) = players.get_mut(&e.uuid) {
                                    p.ping = e.ping;
                                }
                            }
                            PlayerListAction::UpdateDisplayName => {
                                if let Some(p) = players.get_mut(&e.uuid) {
                                    p.display_name = e.display_name.map(|d| chat::format(&d, &self.ui.lang));
                                }
                            }
                        }
                    }
                }
                C::PlayerListHeaderFooter(h) => {
                    let (header, footer) = (chat::format(&h.header, &self.ui.lang), chat::format(&h.footer, &self.ui.lang));
                    self.scoreboard.header = (!header.is_empty()).then_some(header);
                    self.scoreboard.footer = (!footer.is_empty()).then_some(footer);
                }
                C::Title(t) => {
                    // handleTitle: fields a packet type does not carry read as 0, so CLEAR also
                    // zeroes the fade times (vanilla behaviour).
                    use mc_protocol::packets::Title;
                    match t {
                        Title::Title(s) => self.hud.display_title(Some(chat::format(&s, &self.ui.lang)), None, 0, 0, 0),
                        Title::Subtitle(s) => self.hud.display_title(None, Some(chat::format(&s, &self.ui.lang)), 0, 0, 0),
                        Title::Times { fade_in, stay, fade_out } => self.hud.display_title(None, None, fade_in, stay, fade_out),
                        Title::Clear => self.hud.display_title(None, None, 0, 0, 0),
                        Title::Reset => {
                            self.hud.display_title(Some(String::new()), Some(String::new()), -1, -1, -1);
                            self.hud.default_title_times();
                        }
                    }
                }
                C::ScoreboardObjective(o) => self.scoreboard.objective(o),
                C::UpdateScore(u) => self.scoreboard.score(u),
                C::DisplayScoreboard(d) => self.scoreboard.display_slot(d.position, d.name),
                C::Teams(t) => self.scoreboard.team(t),
                C::SoundEffect(se) => {
                    let p = mc_core::Vec3::new(se.x as f64 / 8., se.y as f64 / 8., se.z as f64 / 8.);
                    self.play_sound(&se.name, Some(p), se.volume, se.pitch as f32 / 63.);
                }
                C::Effect(ef) => {
                    // `handleEffect`: server-wide events take `playBroadcastSound`, everything
                    // else `playAuxSFX`.
                    if ef.global {
                        self.play_broadcast_sound(ef.effect_id, ef.pos);
                    } else {
                        self.play_aux_sfx(ef.effect_id, ef.pos, ef.data);
                    }
                }
                C::SpawnPlayer(sp) => {
                    let mut r = entities::Remote::new(sp.entity_id, entities::Kind::Player, sp.x, sp.y, sp.z, sp.yaw, sp.pitch);
                    r.name = self.player_names.get(&sp.uuid).cloned().unwrap_or_default();
                    r.uuid = Some(sp.uuid);
                    r.held = (sp.current_item > 0).then(|| self.catalogue.index_of(sp.current_item as u16, 0)).flatten().map(|i| (i, 1));
                    r.held_stack = r.held.map(|(i, c)| self.catalogue.stack(i, c));
                    apply_meta(&mut r, &sp.metadata);
                    self.remote.map.insert(sp.entity_id, r);
                }
                C::SpawnMob(m) => {
                    let mut r = entities::Remote::new(m.entity_id, entities::Kind::Mob(m.kind), m.x, m.y, m.z, m.yaw, m.pitch);
                    r.head_yaw = entities::angle(m.head_pitch);
                    r.prev_head_yaw = r.head_yaw;
                    (r.width, r.height) = entities::mob_size(m.kind);
                    apply_meta(&mut r, &m.metadata);
                    self.remote.map.insert(m.entity_id, r);
                }
                C::SpawnObject(o) => {
                    let mut r = entities::Remote::new(o.entity_id, entities::Kind::Object(o.kind), o.x, o.y, o.z, o.yaw, o.pitch);
                    r.data = o.data;
                    let v = mc_core::Vec3::new(o.velocity.0 as f64 / 8000., o.velocity.1 as f64 / 8000., o.velocity.2 as f64 / 8000.);
                    if o.kind == 2 {
                        self.pending_items.insert(o.entity_id, (r.pos, v));
                    }
                    // handleSpawnObject: the motion only rides along when the data is > 0.
                    if o.data > 0 {
                        r.set_velocity(v);
                    }
                    self.remote.map.insert(o.entity_id, r);
                }
                C::EntityMetadata(m) => {
                    if let Some((pos, v)) = self.pending_items.remove(&m.entity_id) {
                        let stack = m.metadata.iter().find_map(|(i, v)| match (i, v) {
                            (10, mc_protocol::MetaValue::Item(s)) => net::slot_stack(&self.catalogue, s),
                            _ => None,
                        });
                        if let Some(s) = stack {
                            let mut it = mc_entity::item::ItemEntity::new(m.entity_id as u32, pos, (s.item, s.count), &mut self.rand);
                            (it.damage, it.tag) = (s.damage, s.tag);
                            it.motion = v;
                            it.pickup_delay = 32767;
                            self.items.push(it);
                        } else {
                            self.pending_items.insert(m.entity_id, (pos, v));
                        }
                    }
                    if let Some(r) = self.remote.map.get_mut(&m.entity_id) {
                        apply_meta(r, &m.metadata);
                    }
                    if m.entity_id == n.entity_id {
                        // DataWatcher.updateWatchedObjectsFromList on the local player.
                        for (i, v) in &m.metadata {
                            match (i, v) {
                                // `Entity.setFlag(5, invisible)`.
                                (0, mc_protocol::MetaValue::Byte(b)) => {
                                    self.particles.set_local_flag_invisible(b & 0x20 != 0);
                                    self.sync_local_potion();
                                }
                                (1, mc_protocol::MetaValue::Short(a)) => self.player.air = *a as i32,
                                (6, mc_protocol::MetaValue::Float(hp)) => self.player.health = hp.clamp(0., self.player.max_health),
                                // `updatePotionMetadata`'s colour and ambience (data watcher 7/8).
                                (7, mc_protocol::MetaValue::Int(c)) => {
                                    let (a, i) = (self.particles.local_ambient(), self.particles.local_invisible());
                                    self.particles.set_local_potion(*c, a, i);
                                }
                                (8, mc_protocol::MetaValue::Byte(a)) => {
                                    let (c, i) = (self.particles.local_color(), self.particles.local_invisible());
                                    self.particles.set_local_potion(c, *a > 0, i);
                                }
                                (17, mc_protocol::MetaValue::Float(a)) => self.player.absorption = *a,
                                (18, mc_protocol::MetaValue::Int(s)) => self.score = *s,
                                _ => {}
                            }
                        }
                    }
                }
                C::EntityEquipment(e) => {
                    if (1..=4).contains(&e.slot) {
                        let stack = net::slot_stack(&self.catalogue, &e.item);
                        if let Some(r) = self.remote.map.get_mut(&e.entity_id) {
                            r.armor[e.slot as usize - 1] = stack;
                        }
                    }
                    if e.slot == 0 {
                        let stack = net::slot_stack(&self.catalogue, &e.item);
                        let held = stack.map(|s| (s.item, s.count));
                        if let Some(r) = self.remote.map.get_mut(&e.entity_id) {
                            r.held = held;
                            r.held_stack = stack;
                        // A new held stack object ends the use (itemstack != itemInUse).
                            r.in_use = false;
                            r.use_count = 0;
                        }
                    }
                }
                C::EntityVelocity(v) => {
                    let m = mc_core::Vec3::new(v.vx as f64 / 8000., v.vy as f64 / 8000., v.vz as f64 / 8000.);
                    if v.entity_id == n.entity_id {
                        // Knockback: Entity.setVelocity.
                        self.entity.motion = m;
                    } else if let Some(it) = self.items.iter_mut().find(|i| i.id == v.entity_id as u32) {
                        it.motion = m;
                    } else if let Some(r) = self.remote.map.get_mut(&v.entity_id) {
                        r.set_velocity(m);
                    }
                }
                C::Explosion(x) => {
                    let (a, b, c) = x.player_motion;
                    // `handleExplosion`: `doExplosionB(true)` first, then the knockback.
                    self.explosion_b(&x, edits);
                    // Non-finite or huge motion would poison the player's position for good.
                    let sane = |v: f32| if v.is_finite() { v.clamp(-100., 100.) as f64 } else { 0. };
                    self.entity.motion.x += sane(a);
                    self.entity.motion.y += sane(b);
                    self.entity.motion.z += sane(c);
                }
                C::DestroyEntities(d) => {
                    for id in d.ids {
                        self.remote.map.remove(&id);
                        self.pending_items.remove(&id);
                        self.items.retain(|i| i.id != id as u32);
                        self.paintings.retain(|p| p.id != id);
                        self.lightning.retain(|l| l.id != id);
                        self.attached.remove(&id);
                        self.attached.retain(|_, vehicle| *vehicle != id);
                        self.break_anims.retain(|_, (breaker, _)| *breaker != id);
                    }
                }
                C::CollectItem(c) => {
                    // handleCollectItem
                    if let Some(p) = self.items.iter().find(|i| i.id == c.collected_id as u32).map(|i| i.pos) {
                        let pitch = ((rand_f64() as f32 - rand_f64() as f32) * 0.7 + 1.0) * 2.0;
                        self.play_sound("random.pop", Some(p), 0.2, pitch);
                    }
                    self.items.retain(|i| i.id != c.collected_id as u32);
                    self.remote.map.remove(&c.collected_id);
                }
                C::EntityRelMove(m) => self.remote_move(m.entity_id, (m.dx as i32, m.dy as i32, m.dz as i32), None, Some(m.on_ground), true),
                C::EntityLookMove(m) => self.remote_move(m.entity_id, (m.dx as i32, m.dy as i32, m.dz as i32), Some((m.yaw, m.pitch)), Some(m.on_ground), true),
                C::EntityLook(m) => self.remote_move(m.entity_id, (0, 0, 0), Some((m.yaw, m.pitch)), Some(m.on_ground), true),
                C::EntityTeleport(t) => self.remote_move(t.entity_id, (t.x, t.y, t.z), Some((t.yaw, t.pitch)), Some(t.on_ground), false),
                C::EntityHeadLook(h) => {
                    if let Some(r) = self.remote.map.get_mut(&h.entity_id) {
                        r.head_yaw = entities::angle(h.yaw);
                    }
                }
                C::AnimationS(a) => {
                    if let Some(r) = self.remote.map.get_mut(&a.entity_id) {
                        match a.animation {
                            0 => r.swing_arm(),
                            1 => r.hurt_time = 10,
                            _ => {}
                        }
                    }
                }
                C::EntityStatus(s) if s.entity_id == n.entity_id => match s.status {
                    // EntityLivingBase.handleStatusUpdate
                    2 => {
                        self.entity.limb_swing_amount = 1.5;
                        self.entity.hurt_resistant_time = 20;
                        self.entity.hurt_time = 10;
                        self.entity.max_hurt_time = 10;
                        self.entity.attacked_at_yaw = 0.;
                        let pitch = (rand_f64() as f32 - rand_f64() as f32) * 0.2 + 1.0;
                        self.play_sound("game.player.hurt", Some(self.entity.pos), 1.0, pitch);
                    }
                    3 => {
                        let pitch = (rand_f64() as f32 - rand_f64() as f32) * 0.2 + 1.0;
                        self.play_sound("game.player.die", Some(self.entity.pos), 1.0, pitch);
                    }
                    // EntityPlayer.handleStatusUpdate(9)
                    9 => self.on_item_use_finish(),
                    _ => {}
                },
                C::EntityStatus(s) => {
                    if let Some(r) = self.remote.map.get_mut(&s.entity_id) {
                        r.status(s.status);
                    }
                }
                C::EntityEffect(e) => {
                    // handleEntityEffect: the packet has no ambient flag, and its
                    // "hideParticles" byte is really `getIsShowParticles`.
                    let effect = mc_entity::effects::PotionEffect {
                        id: e.effect_id as u8,
                        duration: e.duration,
                        amplifier: e.amplifier as i32,
                        ambient: false,
                        show_particles: e.hide_particles != 0,
                        duration_max: e.duration == 32767,
                    };
                    if e.entity_id == n.entity_id {
                        self.entity.effects.add(effect);
                        self.sync_local_potion();
                    } else if let Some(r) = self.remote.map.get_mut(&e.entity_id).filter(|r| r.is_living()) {
                        r.effects.add(effect);
                    }
                }
                C::RemoveEntityEffect(e) => {
                    if e.entity_id == n.entity_id {
                        self.entity.effects.remove(e.effect_id);
                        self.sync_local_potion();
                    } else if let Some(r) = self.remote.map.get_mut(&e.entity_id).filter(|r| r.is_living()) {
                        r.effects.remove(e.effect_id);
                    }
                }
                C::EntityProperties(pr) if pr.entity_id == n.entity_id => {
                    for a in &pr.attributes {
                        let mods = a.modifiers.iter().map(|m| mc_entity::effects::Modifier { uuid: (m.uuid.0, m.uuid.1), amount: m.amount, op: m.operation as u8 });
                        self.entity.attributes.apply_snapshot(&a.key, a.base, mods);
                    }
                    self.player.max_health = self.entity.attributes.value(mc_entity::effects::MAX_HEALTH) as f32;
                }
                C::SetExperience(x) => {
                    self.player.experience = x.bar;
                    self.player.level = x.level;
                    if let Some(w) = &self.open_window { w.lock().unwrap().level = x.level; }
                }
                C::UpdateHealth(h) => {
                    self.set_player_sp_health(h.health);
                    self.player.food = h.food;
                    self.player.saturation = h.saturation;
                    self.entity.food_level = h.food;
                    if h.health <= 0. && !self.entity.dead {
                        self.entity.dead = true;
                        self.open(Box::new(screens::DeathScreen::new(false, self.net.is_some(), self.score)), false);
                    }
                }
                C::Respawn(r) => {
                    self.forge_dimension = r.dimension;
                    self.forge_generation = self.forge_generation.wrapping_add(1);
                    self.has_valid_health = false;
                    self.world.has_sky = r.dimension == 0;
                    // `Minecraft.loadWorld` resets `renderViewEntity` to the player.
                    self.camera_entity = None;
                    let keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
                    for (x, z) in keys {
                        self.unload_chunk(x, z);
                    }
                    self.entity.dead = false;
                    self.entity.effects.clear();
                    self.entity.attributes = Default::default();
                    self.sync_local_potion();
                    self.player.max_health = 20.;
                    self.entity.set_game_type(level::GameType::by_id(r.game_mode as i32));
                    self.info.difficulty = r.difficulty;
                }
                C::ChangeGameState(g) => self.change_game_state(g),
                C::PlayerAbilitiesS(a) => {
                    // handlePlayerAbilities
                    self.entity.disable_damage = a.flags & 1 != 0;
                    self.entity.flying = a.flags & 2 != 0;
                    self.entity.creative = a.flags & 8 != 0;
                    self.entity.allow_flying = a.flags & 4 != 0;
                    self.entity.fly_speed = a.fly_speed;
                    self.entity.walk_speed = a.walk_speed;
                }
                C::HeldItemChangeS(h) => {
                    if (0..9).contains(&h.slot) {
                        self.player.selected_slot = h.slot as usize;
                    }
                }
                C::SetSlot(s) => {
                    let stack = net::slot_stack(&self.catalogue, &s.item);
                    let open = self.open_window.clone().filter(|w| w.lock().unwrap().id as i8 == s.window_id);
                    if s.window_id == -1 && s.slot == -1 {
                        if let Some(w) = &self.open_window {
                            w.lock().unwrap().cursor = stack;
                        }
                    } else if let Some(w) = open {
                        w.lock().unwrap().set(&mut self.ui, s.slot as usize, stack);
                    } else if s.window_id == 0 {
                        self.set_container_slot(s.slot as usize, stack);
                    } else if s.window_id == -2 && (0..36).contains(&s.slot) {
                        self.ui.inventory[s.slot as usize] = stack;
                    }
                }
                C::WindowItems(w) => {
                    if let Some(win) = self.open_window.clone().filter(|x| x.lock().unwrap().id == w.window_id && w.window_id != 0) {
                        let mut win = win.lock().unwrap();
                        for (i, it) in w.items.iter().enumerate() {
                            let stack = net::slot_stack(&self.catalogue, it);
                            win.set(&mut self.ui, i, stack);
                        }
                    } else if w.window_id == 0 {
                        for (i, it) in w.items.iter().enumerate() {
                            let stack = net::slot_stack(&self.catalogue, it);
                            self.set_container_slot(i, stack);
                        }
                    }
                }
                C::SpawnExperienceOrb(o) => {
                    // EntityXPOrb.setSize(0.5, 0.5); `xpValue` is the packet's count.
                    let mut r = entities::Remote::new(o.entity_id, entities::Kind::XpOrb, o.x, o.y, o.z, 0, 0);
                    r.data = o.count as i32;
                    self.remote.map.insert(o.entity_id, r);
                }
                C::SpawnPainting(p) => {
                    // EntityPainting.EnumArt by title; unknown titles keep the first art.
                    let art = mc_render::objects::art_by_title(&p.title).unwrap_or(0);
                    let (wpx, hpx) = mc_render::objects::ARTS[art].size;
                    // EntityHanging.updateBoundingBox: the hanging block centre, pulled back
                    // along the facing by 0.46875, centred on the art, then pushed half a block
                    // along the counter-clockwise direction for even 32-pixel sizes.
                    let (fx, fz) = match p.direction {
                        0 => (0., 1.),  // SOUTH
                        1 => (-1., 0.), // WEST
                        2 => (0., -1.), // NORTH
                        _ => (1., 0.),  // EAST
                    };
                    let (cx, cz) = match p.direction {
                        0 => (1., 0.),  // rotateYCCW(SOUTH) = EAST
                        1 => (0., 1.),  // rotateYCCW(WEST)  = SOUTH
                        2 => (-1., 0.), // rotateYCCW(NORTH) = WEST
                        _ => (0., -1.), // rotateYCCW(EAST)  = NORTH
                    };
                    let d4 = if wpx % 32 == 0 { 0.5 } else { 0. };
                    let d5 = if hpx % 32 == 0 { 0.5 } else { 0. };
                    let pos = mc_core::Vec3::new(
                        p.pos.x as f64 + 0.5 - fx * 0.46875 + d4 * cx,
                        p.pos.y as f64 + 0.5 + d5,
                        p.pos.z as f64 + 0.5 - fz * 0.46875 + d4 * cz,
                    );
                    self.paintings.push(entities::Painting { id: p.entity_id, pos, yaw: p.direction as f32 * 90., art });
                }
                C::SpawnGlobalEntity(g) => {
                    // Only EntityLightningBolt (type 1) exists.
                    if g.kind == 1 {
                        let p = mc_core::Vec3::new(g.x as f64 / 32., g.y as f64 / 32., g.z as f64 / 32.);
                        self.lightning.push(entities::Lightning { id: g.entity_id, pos: p, ticks: 0 });
                        self.play_sound("ambient.weather.thunder", Some(p), 10000.0, 0.8 + rand_f64() as f32 * 0.2);
                        self.play_sound("ambient.weather.lightning.impact", Some(p), 2.0, 0.5 + rand_f64() as f32 * 0.2);
                    }
                }
                C::UseBed(b) => {
                    if b.entity_id == n.entity_id {
                        self.bed = Some(b.pos);
                    }
                }
                C::AttachEntity(a) => {
                    if a.leash != 0 {
                        // handleEntityAttach: only `EntityLiving` (mobs) take a leash; players
                        // and other living bases ignore it. A holder that is not in the world
                        // clears the leash.
                        if self.remote.map.get(&a.entity_id).is_some_and(|r| matches!(r.kind, entities::Kind::Mob(_))) {
                            let known = a.vehicle_id == n.entity_id || self.remote.map.contains_key(&a.vehicle_id);
                            self.remote.map.get_mut(&a.entity_id).unwrap().leash_holder = known.then_some(a.vehicle_id);
                        }
                    } else if a.entity_id == n.entity_id || self.remote.map.contains_key(&a.entity_id) {
                        // EntityLivingBase.mountEntity(null) dismounts on the client without repositioning.
                        if a.vehicle_id == -1 || !self.remote.map.contains_key(&a.vehicle_id) {
                            self.attached.remove(&a.entity_id);
                        } else {
                            self.attached.insert(a.entity_id, a.vehicle_id);
                        }
                    }
                }
                // `handleEntityMovement` for `S14PacketEntity`: no relative movement, but the
                // server position is applied and `onGround` (unset on this packet) becomes false.
                C::EntityIdle(e) => self.remote_move(e.entity_id, (0, 0, 0), None, Some(false), true),
                C::BlockAction(b) => {
                    self.block_action(b.pos, b.block_id, b.byte1, b.byte2);
                }
                C::BlockBreakAnim(b) => {
                    if b.stage < 0 {
                        self.break_anims.remove(&b.pos);
                    } else {
                        self.break_anims.insert(b.pos, (b.breaker_id, b.stage as u8));
                    }
                }
                C::ServerDifficulty(d) => self.info.difficulty = d.difficulty,
                C::CombatEvent(_) => {
                    // Vanilla only feeds Twitch metadata here; nothing is drawn.
                }
                C::Camera(c) => {
                    // handleCamera: `packetIn.getEntity` is null for an unknown id, and
                    // `setRenderViewEntity(entity)` on the player itself restores the normal view.
                    if c.entity_id == n.entity_id {
                        self.camera_entity = None;
                    } else if self.remote.map.contains_key(&c.entity_id) {
                        self.camera_entity = Some(c.entity_id);
                    }
                }
                C::WorldBorder(w) => self.world_border(w),
                C::PluginMessageS(mut p) => {
                    if p.channel == "FML|MP" {
                        let Some((channel, data)) = n.forge_multipart(&p.data) else { return };
                        p.channel = channel;
                        p.data = data;
                    }
                    if p.channel == "REGISTER" || p.channel == "UNREGISTER" {
                        n.update_channels(p.channel == "REGISTER", &p.data);
                    } else if p.channel == "FML|HS" {
                        n.forge_message(&p.data);
                    } else if p.channel == "FORGE" {
                        n.forge_runtime(&p.data);
                    } else if p.channel == "FML" {
                        n.forge_internal();
                    } else if p.channel == "MC|Brand" {
                        // The payload is a VarInt-prefixed UTF-8 string.
                        self.brand = packet_string(&p.data);
                    } else if p.channel == "MC|BOpen" {
                        // `handleCustomPayload`: the held written book opens `GuiScreenBook`
                        // read-only (`isUnsigned` false).
                        let held = self.ui.inventory.get(self.player.selected_slot).copied().flatten();
                        if let Some(stack) = held.filter(|s| self.catalogue.items[s.item].id == 387) {
                            self.open(Box::new(special_screens::BookScreen::from_stack(&stack)), false);
                        }
                    } else if p.channel == "MC|TrList" {
                        // Merchant recipes for the open villager window.
                        if let Some((window_id, raw)) = special_screens::parse_merchant_list(&p.data) {
                            let open = self.open_window.clone().filter(|w| {
                                let w = w.lock().unwrap();
                                w.kind == "minecraft:villager" && w.id as i32 == window_id
                            });
                            if open.is_some() {
                                let recipes: Vec<mc_gui::screens::Trade> = raw
                                    .into_iter()
                                    .map(|r| mc_gui::screens::Trade {
                                        buy: net::slot_stack(&self.catalogue, &r.buy),
                                        buy2: net::slot_stack(&self.catalogue, &r.buy2),
                                        sell: net::slot_stack(&self.catalogue, &r.sell),
                                        uses: r.uses,
                                        max_uses: r.max_uses,
                                    })
                                    .collect();
                                if let Some(s) = &mut self.screen {
                                    s.merchant_recipes(window_id, recipes);
                                }
                            }
                        }
                    }
                }
                C::Statistics(s) => {
                    // `handleStatistics`
                    let mut flag = false;
                    for (k, v) in s.entries {
                        if let Some(a) = mc_gui::achievement::by_stat_id(&k).filter(|_| v > 0) {
                            if self.hud.achievement.statistics_seen && mc_gui::achievement::read_stat(&self.stats, &k) == 0 {
                                let icon = self.achievement_icon(a);
                                self.hud.achievement.display_achievement(&self.ui, a, icon);
                                if k == mc_gui::achievement::OPEN_INVENTORY {
                                    self.ui.settings.show_inventory_achievement_hint = false;
                                    self.ui.settings.save();
                                }
                            }
                            flag = true;
                        }
                        self.stats.insert(k, v);
                    }
                    if !self.hud.achievement.statistics_seen && !flag && self.ui.settings.show_inventory_achievement_hint {
                        if let Some(a) = mc_gui::achievement::by_stat_id(mc_gui::achievement::OPEN_INVENTORY) {
                            let icon = self.achievement_icon(a);
                            self.hud.achievement.display_unformatted_achievement(&self.ui, a, icon);
                        }
                    }
                    self.hud.achievement.statistics_seen = true;
                }
                C::TabCompleteS(t) => {
                    if self.tab_complete.take().is_some() {
                        // `GuiChat.onAutocompleteResponse`
                        if let Some(screen) = &mut self.screen {
                            if let Some(line) = screen.autocomplete(&mut self.ui, t.matches) {
                                self.hud.add_chat(line);
                            }
                        }
                    }
                }
                C::UpdateSignS(s) => {
                    self.tiles.insert(s.pos);
                    self.signs.insert(s.pos, s.lines);
                }
                C::SignEditorOpen(s) => {
                    // `handleSignEditorOpen`: the client tile entity is used when it exists,
                    // otherwise an empty sign is created for the editor.
                    self.tiles.insert(s.pos);
                    let raw = self.sign_lines(s.pos);
                    let lines = raw.clone().map(|l| chat::unformatted(&l, &self.ui.lang));
                    self.open(Box::new(special_screens::SignScreen::new(s.pos, raw, lines)), false);
                }
                C::UpdateTileEntity(t) => match t.nbt {
                    Some(nbt) => self.tiles.on_nbt(t.pos, Some(nbt)),
                    None => self.tiles.remove(t.pos),
                },
                C::Maps(m) => {
                    let e = self.maps.entry(m.map_id).or_insert_with(|| level::MapData { scale: m.scale, colors: vec![0; 128 * 128], icons: Vec::new() });
                    e.scale = m.scale;
                    e.icons = m.icons.iter().map(|i| (i.kind, i.x as u8, i.z as u8, i.direction)).collect();
                    if m.columns > 0 && m.rows > 0 {
                        for i in 0..m.columns as usize {
                            for j in 0..m.rows as usize {
                                let (x, z) = (m.x as usize + i, m.z as usize + j);
                                if x < 128 && z < 128 {
                                    if let Some(b) = m.data.get(i + j * m.columns as usize) {
                                        e.colors[x + z * 128] = *b;
                                    }
                                }
                            }
                        }
                    }
                }
                C::UpdateEntityNbt(_) => {
                    // Entity.clientUpdateEntityNBT is a no-op for every 1.8.9 entity.
                }
                C::ResourcePackSend(p) => {
                    self.handle_resource_pack(p.url, p.hash);
                }
                C::Particles(p) => self.spawn_particles(p),
                C::SetCompression(_) => {
                    // `PlayReader::recv_play` applies the threshold while decoding this packet, the
                    // same point `NetworkManager.setCompressionThreshold` runs in vanilla.
                }
                C::Disconnect(d) => {
                    n.closed = Some(d.reason);
                    n.disconnect();
                }
                _ => {}
        }
    }

    /// `EntityLivingBase.updatePotionMetadata` for the local player. The server never syncs
    /// data watcher 7/8 to the player itself, so the client derives the same values here.
    fn sync_local_potion(&mut self) {
        let effects = &self.entity.effects;
        let color = if effects.is_empty() {
            0
        } else {
            mc_data::potion::liquid_color(effects.iter().map(|e| (e.id, e.amplifier, e.show_particles))) as i32
        };
        // `PotionHelper.getAreAmbient`: an empty map does not read as ambient.
        let ambient = !effects.is_empty() && effects.iter().all(|e| e.ambient);
        let invisible = effects.has(mc_entity::effects::INVISIBILITY) || self.particles.local_flag_invisible();
        self.particles.set_local_potion(color, ambient, invisible);
    }

    /// `NetHandlerPlayClient.handleChangeGameState`: every state, including the demo prompts
    /// and the End credits.
    pub(crate) fn change_game_state(&mut self, g: mc_protocol::packets::ChangeGameState) {
        // `S2BPacketChangeGameState.MESSAGE_NAMES`: only state 0 has a chat message.
        const MESSAGE_NAMES: &[Option<&str>] = &[Some("tile.bed.notValid")];
        // The message is printed before the state switch, and only for a named index.
        if let Some(Some(key)) = MESSAGE_NAMES.get(g.reason as usize) {
            self.hud.add_chat(self.ui.lang.get(key));
        }
        let f = g.value;
        let j = (f + 0.5).floor() as i32;
        match g.reason {
            1 => {
                self.info.raining = true;
                self.weather.set_rain(0.);
            }
            2 => {
                self.info.raining = false;
                self.weather.set_rain(1.);
            }
            // `PlayerControllerMP.setGameType`: the game type and its capabilities only. The
            // `gameMode.changed` line is printed by `CommandGameMode`, not by this packet.
            3 => self.entity.set_game_type(level::GameType::by_id(j)),
            4 => {
                // displayGuiScreen(new GuiWinGame()): the credits scroll and then respawn.
                let end = self.assets.read("texts/end.txt");
                let credits = self.assets.read("texts/credits.txt");
                self.open(Box::new(screens::WinGame::new(end, credits, self.name.clone())), false);
                // `GuiWinGame.updateScreen`'s first tick: `func_181557_a` then the credits track.
                if let Some(s) = &mut self.sound {
                    self.music.reset(s);
                    self.music.play(s, music::MusicType::Credits);
                }
            }
            5 => {
                let keys = [
                    mc_gui::keys::key_name(self.ui.settings.key("key.forward")),
                    mc_gui::keys::key_name(self.ui.settings.key("key.left")),
                    mc_gui::keys::key_name(self.ui.settings.key("key.back")),
                    mc_gui::keys::key_name(self.ui.settings.key("key.right")),
                    mc_gui::keys::key_name(self.ui.settings.key("key.jump")),
                    mc_gui::keys::key_name(self.ui.settings.key("key.inventory")),
                ];
                if f == 0.0 {
                    self.open(Box::new(screens::DemoScreen::default()), false);
                } else if f == 101.0 {
                    let text = self.ui.lang.format("demo.help.movement", &[&keys[0], &keys[1], &keys[2], &keys[3]]);
                    self.hud.add_chat(text);
                } else if f == 102.0 {
                    let text = self.ui.lang.format("demo.help.jump", &[&keys[4]]);
                    self.hud.add_chat(text);
                } else if f == 103.0 {
                    let text = self.ui.lang.format("demo.help.inventory", &[&keys[5]]);
                    self.hud.add_chat(text);
                }
                // 1.8.9 sends 101, 102 and 103 only; there is no fourth help line.
            }
            6 => {
                let p = self.entity.pos;
                let eye = mc_core::Vec3::new(p.x, p.y + self.entity.eye_height() as f64, p.z);
                self.play_sound("random.successful_hit", Some(eye), 0.18, 0.45);
            }
            7 => self.weather.set_rain(f),
            8 => self.weather.set_thunder(f),
            10 => {
                // Elder guardian curse: the mob-appearance particle at the player's feet.
                let p = self.entity.pos;
                self.particles.spawn(41, p.x, p.y, p.z, 0., 0., 0., &[], &mut self.rand, &self.models);
                self.play_sound("mob.guardian.curse", Some(p), 1.0, 1.0);
            }
            _ => {}
        }
    }

    /// `S24PacketBlockAction` → `World.addBlockEvent` → `Block.onBlockEventReceived`.
    pub(crate) fn block_action(&mut self, pos: BlockPos, block_id: i32, byte1: u8, byte2: u8) {
        let state = self.world.state_at(pos.x, pos.y, pos.z);
        if state.block().id as i32 != block_id {
            return;
        }
        match block_id {
            // BlockNote: the note block's instrument comes from its state; `byte2` is the
            // pitch step (BlockNote ignores the event id).
            25 => {
                let name = match state.value("instrument") {
                    Some("bass") => "note.bass",
                    Some("snare") => "note.snare",
                    Some("hat") => "note.hat",
                    Some("basedrum") => "note.basedrum",
                    _ => "note.harp",
                };
                let pitch = 2f32.powf((byte2 as f32 - 12.) / 12.);
                let c = mc_core::Vec3::new(pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5);
                self.play_sound(name, Some(c), 3.0, pitch);
            }
            // BlockChest/BlockEnderChest use event 1 to set the open count for the lid
            // animation: `BlockContainer.onBlockEventReceived` forwards it to the tile entity's
            // `receiveClientEvent`, which stores it as `numPlayersUsing`.
            54 | 130 | 146 => {
                self.tiles.receive_event(pos, byte1, byte2);
            }
            _ => {}
        }
    }

    /// The raw `Text1..Text4` of the sign at `pos`: `S33PacketUpdateSign` first, then the tile
    /// entity's NBT, else `""` lines.
    fn sign_lines(&self, pos: BlockPos) -> [String; 4] {
        match self.signs.get(&pos) {
            Some(lines) => lines.clone(),
            None => std::array::from_fn(|i| {
                self.world
                    .tile_entity(pos)
                    .and_then(|nbt| nbt.get(&format!("Text{}", i + 1)))
                    .and_then(mc_core::nbt::Tag::as_str)
                    .unwrap_or("\"\"")
                    .to_string()
            }),
        }
    }

    /// `getRenderViewEntity().posX/Y/Z`, used by the range check of `spawnEntityFX`.
    fn render_view_pos(&self) -> mc_core::Vec3 {
        self.camera_entity
            .and_then(|id| self.remote.map.get(&id))
            .map_or(self.entity.pos, |r| r.pos)
    }

    /// `RenderGlobal.spawnEntityFX`: particle-setting downgrade and the 16-block cut-off.
    fn particle_allowed(&mut self, ignore_range: bool, x: f64, y: f64, z: f64) -> bool {
        let mut setting = self.ui.settings.particles;
        if setting == 1 && self.rand.next_int_bounded(3) == 0 {
            setting = 2;
        }
        if !ignore_range {
            let view = self.render_view_pos();
            let (dx, dy, dz) = (view.x - x, view.y - y, view.z - z);
            if dx * dx + dy * dy + dz * dz > 256.0 || setting > 1 {
                return false;
            }
        }
        true
    }

    /// `RenderGlobal.spawnEntityFX` for a particle that needs no post-construction tweaks.
    #[allow(clippy::too_many_arguments)]
    fn spawn_world_particle(&mut self, id: i32, ignore_range: bool, x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, args: &[i32]) {
        if !self.particle_allowed(ignore_range, x, y, z) {
            return;
        }
        self.particles.spawn(id, x, y, z, vx, vy, vz, args, &mut self.rand, &self.models);
    }

    /// `RenderGlobal.playBroadcastSound`: only the wither spawn and dragon death scream.
    pub(crate) fn play_broadcast_sound(&mut self, id: i32, pos: BlockPos) {
        if id != 1013 && id != 1018 {
            return;
        }
        let view = self.render_view_pos();
        let (d0, d1, d2) = (pos.x as f64 - view.x, pos.y as f64 - view.y, pos.z as f64 - view.z);
        let d3 = (d0 * d0 + d1 * d1 + d2 * d2).sqrt();
        let (mut d4, mut d5, mut d6) = (view.x, view.y, view.z);
        if d3 > 0.0 {
            d4 += d0 / d3 * 2.0;
            d5 += d1 / d3 * 2.0;
            d6 += d2 / d3 * 2.0;
        }
        // `theWorld.playSound` (no distance delay): at the offset point.
        let p = mc_core::Vec3::new(d4, d5, d6);
        if id == 1013 {
            self.play_sound("mob.wither.spawn", Some(p), 1.0, 1.0);
        } else {
            self.play_sound("mob.enderdragon.end", Some(p), 5.0, 1.0);
        }
    }

    /// `RenderGlobal.playAuxSFX` for the non-broadcast form of `S28PacketEffect`.
    pub(crate) fn play_aux_sfx(&mut self, id: i32, pos: BlockPos, data: i32) {
        let c = mc_core::Vec3::new(pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5);
        let pitch = |r: &mut mc_core::random::JavaRandom| (r.next_float() - r.next_float()) * 0.2 + 1.0;
        match id {
            // `playSoundAtPos` uses the block centre for every sound case.
            1000 => self.play_sound("random.click", Some(c), 1.0, 1.0),
            1001 => self.play_sound("random.click", Some(c), 1.0, 1.2),
            1002 => self.play_sound("random.bow", Some(c), 1.0, 1.2),
            1003 => {
                let p = self.rand.next_float() * 0.1 + 0.9;
                self.play_sound("random.door_open", Some(c), 1.0, p);
            }
            1004 => {
                let p = 2.6 + (self.rand.next_float() - self.rand.next_float()) * 0.8;
                self.play_sound("random.fizz", Some(c), 0.5, p);
            }
            1005 => {
                // `World.playRecord`: the previous record at this position is stopped first.
                let key = format!("record:{}:{}:{}", pos.x, pos.y, pos.z);
                let name = u16::try_from(data).ok().and_then(mc_data::item::Item::by_id)
                    .filter(|i| i.classes.contains(&"ItemRecord"))
                    .map(|i| i.name.trim_start_matches("minecraft:record_").to_string());
                if let Some(s) = &mut self.sound {
                    s.stop_stream(&key);
                    if let Some(name) = &name {
                        // `PositionedSoundRecord.create(location, x, y, z)`: volume 4, linear attenuation.
                        s.play_stream_at(&key, &format!("records.{name}"), Some([pos.x as f64, pos.y as f64, pos.z as f64]), 4.0, 1.0);
                    }
                }
                if let Some(name) = name {
                    let desc = self.ui.lang.get(&format!("item.record.{name}.desc"));
                    let msg = self.ui.lang.format("record.nowPlaying", &[&desc]);
                    self.hud.set_action_bar(msg);
                }
            }
            1006 => {
                let p = self.rand.next_float() * 0.1 + 0.9;
                self.play_sound("random.door_close", Some(c), 1.0, p);
            }
            1007 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.ghast.charge", Some(c), 10.0, p);
            }
            1008 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.ghast.fireball", Some(c), 10.0, p);
            }
            1009 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.ghast.fireball", Some(c), 2.0, p);
            }
            1010 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.zombie.wood", Some(c), 2.0, p);
            }
            1011 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.zombie.metal", Some(c), 2.0, p);
            }
            1012 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.zombie.woodbreak", Some(c), 2.0, p);
            }
            1014 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.wither.shoot", Some(c), 2.0, p);
            }
            1015 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.bat.takeoff", Some(c), 0.05, p);
            }
            1016 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.zombie.infect", Some(c), 2.0, p);
            }
            1017 => {
                let p = pitch(&mut self.rand);
                self.play_sound("mob.zombie.unfect", Some(c), 2.0, p);
            }
            1020 => {
                let p = self.rand.next_float() * 0.1 + 0.9;
                self.play_sound("random.anvil_break", Some(c), 1.0, p);
            }
            1021 => {
                let p = self.rand.next_float() * 0.1 + 0.9;
                self.play_sound("random.anvil_use", Some(c), 1.0, p);
            }
            1022 => {
                let p = self.rand.next_float() * 0.1 + 0.9;
                self.play_sound("random.anvil_land", Some(c), 0.3, p);
            }
            2000 => {
                // Dispenser smoke: a 3×3 grid offset, 10 smoke columns.
                let l = data % 3 - 1;
                let i = data / 3 % 3 - 1;
                let d15 = pos.x as f64 + l as f64 * 0.6 + 0.5;
                let d17 = pos.y as f64 + 0.5;
                let d19 = pos.z as f64 + i as f64 * 0.6 + 0.5;
                for _ in 0..10 {
                    let d20 = self.rand.next_double() * 0.2 + 0.01;
                    let d21 = d15 + l as f64 * 0.01 + (self.rand.next_double() - 0.5) * i as f64 * 0.5;
                    let d4 = d17 + (self.rand.next_double() - 0.5) * 0.5;
                    let d6 = d19 + i as f64 * 0.01 + (self.rand.next_double() - 0.5) * l as f64 * 0.5;
                    let d8 = l as f64 * d20 + self.rand.next_gaussian() * 0.01;
                    let d10 = -0.03 + self.rand.next_gaussian() * 0.01;
                    let d12 = i as f64 * d20 + self.rand.next_gaussian() * 0.01;
                    self.spawn_world_particle(11, false, d21, d4, d6, d8, d10, d12, &[]);
                }
            }
            2001 => {
                let st = mc_data::StateId::try_from_network(data).unwrap_or(mc_data::StateId::AIR);
                if st.block().id != 0 {
                    self.block_break_sound(st, pos);
                    // `addBlockDestroyEffects`: destroy particles for the state the server
                    // broke, not whatever the client already applied. `getActualState` of the
                    // server's state against a different block there throws for stairs/fences
                    // (property missing); vanilla would crash, we skip those particles.
                    let (particles, world, models, rand) = (&mut self.particles, &self.world, &self.models, &mut self.rand);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| particles.block_destroy(st, pos, world, models, rand)));
                }
            }
            2002 => {
                let (d13, d14, d16) = (pos.x as f64, pos.y as f64, pos.z as f64);
                for _ in 0..8 {
                    let vx = self.rand.next_gaussian() * 0.15;
                    let vy = self.rand.next_double() * 0.2;
                    let vz = self.rand.next_gaussian() * 0.15;
                    self.spawn_world_particle(36, false, d13, d14, d16, vx, vy, vz, &[373, data]);
                }
                let color = mc_data::potion::meta_color(data);
                let (r, g, b) = (
                    (color >> 16 & 255) as f32 / 255.0,
                    (color >> 8 & 255) as f32 / 255.0,
                    (color & 255) as f32 / 255.0,
                );
                let instant = mc_data::potion::effects(data).iter().any(|e| mc_data::potion::by_id(e.id).is_some_and(|p| p.instant));
                // `EntitySpellParticleFX` is SPELL / SPELL_INSTANT; the caller tints it with
                // `setRBGColorF` and calls `multiplyVelocity((float)d22)` afterwards. The
                // brightness draw only happens when the particle actually spawned.
                for _ in 0..100 {
                    let d22 = self.rand.next_double() * 4.0;
                    let d23 = self.rand.next_double() * std::f64::consts::PI * 2.0;
                    let d24 = d23.cos() * d22;
                    let d9 = 0.01 + self.rand.next_double() * 0.5;
                    let d11 = d23.sin() * d22;
                    if !self.particle_allowed(false, d13 + d24 * 0.1, d14 + 0.3, d16 + d11 * 0.1) {
                        continue;
                    }
                    let f3 = 0.75 + self.rand.next_float() * 0.25;
                    let id = if instant { 14 } else { 13 };
                    self.particles.spawn_fx(
                        id,
                        d13 + d24 * 0.1,
                        d14 + 0.3,
                        d16 + d11 * 0.1,
                        d24,
                        d9,
                        d11,
                        &[],
                        particles::FxMods { tint: Some(([r, g, b], f3)), velocity: Some(d22 as f32) },
                        &mut self.rand,
                        &self.models,
                    );
                }
                let p = self.rand.next_float() * 0.1 + 0.9;
                self.play_sound("game.potion.smash", Some(c), 1.0, p);
            }
            2003 => {
                let (d0, d1, d2) = (pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5);
                for _ in 0..8 {
                    let vx = self.rand.next_gaussian() * 0.15;
                    let vy = self.rand.next_double() * 0.2;
                    let vz = self.rand.next_gaussian() * 0.15;
                    self.spawn_world_particle(36, false, d0, d1, d2, vx, vy, vz, &[381]);
                }
                let step = 0.15707963267948966f64;
                let mut d18 = 0.0;
                while d18 < std::f64::consts::PI * 2.0 {
                    let (cos, sin) = (d18.cos(), d18.sin());
                    self.spawn_world_particle(24, false, d0 + cos * 5.0, d1 - 0.4, d2 + sin * 5.0, cos * -5.0, 0.0, sin * -5.0, &[]);
                    self.spawn_world_particle(24, false, d0 + cos * 5.0, d1 - 0.4, d2 + sin * 5.0, cos * -7.0, 0.0, sin * -7.0, &[]);
                    d18 += step;
                }
            }
            2004 => {
                for _ in 0..20 {
                    let d3 = pos.x as f64 + 0.5 + (self.rand.next_float() as f64 - 0.5) * 2.0;
                    let d5 = pos.y as f64 + 0.5 + (self.rand.next_float() as f64 - 0.5) * 2.0;
                    let d7 = pos.z as f64 + 0.5 + (self.rand.next_float() as f64 - 0.5) * 2.0;
                    self.spawn_world_particle(11, false, d3, d5, d7, 0.0, 0.0, 0.0, &[]);
                    self.spawn_world_particle(26, false, d3, d5, d7, 0.0, 0.0, 0.0, &[]);
                }
            }
            2005 => {
                // `ItemDye.spawnBonemealParticles`: amount 0 means 15; only non-air blocks.
                // Vanilla loops `amount` times; a hostile count would freeze the client.
                let amount = if data == 0 { 15 } else { data.clamp(0, 4096) };
                let state = self.world.state_at(pos.x, pos.y, pos.z);
                if state.block().id != 0 {
                    let max_y = state.block().default_bounds[4];
                    for _ in 0..amount.max(0) {
                        let (gx, gy, gz) = (
                            self.rand.next_gaussian() * 0.02,
                            self.rand.next_gaussian() * 0.02,
                            self.rand.next_gaussian() * 0.02,
                        );
                        // `(double)((float)pos.getX() + itemRand.nextFloat())`
                        let x = (pos.x as f32 + self.rand.next_float()) as f64;
                        let y = pos.y as f64 + self.rand.next_float() as f64 * max_y;
                        let z = (pos.z as f32 + self.rand.next_float()) as f64;
                        self.spawn_world_particle(21, false, x, y, z, gx, gy, gz, &[]);
                    }
                }
            }
            _ => {}
        }
    }

    /// `Explosion.doExplosionB(true)` for `S27PacketExplosion`: sound, the big flash and the
    /// per-block normal/smoke particles, then the block removal.
    fn explosion_b(&mut self, x: &mc_protocol::packets::Explosion, edits: &mut Vec<BlockPos>) {
        let (ex, ey, ez) = (x.x as f64, x.y as f64, x.z as f64);
        let p = (1.0 + (self.rand.next_float() - self.rand.next_float()) * 0.2) * 0.7;
        self.play_sound("random.explode", Some(mc_core::Vec3::new(ex, ey, ez)), 4.0, p);
        if x.strength >= 2.0 {
            self.spawn_world_particle(2, true, ex, ey, ez, 1.0, 0.0, 0.0, &[]);
        } else {
            self.spawn_world_particle(1, true, ex, ey, ez, 1.0, 0.0, 0.0, &[]);
        }
        let base = BlockPos::new(x.x as i32, x.y as i32, x.z as i32);
        let strength = x.strength as f64;
        for &(dx, dy, dz) in &x.records {
            let bp = BlockPos::new(base.x.wrapping_add(dx as i32), base.y.wrapping_add(dy as i32), base.z.wrapping_add(dz as i32));
            let block = self.world.state_at(bp.x, bp.y, bp.z);
            // Java computes these in `float` and widens afterwards.
            let d0 = (bp.x as f32 + self.rand.next_float()) as f64;
            let d1 = (bp.y as f32 + self.rand.next_float()) as f64;
            let d2 = (bp.z as f32 + self.rand.next_float()) as f64;
            let (mut d3, mut d4, mut d5) = (d0 - ex, d1 - ey, d2 - ez);
            let d6 = (d3 * d3 + d4 * d4 + d5 * d5).sqrt();
            d3 /= d6;
            d4 /= d6;
            d5 /= d6;
            let mut d7 = 0.5 / (d6 / strength + 0.1);
            d7 *= (self.rand.next_float() * self.rand.next_float() + 0.3) as f64;
            d3 *= d7;
            d4 *= d7;
            d5 *= d7;
            self.spawn_world_particle(0, true, (d0 + ex) / 2.0, (d1 + ey) / 2.0, (d2 + ez) / 2.0, d3, d4, d5, &[]);
            self.spawn_world_particle(11, false, d0, d1, d2, d3, d4, d5, &[]);
            if block.block().id != 0 {
                self.world.set_block_state(bp, mc_data::StateId::AIR);
                edits.push(bp);
            }
        }
    }

    /// `EffectRenderer`'s server particle packet (`S2APacketParticles`).
    pub(crate) fn spawn_particles(&mut self, p: mc_protocol::packets::Particles) {
        let (x, y, z) = (p.pos.0 as f64, p.pos.1 as f64, p.pos.2 as f64);
        let (ox, oy, oz) = (p.offset.0 as f64, p.offset.1 as f64, p.offset.2 as f64);
        let speed = p.speed as f64;
        if p.count == 0 {
            let d0 = speed * ox;
            let d2 = speed * oy;
            let d4 = speed * oz;
            self.particles.spawn(p.particle_id, x, y, z, d0, d2, d4, &p.args, &mut self.rand, &self.models);
            return;
        }
        // Vanilla loops `count` times; a hostile count of 2^31 would freeze the client.
        for _ in 0..p.count.min(4096) {
            let d1 = self.av_rand.next_gaussian() * ox;
            let d3 = self.av_rand.next_gaussian() * oy;
            let d5 = self.av_rand.next_gaussian() * oz;
            let d6 = self.av_rand.next_gaussian() * speed;
            let d7 = self.av_rand.next_gaussian() * speed;
            let d8 = self.av_rand.next_gaussian() * speed;
            self.particles.spawn(p.particle_id, x + d1, y + d3, z + d5, d6, d7, d8, &p.args, &mut self.rand, &self.models);
        }
    }

    /// `EffectRenderer.renderParticles`: camera-facing quads for the current frame.
    pub(crate) fn particle_quads(&mut self, pt: f32) -> Vec<mc_render::avatar::ParticleQuad> {
        let (pos, yaw, pitch) = (self.camera.pos, self.camera.yaw, self.camera.pitch);
        let table = &self.lightmap;
        let color = |sky: u8, block: u8| {
            let i = (sky as usize * 16 + block as usize) * 4;
            [table[i] as f32 / 255., table[i + 1] as f32 / 255., table[i + 2] as f32 / 255.]
        };
        self.particles.quads(pos, yaw, pitch, pt, &self.world, color)
    }

    /// `S44PacketWorldBorder.func_179788_a` for each action.
    pub(crate) fn world_border(&mut self, w: mc_protocol::packets::WorldBorder) {
        use mc_protocol::packets::WorldBorder as W;
        match w {
            W::SetSize { diameter } => {
                self.border.size = diameter;
                self.border.target_size = diameter;
                self.border.lerp_end = 0.;
            }
            W::LerpSize { old_diameter, new_diameter, speed } => {
                // WorldBorder.setTransition(oldSize, newSize, timeInMs)
                self.border.size = old_diameter;
                self.border.target_size = new_diameter;
                self.border.lerp_start = level::now_seconds();
                self.border.lerp_end = self.border.lerp_start + speed as f64 / 1000.;
            }
            W::SetCenter { x, z } => self.border.center = (x, z),
            W::Initialize { x, z, old_diameter, new_diameter, speed, warning_distance, warning_time, .. } => {
                self.border.center = (x, z);
                self.border.size = old_diameter;
                self.border.target_size = new_diameter;
                self.border.lerp_start = level::now_seconds();
                self.border.lerp_end = self.border.lerp_start + speed as f64 / 1000.;
                self.border.warning_blocks = warning_distance;
                self.border.warning_time = warning_time;
            }
            W::SetWarningTime { time } => self.border.warning_time = time,
            W::SetWarningBlocks { blocks } => self.border.warning_blocks = blocks,
        }
    }

    /// `handleEntityMovement` / `handleEntityTeleport`: relative (×32 fixed point) or absolute.
    pub(crate) fn remote_move(&mut self, id: i32, d: (i32, i32, i32), look: Option<(i8, i8)>, on_ground: Option<bool>, relative: bool) {
        if let Some(it) = self.items.iter_mut().find(|i| i.id == id as u32) {
            // Entity.setPositionAndRotation2 for items: snap to the server position.
            let base = self.remote.map.get(&id).map(|r| r.server).unwrap_or((0, 0, 0));
            let s = if relative { (base.0.wrapping_add(d.0), base.1.wrapping_add(d.1), base.2.wrapping_add(d.2)) } else { d };
            it.pos = mc_core::Vec3::new(s.0 as f64 / 32., s.1 as f64 / 32., s.2 as f64 / 32.);
            it.bb = mc_core::Aabb::new(it.pos.x - 0.125, it.pos.y, it.pos.z - 0.125, it.pos.x + 0.125, it.pos.y + 0.25, it.pos.z + 0.125);
        }
        let Some(r) = self.remote.map.get_mut(&id) else { return };
        r.server = if relative { (r.server.0.wrapping_add(d.0), r.server.1.wrapping_add(d.1), r.server.2.wrapping_add(d.2)) } else { d };
        let (yaw, pitch) = look.map_or((r.yaw, r.pitch), |(y, p)| (entities::angle(y), entities::angle(p)));
        r.move_to(yaw, pitch, !relative);
        if let Some(g) = on_ground {
            r.on_ground = g;
        }
    }

    /// `PlayerControllerMP.sendSlotPacket`: in creative the client owns the inventory and
    /// reports each changed slot (container numbering) with C10; thrown stacks use slot -1.
    pub(crate) fn sync_creative(&mut self) {
        if self.net.is_none() || !self.entity.creative {
            self.creative_sync = None;
            return;
        }
        let now = (self.ui.inventory, self.ui.armor);
        let Some((inv, armor)) = self.creative_sync.replace(now) else { return };
        let mut packets = Vec::new();
        for i in 0..36 {
            if inv[i] != now.0[i] {
                let slot = if i < 9 { 36 + i } else { i };
                packets.push((slot as i16, now.0[i]));
            }
        }
        for i in 0..4 {
            if armor[i] != now.1[i] {
                packets.push((5 + i as i16, now.1[i]));
            }
        }
        for (slot, st) in packets {
            let item = net::stack_slot(&self.catalogue, st);
            self.send(mc_protocol::packets::PlayServerbound::CreativeInventoryAction(mc_protocol::packets::CreativeInventoryAction { slot, item }));
        }
    }

    /// Sends the open window's queued clicks (`PlayerControllerMP.windowClick`) and its close.
    pub(crate) fn flush_window(&mut self) {
        let Some(w) = self.open_window.clone() else { return };
        let (clicks, closed, id, mut tx) = {
            let mut w = w.lock().unwrap();
            (std::mem::take(&mut w.clicks), w.closed, w.id, w.transaction)
        };
        for c in clicks {
            tx += 1;
            let item = net::stack_slot(&self.catalogue, c.item);
            self.send(mc_protocol::packets::PlayServerbound::ClickWindow(mc_protocol::packets::ClickWindow { window_id: id, slot: c.slot, button: c.button, action: tx, mode: c.mode, item }));
        }
        w.lock().unwrap().transaction = tx;
        if closed {
            self.send(mc_protocol::packets::PlayServerbound::CloseWindowC(mc_protocol::packets::CloseWindowC { window_id: id }));
            self.open_window = None;
        }
    }

    /// `ContainerPlayer` slot numbers to the client inventory.
    pub(crate) fn set_container_slot(&mut self, slot: usize, stack: Option<mc_gui::ItemStack>) {
        match slot {
            5..=8 => self.ui.armor[slot - 5] = stack,
            9..=35 => self.ui.inventory[slot] = stack,
            36..=44 => self.ui.inventory[slot - 36] = stack,
            _ => {}
        }
    }

    /// `WorldClient.doPreChunk(x, z, false)`
    /// `Achievement.theItemStack`, drawn by the catalogue's item renderer in the toast.
    fn achievement_icon(&self, a: &mc_gui::achievement::Achievement) -> Option<mc_gui::achievement::ItemIcon> {
        let cat = self.catalogue.clone();
        mc_gui::achievement::item_icon(a, cat.index_of(a.item, a.damage as u16), move |ui, b, stack, x, y| cat.draw(ui, b, stack, x, y))
    }

    pub(crate) fn unload_chunk(&mut self, x: i32, z: i32) {
        self.tiles.unload_chunk(x, z);
        self.signs.retain(|pos, _| pos.x >> 4 != x || pos.z >> 4 != z);
        if self.world.remove(x, z).is_some() {
            if let Some(pool) = &mut self.mesh_workers { for sy in 0..16 { pool.invalidate((x, sy, z), false); } }
            if let Some(r) = &mut self.renderer {
                let empty = mc_render::mesh::ChunkMesh::default();
                for sy in 0..16 {
                    r.upload_section(x, sy, z, &empty);
                }
            }
            // `WorldClient.doPreChunk(x, z, false)`
            self.world.mark_block_range_for_render_update(x * 16, 0, z * 16, x * 16 + 15, 256, z * 16 + 15);
        }
    }

}

#[cfg(test)]
#[path = "../../mc-protocol/tests/seeds/mod.rs"]
mod seeds;

#[cfg(test)]
mod hostile_tests {
    //! Hostile-but-valid packets through a real socket into `App::update`: whatever the
    //! server sends, the client must keep running (or show the disconnect screen).
    use super::*;
    use mc_protocol::packets::*;
    use std::io::{Read, Write};

    use super::seeds;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        fn byte(&mut self) -> u8 {
            match self.below(6) {
                0 => 0,
                1 => 0xff,
                2 => 0x7f,
                3 => 0x80,
                _ => self.next() as u8,
            }
        }
    }

    /// Serves a login, then streams `bodies` and hangs up without a Disconnect packet.
    fn serve(bodies: Vec<Vec<u8>>) -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut d = mc_protocol::frame::Decoder::new();
            let (mut buf, mut seen) = ([0u8; 4096], 0);
            while seen < 2 {
                while let Ok(Some(_)) = d.next_packet() {
                    seen += 1;
                }
                if seen < 2 {
                    let n = s.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    d.feed(&buf[..n]);
                }
            }
            let mut out = Vec::new();
            let ok = LoginClientbound::LoginSuccess(LoginSuccess { uuid: "0-0".into(), name: "a".into() });
            mc_protocol::frame::encode(&ok.to_bytes(), -1, &mut out);
            for b in bodies {
                mc_protocol::frame::encode(&b, -1, &mut out);
            }
            let _ = s.write_all(&out);
            std::thread::sleep(std::time::Duration::from_millis(300));
        });
        port
    }

    /// Runs the packets through `App::update`; `None` when the jar is not available.
    fn run(bodies: Vec<Vec<u8>>) -> Option<String> {
        let n = bodies.len();
        let jar = find_jar()?;
        let mut assets = AssetSource::new();
        assets.push(&jar).unwrap();
        let models = BlockModels::load(&assets);
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let mut app = App::new(assets, models, world, (8, 8), Settings::default());
        app.in_game = true;
        let port = serve(bodies);
        app.net = Some(net::Net::connect("127.0.0.1", port, "a", None, &Default::default(), &Default::default(), &Default::default(), false).unwrap());
        let t = Instant::now();
        while app.net.is_some() && t.elapsed().as_secs() < 60 {
            app.update(0.05);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for _ in 0..40 {
            app.update(0.05);
        }
        let closed = if app.net.is_none() { Some("disconnected".to_string()) } else { None };
        eprintln!("hostile: {n} packets processed, connection ended with {closed:?}");
        closed
    }

    fn skipped(p: &PlayClientbound) -> bool {
        // These need the network or change framing, not game state.
        matches!(p, PlayClientbound::Disconnect(_) | PlayClientbound::SetCompression(_) | PlayClientbound::ResourcePackSend(_))
    }

    #[test]
    fn hand_picked_hostile_packets() {
        let pos = BlockPos::new(1 << 24, 255, 0);
        let mut sections: [Option<mc_protocol::chunk::WireSection>; 16] = Default::default();
        sections[15] = Some(mc_protocol::chunk::WireSection::empty(true));
        let (mask, data) = mc_protocol::chunk::encode(&sections, Some(&[0; 256]), true);
        let p: Vec<PlayClientbound> = vec![
            ChunkData { x: i32::MAX, z: i32::MIN, full: true, bitmask: mask, data: data.clone() }.into(),
            ChunkData { x: 0, z: 0, full: false, bitmask: mask, data: vec![1, 2, 3] }.into(),
            ChunkData { x: 0, z: 0, full: true, bitmask: 0, data: vec![] }.into(),
            MultiBlockChange { chunk_x: i32::MAX, chunk_z: i32::MIN, records: vec![(0xFFFF, i32::MAX), (0, -1)] }.into(),
            BlockChange { pos, state: -1 }.into(),
            BlockChange { pos: BlockPos::new(0, 4000, 0), state: 1 << 4 }.into(),
            Explosion { x: 1e30, y: f32::NAN, z: -1e30, strength: f32::INFINITY, records: vec![(127, -128, 127)], player_motion: (f32::NAN, 0., 0.) }.into(),
            Particles { particle_id: 41, long_distance: true, pos: (f32::NAN, 0., 0.), offset: (1., 1., 1.), speed: 1., count: i32::MAX, args: vec![9999999, 1] }.into(),
            EntityTeleport { entity_id: 5, x: i32::MAX, y: i32::MIN, z: 0, yaw: 0, pitch: 0, on_ground: false }.into(),
            SpawnMob { entity_id: 5, kind: 255, x: 0, y: 0, z: 0, yaw: 0, pitch: 0, head_pitch: 0, vx: 0, vy: 0, vz: 0, metadata: vec![] }.into(),
            EntityTeleport { entity_id: 5, x: i32::MAX, y: i32::MAX, z: i32::MAX, yaw: 0, pitch: 0, on_ground: false }.into(),
            EntityRelMove { entity_id: 5, dx: 127, dy: 127, dz: 127, on_ground: true }.into(),
            EntityMetadata { entity_id: 99999, metadata: vec![(6, mc_protocol::MetaValue::Byte(1)), (16, mc_protocol::MetaValue::String("x".into()))] }.into(),
            DestroyEntities { ids: vec![-1, i32::MIN, 12345] }.into(),
            EntityEquipment { entity_id: -7, slot: -3, item: None }.into(),
            OpenWindow { window_id: 3, kind: "minecraft:furnace".into(), title: "\"\"".into(), slots: 0, entity_id: 0 }.into(),
            SetSlot { window_id: 3, slot: -5, item: seeds::item() }.into(),
            SetSlot { window_id: 3, slot: i16::MAX, item: seeds::item() }.into(),
            SetSlot { window_id: -1, slot: 5, item: seeds::item() }.into(),
            SetSlot { window_id: 0, slot: -1, item: seeds::item() }.into(),
            SetSlot { window_id: -2, slot: i16::MIN, item: seeds::item() }.into(),
            WindowProperty { window_id: 3, property: -1, value: 0 }.into(),
            WindowItems { window_id: 9, items: vec![seeds::item(); 500] }.into(),
            WindowItems { window_id: 3, items: vec![seeds::item(); 500] }.into(),
            OpenWindow { window_id: 4, kind: "minecraft:chest".into(), title: "\"\"".into(), slots: 255, entity_id: 0 }.into(),
            WindowItems { window_id: 4, items: vec![seeds::item(); 400] }.into(),
            CloseWindowS { window_id: 77 }.into(),
            Respawn { dimension: i32::MIN, difficulty: 200, game_mode: 99, level_type: "x".into() }.into(),
            Respawn { dimension: -1, difficulty: 0, game_mode: 0, level_type: "x".into() }.into(),
            PlayerPosLookS { x: 0., y: 70., z: 0., yaw: 1e30, pitch: 1e30, flags: 0 }.into(),
            PlayerPosLookS { x: 1e300, y: -1e300, z: 0., yaw: f32::INFINITY, pitch: f32::NEG_INFINITY, flags: 0 }.into(),
            PlayerPosLookS { x: f64::NAN, y: f64::INFINITY, z: 1e300, yaw: f32::NAN, pitch: 1e30, flags: 0xFF }.into(),
            HeldItemChangeS { slot: -1 }.into(),
            Maps { map_id: -1, scale: -1, icons: vec![], columns: 255, rows: 255, x: 255, z: 255, data: vec![1; 10] }.into(),
            BlockAction { pos, byte1: 255, byte2: 255, block_id: 54 }.into(),
            BlockBreakAnim { breaker_id: 1, pos, stage: -1 }.into(),
            SpawnPainting { entity_id: 8, title: "".into(), pos, direction: 255 }.into(),
            ChangeGameState { reason: 255, value: f32::NAN }.into(),
            // The demo/credits states open screens built from the jar.
            ChangeGameState { reason: 4, value: 0.0 }.into(),
            ChangeGameState { reason: 5, value: 0.0 }.into(),
            ChangeGameState { reason: 5, value: 101.0 }.into(),
            ChangeGameState { reason: 5, value: 103.0 }.into(),
            ChangeGameState { reason: 10, value: 0.0 }.into(),
            EntityProperties { entity_id: 0, attributes: vec![AttributeSnapshot { key: "".into(), base: f64::NAN, modifiers: vec![] }] }.into(),
            UpdateHealth { health: f32::NAN, food: -5, saturation: f32::NAN }.into(),
            WorldBorder::LerpSize { old_diameter: f64::NAN, new_diameter: -1., speed: i64::MIN }.into(),
            TimeUpdate { world_age: i64::MIN, time_of_day: i64::MIN }.into(),
            TimeUpdate { world_age: i64::MAX, time_of_day: i64::MAX }.into(),
            SoundEffect { name: "no.such.sound".into(), x: i32::MAX, y: i32::MIN, z: 0, volume: f32::NAN, pitch: 255 }.into(),
            Effect { effect_id: 2001, pos, data: -1, global: true }.into(),
            Effect { effect_id: 2001, pos, data: i32::MAX, global: true }.into(),
            JoinGame { entity_id: 0, game_mode: 255, hardcore: false, dimension: 9, difficulty: 9, max_players: 0, level_type: "".into(), reduced_debug_info: false }.into(),
        ];
        let bodies = p.iter().map(|p| p.to_bytes()).collect();
        // The server hangs up afterwards: that is the "closed abruptly" case.
        if let Some(c) = run(bodies) {
            assert!(!c.contains("Internal Exception"), "{c}");
        }
    }

    #[test]
    fn mutated_packets_never_crash_the_client() {
        let rounds: u64 = std::env::var("RUSTMC_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
        for round in 0..rounds {
        let mut rng = Rng(0xA5A5_1234_5678_9ABC ^ round.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let seeds: Vec<Vec<u8>> = seeds::clientbound().iter().filter(|p| !skipped(p)).map(|p| p.to_bytes()).collect();
        let mut bodies = Vec::new();
        for _ in 0..4000 {
            let mut b = seeds[rng.below(seeds.len())].clone();
            for _ in 0..1 + rng.below(3) {
                if b.is_empty() {
                    break;
                }
                let k = rng.below(b.len());
                match rng.below(3) {
                    0 => b[k] = rng.byte(),
                    1 => b.insert(k, rng.byte()),
                    _ => {
                        b.remove(k);
                    }
                }
            }
            if let Ok(p) = PlayClientbound::read(&b) {
                if !skipped(&p) {
                    bodies.push(b);
                }
            }
        }
        assert!(bodies.len() > 500, "{}", bodies.len());
        if let Some(c) = run(bodies) {
            assert!(!c.contains("Internal Exception"), "{c}");
        }
        }
    }
}

#[cfg(test)]
mod attribute_tests {
    //! `ModifiableAttributeInstance.computeValue` and the `S20PacketEntityProperties` snapshot.
    use mc_entity::effects::{Attribute, Attributes, Modifier, MOVEMENT_SPEED};

    #[test]
    fn compute_value_applies_operations_in_order() {
        let mut a = Attribute::new(0.1, 0.0, f64::MAX);
        // Applied out of order: op 0 first, then op 1 multiplies the post-add base, then
        // op 2 scales the total.
        a.apply(Modifier { uuid: (1, 1), amount: 0.3, op: 2 });
        a.apply(Modifier { uuid: (2, 2), amount: 0.2, op: 0 });
        a.apply(Modifier { uuid: (3, 3), amount: 0.5, op: 1 });
        let expected = ((0.1 + 0.2) * (1.0 + 0.5)) * (1.0 + 0.3);
        assert!((a.value() - expected).abs() < 1e-12, "{}", a.value());
    }

    #[test]
    fn snapshot_replaces_base_and_modifiers() {
        let mut attrs = Attributes::default();
        attrs.apply_snapshot(MOVEMENT_SPEED, 0.2, [
            Modifier { uuid: (1, 2), amount: 0.5, op: 1 },
            Modifier { uuid: (3, 4), amount: 0.3, op: 2 },
        ]);
        assert!((attrs.value(MOVEMENT_SPEED) - 0.2 * 1.5 * 1.3).abs() < 1e-12);
        // A second snapshot drops the previous modifiers (`removeAllModifiers`).
        attrs.apply_snapshot(MOVEMENT_SPEED, 0.1, []);
        assert!((attrs.value(MOVEMENT_SPEED) - 0.1).abs() < 1e-12);
    }
}

#[cfg(test)]
mod effect_tests {
    //! `RenderGlobal.playAuxSFX` particle counts and `Explosion.doExplosionB`. Needs the
    //! vanilla jar for the particle sprite atlas; skips without it like the other GUI tests.
    use super::*;

    fn app() -> Option<App> {
        let jar = find_jar()?;
        let mut assets = AssetSource::new();
        assets.push(&jar).ok()?;
        let models = BlockModels::load(&assets);
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let mut app = App::new(assets, models, world, (8, 8), Settings::default());
        app.in_game = true;
        app.entity.pos = mc_core::Vec3::new(0.5, 64.5, 0.5);
        app.camera_entity = None;
        Some(app)
    }

    #[test]
    fn aux_sfx_spawns_the_vanilla_particle_counts() {
        let Some(mut app) = app() else { return };
        let pos = BlockPos::new(0, 64, 0);
        let count = |app: &mut App, id: i32, p: BlockPos, data: i32| {
            app.particles.clear();
            app.play_aux_sfx(id, p, data);
            app.particles.len()
        };
        // 2000: ten dispenser smoke columns.
        assert_eq!(count(&mut app, 2000, pos, 0), 10);
        // 2004: twenty smoke + twenty flame.
        assert_eq!(count(&mut app, 2004, pos, 0), 40);
        // 2001: the 4×4×4 block destroy burst for the server's state.
        app.world.set_block_state(pos, mc_data::StateId::try_from_network(1).unwrap());
        assert_eq!(count(&mut app, 2001, pos, 1), 64);
        // 2002: eight item-crack plus one hundred spell particles (a sprite may be missing).
        let n = count(&mut app, 2002, pos, 0);
        assert!((100..=108).contains(&n), "{n} potion particles");
        // 2003: eight ender-eye item cracks plus forty portal pairs.
        let n = count(&mut app, 2003, pos, 0);
        assert!((80..=88).contains(&n), "{n} ender-eye particles");
        // 2005: fifteen bonemeal sparkles on a solid block.
        let p = BlockPos::new(0, 63, 0);
        app.world.set_block_state(p, mc_data::StateId::try_from_network(1).unwrap());
        assert_eq!(count(&mut app, 2005, p, 0), 15);
        // 2005 with an explicit amount.
        assert_eq!(count(&mut app, 2005, p, 7), 7);
        // Broadcast-only events never fall through to the aux table.
        app.particles.clear();
        app.play_broadcast_sound(2004, pos);
        assert_eq!(app.particles.len(), 0);
    }

    #[test]
    fn explosion_b_plays_the_flash_particles_and_removes_the_blocks() {
        let Some(mut app) = app() else { return };
        let pos = BlockPos::new(0, 64, 0);
        app.world.set_block_state(pos, mc_data::StateId::try_from_network(1).unwrap());
        app.particles.clear();
        let x = mc_protocol::packets::Explosion {
            x: 0.5,
            y: 64.5,
            z: 0.5,
            strength: 1.0,
            // `S27` offsets: zero is the block at `(int)x, (int)y, (int)z`.
            records: vec![(0, 0, 0)],
            player_motion: (0.0, 0.0, 0.0),
        };
        let mut edits = Vec::new();
        app.explosion_b(&x, &mut edits);
        // One large-explosion flash plus one normal/smoke pair for the affected block.
        assert_eq!(app.particles.len(), 3);
        assert_eq!(app.world.state_at(pos.x, pos.y, pos.z).block().id, 0);
        assert_eq!(edits, vec![pos]);
    }
}
