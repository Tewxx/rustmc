//! `WorldInfo` / `GameRules` / `WorldSettings.GameType`: per-world state the integrated server
//! keeps outside the chunks.
use mc_core::BlockPos;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameType {
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl GameType {
    pub fn by_id(id: i32) -> GameType {
        match id {
            1 => GameType::Creative,
            2 => GameType::Adventure,
            3 => GameType::Spectator,
            _ => GameType::Survival,
        }
    }
    #[cfg(feature = "singleplayer")]
    pub fn name(self) -> &'static str {
        match self {
            GameType::Survival => "survival",
            GameType::Creative => "creative",
            GameType::Adventure => "adventure",
            GameType::Spectator => "spectator",
        }
    }
    /// `isAdventure`: adventure and spectator players can't edit blocks.
    pub fn is_adventure(self) -> bool {
        matches!(self, GameType::Adventure | GameType::Spectator)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueType {
    Any,
    Boolean,
    Numerical,
}

/// `GameRules`: a sorted map of string values, typed for the vanilla rules.
#[derive(Clone, Debug)]
pub struct GameRules {
    rules: BTreeMap<String, (String, ValueType)>,
}

impl Default for GameRules {
    fn default() -> Self {
        let mut r = GameRules { rules: BTreeMap::new() };
        for (k, v, t) in [
            ("doFireTick", "true", ValueType::Boolean),
            ("mobGriefing", "true", ValueType::Boolean),
            ("keepInventory", "false", ValueType::Boolean),
            ("doMobSpawning", "true", ValueType::Boolean),
            ("doMobLoot", "true", ValueType::Boolean),
            ("doTileDrops", "true", ValueType::Boolean),
            ("doEntityDrops", "true", ValueType::Boolean),
            ("commandBlockOutput", "true", ValueType::Boolean),
            ("naturalRegeneration", "true", ValueType::Boolean),
            ("doDaylightCycle", "true", ValueType::Boolean),
            ("logAdminCommands", "true", ValueType::Boolean),
            ("showDeathMessages", "true", ValueType::Boolean),
            ("randomTickSpeed", "3", ValueType::Numerical),
            ("sendCommandFeedback", "true", ValueType::Boolean),
            ("reducedDebugInfo", "false", ValueType::Boolean),
        ] {
            r.rules.insert(k.into(), (v.into(), t));
        }
        r
    }
}

impl GameRules {
    #[cfg(feature = "singleplayer")]
    pub fn names(&self) -> Vec<&str> {
        self.rules.keys().map(String::as_str).collect()
    }
    #[cfg(feature = "singleplayer")]
    pub fn has(&self, name: &str) -> bool {
        self.rules.contains_key(name)
    }
    #[cfg(feature = "singleplayer")]
    pub fn get_string(&self, name: &str) -> String {
        self.rules.get(name).map_or(String::new(), |r| r.0.clone())
    }
    /// `Boolean.parseBoolean`
    pub fn get_bool(&self, name: &str) -> bool {
        self.rules.get(name).is_some_and(|r| r.0.eq_ignore_ascii_case("true"))
    }
    /// `Integer.parseInt` falling back to 0, as `GameRules.Value` stores it.
    #[allow(dead_code)]
    pub fn get_int(&self, name: &str) -> i32 {
        self.rules.get(name).and_then(|r| r.0.parse().ok()).unwrap_or(0)
    }
    #[cfg(feature = "singleplayer")]
    pub fn is_boolean(&self, name: &str) -> bool {
        self.rules.get(name).is_some_and(|r| r.1 == ValueType::Boolean)
    }
    /// `setOrCreateGameRule`
    pub fn set(&mut self, name: &str, value: &str) {
        match self.rules.get_mut(name) {
            Some(r) => r.0 = value.into(),
            None => {
                self.rules.insert(name.into(), (value.into(), ValueType::Any));
            }
        }
    }
}

/// `EnumDifficulty`
#[cfg(feature = "singleplayer")]
pub const DIFFICULTY_KEYS: [&str; 4] =
    ["options.difficulty.peaceful", "options.difficulty.easy", "options.difficulty.normal", "options.difficulty.hard"];

#[derive(Clone, Debug)]
pub struct WorldInfo {
    pub seed: i64,
    /// `worldTime`: time of day (and day count).
    pub time: i64,
    /// `totalTime`: ticks since creation.
    pub total_time: i64,
    pub rules: GameRules,
    pub raining: bool,
    pub thundering: bool,
    pub rain_time: i32,
    pub thunder_time: i32,
    pub clean_weather_time: i32,
    pub difficulty: u8,
    #[cfg(feature = "singleplayer")]
    pub game_type: GameType,
    pub spawn: BlockPos,
}

impl WorldInfo {
    pub fn new(seed: i64, spawn: BlockPos) -> Self {
        Self {
            seed,
            time: 0,
            total_time: 0,
            rules: GameRules::default(),
            raining: false,
            thundering: false,
            rain_time: 0,
            thunder_time: 0,
            clean_weather_time: 0,
            difficulty: 2,
            #[cfg(feature = "singleplayer")]
            game_type: GameType::Creative,
            spawn,
        }
    }
}

/// `World.updateWeather` (overworld): rain/thunder timers and strengths.
#[derive(Clone, Copy, Debug, Default)]
pub struct Weather {
    pub prev_rain: f32,
    pub rain: f32,
    pub prev_thunder: f32,
    pub thunder: f32,
}

impl Weather {
    /// `World.setRainStrength`
    pub fn set_rain(&mut self, f: f32) {
        self.prev_rain = f;
        self.rain = f;
    }
    /// `World.setThunderStrength`
    pub fn set_thunder(&mut self, f: f32) {
        self.prev_thunder = f;
        self.thunder = f;
    }
    /// `updateWeather` on a `WorldClient`: the timers belong to the server.
    pub fn tick_remote(&mut self, info: &WorldInfo) {
        self.prev_thunder = self.thunder;
        self.thunder = (self.thunder + if info.thundering { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        self.prev_rain = self.rain;
        self.rain = (self.rain + if info.raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
    }
    pub fn tick(&mut self, info: &mut WorldInfo, rand: &mut mc_core::random::JavaRandom) {
        if info.clean_weather_time > 0 {
            info.clean_weather_time -= 1;
            info.thunder_time = if info.thundering { 1 } else { 2 };
            info.rain_time = if info.raining { 1 } else { 2 };
        }
        if info.thunder_time <= 0 {
            info.thunder_time = if info.thundering { rand.next_int_bounded(12000) + 3600 } else { rand.next_int_bounded(168000) + 12000 };
        } else {
            info.thunder_time -= 1;
            if info.thunder_time <= 0 {
                info.thundering = !info.thundering;
            }
        }
        self.prev_thunder = self.thunder;
        self.thunder = (self.thunder + if info.thundering { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        if info.rain_time <= 0 {
            info.rain_time = if info.raining { rand.next_int_bounded(12000) + 12000 } else { rand.next_int_bounded(168000) + 12000 };
        } else {
            info.rain_time -= 1;
            if info.rain_time <= 0 {
                info.raining = !info.raining;
            }
        }
        self.prev_rain = self.rain;
        self.rain = (self.rain + if info.raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
    }
    /// `getRainStrength(partialTicks)`
    pub fn rain_strength(&self, pt: f32) -> f32 {
        self.prev_rain + (self.rain - self.prev_rain) * pt
    }
    /// `getThunderStrength(partialTicks)` (already multiplied by rain strength).
    pub fn thunder_strength(&self, pt: f32) -> f32 {
        (self.prev_thunder + (self.thunder - self.prev_thunder) * pt) * self.rain_strength(pt)
    }
}

/// `WorldBorder`: the server-driven border the client keeps (`S44PacketWorldBorder`).
#[derive(Clone, Debug)]
pub struct WorldBorder {
    pub center: (f64, f64),
    pub size: f64,
    /// `getTargetSize` and the lerp the client interpolates toward.
    pub target_size: f64,
    pub lerp_start: f64,
    pub lerp_end: f64,
    pub warning_blocks: i32,
    pub warning_time: i32,
}

impl Default for WorldBorder {
    fn default() -> Self {
        // WorldBorder's field defaults: a 60M-block square at 0,0.
        Self { center: (0., 0.), size: 6.0e7, target_size: 6.0e7, lerp_start: 0., lerp_end: 0., warning_blocks: 5, warning_time: 15 }
    }
}

impl WorldBorder {
    /// `WorldBorder.getSize` at wall-clock seconds `now` (`System.currentTimeMillis()/1000`).
    #[allow(dead_code)] // The border renderer and outside damage land with S9/S10.
    pub fn size_at(&self, now: f64) -> f64 {
        if now >= self.lerp_end {
            self.size
        } else {
            self.size + (self.target_size - self.size) * ((now - self.lerp_start) / (self.lerp_end - self.lerp_start)).clamp(0., 1.)
        }
    }

    /// `WorldBorder.contains(BlockPos)`.
    #[allow(dead_code)]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        let h = self.size_at(now_seconds()) / 2.;
        (x as f64 + 1.) > self.center.0 - h && (x as f64) < self.center.0 + h && (z as f64 + 1.) > self.center.1 - h && (z as f64) < self.center.1 + h
    }
}

/// `System.currentTimeMillis() / 1000.0`, the clock `WorldBorder` lerps on.
pub fn now_seconds() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |d| d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_lerps_and_contains_like_world_border() {
        let mut b = WorldBorder::default();
        assert_eq!(b.size_at(0.), 6.0e7);
        b.center = (100., -50.);
        b.size = 10.;
        b.target_size = 20.;
        b.lerp_start = 100.;
        b.lerp_end = 110.;
        assert_eq!(b.size_at(100.), 10.);
        assert_eq!(b.size_at(105.), 15.);
        assert_eq!(b.size_at(120.), 10.); // after the lerp, `size` is authoritative
        b.size = 20.;
        assert!(b.contains(100, -50));
        assert!(b.contains(90, -60));
        assert!(!b.contains(89, -50));
        assert!(!b.contains(110, -50));
    }

    #[test]
    fn map_data_is_128_squared() {
        let m = MapData { scale: 2, colors: vec![0; 128 * 128], icons: Vec::new() };
        assert_eq!(m.colors.len(), 16384);
        assert_eq!(m.scale, 2);
    }
}

/// `MapData`: `S34PacketMaps` colour bytes plus the scale and decorations.
#[derive(Clone, Debug)]
pub struct MapData {
    pub scale: i8,
    /// 128×128 colour indices, row-major (`byte[] colors`).
    pub colors: Vec<u8>,
    pub icons: Vec<(u8, u8, u8, u8)>, // type, x, y, rotation
}
