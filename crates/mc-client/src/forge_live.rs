//! Synchronization boundaries between Rust simulation and the Java mod object graph.
use crate::*;
use serde_json::{json, Value};

thread_local! { static LAST_PUBLISHED: std::cell::RefCell<Option<Value>> = const { std::cell::RefCell::new(None) }; }
/// Forces the next `forge_publish` to sync Java even if the snapshot is unchanged.
pub(crate) fn forge_republish() { LAST_PUBLISHED.with(|l|*l.borrow_mut()=None); }

/// Whether the mirrored world changed in a way `Minecraft.loadWorld` must observe: entering or leaving a world, or a
/// new world (join/respawn/dimension) while in one. A generation bump while already out of a world is still the same
/// empty world and must not fire another `loadWorld(null)`.
fn load_world_due(last: Option<(u64, bool)>, generation: u64, active: bool) -> bool {
    match last {
        None => true,
        Some((last_generation, was_active)) => was_active != active || (active && last_generation != generation),
    }
}

fn entity_view(id: i32, class: &str, name: &str, uuid: String, pos: mc_core::Vec3,
    prev: mc_core::Vec3, last: mc_core::Vec3, motion: mc_core::Vec3, yaw: f32, pitch: f32,
    prev_yaw: f32, prev_pitch: f32, flags: u8, on_ground: bool, dead: bool, no_clip: bool,
    ticks: i32, width: f64, height: f64, fall_distance: f32, health: f32, hurt_time: i32) -> Value {
    json!({"id":id,"class":class,"name":name,"uuid":uuid,
        "x":pos.x,"y":pos.y,"z":pos.z,"prev_x":prev.x,"prev_y":prev.y,"prev_z":prev.z,
        "last_x":last.x,"last_y":last.y,"last_z":last.z,"motion_x":motion.x,"motion_y":motion.y,"motion_z":motion.z,
        "yaw":yaw,"pitch":pitch,"prev_yaw":prev_yaw,"prev_pitch":prev_pitch,"flags":flags,
        "on_ground":on_ground,"dead":dead,"no_clip":no_clip,"ticks":ticks,"width":width,"height":height,
        "fall_distance":fall_distance,"health":health,"hurt_time":hurt_time})
}

fn java_entity_class(kind: &entities::Kind) -> Option<String> {
    let (package, name) = match kind {
        entities::Kind::Player => return Some("net.minecraft.client.entity.EntityOtherPlayerMP".into()),
        entities::Kind::Mob(id) => match id {
            50 => ("monster", "EntityCreeper"), 51 => ("monster", "EntitySkeleton"),
            52 => ("monster", "EntitySpider"), 53 => ("monster", "EntityGiantZombie"),
            54 => ("monster", "EntityZombie"), 55 => ("monster", "EntitySlime"),
            56 => ("monster", "EntityGhast"), 57 => ("monster", "EntityPigZombie"),
            58 => ("monster", "EntityEnderman"), 59 => ("monster", "EntityCaveSpider"),
            60 => ("monster", "EntitySilverfish"), 61 => ("monster", "EntityBlaze"),
            62 => ("monster", "EntityMagmaCube"), 63 => ("boss", "EntityDragon"),
            64 => ("boss", "EntityWither"), 65 => ("passive", "EntityBat"),
            66 => ("monster", "EntityWitch"), 67 => ("monster", "EntityEndermite"),
            68 => ("monster", "EntityGuardian"), 90 => ("passive", "EntityPig"),
            91 => ("passive", "EntitySheep"), 92 => ("passive", "EntityCow"),
            93 => ("passive", "EntityChicken"), 94 => ("passive", "EntitySquid"),
            95 => ("passive", "EntityWolf"), 96 => ("passive", "EntityMooshroom"),
            97 => ("monster", "EntitySnowman"), 98 => ("passive", "EntityOcelot"),
            99 => ("monster", "EntityIronGolem"), 100 => ("passive", "EntityHorse"),
            101 => ("passive", "EntityRabbit"), 120 => ("passive", "EntityVillager"), _ => return None,
        },
        entities::Kind::Object(id) => match id {
            1 => ("item", "EntityBoat"), 2 => ("item", "EntityItem"), 10 => ("item", "EntityMinecartEmpty"),
            50 => ("item", "EntityTNTPrimed"), 51 => ("item", "EntityEnderCrystal"),
            60 => ("projectile", "EntityArrow"), 61 => ("projectile", "EntitySnowball"),
            62 => ("projectile", "EntityEgg"), 63 => ("projectile", "EntityLargeFireball"),
            64 => ("projectile", "EntitySmallFireball"), 65 => ("item", "EntityEnderPearl"),
            66 => ("projectile", "EntityWitherSkull"), 70 => ("item", "EntityFallingBlock"),
            71 => ("item", "EntityItemFrame"), 72 => ("item", "EntityEnderEye"),
            73 => ("projectile", "EntityPotion"), 75 => ("item", "EntityExpBottle"),
            76 => ("item", "EntityFireworkRocket"), 77 => ("item", "EntityLeashKnot"),
            78 => ("item", "EntityArmorStand"), 90 => ("projectile", "EntityFishHook"), _ => return None,
        },
        entities::Kind::XpOrb => ("item", "EntityXPOrb"),
    };
    Some(format!("net.minecraft.entity.{package}.{name}"))
}

