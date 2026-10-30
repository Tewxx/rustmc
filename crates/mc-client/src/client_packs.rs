//! Resource packs chosen in `GuiScreenResourcePacks`: startup selection from `options.txt`,
//! `Minecraft.refreshResources` (shared with server packs), pack sounds and the screen's
//! GUI textures.

use crate::*;
use mc_assets::repository;
use std::sync::atomic::{AtomicBool, Ordering};

static BOOTED: AtomicBool = AtomicBool::new(false);

impl App {
    /// Runs once per frame before the GUI batch is built: applies the pack selection saved in
    /// `options.txt` the first time, then the screen's requests (reload, folder) and textures.
    pub(crate) fn sync_pack_ui(&mut self) {
        if !BOOTED.swap(true, Ordering::Relaxed) && !self.ui.settings.resource_packs.is_empty() {
            self.boot_resource_packs();
        }
        if let Some(dir) = self.ui.open_folder.take() {
            open_web_link(&dir.to_string_lossy());
        }
        if std::mem::take(&mut self.ui.pack_reload) {
            let server = self.packs.applied.as_ref().map(|(_, p)| p.clone());
            if let Err(e) = self.reload_resources(server) {
                eprintln!("resource pack reload failed: {e}");
                self.hud.add_chat(format!("Resource pack reload failed: {e}"));
            }
        }
        for (path, w, h, rgba) in mc_gui::resource_packs::take_textures() {
            if let Some(r) = &mut self.renderer {
                r.register_gui_texture(&path, w, h, &rgba);
            }
        }
    }

    /// The `ResourcePackRepository` constructor: drops selected packs that vanished or are no
    /// longer compatible, then loads what is left.
    fn boot_resource_packs(&mut self) {
        let s = &self.ui.settings;
        let all = repository::scan(&s.game_dir().join("resourcepacks"));
        let (_, removed) = repository::restore_selection(&all, &s.resource_packs, &s.incompatible_resource_packs);
        let gone: Vec<String> = s.resource_packs.iter().filter(|n| !all.iter().any(|e| &e.name == *n)).cloned().collect();
        if !removed.is_empty() || !gone.is_empty() {
            for n in removed.iter().chain(&gone) {
                eprintln!("Removed selected resource pack {n}: missing or no longer compatible");
            }
            self.ui.settings.resource_packs.retain(|n| !removed.contains(n) && !gone.contains(n));
            self.ui.settings.incompatible_resource_packs.retain(|n| !removed.contains(n) && !gone.contains(n));
            self.ui.settings.save();
        }
        if self.ui.settings.resource_packs.is_empty() {
            return;
        }
        let server = self.packs.applied.as_ref().map(|(_, p)| p.clone());
        if let Err(e) = self.reload_resources(server) {
            eprintln!("resource packs could not be applied: {e}");
        }
    }

    /// The pack paths to stack under the server pack, lowest priority first.
    pub(crate) fn selected_pack_paths(&self) -> Vec<PathBuf> {
        let s = &self.ui.settings;
        repository::selected_paths(&s.game_dir().join("resourcepacks"), &s.resource_packs, &s.incompatible_resource_packs)
    }

    /// `Minecraft.refreshResources`: rebuild the stack as jar + `--pack`s + the selected packs
    /// + the server pack, then re-read models, atlases, the item catalogue, font/lang/splash,
    /// sounds and skins and remesh the loaded world (baked atlas UVs live in the vertex data).
    pub(crate) fn reload_resources(&mut self, server: Option<PathBuf>) -> Result<(), String> {
        let t = Instant::now();
        let selected = self.selected_pack_paths();
        let stack = |s: &Self| repository::build_stack(&s.jar, &s.base_packs, &selected, server.as_deref()).map_err(|e| format!("{}: {e}", s.jar.display()));
        self.assets = stack(self)?;
        self.mesh_workers = None;
        self.models = BlockModels::load(&self.assets);
        self.catalogue = Arc::new(inventory::Catalogue::load(&self.assets, &self.models));
        self.ui.reload(&self.assets);
        if self.sound.is_some() {
            // `SoundHandler.onResourceManagerReload`: every pack's sounds.json, sound files
            // from the highest pack that has them.
            let packs = Arc::new(stack(self)?);
            let json = packs.read_all("sounds.json");
            let overlay: mc_sound::manager::Reader = Arc::new(move |p: &str| p.strip_prefix("minecraft/").and_then(|r| packs.read(r)));
            if let Some(s) = &mut self.sound {
                s.set_packs(overlay, &json);
            }
        }
        // The renderer owns every GPU texture and bind group built from the old stack.
        if let Some(win) = self.window.clone() {
            self.renderer = None;
            let renderer = pollster::block_on(Renderer::new(win, &self.models, &self.assets));
            for (cell, rgba) in self.skins.cells() {
                renderer.upload_skin(cell, &rgba);
            }
            self.renderer = Some(renderer);
        }
        if let Some(r) = &mut self.renderer {
            r.clear_sections();
        }
        for &(x, z) in self.world.chunks.keys() {
            for sy in 0..16 {
                self.world.dirty.insert((x, sy, z));
            }
        }
        self.refresh_edits(&[]);
        println!("resource packs applied in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        Ok(())
    }
}
