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
pub type Reader = std::sync::Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

pub struct SoundManager {
    events: HashMap<String, Event>,
    cache: HashMap<String, Option<Arc<Decoded>>>,
    read: Reader,
    /// The vanilla asset index, under any resource pack overlay.
    base: Reader,
    pub mixer: Mixer,
    pub volumes: HashMap<Category, f32>,
    listener: ([f64; 3], f32),
    rand: u64,
    /// Streamed sounds by caller key.
    streams: HashMap<String, Stream>,
    /// Started non-streaming voices, so listener movement and category changes re-apply their
    /// gains like paulscode's per-frame source updates.
    playing: Vec<Playing>,
}

/// A running non-streaming channel (`SoundManager.playingSounds` entry).
struct Playing {
    id: u64,
    category: Category,
    /// `sound.getVolume() * entry.volume` before the category volume is applied.
    base: f32,
    pos: Option<[f64; 3]>,
    fade: f32,
    stereo: bool,
}

struct Stream {
    /// Mixer voice id; `u64::MAX` while decoding, 0 when stopped or failed.
    voice: std::sync::Arc<std::sync::atomic::AtomicU64>,
    category: Category,
    /// `sound.getVolume() * entry.volume` before the category volume is applied.
    base: f32,
    /// `AttenuationType.LINEAR` position; `None` for `AttenuationType.NONE` (music).
    pos: Option<[f64; 3]>,
    /// `16 × max(volume, 1)` fade distance of the paulscode source.
    fade: f32,
}

/// `SoundManager.getNormalizedVolume`: the caller/entry product is clamped to `[0,1]`
/// *before* the category volume is multiplied in.
fn normalized_volume(volume: f32, category: f32) -> f32 {
    volume.clamp(0.0, 1.0) * category
}

/// `SoundManager.getNormalizedPitch`.
fn normalized_pitch(pitch: f32) -> f32 {
    pitch.clamp(0.5, 2.0)
}

/// The `fadeDistance` `SoundManager.playSound` hands to paulscode: `16 × max(volume, 1)`.
fn fade_distance(volume: f32) -> f32 {
    16.0 * volume.max(1.0)
}

/// `SourceLWJGLOpenAL.calculateGain` for `ATTENUATION_LINEAR` (type 2).
fn attenuation(distance: f32, fade: f32) -> f32 {
    if distance <= 0.0 {
        1.0
    } else if distance >= fade {
        0.0
    } else {
        (1.0 - distance / fade).clamp(0.0, 1.0)
    }
}

