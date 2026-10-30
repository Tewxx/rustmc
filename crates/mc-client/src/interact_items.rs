//! Right click, block placement and item use.

use crate::*;

impl App {
    pub(crate) fn interact(&mut self, button: MouseButton) {
        if self.entity.dead {
            return;
        }
        if button == MouseButton::Left {
            // clickMouse
            if self.left_click_counter > 0 {
                return;
            }
            self.swing();
            if let Some(id) = self.pointed_entity {
                self.attack_entity(id);
                return;
            }
            match self.target {
                None => {
                    if !self.entity.creative {
                        self.left_click_counter = 10;
                    }
                }
                Some(t) => {
                    if self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().id != 0 {
                        self.click_block(t.pos);
                    }
                }
            }
            return;
        }
        if button == MouseButton::Right {
            self.right_click_mouse();
            self.sync_view();
            return;
        }
        let Some(t) = self.target else { return };
        if button == MouseButton::Middle {
            let (id, meta) = self.world.get(t.pos.x, t.pos.y, t.pos.z);
            if let Some(item) = self.catalogue.items.iter().position(|i| i.block == id && i.meta == meta as u16) {
                if let Some(slot) = self.ui.inventory[..9].iter().position(|s| s.is_some_and(|s| s.item == item)) {
                    self.player.selected_slot = slot;
                } else if self.entity.creative {
                    self.ui.inventory[self.player.selected_slot] = Some(self.catalogue.stack(item, 1));
                }
            }
            return;
        }
        self.sync_view();
    }


    /// `ItemBlock.onItemUse` on the client: place the held block against `t` (prediction).
    pub(crate) fn place_held_block(&mut self, t: player::Target) -> bool {
        let Some(stack) = self.ui.inventory[self.player.selected_slot] else {
            return false;
        };
        // EntityPlayer.canPlayerEdit
        if !self.entity.allow_edit() {
            return false;
        }
        let item = &self.catalogue.items[stack.item];
        if item.block == 0 {
            return false;
        }
        let old = self.world.state_at(t.pos.x, t.pos.y, t.pos.z);
        let p = if old.block().material.props().replaceable {
            t.pos
        } else {
            t.pos.offset_facing(t.face)
        };
        if !self.world.state_at(p.x, p.y, p.z).block().material.props().replaceable {
            return false;
        }
        let block = mc_data::Block::by_id(item.block).unwrap();
        let mut meta = (item.meta & 15) as u8;
        let facing = Facing::from_angle(self.entity.yaw as f64);
        if block.class == "BlockStairs" {
            meta = match facing {
                Facing::East => 0,
                Facing::West => 1,
                Facing::South => 2,
                _ => 3,
            };
            if t.face == Facing::Down || t.face != Facing::Up && t.hit.y - t.pos.y as f64 > 0.5 {
                meta |= 4;
            }
        } else if matches!(block.id, 17 | 162) {
            meta = (meta & 3)
                | match t.face {
                    Facing::East | Facing::West => 4,
                    Facing::North | Facing::South => 8,
                    _ => 0,
                };
        } else if matches!(block.id, 54 | 61 | 62 | 65 | 130 | 146) {
            meta = facing.opposite().index() as u8;
        } else if matches!(block.id, 44 | 126 | 182) {
            if t.face == Facing::Down || t.face != Facing::Up && t.hit.y - t.pos.y as f64 > 0.5 {
                meta |= 8;
            }
        } else if matches!(block.id, 50 | 75 | 76) {
            meta = match t.face {
                Facing::East => 1,
                Facing::West => 2,
                Facing::South => 3,
                Facing::North => 4,
                _ => 5,
            };
        }
        let Some(state) = block.state_from_meta(meta) else { return false };
        let mut boxes = Vec::new();
        self.geometry.add_collision_boxes(
            state,
            &self.world,
            p,
            &self.entity.bounds(),
            &mut boxes,
            mc_data::geometry::CollisionEntity::Other,
        );
        if !boxes.is_empty() {
            return false;
        }
        // PlayerControllerMP.onPlayerRightClick: the client predicts the placement itself; only the
        // integrated server runs the full block logic.
        #[cfg(feature = "singleplayer")]
        let placed = if self.net.is_none() {
            self.with_level(|lv| mc_logic::Level::set_state(lv, p, state, mc_logic::flags::DEFAULT))
        } else {
            self.world.set_block(p, block.id, meta)
        };
        #[cfg(not(feature = "singleplayer"))]
        let placed = self.world.set_block(p, block.id, meta);
        if placed {
            self.tiles.placed(p, state);
            if !self.entity.creative {
                self.ui.inventory[self.player.selected_slot] = if stack.count > 1 {
                    Some(mc_gui::ItemStack {
                        count: stack.count - 1,
                        ..stack
                    })
                } else {
                    None
                };
            }
            let st = block.sound;
            let c = mc_core::Vec3::new(p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5);
            self.play_sound(st.place_sound, Some(c), (st.volume + 1.0) / 2.0, st.frequency * 0.8);
            self.refresh_edits(&[p]);
        }
        placed
    }

