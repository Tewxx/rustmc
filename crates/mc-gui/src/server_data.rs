//! `ServerData` and `ServerList`: the saved server list and its `servers.dat` file.
//!
//! `servers.dat` is uncompressed NBT, `{servers: [{name, ip, icon?, acceptTextures?}]}`, the
//! file vanilla and Prism share.

use mc_core::nbt::{self, Compound, Tag};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// `ServerData.ServerResourceMode`, in ordinal order (the Edit screen cycles through it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceMode {
    Enabled,
    Disabled,
    Prompt,
}

impl ResourceMode {
    pub fn next(self) -> Self {
        match self {
            Self::Enabled => Self::Disabled,
            Self::Disabled => Self::Prompt,
            Self::Prompt => Self::Enabled,
        }
    }

    /// The `addServer.resourcePack.<name>` language key.
    pub fn lang_key(self) -> &'static str {
        match self {
            Self::Enabled => "addServer.resourcePack.enabled",
            Self::Disabled => "addServer.resourcePack.disabled",
            Self::Prompt => "addServer.resourcePack.prompt",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ServerData {
    pub name: String,
    pub ip: String,
    /// `populationInfo`: `online/max` with colour codes.
    pub population_info: String,
    /// `serverMOTD`: formatted text (`getFormattedText`).
    pub motd: String,
    /// `pingToServer`: ms, -1 no connection, -2 not pinged yet.
    pub ping: i64,
    pub version: i32,
    pub game_version: String,
    /// `field_78841_f`: a ping has been started.
    pub pinged: bool,
    /// `playerList`: tooltip text, one name per line.
    pub player_list: Option<String>,
    pub resource_mode: ResourceMode,
    /// Base64 PNG, no `data:` prefix.
    pub icon: Option<String>,
    pub lan: bool,
}

pub type Handle = Arc<Mutex<ServerData>>;

impl ServerData {
    pub fn new(name: &str, ip: &str, lan: bool) -> Self {
        Self {
            name: name.into(),
            ip: ip.into(),
            population_info: String::new(),
            motd: String::new(),
            ping: 0,
            version: 47,
            game_version: "1.8.9".into(),
            pinged: false,
            player_list: None,
            resource_mode: ResourceMode::Prompt,
            icon: None,
            lan,
        }
    }

    /// `getNBTCompound`
    pub fn to_nbt(&self) -> Compound {
        let mut c = Compound::new();
        c.insert("name".into(), Tag::String(self.name.clone()));
        c.insert("ip".into(), Tag::String(self.ip.clone()));
        if let Some(icon) = &self.icon {
            c.insert("icon".into(), Tag::String(icon.clone()));
        }
        match self.resource_mode {
            ResourceMode::Enabled => {
                c.insert("acceptTextures".into(), Tag::Byte(1));
            }
            ResourceMode::Disabled => {
                c.insert("acceptTextures".into(), Tag::Byte(0));
            }
            ResourceMode::Prompt => {}
        }
        c
    }

    /// `getServerDataFromNBTCompound`
    pub fn from_nbt(c: &Compound) -> Self {
        let s = |k: &str| c.get(k).and_then(Tag::as_str).unwrap_or("").to_string();
        let mut d = Self::new(&s("name"), &s("ip"), false);
        if let Some(Tag::String(icon)) = c.get("icon") {
            d.icon = Some(icon.clone());
        }
        d.resource_mode = match c.get("acceptTextures") {
            Some(Tag::Byte(0)) => ResourceMode::Disabled,
            Some(Tag::Byte(_)) => ResourceMode::Enabled,
            _ => ResourceMode::Prompt,
        };
        d
    }

    /// `copyFrom`
    pub fn copy_from(&mut self, o: &ServerData) {
        self.ip = o.ip.clone();
        self.name = o.name.clone();
        self.resource_mode = o.resource_mode;
        self.icon = o.icon.clone();
        self.lan = o.lan;
    }
}

/// `ServerList`
pub struct ServerList {
    path: PathBuf,
    pub servers: Vec<Handle>,
}

impl ServerList {
    /// Loads `servers.dat` from `dir` (an unreadable or missing file gives an empty list).
    pub fn load(dir: &Path) -> Self {
        let mut l = Self { path: dir.join("servers.dat"), servers: Vec::new() };
        l.reload();
        l
    }

    /// `loadServerList`
    pub fn reload(&mut self) {
        self.servers.clear();
        let Ok(bytes) = std::fs::read(&self.path) else { return };
        let Ok((_, Tag::Compound(root))) = nbt::read(&bytes) else {
            eprintln!("Couldn't load server list");
            return;
        };
        if let Some(Tag::List(_, list)) = root.get("servers") {
            for t in list {
                if let Tag::Compound(c) = t {
                    self.servers.push(Arc::new(Mutex::new(ServerData::from_nbt(c))));
                }
            }
        }
    }

    /// `saveServerList` / `CompressedStreamTools.safeWrite`: write a temp file, then swap it in.
    pub fn save(&self) {
        let list: Vec<Tag> = self.servers.iter().map(|s| Tag::Compound(s.lock().unwrap().to_nbt())).collect();
        let mut root = Compound::new();
        root.insert("servers".into(), Tag::List(10, list));
        let bytes = nbt::write("", &Tag::Compound(root));
        let tmp = self.path.with_file_name("servers.dat_tmp");
        let old = self.path.with_file_name("servers.dat_old");
        let result = (|| -> std::io::Result<()> {
            std::fs::write(&tmp, &bytes)?;
            if old.exists() {
                std::fs::remove_file(&old)?;
            }
            if self.path.exists() {
                std::fs::rename(&self.path, &old)?;
            }
            std::fs::rename(&tmp, &self.path)?;
            if old.exists() {
                std::fs::remove_file(&old)?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("Couldn't save server list: {e}");
        }
    }

    pub fn count(&self) -> usize {
        self.servers.len()
    }

    pub fn get(&self, i: usize) -> Handle {
        self.servers[i].clone()
    }

    /// `swapServers` (also saves).
    pub fn swap(&mut self, a: usize, b: usize) {
        self.servers.swap(a, b);
        self.save();
    }
}

/// Netty `Base64.decode` as the server icon code uses it: characters outside the alphabet
/// (line breaks) are skipped, `=` ends the data.
pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => continue,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// The server icon is a `DynamicTexture` in vanilla; here a PNG registered with the asset
/// source under a path that is unique per icon. `None` when it is not a 64x64 PNG.
pub fn register_icon(ip: &str, base64: &str) -> Option<String> {
    let bytes = base64_decode(base64);
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
    if img.width() != 64 || img.height() != 64 {
        return None;
    }
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    let path = format!("servers/{ip}/icon/{hash:016x}");
    mc_assets::register_dynamic(&path, bytes);
    Some(path)
}

/// The five-level ping icon row of `ServerListEntryNormal` (offset into `icons.png` in units
/// of 8 texels below y 176): 0 best, 4 worst, 5 no connection.
pub fn ping_level(ping: i64) -> i32 {
    match ping {
        p if p < 0 => 5,
        p if p < 150 => 0,
        p if p < 300 => 1,
        p if p < 600 => 2,
        p if p < 1000 => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rustmc-servers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn servers_dat_round_trips() {
        let d = dir("rt");
        let mut l = ServerList::load(&d);
        assert_eq!(l.count(), 0);
        let mut a = ServerData::new("Alpha", "a.example:25566", false);
        a.icon = Some("iVBORw0KGgo=".into());
        a.resource_mode = ResourceMode::Enabled;
        let mut b = ServerData::new("Beta", "b.example", false);
        b.resource_mode = ResourceMode::Disabled;
        let c = ServerData::new("Gamma", "c.example", false);
        for s in [a, b, c] {
            l.servers.push(Arc::new(Mutex::new(s)));
        }
        l.save();
        assert!(!d.join("servers.dat_tmp").exists() && !d.join("servers.dat_old").exists());
        // uncompressed NBT: a TAG_Compound with the empty root name
        let raw = std::fs::read(d.join("servers.dat")).unwrap();
        assert_eq!(&raw[..3], &[10, 0, 0]);
        let l2 = ServerList::load(&d);
        assert_eq!(l2.count(), 3);
        let (a, b, c) = (l2.get(0), l2.get(1), l2.get(2));
        let (a, b, c) = (a.lock().unwrap(), b.lock().unwrap(), c.lock().unwrap());
        assert_eq!((a.name.as_str(), a.ip.as_str(), a.icon.as_deref(), a.resource_mode), ("Alpha", "a.example:25566", Some("iVBORw0KGgo="), ResourceMode::Enabled));
        assert_eq!((b.name.as_str(), b.resource_mode, b.icon.clone()), ("Beta", ResourceMode::Disabled, None));
        assert_eq!(c.resource_mode, ResourceMode::Prompt);
        let nbt = c.to_nbt();
        assert!(!nbt.contains_key("acceptTextures") && !nbt.contains_key("icon"));
    }

    #[test]
    fn swap_saves_and_reorders() {
        let d = dir("swap");
        let mut l = ServerList::load(&d);
        for n in ["one", "two"] {
            l.servers.push(Arc::new(Mutex::new(ServerData::new(n, n, false))));
        }
        l.swap(0, 1);
        let l2 = ServerList::load(&d);
        assert_eq!(l2.get(0).lock().unwrap().name, "two");
    }

    #[test]
    fn accept_textures_maps_to_resource_mode() {
        let mut c = Compound::new();
        c.insert("name".into(), Tag::String("n".into()));
        c.insert("ip".into(), Tag::String("i".into()));
        assert_eq!(ServerData::from_nbt(&c).resource_mode, ResourceMode::Prompt);
        c.insert("acceptTextures".into(), Tag::Byte(1));
        assert_eq!(ServerData::from_nbt(&c).resource_mode, ResourceMode::Enabled);
        c.insert("acceptTextures".into(), Tag::Byte(0));
        assert_eq!(ServerData::from_nbt(&c).resource_mode, ResourceMode::Disabled);
        assert_eq!(ResourceMode::Prompt.next(), ResourceMode::Enabled);
    }

    #[test]
    fn ping_bars_use_vanilla_thresholds() {
        let cases = [(-1, 5), (0, 0), (149, 0), (150, 1), (299, 1), (300, 2), (599, 2), (600, 3), (999, 3), (1000, 4), (60000, 4)];
        for (ms, level) in cases {
            assert_eq!(ping_level(ms), level, "{ms} ms");
        }
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_decode("Zm9vYmFy"), b"foobar");
        assert_eq!(base64_decode("Zm9vYg=="), b"foob");
        assert_eq!(base64_decode("Zm9v\nYmFy"), b"foobar");
    }

    #[test]
    fn icons_must_be_64_by_64() {
        let enc = |w: u32, h: u32| {
            let mut png = Vec::new();
            image::RgbaImage::new(w, h).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
            let t = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for ch in png.chunks(3) {
                let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
                for i in 0..4 {
                    if i <= ch.len() {
                        s.push(t[(n >> (18 - 6 * i) & 63) as usize] as char);
                    }
                }
            }
            s
        };
        assert!(register_icon("x", &enc(64, 64)).is_some());
        assert!(register_icon("x", &enc(32, 64)).is_none());
        assert!(register_icon("x", "not a png").is_none());
    }
}
