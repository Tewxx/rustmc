//! `SoundHandler` + `SoundManager`: sounds.json events, weighted variants, categories, and
//! paulscode's linear attenuation (`16 × max(volume, 1)` blocks) with stereo panning.
use crate::mixer::Mixer;
use crate::vorbis::{self, Decoded};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Master,
    Music,
    Records,
    Weather,
    Blocks,
    Mobs,
    Animals,
    Players,
    Ambient,
}

impl Category {
    pub fn by_name(s: &str) -> Category {
        match s {
            "music" => Category::Music,
            "record" => Category::Records,
            "weather" => Category::Weather,
            "block" => Category::Blocks,
            "hostile" => Category::Mobs,
            "neutral" => Category::Animals,
            "player" => Category::Players,
            "ambient" => Category::Ambient,
            _ => Category::Master,
        }
    }
    /// `options.txt` key suffix (`soundCategory_<name>`).
    pub fn key(self) -> &'static str {
        match self {
            Category::Master => "master",
            Category::Music => "music",
            Category::Records => "record",
            Category::Weather => "weather",
            Category::Blocks => "block",
            Category::Mobs => "hostile",
            Category::Animals => "neutral",
            Category::Players => "player",
            Category::Ambient => "ambient",
        }
    }
    pub const ALL: [Category; 9] = [
        Category::Master,
        Category::Music,
        Category::Records,
        Category::Weather,
        Category::Blocks,
        Category::Mobs,
        Category::Animals,
        Category::Players,
        Category::Ambient,
    ];
}

#[derive(Clone, Debug)]
struct Entry {
    /// File name (`sounds/<name>.ogg`) or another event when `event`.
    name: String,
    event: bool,
    weight: u32,
    volume: f32,
    pitch: f32,
    stream: bool,
}

#[derive(Clone, Debug)]
struct Event {
    category: Category,
    entries: Vec<Entry>,
}

/// Where sound files come from: resource packs first, then the vanilla asset index.
pub type Reader = Box<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

pub struct SoundManager {
    events: HashMap<String, Event>,
    cache: HashMap<String, Option<Arc<Decoded>>>,
    read: Reader,
    pub mixer: Mixer,
    pub volumes: HashMap<Category, f32>,
    listener: ([f64; 3], f32),
    rand: u64,
    /// Voices of looping/streamed sounds by caller key.
    streams: HashMap<String, u64>,
}