    /// `Minecraft.rightClickMouse`
    pub(crate) fn right_click_mouse(&mut self) {
        if self.hitting.is_some() {
            return;
        }
        self.right_click_delay = 4;
        let mut flag = true;
        let sel = self.player.selected_slot;
        let stack = self.ui.inventory[sel];
        if let Some(id) = self.pointed_entity {
            // isPlayerRightClickingOnEntity, then interactWithEntitySendPacket. Neither players
            // nor items take interactions, so both return false and the item is used after.
            let sel = sel as i32;
            if let Some(n) = &mut self.net {
                n.sync_current_item(sel);
            }
            let rel = self.remote.map.get(&id).map(|r| r.pos).map(|p| ((self.pointed_hit.x - p.x) as f32, (self.pointed_hit.y - p.y) as f32, (self.pointed_hit.z - p.z) as f32));
            use mc_protocol::packets::{PlayServerbound as S, UseEntity, UseEntityAction};
            self.send(S::UseEntity(UseEntity { entity_id: id, action: UseEntityAction::InteractAt, hit: rel }));
            if let Some(n) = &mut self.net {
                n.sync_current_item(sel);
            }
            self.send(S::UseEntity(UseEntity { entity_id: id, action: UseEntityAction::Interact, hit: None }));
        } else if let Some(t) = self.target {
            if self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block().id != 0 {
                let i = stack.map_or(0, |s| s.count);
                if self.on_player_right_click(stack, t) {
                    flag = false;
                    self.swing();
                }
                if stack.is_none() {
                    return;
                }
                match self.ui.inventory[sel] {
                    None => {}
                    Some(now) if now.count != i || self.entity.creative => self.entity.reset_equipped_progress(),
                    _ => {}
                }
            }
        }
        if flag {
            if let Some(s1) = self.ui.inventory[sel] {
                if self.send_use_item(s1) {
                    self.entity.reset_equipped_progress();
                }
            }
        }
    }

