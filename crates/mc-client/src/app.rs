//! The `App` state and its constructor.

use crate::*;

pub(crate) struct App {
    pub(crate) entity: player::Player,
    pub(crate) spawn: mc_core::Vec3,
    pub(crate) catalogue: Arc<inventory::Catalogue>,
    pub(crate) target: Option<player::Target>,
    pub(crate) buttons: HashSet<MouseButton>,
    /// Presses since the last tick, handled at the start of the next one like LWJGL's event queue.
    pub(crate) clicks: Vec<MouseButton>,
    pub(crate) right_click_delay: u8,
    pub(crate) left_click_counter: u8,
    // PlayerControllerMP
    pub(crate) hitting: Option<BlockPos>,
    pub(crate) block_damage: f32,
    pub(crate) block_hit_delay: u8,
    /// Dropped item entities and the world's entity id counter.
    pub(crate) items: Vec<mc_entity::item::ItemEntity>,
    pub(crate) next_entity_id: u32,
    pub(crate) rand: mc_core::random::JavaRandom,
    /// `WorldClient.effectRenderer`: server particles (`S2APacketParticles`).
    pub(crate) particles: particles::Particles,
    /// `NetHandlerPlayClient.avRandomizer`, separate from `world.rand` like vanilla.
    pub(crate) av_rand: mc_core::random::JavaRandom,
    /// Integrated server tick state (singleplayer).
    pub(crate) server: server::Server,
    /// Q presses since the last tick (with Ctrl held: whole stack).
    pub(crate) drop_presses: Vec<bool>,
    pub(crate) info: level::WorldInfo,
    pub(crate) weather: level::Weather,
    // EntityRenderer.updateTorchFlicker
    pub(crate) torch_flicker_x: f32,
    pub(crate) torch_flicker_dx: f32,
    pub(crate) lightmap: [u8; 1024],
    pub(crate) assets: AssetSource,
    pub(crate) models: BlockModels,
    pub(crate) world: World,
    pub(crate) geometry: BlockGeometry,
    pub(crate) renderer: Option<Renderer>,
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) camera: Camera,
    pub(crate) ui: Ui,
    pub(crate) hud: Hud,
    pub(crate) player: PlayerHud,
    /// `EntityPlayerSP.hasValidHealth`
    pub(crate) has_valid_health: bool,
    /// `EntityPlayer.getScore` (data watcher 18), shown on the death screen.
    pub(crate) score: i32,
    pub(crate) batch: GuiBatch,
    /// Open screen and the screens it was opened from (parents).
    pub(crate) screen: Option<Box<dyn Screen>>,
    pub(crate) parents: Vec<Box<dyn Screen>>,
    pub(crate) in_game: bool,
    pub(crate) keys: HashSet<KeyCode>,
    pub(crate) grabbed: bool,
    pub(crate) mouse_px: (f64, f64),
    /// Applied settings, to notice changes made in option screens.
    pub(crate) applied: Settings,
    pub(crate) last_frame: Instant,
    pub(crate) tick_accum: f32,
    pub(crate) fps_timer: Instant,
    pub(crate) frames: u32,
    pub(crate) fps: u32,
    pub(crate) screenshot: Option<(PathBuf, String)>,
    pub(crate) bench: Option<Bench>,
    /// The server connection when playing multiplayer.
    pub(crate) net: Option<net::Net>,
    /// `GuiDisconnected` already shown for this connection.
    pub(crate) disconnect_shown: bool,
    pub(crate) name: String,
    pub(crate) remote: entities::Entities,
    pub(crate) music: music::MusicTicker,
    pub(crate) skins: skins::Skins,
    /// Inventory as last synced, to send creative edits (C10) for what changed.
    pub(crate) creative_sync: Option<([Option<mc_gui::ItemStack>; 36], [Option<mc_gui::ItemStack>; 4])>,
    /// The open server window (`openContainer` other than the player's).
    pub(crate) open_window: Option<Arc<std::sync::Mutex<container::Window>>>,
    /// Tab list names by profile UUID (`playerInfoMap`).
    pub(crate) player_names: std::collections::HashMap<mc_protocol::Uuid, String>,
    /// The world scoreboard and the tab list's `playerInfoMap`.
    pub(crate) scoreboard: scoreboard::Scoreboard,
    /// `GuiIngame.highlightingItemStack`
    pub(crate) highlight_stack: Option<mc_gui::ItemStack>,
    /// Screenshot cases show the player list without the key held.
    pub(crate) force_tab: bool,
    pub(crate) sound: Option<mc_sound::SoundManager>,
    /// `PlayerControllerMP.stepSoundTickCounter`
    pub(crate) step_sound_counter: f32,
    /// `objectMouseOver` when it is an entity.
    pub(crate) pointed_entity: Option<i32>,
    /// Where the look ray meets the pointed entity's box (`objectMouseOver.hitVec`).
    pub(crate) pointed_hit: mc_core::Vec3,
    /// `EntityPlayer.itemInUse` / `itemInUseCount`.
    pub(crate) item_in_use: Option<mc_gui::ItemStack>,
    pub(crate) item_in_use_count: i32,
    /// Item entities announced by `SpawnObject`, waiting for their stack: (pos, velocity).
    pub(crate) pending_items: std::collections::HashMap<i32, (mc_core::Vec3, mc_core::Vec3)>,
    /// `EntityXPOrb`s in the client world.
    /// `EntityPainting`s in the client world.
    pub(crate) paintings: Vec<entities::Painting>,
    /// `EntityLightningBolt`s in the client world.
    pub(crate) lightning: Vec<entities::Lightning>,
    /// `Entity.ridingEntity` by rider id (`AttachEntity`).
    pub(crate) attached: std::collections::HashMap<i32, i32>,
    /// `WorldClient.mapBlockBreakProgress`: block -> (breaker, stage 0..9).
    pub(crate) break_anims: std::collections::HashMap<BlockPos, (i32, u8)>,
    /// `EntityPlayerSP.sleeping`/`bedLocation`.
    pub(crate) bed: Option<BlockPos>,
    /// `WorldClient.getWorldBorder`.
    pub(crate) border: level::WorldBorder,
    /// `NetHandlerPlayClient.clientBrand` (server brand from `MC|Brand`).
    pub(crate) brand: String,
    /// `Statistics`
    pub(crate) stats: std::collections::HashMap<String, i32>,
    /// `WorldClient.mapStorage` map data (`S34`).
    pub(crate) maps: std::collections::HashMap<i32, level::MapData>,
    /// `WorldClient.tileEntityMap`: chests keep their `S35` NBT and their `update()` state here;
    /// skulls, banners, beacons and spawners keep their NBT for their future renderers.
    pub(crate) tiles: tiles::Tiles,
    /// `TileEntityChestRenderer.isChristmas`: December 24th-26th.
    pub(crate) christmas: bool,
    /// Sign text (`S33PacketUpdateSign`).
    pub(crate) signs: std::collections::HashMap<BlockPos, [String; 4]>,
    /// `Minecraft.setRenderViewEntity` (`S43PacketCamera`).
    pub(crate) camera_entity: Option<i32>,
    /// `GuiChat`'s pending tab completion: the text it was requested for and the matches.
    pub(crate) tab_complete: Option<(String, Vec<String>)>,
    /// `ServerData.ServerResourceMode`, the pending prompt and the download in flight.
    pub(crate) packs: pack::ResourcePacks,
    /// Remembered per-server pack choices and cached packs (`server-packs.json`).
    pub(crate) pack_store: pack::Store,
    /// The `host:port` key for `pack_store`.
    pub(crate) server_key: String,
    /// The base pack stack (`--jar` and `--pack`), needed to rebuild `assets` when a server
    /// pack replaces the previous one instead of stacking forever.
    pub(crate) jar: PathBuf,
    pub(crate) base_packs: Vec<PathBuf>,
}