impl App {
    pub(crate) fn forge_snapshot(&self) -> Value {
        let e = &self.entity;
        let mut entities = Vec::new();
        let id = self.net.as_ref().map_or(0, |n| n.entity_id);
        if self.in_game {
            entities.push(entity_view(id,"net.minecraft.client.entity.EntityPlayerSP",&self.name,
                self.net.as_ref().map(|n|n.uuid.to_string()).unwrap_or_default(),e.pos,e.prev_pos,e.last_tick_pos,e.motion,
                e.yaw,e.pitch,e.prev_yaw,e.prev_pitch,((e.is_sneaking() as u8)<<1)|((e.sprinting as u8)<<3),
                e.on_ground,e.dead,e.no_clip,e.ticks_existed,0.6,1.8,e.fall_distance,self.player.health,e.hurt_time));
            let mut remotes: Vec<_> = self.remote.map.values().filter(|r|r.id!=id).collect();
            remotes.sort_by_key(|r|r.id);
            for r in remotes {
                let Some(class)=java_entity_class(&r.kind) else { continue };
                entities.push(entity_view(r.id,&class,&r.name,r.uuid.map(|u|u.to_string()).unwrap_or_default(),
                    r.pos,r.prev_pos,r.prev_pos,mc_core::Vec3::new(0.,0.,0.),r.yaw,r.pitch,r.prev_yaw,r.prev_pitch,
                    r.meta_byte(0) as u8,r.on_ground,false,false,r.ticks_existed,r.width,r.height,0.,r.meta_float(6).unwrap_or(20.),r.hurt_time));
            }
        }
        let stack = |s: Option<mc_gui::ItemStack>| s.map(|s| {
            let def=&self.catalogue.items[s.item];
            let item=mc_data::item::Item::by_id(def.id);
            json!({"id":def.id,"count":s.count,"damage":s.damage,"name":def.name,"max":def.max,
                "class":format!("net.minecraft.item.{}",item.and_then(|i|i.classes.first()).unwrap_or(&"Item")),
                "nbt":s.tag.map(|t|mc_core::nbt::write("",&mc_core::nbt::Tag::Compound((*t.get()).clone())))})
        });
        let inventory:Vec<_>=self.ui.inventory.iter().chain(self.ui.armor.iter()).map(|s|stack(*s)).collect();
        // RenderManager.renderPos is the interpolated feet position; the camera supplies the view angles.
        let pt=self.tick_accum as f64;
        let lerp=|a:f64,b:f64|a+(b-a)*pt;
        let feet=mc_core::Vec3::new(lerp(e.last_tick_pos.x,e.pos.x),lerp(e.last_tick_pos.y,e.pos.y),lerp(e.last_tick_pos.z,e.pos.z));
        let target=match (self.pointed_entity,self.target) {
            (Some(id),_)=>json!({"type":"entity","id":id,"hx":self.pointed_hit.x,"hy":self.pointed_hit.y,"hz":self.pointed_hit.z}),
            (None,Some(t))=>json!({"type":"block","x":t.pos.x,"y":t.pos.y,"z":t.pos.z,"side":t.face.index(),"hx":t.hit.x,"hy":t.hit.y,"hz":t.hit.z}),
            _=>json!({"type":"miss"}),
        };
        // The tab list (NetHandlerPlayClient.playerInfoMap) and the world scoreboard's teams.
        let sb=&self.scoreboard;
        let mut players:Vec<_>=sb.players.iter().map(|(u,p)|json!({"uuid":u.to_string(),"name":p.name,"display":p.display_name,
            "ping":p.ping,"game_mode":p.game_mode})).collect();
        players.sort_by(|a,b|a["name"].as_str().cmp(&b["name"].as_str()));
        let mut teams:Vec<_>=sb.teams.iter().map(|(n,t)|{
            let mut members:Vec<&String>=sb.membership.iter().filter(|(_,m)|*m==n).map(|(p,_)|p).collect(); members.sort();
            json!({"name":n,"display":t.display_name,"prefix":t.prefix,"suffix":t.suffix,"color":t.color,
                "friendly_flags":t.friendly_flags,"visibility":t.name_tag_visibility,"members":members})}).collect();
        teams.sort_by(|a,b|a["name"].as_str().cmp(&b["name"].as_str()));
        // Local-player state without an entity JSON key of its own; `Entities.state` mirrors it into fields.
        let block=self.item_in_use.is_some_and(|s|self.catalogue.items[s.item].name.to_lowercase().contains("sword"));
        let state=json!({"id":id,"absorb":self.player.absorption,"armor":self.player.armor,
            "xp_level":self.player.level,"xp_progress":self.player.experience,"xp_total":0,
            "food":self.player.food,"sat":self.player.saturation,"swing":e.swing_progress,
            "swing_int":e.swing_progress_int,"hurt_resist":e.hurt_time,
            "block":if block {1} else {0},"arrows":0,"skin":0x7f,
            "game":if e.game_type==level::GameType::Spectator {3} else if e.creative {1} else {0},
            "use_count":self.item_in_use_count,"using":stack(self.item_in_use),
            "effects":e.effects.iter().map(|f|json!([f.id,f.duration,f.amplifier])).collect::<Vec<_>>(),
            "armor_items":self.ui.armor.iter().map(|s|stack(*s)).collect::<Vec<_>>(),
            "riding":self.attached.get(&id).copied().unwrap_or(-1)});
        json!({"generation":self.forge_generation,"active":self.in_game,"dimension":self.forge_dimension,
            "state":state,
            "tab":{"players":players,"teams":teams,"header":sb.header,"footer":sb.footer},
            "view":{"x":feet.x,"y":feet.y,"z":feet.z,"yaw":self.camera.yaw,"pitch":self.camera.pitch,"target":target,
                "sections_drawn":self.renderer.as_ref().map_or(0,|r|r.drawn_chunks),"sections_total":self.world.chunks.len()*16},
            "time":self.info.time,"total_time":self.info.total_time,"entities":entities,
            "selected_slot":self.player.selected_slot,"inventory":inventory,"partial_ticks":self.tick_accum,
            "input":self.forge_input(),
            "capabilities":{"flying":e.flying,"allow_flying":e.allow_flying,"creative":e.creative,"invulnerable":e.disable_damage},
            "settings":{"keys":self.forge_key_codes(),"third_person":e.perspective,"fov":self.ui.settings.fov,"sensitivity":self.ui.settings.mouse_sensitivity,"render_distance":self.ui.settings.render_distance}})
    }