    /// `PlayerControllerMP.onPlayerRightClick`
    pub(crate) fn on_player_right_click(&mut self, stack: Option<mc_gui::ItemStack>, t: player::Target) -> bool {
        let sel = self.player.selected_slot as i32;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel);
        }
        if self.entity.game_type == level::GameType::Spectator {
            let item = net::stack_slot(&self.catalogue, stack);
            let c = ((t.hit.x - t.pos.x as f64) as f32, (t.hit.y - t.pos.y as f64) as f32, (t.hit.z - t.pos.z as f64) as f32);
            self.send(mc_protocol::packets::PlayServerbound::BlockPlacement(mc_protocol::packets::BlockPlacement { pos: t.pos, direction: t.face.index() as u8, item, cursor: c }));
            return false;
        }
        let block = self.world.state_at(t.pos.x, t.pos.y, t.pos.z).block();
        let activated = (!self.entity.is_sneaking() || stack.is_none()) && item_use::activates(block);
        if !activated {
            // ItemBlock.canPlaceBlockOnSide failing sends nothing.
            let blocked = stack.is_some_and(|s| {
                let b = self.catalogue.items[s.item].block;
                let replaceable = |p: BlockPos| self.world.state_at(p.x, p.y, p.z).block().material.props().replaceable;
                b != 0 && !replaceable(t.pos) && !replaceable(t.pos.offset_facing(t.face))
            });
            if blocked {
                return false;
            }
        }
        let item = net::stack_slot(&self.catalogue, stack);
        let c = ((t.hit.x - t.pos.x as f64) as f32, (t.hit.y - t.pos.y as f64) as f32, (t.hit.z - t.pos.z as f64) as f32);
        self.send(mc_protocol::packets::PlayServerbound::BlockPlacement(mc_protocol::packets::BlockPlacement { pos: t.pos, direction: t.face.index() as u8, item, cursor: c }));
        if activated {
            return true;
        }
        if stack.is_none() {
            return false;
        }
        // ItemStack.onItemUse: only block items are modelled.
        self.place_held_block(t)
    }

    /// `PlayerControllerMP.sendUseItem`
    pub(crate) fn send_use_item(&mut self, stack: mc_gui::ItemStack) -> bool {
        if self.entity.game_type == level::GameType::Spectator {
            return false;
        }
        let sel = self.player.selected_slot;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel as i32);
        }
        let item = net::stack_slot(&self.catalogue, Some(stack));
        self.send(mc_protocol::packets::PlayServerbound::BlockPlacement(mc_protocol::packets::BlockPlacement {
            pos: BlockPos::new(-1, -1, -1),
            direction: 255,
            item,
            cursor: (0., 0., 0.),
        }));
        let id = self.catalogue.items[stack.item].id;
        let cx = item_use::Context {
            creative: self.entity.creative,
            food_level: self.player.food,
            has_arrow: self.ui.inventory.iter().flatten().any(|s| self.catalogue.items[s.item].id == 262),
            armor_empty: std::array::from_fn(|i| self.ui.armor[3 - i].is_none()),
        };
        match item_use::right_click(id, stack.damage, &cx) {
            item_use::RightClick::StartUsing => {
                // setItemInUse: the same stack object, so the stack itself is unchanged.
                self.item_in_use = Some(stack);
                self.item_in_use_count = item_use::action(id).1;
                false
            }
            item_use::RightClick::Consume => {
                self.ui.inventory[sel] = (stack.count > 1).then_some(mc_gui::ItemStack { count: stack.count - 1, ..stack });
                true
            }
            item_use::RightClick::Equip(slot) => {
                self.ui.armor[3 - slot] = Some(mc_gui::ItemStack { obj: mc_gui::ItemStack::fresh_obj(), ..stack });
                self.ui.inventory[sel] = None;
                true
            }
            item_use::RightClick::Swing => {
                self.entity.swing_item();
                false
            }
            item_use::RightClick::Nothing => false,
        }
    }

    /// The `itemInUse` part of `EntityPlayer.onUpdate`.
    pub(crate) fn tick_item_in_use(&mut self) {
        let Some(using) = self.item_in_use else { return };
        let current = self.ui.inventory[self.player.selected_slot];
        if current.is_some_and(|c| c.same_object(&using)) {
            if self.item_in_use_count <= 25 && self.item_in_use_count % 4 == 0 {
                self.update_item_use(using, 5);
            }
            self.item_in_use_count -= 1;
            // Only the integrated server finishes on its own; a remote server sends status 9.
            if self.item_in_use_count == 0 && self.is_local() {
                self.on_item_use_finish();
            }
        } else {
            self.clear_item_in_use();
        }
    }

    pub(crate) fn clear_item_in_use(&mut self) {
        self.item_in_use = None;
        self.item_in_use_count = 0;
    }

    /// `EntityPlayer.updateItemUse`: drinking/eating sounds (crumb particles are not modelled).
    pub(crate) fn update_item_use(&mut self, stack: mc_gui::ItemStack, _n: i32) {
        let (action, _) = item_use::action(self.catalogue.items[stack.item].id);
        let p = self.entity.pos;
        match action {
            item_use::Action::Drink => self.play_sound("random.drink", Some(p), 0.5, rand_f64() as f32 * 0.1 + 0.9),
            item_use::Action::Eat => {
                let volume = 0.5 + 0.5 * (rand_f64() * 2.) as i32 as f32;
                let pitch = (rand_f64() as f32 - rand_f64() as f32) * 0.2 + 1.0;
                self.play_sound("random.eat", Some(p), volume, pitch);
            }
            _ => {}
        }
    }

    /// `EntityPlayer.onItemUseFinish` → `Item.onItemUseFinish`.
    pub(crate) fn on_item_use_finish(&mut self) {
        let Some(using) = self.item_in_use else { return };
        self.update_item_use(using, 16);
        let sel = self.player.selected_slot;
        let def_id = self.catalogue.items[using.item].id;
        let item = mc_data::item::Item::by_id(def_id);
        let creative = self.entity.creative;
        let dec = |s: mc_gui::ItemStack| mc_gui::ItemStack { count: s.count.saturating_sub(1), ..s };
        let result = if item.is_some_and(|i| i.is_a("ItemFood")) {
            if self.is_local() {
                // The integrated server's half: food stats and the burp everyone hears.
                let heal = item.map_or(0., |i| i.num("healAmount")) as i32;
                self.player.food = (self.player.food + heal).min(20);
                self.entity.food_level = self.player.food;
                self.play_sound("random.burp", Some(self.entity.pos), 0.5, rand_f64() as f32 * 0.1 + 0.9);
            }
            Some(dec(using))
        } else if item.is_some_and(|i| i.is_a("ItemPotion")) {
            if creative {
                Some(using)
            } else {
                let left = dec(using);
                if left.count == 0 {
                    self.catalogue.index_of(374, 0).map(|b| self.catalogue.stack(b, 1))
                } else {
                    if let Some(b) = self.catalogue.index_of(374, 0) {
                        let bottle = self.catalogue.stack(b, 1);
                        self.add_to_inventory(bottle, 1);
                    }
                    Some(left)
                }
            }
        } else if def_id == 335 {
            let left = if creative { using } else { dec(using) };
            if left.count == 0 {
                self.catalogue.index_of(325, 0).map(|b| self.catalogue.stack(b, 1))
            } else {
                Some(left)
            }
        } else {
            Some(using)
        };
        let changed = match result {
            Some(r) => !r.same_object(&using) || r.count != using.count,
            None => true,
        };
        if changed {
            self.ui.inventory[sel] = result.filter(|r| r.count > 0);
        }
        self.clear_item_in_use();
    }

    /// `PlayerControllerMP.onStoppedUsingItem` → `EntityPlayer.stopUsingItem`.
    pub(crate) fn on_stopped_using_item(&mut self) {
        let sel = self.player.selected_slot as i32;
        if let Some(n) = &mut self.net {
            n.sync_current_item(sel);
        }
        self.send_dig(mc_protocol::packets::DiggingStatus::ReleaseUseItem, BlockPos::new(0, 0, 0), Facing::Down);
        if let Some(using) = self.item_in_use {
            let id = self.catalogue.items[using.item].id;
            if item_use::action(id).0 == item_use::Action::Bow {
                self.bow_released(using);
            }
        }
        self.clear_item_in_use();
    }

    /// `ItemBow.onPlayerStoppedUsing` on the client: bow damage and the arrow it used up.
    pub(crate) fn bow_released(&mut self, bow: mc_gui::ItemStack) {
        let infinite = self.entity.creative || tooltip::enchantment_level(Some(&bow), 51) > 0;
        let arrow_slot = self.ui.inventory.iter().position(|s| s.is_some_and(|s| self.catalogue.items[s.item].id == 262));
        if !infinite && arrow_slot.is_none() {
            return;
        }
        let i = 72000 - self.item_in_use_count;
        let mut f = i as f32 / 20.0;
        f = (f * f + f * 2.0) / 3.0;
        if (f as f64) < 0.1 {
            return;
        }
        let sel = self.player.selected_slot;
        if self.ui.inventory[sel].is_some_and(|c| c.same_object(&bow)) {
            self.damage_item(sel, 1);
        }
        if !infinite {
            if let Some(a) = arrow_slot {
                let s = self.ui.inventory[a].unwrap();
                self.ui.inventory[a] = (s.count > 1).then_some(mc_gui::ItemStack { count: s.count - 1, ..s });
            }
        }
    }

}
