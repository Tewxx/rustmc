//! Player state, digging, attacking, inventory and chat helpers.

use crate::*;

impl App {
    /// The `Item` in the selected hotbar slot.
    pub(crate) fn held_registry_item(&self) -> Option<&'static mc_data::item::Item> {
        let s = self.ui.inventory[self.player.selected_slot]?;
        mc_data::item::Item::by_id(self.catalogue.items[s.item].id)
    }

    /// `EntityPlayer.getTotalArmorValue`: the armor items' `damageReduceAmount`.
    pub(crate) fn total_armor_value(&self) -> i32 {
        self.ui
            .armor
            .iter()
            .flatten()
            .filter_map(|s| mc_data::item::Item::by_id(self.catalogue.items[s.item].id))
            .filter(|i| i.is_a("ItemArmor"))
            .map(|i| i.num("damageReduceAmount") as i32)
            .sum()
    }

    /// `EntityPlayerSP.setPlayerSPHealth`
    pub(crate) fn set_player_sp_health(&mut self, health: f32) {
        let max = self.player.max_health;
        if !self.has_valid_health {
            self.player.health = health.clamp(0., max);
            self.has_valid_health = true;
            return;
        }
        let f = self.player.health - health;
        if f <= 0. {
            self.player.health = health.clamp(0., max);
            if f < 0. {
                self.entity.hurt_resistant_time = 10;
            }
        } else {
            self.entity.hurt_resistant_time = 20;
            self.damage_entity_generic(f);
            self.entity.hurt_time = 10;
            self.entity.max_hurt_time = 10;
        }
    }

    /// `EntityPlayer.damageEntity(DamageSource.generic, amount)` as the client runs it.
    /// Armor durability and protection enchantments need item damage/NBT (not modelled yet).
    pub(crate) fn damage_entity_generic(&mut self, mut amount: f32) {
        // isBlocking: a sword held up halves the damage (rounded up by the +1).
        let blocking = self.item_in_use.is_some_and(|s| item_use::action(self.catalogue.items[s.item].id).0 == item_use::Action::Block);
        if blocking && amount > 0. {
            amount = (1.0 + amount) * 0.5;
        }
        // applyArmorCalculations
        let i = 25 - self.total_armor_value();
        let f = amount * i as f32;
        amount = f / 25.0;
        // applyPotionDamageCalculations
        if let Some(e) = self.entity.effects.get(11) {
            let j = 25 - (e.amplifier + 1) * 5;
            let f = amount * j as f32;
            amount = f / 25.0;
        }
        if amount <= 0. {
            amount = 0.;
        }
        let before = amount;
        amount = (amount - self.player.absorption).max(0.);
        self.player.absorption -= before - amount;
        if amount != 0. {
            self.player.health = (self.player.health - amount).clamp(0., self.player.max_health);
        }
    }

    /// `Entity.isInsideOfMaterial`: the eye below the liquid surface.
    pub(crate) fn is_inside_of_material(&self, m: mc_data::Material) -> bool {
        let e = &self.entity;
        let d0 = e.pos.y + e.eye_height() as f64;
        let pos = BlockPos::new(e.pos.x.floor() as i32, d0.floor() as i32, e.pos.z.floor() as i32);
        let st = self.world.state_at(pos.x, pos.y, pos.z);
        if st.block().material != m {
            return false;
        }
        let mut meta = st.meta() as i32;
        if meta >= 8 {
            meta = 0;
        }
        let f = (meta + 1) as f32 / 9.0 - 0.11111111;
        let f1 = (pos.y + 1) as f32 - f;
        d0 < f1 as f64
    }

    /// `Block.getPlayerRelativeBlockHardness`
    pub(crate) fn relative_hardness(&self, pos: BlockPos) -> f32 {
        let block = self.world.state_at(pos.x, pos.y, pos.z).block();
        if block.hardness < 0. {
            return 0.;
        }
        let held = self.held_registry_item();
        // EntityPlayer.getToolDigEfficiency
        let mut speed = tools::dig_speed(held, block);
        let stack = self.ui.inventory[self.player.selected_slot];
        if speed > 1. {
            let i = tooltip::enchantment_level(stack.as_ref(), 32);
            if i > 0 && stack.is_some() {
                speed += (i * i + 1) as f32;
            }
        }
        speed = self.entity.effects.dig_multiplier(speed);
        // getAquaAffinityModifier: the best level over the held item and armor.
        let aqua = std::iter::once(stack).chain(self.ui.armor.iter().copied()).map(|s| tooltip::enchantment_level(s.as_ref(), 6)).max().unwrap_or(0) > 0;
        if self.is_inside_of_material(mc_data::Material::Water) && !aqua {
            speed /= 5.;
        }
        if !self.entity.on_ground {
            speed /= 5.;
        }
        let harvest = tools::can_harvest(held, block);
        speed / block.hardness / if harvest { 30. } else { 100. }
    }

    /// Runs world logic through the integrated server and applies what it produced.
    #[cfg(feature = "singleplayer")]
    pub(crate) fn with_level<R>(&mut self, f: impl FnOnce(&mut server::Lv) -> R) -> R {
        let mut fx = server::Effects::default();
        let r = f(&mut server::Lv {
            world: &mut self.world,
            server: &mut self.server,
            fx: &mut fx,
            total_time: self.info.total_time,
            rules: Some(&self.info.rules),
        });
        for (pos, state, chance, _fortune) in fx.drops {
            // Block.dropBlockAsItemWithChance
            for (id, meta, count) in tools::drops(state, &mut self.rand) {
                if self.rand.next_float() <= chance {
                    if let Some(i) = self.catalogue.index_of(id, meta) {
                        let id = self.alloc_entity_id();
                        let mut e = mc_entity::item::ItemEntity::from_block(id, pos, (i, count), &mut self.rand);
                        e.damage = self.catalogue.items[i].meta as i16;
                        self.items.push(e);
                    }
                }
            }
        }
        for (p, name, vol, pitch) in fx.sounds {
            self.play_sound(&name, Some(mc_core::Vec3::new(p[0], p[1], p[2])), vol, pitch);
        }
        self.refresh_edits(&fx.edits);
        r
    }

    pub(crate) fn destroy_block(&mut self, pos: BlockPos) {
        // PlayerControllerMP.onPlayerDestroyBlock: a creative sword never breaks blocks.
        if self.entity.creative && self.held_registry_item().is_some_and(|i| i.is_a("ItemSword")) {
            return;
        }
        let state = self.world.state_at(pos.x, pos.y, pos.z);
        // PlayerControllerMP.onPlayerDestroyBlock: the client removes the block itself in
        // multiplayer; only the integrated server runs the full block logic.
        #[cfg(feature = "singleplayer")]
        let removed = if self.net.is_none() {
            self.with_level(|lv| mc_logic::Level::set_state(lv, pos, mc_data::StateId::AIR, mc_logic::flags::DEFAULT))
        } else {
            self.world.set_block(pos, 0, 0)
        };
        #[cfg(not(feature = "singleplayer"))]
        let removed = self.world.set_block(pos, 0, 0);
        if removed {
            self.block_break_sound(state, pos);
            if self.is_local() && !self.entity.creative && tools::can_harvest(self.held_registry_item(), state.block()) {
                for (id, meta, count) in tools::drops(state, &mut self.rand) {
                    if let Some(i) = self.catalogue.index_of(id, meta) {
                        let id = self.alloc_entity_id();
                        let mut e = mc_entity::item::ItemEntity::from_block(id, pos, (i, count), &mut self.rand);
                        e.damage = self.catalogue.items[i].meta as i16;
                        self.items.push(e);
                    }
                }
            }
            self.refresh_edits(&[pos]);
        }
        // ItemStack.onBlockDestroyed for the held tool (survival only).
        if !self.entity.creative {
            if let Some(item) = self.held_registry_item() {
                let block = state.block();
                let amount = if item.is_a("ItemSword") {
                    if block.hardness != 0. { 2 } else { 0 }
                } else if item.is_a("ItemTool") {
                    if block.hardness != 0. { 1 } else { 0 }
                } else if item.is_a("ItemShears") {
                    let shearable = block.material == mc_data::Material::Leaves
                        || matches!(block.name, "minecraft:web" | "minecraft:tallgrass" | "minecraft:vine" | "minecraft:tripwire" | "minecraft:wool");
                    if shearable { 1 } else { 0 }
                } else {
                    0
                };
                if amount > 0 {
                    self.damage_item(self.player.selected_slot, amount);
                }
            }
        }
    }

    /// `PlayerControllerMP.attackEntity` and the client half of
    /// `EntityPlayer.attackTargetEntityWithCurrentItem`.
    pub(crate) fn attack_entity(&mut self, id: i32) {
        let sel = self.player.selected_slot as i32;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel);
        }
        self.send(mc_protocol::packets::PlayServerbound::UseEntity(mc_protocol::packets::UseEntity { entity_id: id, action: mc_protocol::packets::UseEntityAction::Attack, hit: None }));
        if self.entity.game_type == level::GameType::Spectator {
            return;
        }
        // EntityPlayer.attackTargetEntityWithCurrentItem. The client's attack damage attribute is
        // never synced (base 1), so the hit always goes through; only EntityOtherPlayerMP's
        // attackEntityFrom returns true on the client, other living entities return false.
        if !self.remote.map.get(&id).is_some_and(|e| e.kind == entities::Kind::Player) {
            return;
        }
        let sel = self.player.selected_slot;
        let held = self.ui.inventory[sel];
        let mut i = tooltip::enchantment_level(held.as_ref(), 19);
        if self.entity.sprinting {
            i += 1;
        }
        if i > 0 {
            self.entity.motion.x *= 0.6;
            self.entity.motion.z *= 0.6;
            self.entity.set_sprinting(false);
        }
        // ItemStack.hitEntity: swords take 1 damage, tools 2.
        if let Some(h) = held {
            let item = mc_data::item::Item::by_id(self.catalogue.items[h.item].id);
            let amount = if item.is_some_and(|i| i.is_a("ItemSword")) {
                1
            } else if item.is_some_and(|i| i.is_a("ItemTool")) {
                2
            } else {
                0
            };
            if amount > 0 {
                self.damage_item(sel, amount);
            }
        }
        // EntityPlayerSP.onCriticalHit / onEnchantmentCritical: a falling, ungrounded hit emits
        // the CRIT emitter at the target; Sharpness adds the MAGIC_CRIT emitter.
        let Some(target) = self.remote.map.get(&id) else { return };
        let (pos, width, height) = (target.pos, target.width, target.height);
        let ladder = {
            let mut w = player::PhysWorld { world: &self.world, geometry: &mut self.geometry };
            self.entity.e.is_on_ladder(&mut w)
        };
        let riding = self.net.as_ref().is_some_and(|n| self.attached.contains_key(&n.entity_id));
        let crit = self.entity.fall_distance > 0.
            && !self.entity.on_ground
            && !self.entity.in_water
            && !self.entity.effects.has(mc_entity::effects::BLINDNESS)
            && !ladder
            && !riding;
        if crit {
            self.particles.emit_at_entity(id, pos, width, height, 9, &mut self.rand, &self.models);
        }
        if tooltip::enchantment_level(held.as_ref(), 16) > 0 {
            self.particles.emit_at_entity(id, pos, width, height, 10, &mut self.rand, &self.models);
        }
    }

    /// `ItemStack.damageItem(amount, player)` on an inventory slot (Unbreaking rolls use the
    /// client's own random, as vanilla's prediction does; the server's SetSlot corrects it).
    pub(crate) fn damage_item(&mut self, slot: usize, amount: i32) {
        if self.entity.creative {
            return;
        }
        let Some(stack) = self.ui.inventory[slot] else { return };
        if !tooltip::damageable(&self.catalogue, &stack) {
            return;
        }
        let def = mc_data::item::Item::by_id(self.catalogue.items[stack.item].id);
        let mut amount = amount;
        // attemptDamageItem / EnchantmentDurability.negateDamage
        let level = tooltip::enchantment_level(Some(&stack), 34);
        if amount > 0 && level > 0 {
            let armor = def.is_some_and(|i| i.is_a("ItemArmor"));
            let k = (0..amount).filter(|_| !(armor && self.rand.next_float() < 0.6) && self.rand.next_int_bounded(level + 1) > 0).count() as i32;
            amount -= k;
            if amount <= 0 {
                return;
            }
        }
        let max = def.map_or(0, |i| i.max_damage) as i32;
        let damage = stack.damage as i32 + amount;
        if damage > max {
            // renderBrokenItemStack
            let p = self.entity.pos;
            self.play_sound("random.break", Some(p), 0.8, 0.8 + rand_f64() as f32 * 0.4);
            self.ui.inventory[slot] = (stack.count > 1).then_some(mc_gui::ItemStack { count: stack.count - 1, damage: 0, ..stack });
        } else {
            self.ui.inventory[slot] = Some(mc_gui::ItemStack { damage: damage as i16, ..stack });
        }
    }

    /// `EntityPlayerSP.swingItem`: every call also sends C0A.
    pub(crate) fn swing(&mut self) {
        self.entity.swing_item();
        self.send(mc_protocol::packets::PlayServerbound::AnimationC(mc_protocol::packets::AnimationC {}));
    }

    pub(crate) fn send(&mut self, p: mc_protocol::packets::PlayServerbound) {
        if let Some(n) = &mut self.net {
            n.send(p);
        }
    }

    pub(crate) fn send_dig(&mut self, status: mc_protocol::packets::DiggingStatus, pos: BlockPos, face: Facing) {
        self.send(mc_protocol::packets::PlayServerbound::PlayerDigging(mc_protocol::packets::PlayerDigging { status, pos, face: face.index() as u8 }));
    }

    pub(crate) fn alloc_entity_id(&mut self) -> u32 {
        self.next_entity_id += 1;
        self.next_entity_id
    }

    /// `EntityPlayer.dropItem(stack, false, true)`
    pub(crate) fn throw_stack(&mut self, stack: mc_gui::ItemStack) {
        if !self.is_local() {
            // The server spawns the item: creative throws use sendPacketDropItem (C10 slot -1);
            // survival throws already went out as window clicks.
            if self.entity.creative {
                let item = net::stack_slot(&self.catalogue, Some(stack));
                self.send(mc_protocol::packets::PlayServerbound::CreativeInventoryAction(mc_protocol::packets::CreativeInventoryAction { slot: -1, item }));
            }
            return;
        }
        let id = self.alloc_entity_id();
        let e = &self.entity;
        let mut item = mc_entity::item::ItemEntity::thrown(id, e.pos, e.eye_height(), e.yaw, e.pitch, (stack.item, stack.count), &mut self.rand);
        (item.damage, item.tag) = (stack.damage, stack.tag);
        self.items.push(item);
    }

    /// `InventoryPlayer.addItemStackToInventory`; returns what didn't fit.
    pub(crate) fn add_to_inventory(&mut self, template: mc_gui::ItemStack, mut count: u8) -> u8 {
        let max = self.catalogue.items[template.item].max.max(1);
        for slot in 0..36 {
            if let Some(s) = &mut self.ui.inventory[slot] {
                if s.stacks_with(&template) && s.count < max {
                    let n = count.min(max - s.count);
                    s.count += n;
                    count -= n;
                    if count == 0 {
                        return 0;
                    }
                }
            }
        }
        while count > 0 {
            let Some(slot) = self.ui.inventory.iter().position(Option::is_none) else { break };
            let n = count.min(max);
            self.ui.inventory[slot] = Some(mc_gui::ItemStack { count: n, ..template });
            count -= n;
        }
        count
    }

    /// Item entity ticks and `EntityItem.onCollideWithPlayer`.
    pub(crate) fn tick_items(&mut self) {
        let mut items = std::mem::take(&mut self.items);
        for it in &mut items {
            it.tick(&mut player::PhysWorld { world: &self.world, geometry: &mut self.geometry }, &mut self.rand);
        }
        let reach = self.entity.bb.expand(1.0, 0.5, 1.0);
        for it in &mut items {
            if self.is_local() && !it.dead && !self.entity.dead && it.pickup_delay == 0 && reach.intersects_with(&it.bb) {
                let pitch = ((rand_f64() as f32 - rand_f64() as f32) * 0.7 + 1.0) * 2.0;
                let p = it.pos;
                if let Some(s) = &mut self.sound {
                    s.play("random.pop", Some([p.x, p.y, p.z]), 0.2, pitch);
                }
                let template = mc_gui::ItemStack { item: it.stack.0, count: 1, damage: it.damage, tag: it.tag, obj: mc_gui::ItemStack::fresh_obj() };
                let left = self.add_to_inventory(template, it.stack.1);
                if left == 0 {
                    it.dead = true;
                } else {
                    it.stack.1 = left;
                }
            }
        }
        items.retain(|i| !i.dead);
        self.items = items;
    }

    /// `PlayerControllerMP.clickBlock`
    pub(crate) fn click_block(&mut self, pos: BlockPos) {
        // PlayerControllerMP.clickBlock: adventure/spectator players can't break blocks.
        if !self.entity.allow_edit() {
            return;
        }
        let face = self.target.map_or(Facing::Down, |t| t.face);
        if self.entity.creative {
            self.send_dig(mc_protocol::packets::DiggingStatus::StartDestroyBlock, pos, face);
            self.destroy_block(pos);
            self.block_hit_delay = 5;
            return;
        }
        if self.hitting == Some(pos) {
            return;
        }
        if let Some(old) = self.hitting {
            self.send_dig(mc_protocol::packets::DiggingStatus::AbortDestroyBlock, old, face);
        }
        self.send_dig(mc_protocol::packets::DiggingStatus::StartDestroyBlock, pos, face);
        let block = self.world.state_at(pos.x, pos.y, pos.z).block();
        if block.material != mc_data::Material::Air && self.relative_hardness(pos) >= 1. {
            self.destroy_block(pos);
        } else {
            self.hitting = Some(pos);
            self.block_damage = 0.;
        }
    }

    /// `Minecraft.sendClickBlockToController` / `PlayerControllerMP.onPlayerDamageBlock`
    pub(crate) fn send_click_block(&mut self, held: bool) {
        if self.left_click_counter > 0 && !self.entity.creative {
            return;
        }
        let target = self.target.filter(|t| self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().id != 0);
        let (true, Some(t), true) = (held, target, self.entity.allow_edit()) else {
            // resetBlockRemoving
            if let Some(old) = self.hitting {
                self.send_dig(mc_protocol::packets::DiggingStatus::AbortDestroyBlock, old, Facing::Down);
            }
            self.hitting = None;
            self.block_damage = 0.;
            return;
        };
        if self.block_hit_delay > 0 {
            self.block_hit_delay -= 1;
        } else if self.entity.creative {
            self.block_hit_delay = 5;
            self.send_dig(mc_protocol::packets::DiggingStatus::StartDestroyBlock, t.pos, t.face);
            self.destroy_block(t.pos);
        } else if self.hitting == Some(t.pos) {
            self.block_damage += self.relative_hardness(t.pos);
            if self.step_sound_counter % 4.0 == 0.0 {
                let st = self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().sound;
                let c = mc_core::Vec3::new(t.pos.x as f64 + 0.5, t.pos.y as f64 + 0.5, t.pos.z as f64 + 0.5);
                self.play_sound(st.step_sound, Some(c), (st.volume + 1.0) / 8.0, st.frequency * 0.5);
            }
            self.step_sound_counter += 1.0;
            if self.block_damage >= 1. {
                self.hitting = None;
                self.step_sound_counter = 0.;
                self.send_dig(mc_protocol::packets::DiggingStatus::StopDestroyBlock, t.pos, t.face);
                self.destroy_block(t.pos);
                self.block_damage = 0.;
                self.block_hit_delay = 5;
            }
        } else {
            self.click_block(t.pos);
            self.block_hit_particles(t.pos, t.face);
        }
        self.swing();
    }

    /// `Minecraft.clickMouse` → `EffectRenderer.addBlockHitEffects` for the first hit of a block.
    pub(crate) fn block_hit_particles(&mut self, pos: BlockPos, face: Facing) {
        let st = self.world.state_at(pos.x, pos.y, pos.z);
        self.particles.block_hit(st, pos, face, &self.world, &self.models, &mut self.rand);
    }

    /// `BlockRendererDispatcher.renderBlockDamage`: the block's model with every quad's UVs moved
    /// from its own sprite onto `destroy_stage_N`.
    pub(crate) fn damage_quads(&self) -> Vec<mc_render::avatar::DamageQuad> {
        let Some((pos, stage)) = self.break_progress() else { return Vec::new() };
        let atlas = &self.models.atlas;
        let crack = atlas.sprite(&format!("blocks/destroy_stage_{}", stage.min(9)));
        let stored = self.world.state_at(pos.x, pos.y, pos.z);
        let actual = stored.actual_state(&self.world, pos);
        let Some(entry) = self.models.get_state(actual) else { return Vec::new() };
        if entry.models.is_empty() {
            return Vec::new();
        }
        let model = &entry.models[mc_assets::model::pick_weighted(&entry.weights, pos.x, pos.y, pos.z)].1;
        let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
        model
            .quads
            .iter()
            .filter_map(|q| {
                let c = [0.25f32, 0.75].map(|t| q.uv[0][0] * t + q.uv[2][0] * (1. - t));
                let own = atlas.sprite_at((c[0] + c[1]) / 2., (q.uv[0][1] + q.uv[2][1]) / 2.)?;
                let uv = q.uv.map(|[u, v]| {
                    let fu = (u - own.min_u) / (own.max_u - own.min_u);
                    let fv = (v - own.min_v) / (own.max_v - own.min_v);
                    [crack.min_u + fu * (crack.max_u - crack.min_u), crack.min_v + fv * (crack.max_v - crack.min_v)]
                });
                Some(mc_render::avatar::DamageQuad { pos: q.pos.map(|p| origin + Vec3::from(p)), uv })
            })
            .collect()
    }

    /// Destroy stage shown on the block being hit (`sendBlockBreakProgress`).
    pub fn break_progress(&self) -> Option<(BlockPos, i32)> {
        self.hitting.map(|p| (p, (self.block_damage * 10.) as i32 - 1)).filter(|(_, s)| *s >= 0)
    }

    /// `GuiScreen.sendChatMessage`; without a connection only the singleplayer build handles it.
    pub(crate) fn send_chat(&mut self, msg: &str) {
        if let Some(n) = &mut self.net {
            // GuiScreen.sendChatMessage: C01 carries at most 100 characters.
            let m: String = msg.chars().take(100).collect();
            n.send(mc_protocol::packets::PlayServerbound::ChatMessage(mc_protocol::packets::ChatMessage { message: m }));
            return;
        }
        #[cfg(feature = "singleplayer")]
        self.local_chat(msg);
    }

    /// Singleplayer chat: the integrated server's command handler (`CommandHandler`).
    #[cfg(feature = "singleplayer")]
    fn local_chat(&mut self, msg: &str) {
        let msg = commands::normalize_space(msg);
        if !msg.starts_with('/') {
            let line = self.ui.lang.format("chat.type.text", &["Player", &msg]);
            self.hud.add_chat(line);
            return;
        }
        let was_dead = self.entity.dead;
        let out = {
            let mut c = commands::Ctx {
                player: &mut self.entity,
                world: &mut self.world,
                ui: &mut self.ui,
                items: &self.catalogue,
                info: &mut self.info,
                name: "Player",
            };
            commands::execute(&msg, &mut c)
        };
        for m in out.messages {
            self.hud.add_chat(m);
        }
        for (stack, fake) in out.drops {
            // dropPlayerItemWithRandomChoice(stack, false), then setNoPickupDelay, or the
            // one-tick fake item of a fully given stack.
            let id = self.alloc_entity_id();
            let e = &self.entity;
            let mut it = mc_entity::item::ItemEntity::thrown(id, e.pos, e.eye_height(), e.yaw, e.pitch, (stack.item, stack.count), &mut self.rand);
            (it.damage, it.tag) = (stack.damage, stack.tag);
            if fake {
                it.pickup_delay = 32767;
                it.age = 5999;
            } else {
                it.pickup_delay = 0;
            }
            self.items.push(it);
        }
        self.refresh_edits(&out.edits);
        self.sync_view();
        if self.entity.dead && !was_dead {
            self.open(Box::new(screens::DeathScreen::new(false, self.net.is_some(), 0)), false);
        }
    }
    /// Singleplayer tab completion from the local command table.
    #[cfg(feature = "singleplayer")]
    pub(crate) fn local_tab_complete(&mut self, prefix: &str) {
        let list = commands::complete(prefix, &self.ui.lang, self.target.map(|t| t.pos));
        if let Some(screen) = &mut self.screen {
            if let Some(line) = screen.autocomplete(&mut self.ui, list) {
                self.hud.add_chat(line);
            }
        }
    }
    pub(crate) fn refresh_edits(&mut self, edits: &[BlockPos]) {
        if edits.is_empty() && self.world.dirty.is_empty() {
            return;
        }
        let t0 = Instant::now();
        // Light is updated incrementally by set_block_state; settle sky-light gaps nearby.
        for p in edits {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    self.world.tick_chunk_light((p.x >> 4) + dx, (p.z >> 4) + dz, false, false);
                }
            }
        }
        // Remesh only the columns whose blocks or light actually changed.
        let _z = crate::bench::zone(crate::bench::Phase::Mesh);
        let dirty = self.world.take_dirty();
        let t1 = Instant::now();
        let n = dirty.len();
        let _ = &n;
        let Some(r) = &mut self.renderer else {
            return;
        };
        let jobs: Vec<_> = dirty.into_iter().filter(|&(x, sy, z)| self.world.chunks.contains_key(&(x, z)) && (0..16).contains(&sy)).collect();
        let smooth = self.ui.settings.ambient_occlusion != 0;
        for ((x, sy, z), m) in mesh_parallel(&self.world, &self.models, &jobs, smooth, self.ui.settings.fancy_graphics) {
            r.upload_section(x, sy, z, &m);
        }
        if std::env::var_os("RUSTMC_TIMING").is_some() {
            eprintln!("edit: light {:?}, mesh {n} sections {:?}", t1 - t0, t1.elapsed());
        }
    }
    pub(crate) fn eye(&self, pt: f32) -> Vec3 {
        self.entity.interpolated(pt) + Vec3::Y * self.entity.eye_height()
    }

    /// `EntityRenderer.getMouseOver`
    pub(crate) fn update_target(&mut self, pt: f32) {
        let eye = self.eye(pt);
        let yaw = self.entity.prev_yaw + (self.entity.yaw - self.entity.prev_yaw) * pt;
        let pitch = self.entity.prev_pitch + (self.entity.pitch - self.entity.prev_pitch) * pt;
        let dir = Camera { pos: eye, yaw, pitch, fov_deg: 70., bob: glam::Mat4::IDENTITY }.look_dir();
        self.target = player::target(
            &self.world,
            &mut self.geometry,
            mc_core::Vec3::new(eye.x as f64, eye.y as f64, eye.z as f64),
            dir,
            if self.entity.creative { 5. } else { 4.5 },
        );
        // getMouseOver: entities within 3 blocks (6 in creative) closer than the block hit.
        self.pointed_entity = None;
        if self.net.is_some() {
            let e = mc_core::Vec3::new(eye.x as f64, eye.y as f64, eye.z as f64);
            let reach = if self.entity.creative { 6.0 } else { 3.0 };
            let block_d = self.target.map_or(reach, |t| ((t.hit.x - e.x).powi(2) + (t.hit.y - e.y).powi(2) + (t.hit.z - e.z).powi(2)).sqrt());
            let look = mc_core::Vec3::new(dir.x as f64, dir.y as f64, dir.z as f64);
            if let Some((id, d)) = self.remote.ray(e, look, reach) {
                if d < block_d || self.target.is_none() {
                    self.pointed_entity = Some(id);
                    self.pointed_hit = mc_core::Vec3::new(e.x + look.x * d, e.y + look.y * d, e.z + look.z * d);
                    self.target = None;
                }
            }
        }
    }

}