    /// `MovementInputFromOptions` as the keys stand (sneaking slows both axes to 0.3).
    fn forge_input(&self) -> Value {
        let down=|key|self.screen.is_none()&&self.bound_down(key);
        let sneak=down("key.sneak"); let slow=|v:f32|if sneak {(v as f64*0.3)as f32}else{v};
        json!({"forward":slow(down("key.forward")as u8 as f32-down("key.back")as u8 as f32),
            "strafe":slow(down("key.left")as u8 as f32-down("key.right")as u8 as f32),"jump":down("key.jump"),"sneak":sneak})
    }

    /// The client's controls for Java's GameSettings bindings (LWJGL codes, mouse buttons -100..-98).
    fn forge_key_codes(&self) -> Value {
        const KEYS: [&str; 33] = ["key.attack","key.use","key.forward","key.left","key.back","key.right","key.jump","key.sneak",
            "key.sprint","key.drop","key.inventory","key.chat","key.playerlist","key.pickItem","key.command","key.screenshot",
            "key.togglePerspective","key.smoothCamera","key.streamStartStop","key.streamPauseUnpause","key.streamCommercial",
            "key.streamToggleMic","key.fullscreen","key.spectatorOutlines","key.hotbar.1","key.hotbar.2","key.hotbar.3",
            "key.hotbar.4","key.hotbar.5","key.hotbar.6","key.hotbar.7","key.hotbar.8","key.hotbar.9"];
        Value::Object(KEYS.iter().map(|k|(k.to_string(),json!(self.ui.settings.key(k)))).collect())
    }