impl SoundManager {
    /// `read("minecraft/sounds.json")` style paths (relative to `assets/`).
    pub fn new(read: Reader) -> SoundManager {
        let mut events = HashMap::new();
        if let Some(json) = read("minecraft/sounds.json").and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            parse_events(&json, &mut events);
        }
        SoundManager {
            events,
            cache: HashMap::new(),
            read,
            mixer: Mixer::new(),
            volumes: Category::ALL.iter().map(|&c| (c, 1.0)).collect(),
            listener: ([0.0; 3], 0.0),
            rand: 0x2545F4914F6CDD1D ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64),
            streams: HashMap::new(),
        }
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    fn next_u32(&mut self) -> u32 {
        self.rand ^= self.rand << 13;
        self.rand ^= self.rand >> 7;
        self.rand ^= self.rand << 17;
        (self.rand >> 16) as u32
    }

    /// `SoundManager.setListener`: eye position and yaw (degrees).
    pub fn set_listener(&mut self, pos: [f64; 3], yaw: f32) {
        self.listener = (pos, yaw);
    }

    /// Resolves an event to a concrete file entry (`SoundEventAccessorComposite.cloneEntry`).
    fn pick(&mut self, event: &str, depth: u32) -> Option<(Entry, Category)> {
        let ev = self.events.get(event)?.clone();
        let total: u32 = ev.entries.iter().map(|e| e.weight).sum();
        if total == 0 || depth > 8 {
            return None;
        }
        let mut r = self.next_u32() % total;
        for e in &ev.entries {
            if r < e.weight {
                if e.event {
                    let (mut inner, _) = self.pick(&e.name, depth + 1)?;
                    inner.volume *= e.volume;
                    inner.pitch *= e.pitch;
                    return Some((inner, ev.category));
                }
                return Some((e.clone(), ev.category));
            }
            r -= e.weight;
        }
        None
    }

    fn load(&mut self, name: &str) -> Option<Arc<Decoded>> {
        if let Some(c) = self.cache.get(name) {
            return c.clone();
        }
        let path = format!("minecraft/sounds/{name}.ogg");
        let pcm = (self.read)(&path).and_then(|b| vorbis::decode(&b).ok()).map(Arc::new);
        self.cache.insert(name.to_string(), pcm.clone());
        pcm
    }

    fn category_volume(&self, c: Category) -> f32 {
        let master = self.volumes.get(&Category::Master).copied().unwrap_or(1.0);
        if c == Category::Master {
            master
        } else {
            master * self.volumes.get(&c).copied().unwrap_or(1.0)
        }
    }

    /// Stereo gains for a mono source at `pos` (paulscode linear attenuation, OpenAL-style
    /// equal-power panning); `None` position = non-positional (GUI clicks).
    fn gains(&self, pos: Option<[f64; 3]>, gain: f32, fade: f32) -> (f32, f32) {
        let Some(p) = pos else { return (gain, gain) };
        let (l, yaw) = self.listener;
        let (dx, dy, dz) = (p[0] - l[0], p[1] - l[1], p[2] - l[2]);
        let dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
        let att = (1.0 - dist / fade).clamp(0.0, 1.0);
        let g = gain * att;
        if dist < 1e-4 {
            return (g, g);
        }
        // Listener right vector for Minecraft yaw (0 = +Z, 90 = -X).
        let y = yaw.to_radians();
        let (rx, rz) = (-(y.cos()), -(y.sin()));
        let pan = ((dx as f32 * rx + dz as f32 * rz) / dist).clamp(-1.0, 1.0);
        let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        (g * a.cos() * std::f32::consts::SQRT_2, g * a.sin() * std::f32::consts::SQRT_2)
    }

    /// `SoundManager.playSound`. `volume`/`pitch` are the caller's (`PositionedSoundRecord`).
    pub fn play(&mut self, event: &str, pos: Option<[f64; 3]>, volume: f32, pitch: f32) {
        if !self.mixer.available {
            return;
        }
        let event = event.strip_prefix("minecraft:").unwrap_or(event);
        let Some((entry, cat)) = self.pick(event, 0) else { return };
        let Some(pcm) = self.load(&entry.name) else { return };
        let base = volume * entry.volume;
        // getNormalizedVolume / getNormalizedPitch
        let gain = (base * self.category_volume(cat)).clamp(0.0, 1.0);
        if gain <= 0.0 {
            return;
        }
        let pitch = (pitch * entry.pitch).clamp(0.5, 2.0);
        let fade = 16.0 * volume.max(1.0);
        let stereo = pcm.channels > 1;
        let g = if stereo { (gain, gain) } else { self.gains(pos, gain, fade) };
        self.mixer.play(pcm, pitch, g, false);
    }

    /// A looping/streamed sound under a caller key (music, rain loops); replaces any previous.
    pub fn play_stream(&mut self, key: &str, event: &str, volume: f32) {
        self.stop_stream(key);
        let Some((entry, cat)) = self.pick(event, 0) else { return };
        let Some(pcm) = self.load(&entry.name) else { return };
        let gain = (volume * entry.volume * self.category_volume(cat)).clamp(0.0, 1.0);
        let id = self.mixer.play(pcm, 1.0, (gain, gain), false);
        self.streams.insert(key.to_string(), id);
    }

    pub fn stream_playing(&self, key: &str) -> bool {
        self.streams.get(key).is_some_and(|&id| self.mixer.is_playing(id))
    }

    pub fn stop_stream(&mut self, key: &str) {
        if let Some(id) = self.streams.remove(key) {
            self.mixer.stop(id);
        }
    }

    pub fn stop_all(&mut self) {
        self.mixer.stop_all();
        self.streams.clear();
    }
}

fn parse_events(json: &Value, out: &mut HashMap<String, Event>) {
    let Some(obj) = json.as_object() else { return };
    for (name, v) in obj {
        let category = Category::by_name(v["category"].as_str().unwrap_or("master"));
        let mut entries = Vec::new();
        for s in v["sounds"].as_array().into_iter().flatten() {
            let e = match s {
                Value::String(n) => Entry { name: n.clone(), event: false, weight: 1, volume: 1.0, pitch: 1.0, stream: false },
                Value::Object(o) => Entry {
                    name: o.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    event: o.get("type").and_then(Value::as_str) == Some("event"),
                    weight: o.get("weight").and_then(Value::as_u64).unwrap_or(1) as u32,
                    volume: o.get("volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    pitch: o.get("pitch").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    stream: o.get("stream").and_then(Value::as_bool).unwrap_or(false),
                },
                _ => continue,
            };
            entries.push(e);
        }
        let _ = entries.iter().any(|e| e.stream);
        out.insert(name.clone(), Event { category, entries });
    }
}

/// Reads files from `.minecraft/assets` through an asset index (`indexes/<id>.json`).
pub fn index_reader(assets_dir: &std::path::Path, index: &str) -> Option<impl Fn(&str) -> Option<Vec<u8>> + Send + Sync> {
    let idx: Value = serde_json::from_slice(&std::fs::read(assets_dir.join("indexes").join(format!("{index}.json"))).ok()?).ok()?;
    let map: HashMap<String, String> = idx["objects"]
        .as_object()?
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v["hash"].as_str()?.to_string())))
        .collect();
    let dir = assets_dir.join("objects");
    Some(move |path: &str| {
        let h = map.get(path)?;
        std::fs::read(dir.join(&h[..2]).join(h)).ok()
    })
}
