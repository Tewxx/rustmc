//! `MusicTicker`: picks the ambient track type and waits vanilla's random delays between songs.
use mc_sound::SoundManager;

/// Streamed music is kept under this one caller key, like `MusicTicker.currentMusic`.
const KEY: &str = "music";

/// `MusicTicker.MusicType`: (event, min delay, max delay) in ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MusicType {
    Menu,
    Game,
    Creative,
    Credits,
    Nether,
    EndBoss,
    End,
}

impl MusicType {
    fn event(self) -> &'static str {
        match self {
            MusicType::Menu => "music.menu",
            MusicType::Game => "music.game",
            MusicType::Creative => "music.game.creative",
            MusicType::Credits => "music.game.end.credits",
            MusicType::Nether => "music.game.nether",
            MusicType::EndBoss => "music.game.end.dragon",
            MusicType::End => "music.game.end",
        }
    }
    fn delays(self) -> (i32, i32) {
        match self {
            MusicType::Menu => (20, 600),
            MusicType::Game => (12000, 24000),
            MusicType::Creative => (1200, 3600),
            MusicType::Credits => (i32::MAX, i32::MAX),
            MusicType::Nether => (1200, 3600),
            MusicType::EndBoss => (0, 0),
            MusicType::End => (6000, 24000),
        }
    }

    /// `Minecraft.getAmbientMusicType`: hell first, then the end (boss only while its status
    /// bar is showing), then creative flight, then the ordinary game track.
    pub fn ambient(in_game: bool, dimension: i32, creative: bool, allow_flying: bool, boss: bool) -> MusicType {
        if !in_game {
            return MusicType::Menu;
        }
        if dimension == -1 {
            return MusicType::Nether;
        }
        if dimension == 1 {
            return if boss { MusicType::EndBoss } else { MusicType::End };
        }
        if creative && allow_flying {
            MusicType::Creative
        } else {
            MusicType::Game
        }
    }
}

pub struct MusicTicker {
    current: Option<MusicType>,
    time_until_next: i32,
    rand: u64,
}

impl Default for MusicTicker {
    fn default() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
        MusicTicker { current: None, time_until_next: 100, rand: seed }
    }
}

impl MusicTicker {
    fn next_int(&mut self, bound: i32) -> i32 {
        self.rand ^= self.rand << 13;
        self.rand ^= self.rand >> 7;
        self.rand ^= self.rand << 17;
        if bound <= 0 {
            0
        } else {
            ((self.rand >> 33) % bound as u64) as i32
        }
    }

    /// `MathHelper.getRandomIntegerInRange`, inclusive on both ends.
    fn random_in_range(&mut self, min: i32, max: i32) -> i32 {
        if min >= max {
            min
        } else {
            self.next_int(max - min + 1) + min
        }
    }

    /// `MusicTicker.update`, once per client tick.
    pub fn update(&mut self, sound: &mut SoundManager, kind: MusicType) {
        if let Some(cur) = self.current {
            if cur != kind {
                sound.stop_stream(KEY);
                // Vanilla uses the *new* ambient type's minimum delay here.
                self.time_until_next = self.random_in_range(0, kind.delays().0 / 2);
            }
            if !sound.stream_playing(KEY) {
                self.current = None;
                let (min, max) = kind.delays();
                self.time_until_next = self.random_in_range(min, max).min(self.time_until_next);
            }
        }
        if self.current.is_none() {
            let t = self.time_until_next;
            self.time_until_next -= 1;
            if t <= 0 {
                self.play(sound, kind);
            }
        }
    }

    /// `MusicTicker.func_181558_a`: start one music type immediately (the end-credits screen).
    pub fn play(&mut self, sound: &mut SoundManager, kind: MusicType) {
        sound.play_stream(KEY, kind.event(), 1.0);
        self.current = Some(kind);
        self.time_until_next = i32::MAX;
    }

    /// `MusicTicker.func_181557_a`: stop the current track and restart the countdown from zero
    /// (the end-credits screen).
    pub fn reset(&mut self, sound: &mut SoundManager) {
        if self.current.is_some() {
            sound.stop_stream(KEY);
            self.current = None;
            self.time_until_next = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn sound() -> SoundManager {
        SoundManager::new(Arc::new(|_: &str| None))
    }

    #[test]
    fn ambient_type_matches_get_ambient_music_type() {
        assert_eq!(MusicType::ambient(false, 0, false, false, false), MusicType::Menu);
        assert_eq!(MusicType::ambient(true, -1, true, true, false), MusicType::Nether);
        assert_eq!(MusicType::ambient(true, 1, true, true, false), MusicType::End);
        assert_eq!(MusicType::ambient(true, 1, true, true, true), MusicType::EndBoss);
        assert_eq!(MusicType::ambient(true, 0, true, true, false), MusicType::Creative);
        assert_eq!(MusicType::ambient(true, 0, true, false, false), MusicType::Game);
        assert_eq!(MusicType::ambient(true, 0, false, false, false), MusicType::Game);
    }

    #[test]
    fn music_type_delays_and_events_match_vanilla() {
        let all = [
            (MusicType::Menu, "music.menu", (20, 600)),
            (MusicType::Game, "music.game", (12000, 24000)),
            (MusicType::Creative, "music.game.creative", (1200, 3600)),
            (MusicType::Credits, "music.game.end.credits", (i32::MAX, i32::MAX)),
            (MusicType::Nether, "music.game.nether", (1200, 3600)),
            (MusicType::EndBoss, "music.game.end.dragon", (0, 0)),
            (MusicType::End, "music.game.end", (6000, 24000)),
        ];
        for (kind, event, delays) in all {
            assert_eq!((kind.event(), kind.delays()), (event, delays), "{kind:?}");
        }
    }

    #[test]
    fn type_change_uses_new_types_half_min_delay() {
        let mut sound = sound();
        let mut ticker = MusicTicker { current: Some(MusicType::Game), time_until_next: 12345, rand: 0x1234_5678_9abc_def1 };
        let mut waiting = 0;
        for _ in 0..64 {
            ticker.current = Some(MusicType::Game);
            ticker.time_until_next = 12345;
            ticker.update(&mut sound, MusicType::Creative);
            // Either it already restarted (time is MAX) or it is waiting in
            // [0, Creative.minDelay/2] = [0, 600]; the old Game type would allow up to 6000.
            if ticker.current.is_none() {
                waiting += 1;
                assert!(ticker.time_until_next <= 600, "{}", ticker.time_until_next);
            }
            ticker.rand = ticker.rand.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        }
        assert!(waiting > 0, "the countdown branch was exercised");
    }

    #[test]
    fn reset_stops_the_current_track() {
        let mut sound = sound();
        let mut ticker = MusicTicker { current: Some(MusicType::Menu), time_until_next: i32::MAX, rand: 1 };
        ticker.reset(&mut sound);
        assert_eq!(ticker.current, None);
        assert_eq!(ticker.time_until_next, 0);
        // A ticker with nothing playing is not disturbed by the credits reset.
        ticker.time_until_next = 25;
        ticker.reset(&mut sound);
        assert_eq!(ticker.time_until_next, 25);
    }

    #[test]
    fn play_starts_a_type_immediately() {
        let mut sound = sound();
        let mut ticker = MusicTicker::default();
        ticker.play(&mut sound, MusicType::Credits);
        assert_eq!(ticker.current, Some(MusicType::Credits));
        assert_eq!(ticker.time_until_next, i32::MAX);
    }
}