    /// Hands the Java resource manager its own copy of the pack stack (as the sound system keeps one).
    pub(crate) fn forge_resources(&self, server: Option<&std::path::Path>) {
        match mc_assets::repository::build_stack(&self.jar, &self.base_packs, &self.selected_pack_paths(), server) {
            Ok(packs) => mc_forge::impls::set_resources(move |full, base| if base { packs.read_base(full) } else { packs.read_over_base(full) }),
            Err(e) => eprintln!("forge resources unavailable: {e}"),
        }
    }

    /// Sends the sections and biome columns that changed since the last call: a section counts as changed when its
    /// copy-on-write buffer is no longer the one mirrored, so steady state costs one pointer compare per section.
    fn forge_sync_world(&mut self) -> mc_forge::jni::errors::Result<()> {
        let (generation, sections, biomes) = &mut self.forge_mirror;
        let lights = &mut self.forge_light;
        if *generation != self.forge_generation || !self.in_game { sections.clear(); biomes.clear(); lights.clear(); *generation = self.forge_generation; }
        if !self.in_game { return Ok(()); }
        let world = &self.world;
        mc_forge::runtime::with_env(|env| -> mc_forge::jni::errors::Result<()> {
            if self.forge_skylight != world.skylight_subtracted {
                mc_forge::live::skylight_subtracted(env, world.skylight_subtracted)?;
                self.forge_skylight = world.skylight_subtracted;
            }
            for (&(cx, cz), chunk) in &world.chunks {
                if biomes.get(&(cx, cz)) != Some(&chunk.biomes) {
                    mc_forge::live::biomes(env, cx, cz, &chunk.biomes)?;
                    biomes.insert((cx, cz), chunk.biomes);
                }
                for (cy, section) in chunk.sections.iter().enumerate() {
                    let key = (cx, cy as i32, cz);
                    match (section, sections.get(&key)) {
                        (Some(s), Some(m)) if s.blocks.same(m) => {}
                        (Some(s), _) => { mc_forge::live::section(env, cx, cy as i32, cz, &s.blocks[..])?; sections.insert(key, s.blocks.clone()); }
                        (None, Some(_)) => { mc_forge::live::section(env, cx, cy as i32, cz, &[])?; sections.remove(&key); }
                        (None, None) => {}
                    }
                    match (section, lights.get(&key)) {
                        (Some(s), Some((sky, block))) if s.sky_light.same(sky) && s.block_light.same(block) => {}
                        (Some(s), _) => { mc_forge::live::light(env, cx, cy as i32, cz, &s.sky_light[..], &s.block_light[..])?; lights.insert(key, (s.sky_light.clone(), s.block_light.clone())); }
                        (None, Some(_)) => { mc_forge::live::light(env, cx, cy as i32, cz, &[], &[])?; lights.remove(&key); }
                        (None, None) => {}
                    }
                }
            }
            let gone: Vec<_> = biomes.keys().filter(|k| !world.chunks.contains_key(k)).copied().collect();
            for (cx, cz) in gone {
                mc_forge::live::biomes(env, cx, cz, &[])?;
                biomes.remove(&(cx, cz));
                for cy in 0..16 {
                    if sections.remove(&(cx, cy, cz)).is_some() { mc_forge::live::section(env, cx, cy, cz, &[])?; }
                    if lights.remove(&(cx, cy, cz)).is_some() { mc_forge::live::light(env, cx, cy, cz, &[], &[])?; }
                }
            }
            Ok(())
        }).unwrap_or(Ok(()))
    }