/// Stereo gains for a mono source at `pos`: paulscode linear attenuation and OpenAL's
/// equal-power panning against the listener's right vector `(-cos yaw, 0, -sin yaw)`.
fn stereo_gains(listener: ([f64; 3], f32), pos: Option<[f64; 3]>, gain: f32, fade: f32) -> (f32, f32) {
    let Some(p) = pos else { return (gain, gain) };
    let (l, yaw) = listener;
    let (dx, dy, dz) = (p[0] - l[0], p[1] - l[1], p[2] - l[2]);
    let dist = (dx * dx + dy * dy + dz * dz).sqrt() as f32;
    let g = gain * attenuation(dist, fade);
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

impl SoundManager {
    /// `read("minecraft/sounds.json")` style paths (relative to `assets/`).
    pub fn new(read: Reader) -> SoundManager {
        let events = load_events(&read, &[]);
        SoundManager {
            events,
            cache: HashMap::new(),
            base: read.clone(),
            read,
            mixer: Mixer::new(),
            volumes: Category::ALL.iter().map(|&c| (c, 1.0)).collect(),
            listener: ([0.0; 3], 0.0),
            rand: 0x2545F4914F6CDD1D ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64),
            streams: HashMap::new(),
            playing: Vec::new(),
        }
    }

    /// `SoundHandler.onResourceManagerReload` for a new resource pack stack: `overlay` reads
    /// sound files from the packs (highest first, `None` falls back to the asset index) and
    /// `sounds_json` holds every pack's `sounds.json`, lowest layer first.
    pub fn set_packs(&mut self, overlay: Reader, sounds_json: &[Vec<u8>]) {
        let base = self.base.clone();
        self.events = load_events(&base, sounds_json);
        self.read = Arc::new(move |p: &str| overlay(p).or_else(|| base(p)));
        self.cache.clear();
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

    /// `SoundManager.setListener`: eye position and yaw (degrees). Positional sources
    /// re-evaluate their gain, like paulscode's `listenerMoved`.
    pub fn set_listener(&mut self, pos: [f64; 3], yaw: f32) {
        self.listener = (pos, yaw);
        self.refresh_gains();
    }

    /// Resolves an event to a concrete file entry (`SoundEventAccessorComposite.cloneEntry`).
    /// An event entry weighs as much as the nested event and does not apply its own
    /// `SoundList.SoundEntry` volume/pitch: `SoundHandler.loadSoundResource` ignores both for
    /// `type: "event"` entries.
    fn pick(&mut self, event: &str, depth: u32) -> Option<(Entry, Category)> {
        let ev = self.events.get(event)?.clone();
        let total = event_weight(&self.events, event, depth);
        if total == 0 || depth > 8 {
            return None;
        }
        let mut r = self.next_u32() % total;
        for e in &ev.entries {
            let weight = if e.event { event_weight(&self.events, &e.name, depth + 1) } else { e.weight };
            if r < weight {
                if e.event {
                    let (inner, _) = self.pick(&e.name, depth + 1)?;
                    return Some((inner, ev.category));
                }
                return Some((e.clone(), ev.category));
            }
            r -= weight;
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

    /// `SoundManager.playSound`. `volume`/`pitch` are the caller's (`PositionedSoundRecord`).
    pub fn play(&mut self, event: &str, pos: Option<[f64; 3]>, volume: f32, pitch: f32) {
        if !self.mixer.available {
            return;
        }
        let event = event.strip_prefix("minecraft:").unwrap_or(event);
        let Some((entry, cat)) = self.pick(event, 0) else { return };
        let Some(pcm) = self.load(&entry.name) else { return };
        let gain = normalized_volume(volume * entry.volume, self.category_volume(cat));
        if gain <= 0.0 {
            return;
        }
        let pitch = normalized_pitch(pitch * entry.pitch);
        let fade = fade_distance(volume);
        let stereo = pcm.channels > 1;
        let g = if stereo { (gain, gain) } else { stereo_gains(self.listener, pos, gain, fade) };
        let base = volume * entry.volume;
        let id = self.mixer.play(pcm, pitch, g, false);
        self.playing.push(Playing { id, category: cat, base, pos, fade, stereo });
    }

    /// A streamed sound (music) under a caller key, decoded off the main thread; replaces any
    /// previous one. It counts as playing from the request on.
    pub fn play_stream(&mut self, key: &str, event: &str, volume: f32) {
        self.play_stream_at(key, event, None, volume, 1.0);
    }

    /// A streamed `PositionedSoundRecord` under a caller key (`World.playRecord`): decoded off
    /// the main thread and attenuated at `pos` with `AttenuationType.LINEAR`.
    pub fn play_stream_at(&mut self, key: &str, event: &str, pos: Option<[f64; 3]>, volume: f32, pitch: f32) {
        self.stop_stream(key);
        let event = event.strip_prefix("minecraft:").unwrap_or(event);
        let Some((entry, cat)) = self.pick(event, 0) else { return };
        let gain = normalized_volume(volume * entry.volume, self.category_volume(cat));
        if gain <= 0.0 {
            return;
        }
        let pitch = normalized_pitch(pitch * entry.pitch);
        let fade = fade_distance(volume);
        let listener = self.listener;
        let pending = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
        self.streams.insert(key.to_string(), Stream { voice: pending.clone(), category: cat, base: volume * entry.volume, pos, fade });
        let (read, mixer) = (self.read.clone(), self.mixer.clone());
        let path = format!("minecraft/sounds/{}.ogg", entry.name);
        std::thread::spawn(move || {
            let Some(pcm) = read(&path).and_then(|b| vorbis::decode(&b).ok()) else {
                pending.store(0, std::sync::atomic::Ordering::Relaxed);
                return;
            };
            if pending.load(std::sync::atomic::Ordering::Relaxed) == u64::MAX {
                let g = if pcm.channels > 1 { (gain, gain) } else { stereo_gains(listener, pos, gain, fade) };
                let id = mixer.play(std::sync::Arc::new(pcm), pitch, g, false);
                // Stopped meanwhile? Then drop it right away.
                if pending.compare_exchange(u64::MAX, id, std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed).is_err() {
                    mixer.stop(id);
                }
            }
        });
    }

    pub fn stream_playing(&self, key: &str) -> bool {
        self.streams.get(key).is_some_and(|s| match s.voice.load(std::sync::atomic::Ordering::Relaxed) {
            u64::MAX => true,
            0 => false,
            id => self.mixer.is_playing(id),
        })
    }

    /// Re-applies category volumes and the listener's distance/panning to running voices (the
    /// sound sliders and camera movement).
    pub fn update_stream_volumes(&mut self) {
        self.refresh_gains();
    }

    /// Recomputes every running voice's stereo gains from its stored source and the listener.
    fn refresh_gains(&mut self) {
        let mixer = self.mixer.clone();
        // Drop channels the mixer has already retired.
        self.playing.retain(|p| mixer.is_playing(p.id));
        for p in &self.playing {
            let gain = normalized_volume(p.base, self.category_volume(p.category));
            if gain <= 0.0 {
                // `SoundManager.setSoundCategoryVolume` stops a source whose volume reaches zero.
                mixer.stop(p.id);
            } else if p.stereo {
                mixer.set_gain(p.id, (gain, gain));
            } else {
                mixer.set_gain(p.id, stereo_gains(self.listener, p.pos, gain, p.fade));
            }
        }
        for s in self.streams.values() {
            let id = s.voice.load(std::sync::atomic::Ordering::Relaxed);
            if id == u64::MAX || id == 0 {
                continue;
            }
            let gain = normalized_volume(s.base, self.category_volume(s.category));
            if gain <= 0.0 {
                mixer.stop(id);
            } else {
                mixer.set_gain(id, stereo_gains(self.listener, s.pos, gain, s.fade));
            }
        }
    }

    pub fn stop_stream(&mut self, key: &str) {
        if let Some(s) = self.streams.remove(key) {
            let id = s.voice.swap(0, std::sync::atomic::Ordering::Relaxed);
            if id != u64::MAX && id != 0 {
                self.mixer.stop(id);
            }
        }
    }

    pub fn stop_all(&mut self) {
        // Mark pending decodes as cancelled so a late stream cannot start after the mix is cleared.
        for s in self.streams.values() {
            s.voice.swap(0, std::sync::atomic::Ordering::Relaxed);
        }
        self.mixer.stop_all();
        self.streams.clear();
        self.playing.clear();
    }
}

/// `SoundEventAccessorComposite.getWeight` for an event name: nested event entries count the
/// nested event's whole pool (`ISoundEventAccessor.getWeight` delegates to the composite).
fn event_weight(events: &HashMap<String, Event>, event: &str, depth: u32) -> u32 {
    let Some(ev) = events.get(event) else { return 0 };
    if depth > 8 {
        return 0;
    }
    ev.entries.iter().map(|e| if e.event { event_weight(events, &e.name, depth + 1) } else { e.weight }).sum()
}

/// The asset index's `sounds.json` first, then each pack's on top (`SoundHandler`).
fn load_events(base: &Reader, packs: &[Vec<u8>]) -> HashMap<String, Event> {
    let mut events = HashMap::new();
    if let Some(json) = base("minecraft/sounds.json").and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        merge_events(&json, &mut events);
    }
    for p in packs {
        match serde_json::from_slice::<Value>(p) {
            Ok(json) => merge_events(&json, &mut events),
            Err(e) => eprintln!("Invalid sounds.json: {e}"),
        }
    }
    events
}

/// `SoundHandler.loadSoundResource`: an event seen before gets the new entries appended
/// unless the list says `"replace": true`; the existing category is kept when appending.
fn merge_events(json: &Value, out: &mut HashMap<String, Event>) {
    let Some(obj) = json.as_object() else { return };
    for (name, v) in obj {
        let category = Category::by_name(v["category"].as_str().unwrap_or("master"));
        let replace = v["replace"].as_bool().unwrap_or(false);
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
        match out.get_mut(name) {
            Some(ev) if !replace => ev.entries.extend(entries),
            _ => {
                out.insert(name.clone(), Event { category, entries });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(docs: &[&str]) -> HashMap<String, Event> {
        let mut m = HashMap::new();
        for d in docs {
            merge_events(&serde_json::from_str(d).unwrap(), &mut m);
        }
        m
    }

    fn names(e: &Event) -> Vec<&str> {
        e.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn malformed_asset_hashes_are_ignored() {
        let dir = std::env::temp_dir().join(format!("rustmc-sound-index-{}", std::process::id()));
        let dir = if std::fs::create_dir_all(dir.join("indexes")).is_ok() {
            dir
        } else {
            // Sandboxes may deny the platform temp directory; keep the scratch data in target/.
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp").join(format!("rustmc-sound-index-{}", std::process::id()));
            std::fs::create_dir_all(dir.join("indexes")).unwrap();
            dir
        };
        std::fs::write(dir.join("indexes/1.8.json"), br#"{"objects":{"short":{"hash":"a"},"invalid":{"hash":"../not-a-hash"}}}"#).unwrap();
        let read = index_reader(&dir, "1.8").unwrap();
        assert!(read("short").is_none());
        assert!(read("invalid").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn packs_append_unless_replace() {
        let base = r#"{"x.hit":{"category":"block","sounds":["a","b"]},"y":{"category":"music","sounds":["m"]}}"#;
        let m = events(&[base, r#"{"x.hit":{"category":"hostile","sounds":["c"]},"z":{"sounds":["n"]}}"#]);
        assert_eq!(names(&m["x.hit"]), ["a", "b", "c"]);
        assert_eq!(m["x.hit"].category, Category::Blocks, "appending keeps the first category");
        assert_eq!(names(&m["z"]), ["n"]);
        let m = events(&[base, r#"{"x.hit":{"replace":true,"category":"hostile","sounds":["c"]}}"#]);
        assert_eq!(names(&m["x.hit"]), ["c"]);
        assert_eq!(m["x.hit"].category, Category::Mobs);
        assert_eq!(names(&m["y"]), ["m"]);
    }

    #[test]
    fn set_packs_reads_overlay_first() {
        let base: Reader = Arc::new(|p| match p {
            "minecraft/sounds.json" => Some(br#"{"e":{"category":"block","sounds":["f"]}}"#.to_vec()),
            "minecraft/sounds/f.ogg" => Some(b"base".to_vec()),
            _ => None,
        });
        let mut m = SoundManager::new(base);
        assert_eq!(m.event_count(), 1);
        let overlay: Reader = Arc::new(|p| (p == "minecraft/sounds/f.ogg").then(|| b"pack".to_vec()));
        m.set_packs(overlay, &[br#"{"e":{"replace":true,"sounds":["g"]}}"#.to_vec()]);
        assert_eq!((m.read)("minecraft/sounds/f.ogg").unwrap(), b"pack");
        assert_eq!((m.read)("minecraft/sounds.json").is_some(), true);
        assert_eq!(names(&m.events["e"]), ["g"]);
    }

    #[test]
    fn normalized_volume_clamps_before_category() {
        // `PositionedSoundRecord.create(location, x, y, z)` uses volume 4 for records.
        assert_eq!(normalized_volume(4.0, 0.4), 0.4);
        assert_eq!(normalized_volume(0.5, 0.5), 0.25);
        assert_eq!(normalized_volume(-1.0, 1.0), 0.0);
        assert_eq!(normalized_volume(2.0, 1.0), 1.0);
    }

    #[test]
    fn normalized_pitch_clamps_to_half_and_two() {
        assert_eq!(normalized_pitch(0.1), 0.5);
        assert_eq!(normalized_pitch(0.5), 0.5);
        assert_eq!(normalized_pitch(1.5), 1.5);
        assert_eq!(normalized_pitch(2.0), 2.0);
        assert_eq!(normalized_pitch(3.0), 2.0);
    }

    #[test]
    fn fade_distance_matches_sound_manager() {
        assert_eq!(fade_distance(0.5), 16.0);
        assert_eq!(fade_distance(1.0), 16.0);
        assert_eq!(fade_distance(4.0), 64.0);
        assert_eq!(fade_distance(10.0), 160.0);
    }

    #[test]
    fn linear_attenuation_matches_paulscode() {
        assert_eq!(attenuation(0.0, 16.0), 1.0);
        assert_eq!(attenuation(-1.0, 16.0), 1.0);
        assert_eq!(attenuation(8.0, 16.0), 0.5);
        assert_eq!(attenuation(16.0, 16.0), 0.0);
        assert_eq!(attenuation(32.0, 16.0), 0.0);
    }

    #[test]
    fn stereo_panning_uses_listener_yaw() {
        // At yaw 0 the listener's right vector is -X.
        let listener = ([0.0, 0.0, 0.0], 0.0);
        let (l, r) = stereo_gains(listener, Some([-10.0, 0.0, 0.0]), 1.0, 16.0);
        assert!(l.abs() < 1e-5, "right source is silent on the left: {l}");
        assert!((r - 0.375 * std::f32::consts::SQRT_2).abs() < 1e-5, "{r}");
        let (l, r) = stereo_gains(listener, Some([10.0, 0.0, 0.0]), 1.0, 16.0);
        assert!((l - 0.375 * std::f32::consts::SQRT_2).abs() < 1e-5, "{l}");
        assert!(r.abs() < 1e-5, "left source is silent on the right: {r}");
        // Straight ahead: equal power on both sides.
        let (l, r) = stereo_gains(listener, Some([0.0, 0.0, 10.0]), 1.0, 16.0);
        assert!((l - 0.375).abs() < 1e-5 && (r - 0.375).abs() < 1e-5, "{l} {r}");
        // Non-positional sounds keep the plain gain.
        assert_eq!(stereo_gains(listener, None, 0.25, 16.0), (0.25, 0.25));
    }

    #[test]
    fn nested_event_entries_weigh_the_nested_event() {
        let m = events(&[r#"{"inner":{"sounds":["a","b","c"]},"outer":{"sounds":[{"name":"inner","type":"event"},"d"]}}"#]);
        assert_eq!(event_weight(&m, "inner", 0), 3);
        assert_eq!(event_weight(&m, "outer", 0), 4, "1 event entry counts inner's 3");
    }

    #[test]
    fn nested_event_entry_ignores_outer_volume_and_pitch() {
        let mut m = SoundManager::new(Arc::new(|_: &str| None));
        m.events = events(&[
            r#"{"inner":{"sounds":[{"name":"f","volume":0.5,"pitch":2.0}]},"outer":{"sounds":[{"name":"inner","type":"event","volume":0.25,"pitch":0.5}]}}"#,
        ]);
        let (entry, cat) = m.pick("outer", 0).unwrap();
        assert_eq!(entry.name, "f");
        assert_eq!((entry.volume, entry.pitch), (0.5, 2.0));
        assert_eq!(cat, Category::Master);
    }
}

/// Reads files from `.minecraft/assets` through an asset index (`indexes/<id>.json`).
pub fn index_reader(assets_dir: &std::path::Path, index: &str) -> Option<impl Fn(&str) -> Option<Vec<u8>> + Send + Sync> {
    let idx: Value = serde_json::from_slice(&std::fs::read(assets_dir.join("indexes").join(format!("{index}.json"))).ok()?).ok()?;
    let map: HashMap<String, String> = idx["objects"]
        .as_object()?
        .iter()
        .filter_map(|(k, v)| {
            let hash = v["hash"].as_str()?;
            (hash.len() == 40 && hash.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
                .then(|| (k.clone(), hash.to_owned()))
        })
        .collect();
    let dir = assets_dir.join("objects");
    Some(move |path: &str| {
        let h = map.get(path)?;
        std::fs::read(dir.join(&h[..2]).join(h)).ok()
    })
}
