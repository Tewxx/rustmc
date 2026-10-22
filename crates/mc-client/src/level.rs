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
    pub fn names(&self) -> Vec<&str> {
        self.rules.keys().map(String::as_str).collect()
    }
    pub fn has(&self, name: &str) -> bool {
        self.rules.contains_key(name)
    }
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
