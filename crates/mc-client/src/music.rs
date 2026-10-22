//! `MusicTicker`: picks the ambient track type and waits vanilla's random delays between songs.
use mc_sound::SoundManager;

/// `MusicTicker.MusicType`: (event, min delay, max delay) in ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MusicType {
    Menu,
    Game,
    Creative,
}

impl MusicType {
    fn event(self) -> &'static str {
        match self {
            MusicType::Menu => "music.menu",
            MusicType::Game => "music.game",
            MusicType::Creative => "music.game.creative",
        }
    }
    fn delays(self) -> (i32, i32) {
        match self {
            MusicType::Menu => (20, 600),
            MusicType::Game => (12000, 24000),
            MusicType::Creative => (1200, 3600),
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

    /// `MusicTicker.update`, once per client tick.
    pub fn update(&mut self, sound: &mut SoundManager, kind: MusicType) {
        const KEY: &str = "music";
        if let Some(cur) = self.current {
            if cur != kind {
                sound.stop_stream(KEY);
                self.time_until_next = self.next_int(cur.delays().0 / 2);
            }
            if !sound.stream_playing(KEY) {
                self.current = None;
                let (min, max) = kind.delays();
                // MathHelper.getRandomIntegerInRange(rand, min, max)
                let r = if min >= max { min } else { self.next_int(max - min + 1) + min };
                self.time_until_next = r.min(self.time_until_next);
            }
        }
        if self.current.is_none() {
            let t = self.time_until_next;
            self.time_until_next -= 1;
            if t <= 0 {
                sound.play_stream(KEY, kind.event(), 1.0);
                self.current = Some(kind);
                self.time_until_next = i32::MAX;
            }
        }
    }
}
