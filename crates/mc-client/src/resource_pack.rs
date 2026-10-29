//! Server resource pack prompt, download and application.

use crate::*;

impl App {
    /// `NetHandlerPlayClient.handleResourcePack`: `level://` is a local save pack, otherwise
    /// the per-server mode decides between prompt, accept and decline.
    pub(crate) fn handle_resource_pack(&mut self, url: String, hash: String) {
        use mc_protocol::packets::ResourcePackResult as R;
        let req = pack::Request { url: url.clone(), hash: hash.clone() };
        if url.starts_with("level://") {
            match pack::level_pack_path(&url, Path::new("saves")) {
                Some(path) => {
                    self.send_pack_status(&hash, R::Accepted);
                    self.finish_pack(&hash, Some(path));
                }
                None => self.send_pack_status(&hash, R::FailedDownload),
            }
            return;
        }
        match self.packs.mode {
            pack::Mode::Enabled => {
                self.send_pack_status(&hash, R::Accepted);
                self.start_pack_download(req);
            }
            pack::Mode::Disabled => self.send_pack_status(&hash, R::Declined),
            pack::Mode::Prompt => {
                self.packs.pending = Some(req);
                self.open(Box::new(screens::ResourcePackPrompt::new()), false);
            }
        }
    }

    pub(crate) fn send_pack_status(&mut self, hash: &str, result: mc_protocol::packets::ResourcePackResult) {
        self.send(mc_protocol::packets::PlayServerbound::ResourcePackStatus(mc_protocol::packets::ResourcePackStatus {
            hash: hash.to_string(),
            result,
        }));
    }

    /// Starts the background download and remembers that this server accepts packs, so a
    /// later join can pre-apply the cached file during startup.
    pub(crate) fn start_pack_download(&mut self, req: pack::Request) {
        let hash = req.hash.clone();
        // `ResourcePackRepository.downloadResourcePack`: a cache entry with the right hash is
        // reused instead of fetched again.
        if let Some(path) = pack::cached_pack(&hash) {
            self.finish_pack(&hash, Some(path));
            return;
        }
        if let Err(e) = self.packs.start_download(&req, pack::cache_dir()) {
            self.hud.add_chat(format!("Resource pack: {e}"));
            self.send_pack_status(&hash, mc_protocol::packets::ResourcePackResult::FailedDownload);
            return;
        }
        self.pack_store.set(&self.server_key, pack::Mode::Enabled, &hash, None);
        self.pack_store.save();
    }

    /// The `GuiYesNo` answer (`GuiYesNoCallback.confirmClicked`).
    pub(crate) fn resource_pack_decision(&mut self, accept: bool) {
        use mc_protocol::packets::ResourcePackResult as R;
        let Some(req) = self.packs.pending.take() else { return };
        self.packs.mode = if accept { pack::Mode::Enabled } else { pack::Mode::Disabled };
        self.pack_store.set(&self.server_key, self.packs.mode, &req.hash, None);
        self.pack_store.save();
        self.close_screen();
        if accept {
            self.send_pack_status(&req.hash, R::Accepted);
            self.start_pack_download(req);
        } else {
            self.send_pack_status(&req.hash, R::Declined);
        }
    }

    pub(crate) fn poll_resource_pack(&mut self) {
        use mc_protocol::packets::ResourcePackResult as R;
        let Some((hash, result)) = self.packs.poll() else { return };
        match result {
            Ok(path) => {
                self.pack_store.set(&self.server_key, pack::Mode::Enabled, &hash, Some(&path));
                self.pack_store.save();
                self.finish_pack(&hash, Some(path));
            }
            Err(e) => {
                self.hud.add_chat(format!("Resource pack download failed: {e}"));
                self.send_pack_status(&hash, R::FailedDownload);
            }
        }
    }

    /// `ResourcePackRepository.setResourcePack` + `Minecraft.refreshResources`: push the pack on
    /// top of the stack, rebuild every atlas/model table, recreate the texture-bound renderer
    /// and remesh the loaded world (baked atlas UVs live in the vertex data).
    pub(crate) fn finish_pack(&mut self, hash: &str, path: Option<PathBuf>) {
        use mc_protocol::packets::ResourcePackResult as R;
        let Some(path) = path else {
            self.send_pack_status(hash, R::FailedDownload);
            return;
        };
        match self.apply_resource_pack(&path) {
            Ok(()) => {
                self.packs.applied = Some((hash.to_string(), path));
                self.send_pack_status(hash, R::SuccessfullyLoaded);
            }
            Err(e) => {
                self.hud.add_chat(format!("Resource pack could not be applied: {e}"));
                self.send_pack_status(hash, R::FailedDownload);
            }
        }
    }

    pub(crate) fn apply_resource_pack(&mut self, path: &Path) -> Result<(), String> {
        if self.packs.applied.as_ref().is_some_and(|(_, p)| p == path) {
            return Ok(());
        }
        // Rebuild the stack instead of appending: `resourcePackRepository` has exactly one
        // server pack, so resources that a new pack removes must disappear again.
        let mut assets = AssetSource::new();
        assets.push(&self.jar).map_err(|e| format!("{}: {e}", self.jar.display()))?;
        for p in &self.base_packs {
            if let Err(e) = assets.push(p) {
                eprintln!("skipping pack {}: {e}", p.display());
            }
        }
        assets.push(path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.assets = assets;
        let t = Instant::now();
        self.models = BlockModels::load(&self.assets);
        self.catalogue = Arc::new(inventory::Catalogue::load(&self.assets, &self.models));
        self.ui.reload(&self.assets);
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
        println!("resource pack applied in {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        Ok(())
    }

}
