//! Server resource packs (`S48PacketResourcePackSend`): per-server mode, download with SHA-1
//! verification, the on-disk cache, and the prompt state machine from
//! `NetHandlerPlayClient.handleResourcePack` / `GuiYesNo`.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// `ServerData.ServerResourceMode`: what to do with a server's pack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Show `GuiYesNo` and remember the answer.
    #[default]
    Prompt,
    /// Accept and download without asking.
    Enabled,
    /// Decline without asking.
    Disabled,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Prompt => "prompt",
            Mode::Enabled => "enabled",
            Mode::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Mode {
        match s {
            "enabled" => Mode::Enabled,
            "disabled" => Mode::Disabled,
            _ => Mode::Prompt,
        }
    }
}

/// One `S48PacketResourcePackSend`.
#[derive(Clone, Debug)]
pub struct Request {
    pub url: String,
    pub hash: String,
}

/// A finished download.
pub type Download = Result<PathBuf, String>;

pub struct ResourcePacks {
    pub mode: Mode,
    /// The request waiting for the `GuiYesNo` answer.
    pub pending: Option<Request>,
    /// `(hash, receiver)` for the download in flight.
    download: Option<(String, Receiver<Download>)>,
    /// The pack currently applied: `(hash, path)`.
    pub applied: Option<(String, PathBuf)>,
}

impl Default for ResourcePacks {
    fn default() -> Self {
        Self { mode: Mode::Prompt, pending: None, download: None, applied: None }
    }
}

impl ResourcePacks {
    pub fn new(mode: Mode) -> Self {
        Self { mode, ..Default::default() }
    }

    /// `ResourcePackRepository.downloadResourcePack`: fetches the URL into the cache and checks
    /// the SHA-1 when the server sent one (vanilla deletes mismatching files).
    pub fn start_download(&mut self, req: &Request, dir: PathBuf) -> Result<(), String> {
        if self.download.is_some() {
            return Err("a resource pack download is already running".into());
        }
        let (url, hash) = (req.url.clone(), req.hash.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch(&url, &hash, &dir));
        });
        self.download = Some((req.hash.clone(), rx));
        Ok(())
    }

    /// Polls the download thread. `None` when nothing finished.
    pub fn poll(&mut self) -> Option<(String, Download)> {        let Some((hash, rx)) = &self.download else { return None };
        match rx.try_recv() {
            Ok(result) => {
                let hash = hash.clone();
                self.download = None;
                Some((hash, result))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                let hash = hash.clone();
                self.download = None;
                Some((hash, Err("resource pack download thread died".into())))
            }
        }
    }
}

