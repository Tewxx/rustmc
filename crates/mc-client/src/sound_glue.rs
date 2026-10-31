//! Sound playback glue.

use crate::*;

impl App {
    /// `World.playSound` at a position (`None`: a GUI sound).
    pub(crate) fn play_sound(&mut self, event: &str, pos: Option<mc_core::Vec3>, volume: f32, pitch: f32) {
        if let Some(s) = &mut self.sound {
            s.play(event, pos.map(|p| [p.x, p.y, p.z]), volume, pitch);
        }
    }

    /// `playAuxSFX(2001)`: a block's break sound at its center.
    pub(crate) fn block_break_sound(&mut self, state: mc_data::StateId, pos: BlockPos) {
        let st = state.block().sound;
        let c = mc_core::Vec3::new(pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5);
        self.play_sound(st.break_sound, Some(c), (st.volume + 1.0) / 2.0, st.frequency * 0.8);
    }

    /// `Minecraft.runGameLoop`'s `mcSoundHandler.setListener(thePlayer, timer.renderPartialTicks)`:
    /// the local player's partial-tick interpolated eye position and yaw. The listener follows
    /// the player even when a camera entity is set (`EntityRenderer` does not pass that entity).
    pub(crate) fn update_sound_listener(&mut self) {
        let pt = self.tick_accum.clamp(0.0, 1.0);
        let e = &self.entity;
        let pos = e.interpolated(pt);
        let yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * pt;
        // `SoundManager.setListener` adds `getEyeHeight()` after interpolating the feet.
        let eye = [pos.x as f64, pos.y as f64 + e.eye_height() as f64, pos.z as f64];
        if let Some(s) = &mut self.sound {
            s.set_listener(eye, yaw);
        }
    }

    /// Drains sounds queued by the player entity and GUI widgets; updates the listener.
    pub(crate) fn flush_sounds(&mut self) {
        let pos = self.entity.pos;
        for (ev, v, p) in std::mem::take(&mut self.entity.e.sounds) {
            self.play_sound(ev, Some(pos), v, p);
        }
        for ev in std::mem::take(&mut self.ui.sounds) {
            // PositionedSoundRecord.create(location, 1.0F)
            self.play_sound(ev, None, 0.25, 1.0);
        }
        if let Some(s) = &mut self.sound {
            for c in mc_sound::Category::ALL {
                s.volumes.insert(c, self.ui.settings.sound_volume(c.key()));
            }
        }
        self.update_sound_listener();
    }

}
