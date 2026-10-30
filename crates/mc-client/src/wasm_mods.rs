//! Native WASM mods use a worker; the render thread only enqueues and consumes finished output.
use crate::*;
use mc_mods::{draw::Draw, rustmc::plugin::{game::PlayerState, types::Vec3 as ModVec}, FrameCtx, Snapshot};

impl App {
    pub(crate) fn mod_tick(&self) {
        let Some(mods) = &self.mods else { return };
        let e = &self.entity;
        let v = |p: mc_core::Vec3| ModVec { x: p.x, y: p.y, z: p.z };
        let p = PlayerState {
            pos: v(e.pos), prev_pos: v(e.last_tick_pos), motion: v(e.motion), yaw: e.yaw, pitch: e.pitch,
            on_ground: e.on_ground, health: self.player.health, food: self.player.food, saturation: self.player.saturation,
            xp_level: self.player.level, selected_slot: self.player.selected_slot as u8,
            sprinting: e.sprinting, sneaking: e.is_sneaking(), flying: e.flying,
            gamemode: match e.game_type { level::GameType::Survival => 0, level::GameType::Creative => 1, level::GameType::Adventure => 2, level::GameType::Spectator => 3 },
        };
        mods.tick(Snapshot { player: p, ticks: self.hud.update_counter as u32 as u64 });
    }
    pub(crate) fn mod_hud(&mut self, sr: Scaled, mx: i32, my: i32) {
        let Some(mods) = &mut self.mods else { return };
        let p = self.camera.pos;
        mods.frame(FrameCtx { partial_ticks: self.tick_accum, fps: self.fps, gui_width: sr.width.max(0) as u32,
            gui_height: sr.height.max(0) as u32, gui_scale: sr.factor.max(0) as u32, mouse_x: mx, mouse_y: my,
            cam: ModVec { x: p.x as f64, y: p.y as f64, z: p.z as f64 }, cam_yaw: self.camera.yaw,
            cam_pitch: self.camera.pitch, fov: self.camera.fov_deg });
        for cmd in mods.latest() {
            match cmd {
                Draw::Rect { left, top, right, bottom, argb } => self.batch.rect(left.clamp(0, sr.width), top.clamp(0, sr.height), right.clamp(0, sr.width), bottom.clamp(0, sr.height), argb),
                Draw::Text { x, y, argb, shadow, text } => {
                    let (x, y) = (x.clamp(0, sr.width) as f32, y.clamp(0, sr.height) as f32);
                    if shadow { self.ui.font.draw_shadow(&mut self.batch, &text, x, y, argb); }
                    else { self.ui.font.draw(&mut self.batch, &text, x, y, argb); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn package_worker_renders_into_gui_batch() {
        let Some(jar) = mc_testutil::jar() else { return };
        let dir = std::env::temp_dir().join(format!("rustmc-mod-client-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut z = zip::ZipWriter::new(std::fs::File::create(dir.join("test.rmod")).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("mod.toml", o).unwrap();
        z.write_all(b"[mod]\nid='test'\nname='Test'\nversion='1.0'\nentry='mod.wasm'\nside='client'\n[api]\nversion='1.0'\nmin='1.0'\n[capabilities]\nhud=true\n").unwrap();
        z.start_file("mod.wasm", o).unwrap(); z.write_all(include_bytes!("../../mc-mods/tests/data/guest.wasm")).unwrap(); z.finish().unwrap();
        let mut assets = AssetSource::new(); assets.push(&jar).unwrap();
        let models = BlockModels::load(&assets);
        let mut world = World::default(); world.insert(mc_world::chunk::Chunk::new(0, 0));
        let mut app = App::new(assets, models, world, (0, 0), Settings::default());
        app.mods = mc_mods::worker::ModWorker::start(&dir);
        assert!(app.mods.is_some());
        let sr = Scaled::new(854, 480, 1);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            app.batch.clear(); app.mod_tick(); app.mod_hud(sr, 0, 0);
            if !app.batch.items.is_empty() { break; }
            assert!(Instant::now() < deadline, "mod did not produce a HUD batch");
            std::thread::sleep(Duration::from_millis(10));
        }
        app.mods.as_ref().unwrap().clear();
        assert!(app.mods.as_mut().unwrap().latest().is_empty(), "draw output from the previous world survived reset");
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