    /// Mirrors the client into Java. Every hook publishes first, but most hooks in one tick see the same state:
    /// an unchanged snapshot is not re-sent (Live.sync parses it and walks every entity by reflection).
    pub(crate) fn forge_publish(&mut self) {
        if !self.forge_on { return; }
        let snapshot=self.forge_snapshot();
        if LAST_PUBLISHED.with(|l|l.borrow().as_ref()==Some(&snapshot)) { return; }
        if self.forge_result(mc_forge::runtime::with_env(|env|mc_forge::live::publish(env,&snapshot))).is_some() {
            LAST_PUBLISHED.with(|l|*l.borrow_mut()=Some(snapshot));
            // `Minecraft.loadWorld(WorldClient)`: the mirror already holds the new world, so the Java method only
            // runs mods' Mixin handlers. Cancelling it still counts as delivered. While out of a world a reset
            // generation stays null, which is not a world change and must not fire another `loadWorld(null)`.
            let world=(self.forge_generation,self.in_game);
            if load_world_due(self.forge_world_hook,self.forge_generation,self.in_game) {
                if self.forge_result(mc_forge::runtime::with_env(mc_forge::hooks::load_world)).is_some() {
                    self.forge_world_hook=Some(world);
                }
            }
        } else { forge_republish(); }
    }

    /// `ClientCommandHandler.executeCommand` over `line` (no leading slash): true when a mod command owned it.
    pub(crate) fn forge_client_command(&self, line: &str) -> bool {
        let mut parts = line.split(' ');
        let name = parts.next().unwrap_or_default().to_owned();
        let args: Vec<String> = parts.map(str::to_owned).collect();
        self.forge_result(mc_forge::runtime::with_env(|env| mc_forge::impls::execute_client_command(env, &name, &args))).unwrap_or(false)
    }
    /// Writes finished mod pings back into their `ServerData`.
    pub(crate) fn forge_finish_pings(&mut self) {
        if self.forge_pings.is_empty() { return; }
        let (done, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.forge_pings).into_iter().partition(|(_, _, d)| d.load(std::sync::atomic::Ordering::Acquire));
        self.forge_pings = pending;
        for (id, h, _) in done {
            let d = h.lock().unwrap().clone();
            let r = mc_forge::impls::PingResult { motd: &d.motd, population: &d.population_info, ping: d.ping, version: d.version, game_version: &d.game_version, player_list: d.player_list.as_deref(), icon: d.icon.as_deref() };
            self.forge_result(mc_forge::runtime::with_env(|env| mc_forge::impls::finish_ping(env, id, &r)));
        }
    }
    fn forge_result<T>(&self,result:Option<mc_forge::jni::errors::Result<T>>)->Option<T> {
        match result? { Ok(v)=>Some(v),Err(e)=>{
            eprintln!("forge live bridge: {e}");
            mc_forge::runtime::with_env(|env|{let _=env.exception_describe();let _=env.exception_clear();}); None
        }}
    }