/// `ResourcePackRepository.downloadResourcePack0`: HTTP GET, size guard, SHA-1 check, then a
/// cache file named after the hash (or the computed hash when the server sent none).
pub fn fetch(url: &str, hash: &str, dir: &Path) -> Download {
    if url.starts_with("level://") {
        return Err("level:// resource packs are resolved by the client, not fetched".into());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("unsupported resource pack URL: {url}"));
    }
    let response = ureq::get(url).call().map_err(|e| format!("download failed: {e}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read failed: {e}"))?;
    if bytes.is_empty() {
        return Err("download failed: empty response".into());
    }
    let computed = sha1_hex(&bytes);
    if !hash.is_empty() && !computed.eq_ignore_ascii_case(hash) {
        return Err(format!("hash mismatch: server {hash}, downloaded {computed}"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cache dir: {e}"))?;
    let name = if hash.is_empty() { computed.clone() } else { hash.to_ascii_lowercase() };
    let path = dir.join(format!("{name}.zip"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Write through a temporary name so a crashed download never looks like a cached pack.
    let tmp = path.with_extension("zip.part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write failed: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename failed: {e}"))?;
    Ok(path)
}

/// `minecraft/server-resource-packs`, kept next to `options.txt` (portable client layout) so
/// the cache lives in the same writable directory as the rest of the client's state.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RUSTMC_PACK_CACHE") {
        return PathBuf::from(dir);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("server-resource-packs")
}

/// `ResourcesUtil`-style cache lookup: a 40-digit hash names `<hash>.zip`; the file is only
/// reused when its contents still hash to that name (vanilla deletes mismatching cache files).
pub fn cached_pack(hash: &str) -> Option<PathBuf> {
    cached_pack_at(&cache_dir(), hash)
}

pub fn cached_pack_at(dir: &Path, hash: &str) -> Option<PathBuf> {
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let path = dir.join(format!("{}.zip", hash.to_ascii_lowercase()));
    let bytes = std::fs::read(&path).ok()?;
    (sha1_hex(&bytes).eq_ignore_ascii_case(hash)).then_some(path)
}

/// `level://<save>` → `<saves>/<save>` when it is a file (`handleResourcePack`'s local case).
pub fn level_pack_path(url: &str, saves: &Path) -> Option<PathBuf> {
    let name = url.strip_prefix("level://")?;
    if name.is_empty() || name.contains("..") {
        return None;
    }
    let path = saves.join(name);
    path.is_file().then_some(path)
}

/// SHA-1 (FIPS 180-1), for the `hash` field servers send.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut message = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn sha1_hex(data: &[u8]) -> String {
    sha1(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// One server's remembered resource-pack choice (`servers.dat`'s `resourceMode` plus the
/// cached pack path, which our client stores separately because the cache outlives the list).
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    pub mode: Mode,
    pub hash: String,
    pub path: String,
}

#[derive(Default)]
pub struct Store {
    pub servers: std::collections::HashMap<String, Stored>,
}

/// `rustmc/server-packs.json` next to the pack cache.
pub fn store_path() -> PathBuf {
    cache_dir().parent().map(|p| p.join("server-packs.json")).unwrap_or_else(|| PathBuf::from("server-packs.json"))
}

impl Store {
    pub fn load() -> Store {
        Self::load_from(&store_path())
    }

    pub fn load_from(path: &Path) -> Store {
        let mut store = Store::default();
        let Ok(bytes) = std::fs::read(path) else { return store };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return store };
        let Some(servers) = json.get("servers").and_then(|v| v.as_object()) else { return store };
        for (key, v) in servers {
            let Some(mode) = v.get("mode").and_then(|m| m.as_str()) else { continue };
            store.servers.insert(
                key.clone(),
                Stored {
                    mode: Mode::parse(mode),
                    hash: v.get("hash").and_then(|h| h.as_str()).unwrap_or_default().to_string(),
                    path: v.get("path").and_then(|p| p.as_str()).unwrap_or_default().to_string(),
                },
            );
        }
        store
    }

    pub fn save(&self) {
        self.save_to(&store_path());
    }

    pub fn save_to(&self, path: &Path) {
        let mut servers = serde_json::Map::new();
        for (key, s) in &self.servers {
            servers.insert(
                key.clone(),
                serde_json::json!({ "mode": s.mode.as_str(), "hash": s.hash, "path": s.path }),
            );
        }
        let doc = serde_json::json!({ "servers": servers });
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        if let Ok(text) = serde_json::to_vec_pretty(&doc) {
            let _ = std::fs::write(path, text);
        }
    }

    pub fn mode_for(&self, key: &str) -> Mode {
        self.servers.get(key).map_or(Mode::Prompt, |s| s.mode)
    }

    pub fn set(&mut self, key: &str, mode: Mode, hash: &str, path: Option<&Path>) {
        self.servers.insert(
            key.to_string(),
            Stored {
                mode,
                hash: hash.to_string(),
                path: path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_fips_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        assert_eq!(sha1_hex(&[0x61; 1000]), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
    }

    #[test]
    fn mode_round_trips_server_data_names() {
        for mode in [Mode::Prompt, Mode::Enabled, Mode::Disabled] {
            assert_eq!(Mode::parse(mode.as_str()), mode);
        }
        assert_eq!(Mode::parse("nonsense"), Mode::Prompt);
    }

    #[test]
    fn level_urls_stay_inside_the_saves_directory() {
        let dir = std::env::temp_dir().join("rustmc-level-pack-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("world.zip");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(level_pack_path("level://world.zip", &dir), Some(file));
        assert_eq!(level_pack_path("level://../escape", &dir), None);
        assert_eq!(level_pack_path("level://missing", &dir), None);
        assert_eq!(level_pack_path("http://example/x", &dir), None);
    }

    #[test]
    fn cached_pack_requires_the_matching_hash() {
        let dir = std::env::temp_dir().join("rustmc-pack-cache-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = b"pack bytes";
        let hash = sha1_hex(bytes);
        let path = dir.join(format!("{hash}.zip"));
        std::fs::write(&path, bytes).unwrap();
        assert!(cached_pack_at(&dir, &hash).is_some());
        assert!(cached_pack_at(&dir, &"0".repeat(40)).is_none());
        assert!(cached_pack_at(&dir, "not-a-hash").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn server_store_round_trips() {
        let path = std::env::temp_dir().join("rustmc-store-test").join("server-packs.json");
        let _ = std::fs::remove_file(&path);
        let mut store = Store::default();
        store.set("example.org:25565", Mode::Enabled, "abc", Some(Path::new("/tmp/abc.zip")));
        store.set("other:25565", Mode::Disabled, "def", None);
        store.save_to(&path);
        let back = Store::load_from(&path);
        assert_eq!(back.mode_for("example.org:25565"), Mode::Enabled);
        assert_eq!(back.mode_for("other:25565"), Mode::Disabled);
        assert_eq!(back.mode_for("unknown"), Mode::Prompt);
        assert_eq!(back.servers["example.org:25565"].hash, "abc");
        let _ = std::fs::remove_file(&path);
    }
}
