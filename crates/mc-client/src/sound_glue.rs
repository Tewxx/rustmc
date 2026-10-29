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
        let eye = self.entity.pos;
        let yaw = self.entity.yaw;
        let h = self.entity.eye_height() as f64;
        if let Some(s) = &mut self.sound {
            s.set_listener([eye.x, eye.y + h, eye.z], yaw);
            for c in mc_sound::Category::ALL {
                s.volumes.insert(c, self.ui.settings.sound_volume(c.key()));
            }
        }
    }

}