impl App {
    pub(crate) fn new(assets: AssetSource, models: BlockModels, world: World, center: (i32, i32), settings: Settings) -> Self {
        let ui = Ui::new(&assets, settings.clone());
        let catalogue = Arc::new(inventory::Catalogue::load(&assets, &models));
        let spawn = find_player_spawn(&world, center);
        let mut entity = player::Player::new(spawn);
        entity.yaw = -45.;
        entity.pitch = 20.;
        entity.prev_yaw = -45.;
        entity.prev_pitch = 20.;
        Self {
            entity,
            spawn,
            catalogue,
            target: None,
            buttons: HashSet::new(),
            clicks: Vec::new(),
            right_click_delay: 0,
            left_click_counter: 0,
            hitting: None,
            block_damage: 0.,
            block_hit_delay: 0,
            items: Vec::new(),
            next_entity_id: 1,
            rand: mc_core::random::JavaRandom::new(0x5DEECE66D),
            particles: particles::Particles::default(),
            av_rand: mc_core::random::JavaRandom::new(0x2545F4914F6CDD1D),
            server: server::Server::default(),
            drop_presses: Vec::new(),
            info: level::WorldInfo::new(0, BlockPos::new(spawn.x.floor() as i32, spawn.y.floor() as i32, spawn.z.floor() as i32)),
            weather: level::Weather::default(),
            torch_flicker_x: 0.,
            torch_flicker_dx: 0.,
            lightmap: lightmap::build(1.0, 0.0, settings.gamma),
            assets,
            models,
            world,
            geometry: BlockGeometry::new(),
            renderer: None,
            window: None,
            camera: Camera {
                pos: Vec3::new(spawn.x as f32, spawn.y as f32 + 1.62, spawn.z as f32),
                yaw: -45.,
                pitch: 20.,
                fov_deg: settings.fov,
                bob: glam::Mat4::IDENTITY,
            },
            ui,
            hud: Hud::default(),
            player: PlayerHud::default(),
            has_valid_health: false,
            score: 0,
            batch: GuiBatch::new(),
            screen: None,
            parents: Vec::new(),
            in_game: false,
            keys: HashSet::new(),
            grabbed: false,
            mouse_px: (0., 0.),
            applied: settings,
            last_frame: Instant::now(),
            tick_accum: 0.,
            fps_timer: Instant::now(),
            frames: 0,
            fps: 0,
            screenshot: None,
            bench: None,
            net: None,
            disconnect_shown: false,
            name: "Player".into(),
            remote: entities::Entities::default(),
            music: music::MusicTicker::default(),
            skins: skins::Skins::default(),
            creative_sync: None,
            open_window: None,
            player_names: Default::default(),
            scoreboard: Default::default(),
            highlight_stack: None,
            force_tab: false,
            sound: None,
            step_sound_counter: 0.,
            pointed_entity: None,
            pointed_hit: mc_core::Vec3::new(0., 0., 0.),
            item_in_use: None,
            item_in_use_count: 0,
            pending_items: Default::default(),
            paintings: Vec::new(),
            lightning: Vec::new(),
            attached: Default::default(),
            break_anims: Default::default(),
            bed: None,
            border: level::WorldBorder::default(),
            brand: String::new(),
            stats: Default::default(),
            maps: Default::default(),
            tiles: Default::default(),
            christmas: tiles::is_christmas(std::time::SystemTime::now()),
            signs: Default::default(),
            camera_entity: None,
            tab_complete: None,
            packs: pack::ResourcePacks::default(),
            pack_store: pack::Store::default(),
            server_key: String::new(),
            jar: PathBuf::new(),
            base_packs: Vec::new(),
        }
    }
    /// `KeyBinding.isKeyDown`: keyboard codes against held keys, negative codes against mouse buttons.
    pub(crate) fn bound_down(&self, name: &str) -> bool {
        let code = self.ui.settings.key(name);
        match code {
            0 => false,
            -100 => self.buttons.contains(&MouseButton::Left),
            -99 => self.buttons.contains(&MouseButton::Right),
            -98 => self.buttons.contains(&MouseButton::Middle),
            c if c < 0 => false,
            c => self.keys.iter().any(|&k| keymap::lwjgl(k) == c),
        }
    }

    pub(crate) fn scaled(&self) -> Scaled {
        let (w, h) = self.renderer.as_ref().map_or((1280, 720), |r| (r.config.width, r.config.height));
        Scaled::new(w, h, self.ui.settings.gui_scale)
    }

    pub(crate) fn mouse_gui(&self) -> (i32, i32) {
        let f = self.scaled().factor as f64;
        ((self.mouse_px.0 / f) as i32, (self.mouse_px.1 / f) as i32)
    }

}