    pub(crate) fn forge_apply(&mut self) {
        if !self.forge_on { return; }
        let Some(changes)=self.forge_result(mc_forge::runtime::with_env(mc_forge::live::changes))else{return};
        if changes["generation"].as_u64()!=Some(self.forge_generation){return;}
        // Java changed state Rust may have clamped or refused: the next publish must overwrite it even if equal.
        if changes["player"].as_object().is_some_and(|p|!p.is_empty()) { forge_republish(); }
        if self.in_game { if let Some(p)=changes.get("player") {
            let f=|k:&str|p[k].as_f64().filter(|v|v.is_finite());
            if p.get("x").is_some()||p.get("y").is_some()||p.get("z").is_some() {
                let (x,y,z)=(f("x").unwrap_or(self.entity.pos.x),f("y").unwrap_or(self.entity.pos.y),f("z").unwrap_or(self.entity.pos.z));
                if x.abs()<=30_000_000. && z.abs()<=30_000_000. && y.abs()<=30_000_000. {self.entity.set_position(mc_core::Vec3::new(x,y,z));}
            }
            if let Some(yaw)=f("yaw"){self.entity.yaw=yaw as f32;}
            if let Some(pitch)=f("pitch"){self.entity.pitch=(pitch as f32).clamp(-90.,90.);}
            if p.get("motion_x").is_some()||p.get("motion_y").is_some()||p.get("motion_z").is_some(){
                let(x,y,z)=(f("motion_x").unwrap_or(self.entity.motion.x),f("motion_y").unwrap_or(self.entity.motion.y),f("motion_z").unwrap_or(self.entity.motion.z));
                self.entity.motion=mc_core::Vec3::new(x.clamp(-100.,100.),y.clamp(-100.,100.),z.clamp(-100.,100.));}
            if let Some(v)=p["on_ground"].as_bool(){self.entity.on_ground=v;}
            if let Some(v)=p["no_clip"].as_bool(){self.entity.no_clip=v;self.entity.mod_no_clip=Some(v);}
            if let Some(v)=p["sprinting"].as_bool(){self.entity.set_sprinting(v);}
            if let Some(v)=p["selected_slot"].as_u64().filter(|v|*v<9){self.player.selected_slot=v as usize;}
            if let Some(held)=p["held"].as_array(){self.forge_held=held.iter().filter_map(|k|k.as_str().map(str::to_owned)).take(64).collect();}
            if ["strafe","forward","jump","sneak"].iter().any(|key|p.get(key).is_some()) {
                let normal=self.forge_input();
                let s=f("strafe").unwrap_or(normal["strafe"].as_f64().unwrap_or(0.));
                let forward=f("forward").unwrap_or(normal["forward"].as_f64().unwrap_or(0.));
                self.entity.movement_override=Some((s.clamp(-1.,1.)as f32,forward.clamp(-1.,1.)as f32,p["jump"].as_bool().unwrap_or(normal["jump"].as_bool().unwrap_or(false)),p["sneak"].as_bool().unwrap_or(normal["sneak"].as_bool().unwrap_or(false))));
            }
        }}
        if let Some(actions)=changes["actions"].as_array(){for action in actions.iter().take(1024){
            if !self.in_game{break;}
            let v=&action["values"];
            match action["kind"].as_str(){
                Some("attack")=>if let Some(id)=v[0].as_i64().and_then(|n|i32::try_from(n).ok()){if self.remote.map.contains_key(&id){self.attack_entity(id);}},
                Some("swing")=>self.swing(),
                Some("local_chat")=>if let Some(json)=v[0].as_str().filter(|s|s.len()<=262_144){self.hud.add_chat(crate::chat::parse(json,&self.ui.lang));},
                Some("chat")=>if let Some(text)=v[0].as_str().filter(|s|s.chars().count()<=100){self.send_chat(text);},
                Some("packet")=>if let Some(data)=v.as_array().filter(|d|d.len()<=2_097_152){
                    let bytes:Option<Vec<u8>>=data.iter().map(|v|v.as_i64().filter(|n|(-128..=255).contains(n)).map(|v|v as u8)).collect();
                    if let Some(bytes)=bytes{match mc_protocol::packets::PlayServerbound::read(&bytes){Ok(p)=>self.send(p),Err(e)=>eprintln!("rejected invalid mod packet: {e}")}}
                },
                _=>{}
            }
        }}
        self.forge_effects();
    }

