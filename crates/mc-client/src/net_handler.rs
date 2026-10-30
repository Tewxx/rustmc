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
            let p = match inc {
                net::Incoming::Packet(p) => *p,
                net::Incoming::Closed(reason) => {
                    if let Some(n) = &mut self.net {
                        if n.closed.is_none() {
                            n.closed = Some(reason);
                        }
                    }
                    continue;
                }
            };
            // `NetworkManager.exceptionCaught`: a handler that throws ends the connection with
            // "Internal Exception" (GuiDisconnected) rather than taking the whole client down.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.handle_packet(p, &mut edits)));
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
    fn handle_packet(&mut self, p: mc_protocol::packets::PlayClientbound, edits: &mut Vec<BlockPos>) {
        use mc_protocol::packets::{PlayClientbound as C, PlayServerbound as S, *};
        let Some(n) = &mut self.net else { return };
        match p {
                C::KeepAliveS(k) => n.send(S::KeepAliveC(KeepAliveC { key: k.key })),
                C::JoinGame(j) => {
                    n.entity_id = j.entity_id;
                    self.particles.clear();
                    // A new WorldClient brings a fresh Scoreboard; the player info map stays.
                    let players = std::mem::take(&mut self.scoreboard.players);
                    self.scoreboard = scoreboard::Scoreboard { players, ..Default::default() };
                    self.player.hardcore = j.hardcore;
                    self.has_valid_health = false;
                    let view = self.ui.settings.render_distance as i8;
                    n.send_join_replies("en_US", view, self.ui.settings.chat_visibility as i8, self.ui.settings.chat_colors, self.ui.settings.model_parts);
                    self.entity.set_game_type(level::GameType::by_id(j.game_mode as i32));
                    self.info.difficulty = j.difficulty;
                    self.player.max_health = 20.;
                    self.entity.effects.clear();
                    self.entity.attributes = Default::default();
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
                    self.world.set_block_state(b.pos, st);
                    edits.push(b.pos);
                }
                C::MultiBlockChange(m) => {
                    for (p, state) in m.records {
                        let pos = BlockPos::new(m.chunk_x.wrapping_mul(16).wrapping_add((p >> 12) as i32), (p & 255) as i32, m.chunk_z.wrapping_mul(16).wrapping_add(((p >> 8) & 15) as i32));
                        let st = u16::try_from(state).ok().and_then(mc_data::StateId::try_from_raw).unwrap_or(mc_data::StateId::AIR);
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
                    let component = chat::parse(&c.json, &self.ui.lang);
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
                    self.open_window = Some(w.clone());
                    self.parents.clear();
                    self.screen = None;
                    self.open(Box::new(container::ContainerScreen::new(w, self.catalogue.clone())), false);
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
                        if w.id == p.window_id && (0..4).contains(&p.property) {
                            w.props[p.property as usize] = p.value;
                        }
                    }
                }
                C::ConfirmTransactionS(t) => {
                    if !t.accepted {
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
                    let p = ef.pos;
                    let c = mc_core::Vec3::new(p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5);
                    match ef.effect_id {
                        // RenderGlobal.playAuxSFX
                        1000 => self.play_sound("random.click", Some(c), 1.0, 1.0),
                        1001 => self.play_sound("random.click", Some(c), 1.0, 1.2),
                        1002 => self.play_sound("random.bow", Some(c), 1.0, 1.2),
                        1003 => self.play_sound("random.door_open", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1006 => self.play_sound("random.door_close", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1007 | 1008 | 1009 => self.play_sound("mob.ghast.fireball", Some(c), 10.0, (rand_f64() as f32 - rand_f64() as f32) * 0.2 + 1.0),
                        1020 => self.play_sound("random.anvil_break", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1021 => self.play_sound("random.anvil_use", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        1022 => self.play_sound("random.anvil_land", Some(c), 0.3, rand_f64() as f32 * 0.1 + 0.9),
                        2001 => {
                            let st = mc_data::StateId::try_from_network(ef.data).unwrap_or(mc_data::StateId::AIR);
                            if st.block().id != 0 {
                                self.block_break_sound(st, p);
                                // `RenderGlobal.playAuxSFX`: destroy particles for the state the
                                // server broke, not whatever the client already applied.
                                // `getActualState` of the server's state against whatever block the
                                // client holds there throws for stairs/fences (property missing) when
                                // they differ; vanilla would crash, we just skip the particles.
                                let (particles, world, models, rand) = (&mut self.particles, &self.world, &self.models, &mut self.rand);
                                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| particles.block_destroy(st, p, world, models, rand)));
                            }
                        }
                        2002 => self.play_sound("game.potion.smash", Some(c), 1.0, rand_f64() as f32 * 0.1 + 0.9),
                        _ => {}
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
                    if o.kind == 2 {
                        let v = mc_core::Vec3::new(o.velocity.0 as f64 / 8000., o.velocity.1 as f64 / 8000., o.velocity.2 as f64 / 8000.);
                        self.pending_items.insert(o.entity_id, (r.pos, v));
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
                                (1, mc_protocol::MetaValue::Short(a)) => self.player.air = *a as i32,
                                (6, mc_protocol::MetaValue::Float(hp)) => self.player.health = hp.clamp(0., self.player.max_health),
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
                    }
                }
                C::Explosion(x) => {
                    let (a, b, c) = x.player_motion;
                    // Non-finite or huge motion would poison the player's position for good.
                    let sane = |v: f32| if v.is_finite() { v.clamp(-100., 100.) as f64 } else { 0. };
                    self.entity.motion.x += sane(a);
                    self.entity.motion.y += sane(b);
                    self.entity.motion.z += sane(c);
                    let base = BlockPos::new(x.x as i32, x.y as i32, x.z as i32);
                    for (dx, dy, dz) in x.records {
                        let p = BlockPos::new(base.x.wrapping_add(dx as i32), base.y.wrapping_add(dy as i32), base.z.wrapping_add(dz as i32));
                        self.world.set_block_state(p, mc_data::StateId::AIR);
                        edits.push(p);
                    }
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
                C::EntityEffect(e) if e.entity_id == n.entity_id => {
                    // handleEntityEffect: the byte is set when particles show.
                    self.entity.effects.add(mc_entity::effects::PotionEffect {
                        id: e.effect_id as u8,
                        duration: e.duration,
                        amplifier: e.amplifier as i32,
                        ambient: false,
                        show_particles: e.hide_particles != 0,
                        duration_max: e.duration == 32767,
                    });
                }
                C::RemoveEntityEffect(e) if e.entity_id == n.entity_id => {
                    self.entity.effects.remove(e.effect_id);
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
                    self.has_valid_health = false;
                    self.world.has_sky = r.dimension == 0;
                    let keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
                    for (x, z) in keys {
                        self.unload_chunk(x, z);
                    }
                    self.entity.dead = false;
                    self.entity.effects.clear();
                    self.entity.attributes = Default::default();
                    self.player.max_health = 20.;
                    self.entity.set_game_type(level::GameType::by_id(r.game_mode as i32));
                    self.info.difficulty = r.difficulty;
                }
                C::ChangeGameState(g) => {
                    let j = (g.value + 0.5).floor() as i32;
                    match g.reason {
                        0 => self.hud.add_chat(self.ui.lang.get("tile.bed.notValid")),
                        1 => {
                            self.info.raining = true;
                            self.weather.set_rain(0.);
                        }
                        2 => {
                            self.info.raining = false;
                            self.weather.set_rain(1.);
                        }
                        3 => {
                            self.hud.add_chat(self.ui.lang.get("gameMode.changed"));
                            self.entity.set_game_type(level::GameType::by_id(j));
                        }
                        7 => self.weather.set_rain(g.value),
                        8 => self.weather.set_thunder(g.value),
                        _ => {}
                    }
                }
                C::PlayerAbilitiesS(a) => {
                    // handlePlayerAbilities
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
                        // Leash to `vehicle_id`; the leash renderer is not ported yet.
                    } else if a.vehicle_id == 0 {
                        self.attached.remove(&a.entity_id);
                    } else {
                        self.attached.insert(a.entity_id, a.vehicle_id);
                    }
                }
                C::EntityIdle(_) => {}
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
                    self.camera_entity = if c.entity_id == n.entity_id { None } else { Some(c.entity_id) };
                }
                C::WorldBorder(w) => self.world_border(w),
                C::PluginMessageS(p) => {
                    if p.channel == "MC|Brand" {
                        // The payload is a VarInt-prefixed UTF-8 string.
                        self.brand = packet_string(&p.data);
                    }
                    // MC|TrList (villager trades) and MC|BOpen (book) need their GUIs, which are
                    // not ported yet; the packet is accepted so the connection stays in sync.
                }
                C::Statistics(s) => {
                    for (k, v) in s.entries {
                        self.stats.insert(k, v);
                    }
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
                    self.signs.insert(s.pos, s.lines);
                }
                C::SignEditorOpen(_) => {
                    // GuiEditSign is not ported yet; the server closes the window itself.
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
                    // A login-state packet; a play-state copy is ignored like vanilla's local
                    // channel check.
                }
                C::Disconnect(d) => {
                    n.closed = Some(d.reason);
                    n.disconnect();
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

    /// `NetHandlerPlayClient.handleParticles`: a single particle with the packet velocity, or
    /// `count` particles scattered with Gaussian offsets and speeds (`avRandomizer`).
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
        r.move_to(yaw, pitch);
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
    pub(crate) fn unload_chunk(&mut self, x: i32, z: i32) {
        self.tiles.unload_chunk(x, z);
        if self.world.remove(x, z).is_some() {
            if let Some(r) = &mut self.renderer {
                let empty = mc_render::mesh::ChunkMesh::default();
                for sy in 0..16 {
                    r.upload_section(x, sy, z, &empty);
                }
            }
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                for sy in 0..16 {
                    self.world.dirty.insert((x.wrapping_add(dx), sy, z.wrapping_add(dz)));
                }
            }
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
        app.net = Some(net::Net::connect("127.0.0.1", port, "a", None, &Default::default(), &Default::default(), &Default::default()).unwrap());
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