    /// Drains what mods asked of the client outside the snapshot delta: sounds and `ClientAction`s.
    fn forge_effects(&mut self) {
        use mc_forge::impls::{self, ClientAction};
        self.forge_finish_pings();
        impls::set_fps(self.fps as i32);
        for s in impls::take_sounds().into_iter().take(256) {
            let pos=s.position().map(|p|mc_core::Vec3::new(p[0],p[1],p[2]));
            self.play_sound(s.event(),pos,s.volume,s.pitch);
        }
        for action in impls::take_client_actions().into_iter().take(1024) {
            match action {
                ClientAction::Chat{json,id} if json.len()<=262_144 => self.hud.add_chat_with_id(crate::chat::parse(&json,&self.ui.lang),id),
                ClientAction::ClearChat => self.hud.chat.clear(),
                ClientAction::DeleteChatLine(id) => self.hud.chat.delete_line(id),
                ClientAction::RefreshChat => self.hud.invalidate_text_layout(),
                ClientAction::ScrollChat(n) => { let open=mc_forge::impls::is_chat_open(); self.hud.chat.scroll(&self.ui,n,open); }
                ClientAction::AddSentMessage(m) => if self.ui.sent_messages.last()!=Some(&m) { self.ui.sent_messages.push(m); },
                ClientAction::Shutdown => self.forge_quit=true,
                ClientAction::ToggleFullscreen => { self.ui.settings.fullscreen=!self.ui.settings.fullscreen; self.ui.settings.save(); self.apply_settings(); }
                ClientAction::Resize{width,height} => if let Some(w)=&self.window { let _=w.request_inner_size(winit::dpi::PhysicalSize::new(width.max(1) as u32,height.max(1) as u32)); },
                ClientAction::SaveOptions => self.ui.settings.save(),
                ClientAction::Screenshot{path} => self.save_screenshot((!path.is_empty()).then(||std::path::PathBuf::from(path))),
                ClientAction::Connect{host,port} => { let name=self.name.clone(); self.start_connect(&format!("{host}:{port}"),&name,self.session.is_some(),None); }
                ClientAction::Disconnect{..} => self.disconnect_to_server_list(),
                ClientAction::WindowClick{window_id,slot,button,mode} => if let Some(w)=self.open_window.clone() {
                    if w.lock().unwrap().id as i32==window_id {
                        crate::container::ContainerScreen::new(w,self.catalogue.clone()).replay_click(&mut self.ui,slot as i16,button as i8,mode as i8);
                    }
                },
                ClientAction::ReloadResources => self.ui.pack_reload=true,
                ClientAction::Ping{address,id} => {
                    // ServerListEntryNormal's pinger thread, then the result is written back on this thread.
                    let h: mc_gui::server_data::Handle = std::sync::Arc::new(std::sync::Mutex::new(mc_gui::server_data::ServerData::new("", &address, false)));
                    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let (h2, d2, lang) = (h.clone(), done.clone(), self.ui.lang.clone());
                    std::thread::spawn(move || {
                        crate::screens_glue::server_list::ping_server(&h2, &lang, &std::sync::atomic::AtomicBool::new(false));
                        d2.store(true, std::sync::atomic::Ordering::Release);
                    });
                    self.forge_pings.push((id, h, done));
                }
                ClientAction::ReloadRenderers => for &(x,z) in self.world.chunks.keys() { for sy in 0..16 { self.world.dirty.insert((x,sy,z)); } },
                ClientAction::StopBreaking => {
                    if let Some(old)=self.hitting.take() { self.send_dig(mc_protocol::packets::DiggingStatus::AbortDestroyBlock,old,mc_core::Facing::Down); }
                    self.block_damage=0.;
                }
                other => eprintln!("forge: client action not wired yet: {other:?}"),
            }
        }
    }

    /// Runs `body` inside a Java hook method whose Mixin HEAD/RETURN wrap it; a cancel skips it. Without Java, or if
    /// the call fails before reaching the body, the body runs natively.
    fn forge_hooked(&mut self,hook:impl FnOnce(&mut dyn FnMut())->Option<mc_forge::jni::errors::Result<bool>>,body:impl FnOnce(&mut Self)){
        let mut body=Some(body);
        let result={let this=&mut *self;hook(&mut ||if let Some(b)=body.take(){b(this)})};
        let failed=!matches!(result,Some(Ok(_)));
        self.forge_result(result);
        if failed {if let Some(b)=body.take(){b(self);}}
    }

    pub(crate) fn forge_click_hook(&mut self,button:MouseButton,body:impl FnOnce(&mut Self)){
        use mc_forge::hooks::Click;
        let kind=match button {MouseButton::Left=>Click::Left,MouseButton::Right=>Click::Right,MouseButton::Middle=>Click::Middle,_=>{body(self);return;}};
        if !self.forge_on||!self.in_game {body(self);return;}
        self.forge_publish();
        self.forge_hooked(|b|mc_forge::hooks::click(kind,b),body);
        self.forge_apply();
    }

    /// `Minecraft.runGameLoop` around one frame (update and draw): mods' render-tick hooks fire at its HEAD.
    pub(crate) fn forge_frame(&mut self,body:impl FnOnce(&mut Self)){
        if !self.forge_on {body(self);return;}
        self.forge_hooked(mc_forge::hooks::game_loop,body);
    }

    pub(crate) fn forge_run_client_tick(&mut self,paused:bool){
        if !self.forge_on {self.client_tick_body(paused);return;}
        self.forge_publish();
        // Once a tick, after the publish that may have reset the Java world for a new generation.
        if let Err(e)=self.forge_sync_world(){eprintln!("forge world mirror: {e}");}
        let result=mc_forge::hooks::run_tick(&mut ||self.client_tick_body(paused));
        self.forge_result(result);
        self.forge_apply();
    }

    /// `EntityPlayer.onUpdate` for the local player: PlayerTickEvent START, `ForgeHooks.onLivingUpdate`
    /// (a cancel skips the update), the Rust body, then PlayerTickEvent END. Java changes apply before physics.
    pub(crate) fn forge_player_update(&mut self,body:impl FnOnce(&mut Self)){
        if !self.forge_on {body(self);return;}
        self.forge_publish();
        let cancelled=self.forge_result(mc_forge::runtime::with_env(|env|{
            mc_forge::game::player_tick(env,true)?;mc_forge::game::living_update(env)})).unwrap_or(false);
        self.forge_apply();
        if !cancelled {body(self);}
        self.forge_publish();
        self.forge_result(mc_forge::runtime::with_env(|env|mc_forge::game::player_tick(env,false)));
        self.forge_apply();
    }

    /// `ForgeHooks.onPlayerAttackTarget`; false if a mod cancelled the attack.
    pub(crate) fn forge_allow_attack(&mut self,id:i32)->bool{
        if !self.forge_on {return true;}
        self.forge_publish();
        let cancelled=self.forge_result(mc_forge::runtime::with_env(|env|mc_forge::game::attack_entity(env,id))).unwrap_or(false);
        self.forge_apply();
        !cancelled
    }

    pub(crate) fn send_walking_update(&mut self){
        if self.forge_on{
            self.forge_publish();
            let result=mc_forge::runtime::with_env(|env|{
                let mc=mc_forge::impls::minecraft(env)?;
                let p=env.get_field(mc,"field_71439_g","Lnet/minecraft/client/entity/EntityPlayerSP;")?.l()?;
                mc_forge::hooks::call(env,&p,"func_175161_p",&mut ||{self.forge_apply();self.send_walking_body();})
            });
            self.forge_result(result);self.forge_apply();
        }else{self.send_walking_body();}
    }
    fn send_walking_body(&mut self){
        if let Some(n)=&mut self.net{let e=&self.entity;
            n.update_walking(e.sprinting,e.is_sneaking(),self.camera_entity.is_none(),None,e.pos.x,e.bb.min_y,e.pos.z,e.yaw,e.pitch,e.on_ground);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_world_due;

    #[test]
    fn load_world_fires_on_join_respawn_and_leave() {
        assert!(load_world_due(None, 0, false), "the first publish has no baseline");
        assert!(!load_world_due(Some((0, false)), 1, false), "a reset while out of a world is not a change");
        assert!(load_world_due(Some((1, false)), 1, true), "join");
        assert!(load_world_due(Some((1, true)), 2, true), "respawn or dimension change");
        assert!(load_world_due(Some((2, true)), 3, false), "leave");
        assert!(!load_world_due(Some((3, false)), 4, false), "reconnect reset before joining");
        assert!(load_world_due(Some((3, false)), 4, true), "reconnect joins the new world");
    }
}
