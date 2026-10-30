//! Fetch the official 1.8 asset index and content-addressed objects after sign-in.
use std::{collections::BTreeMap, fs, io::{Read, Write}, path::{Path, PathBuf}, sync::{atomic::{AtomicUsize, Ordering}, Mutex}};
use serde_json::Value;

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const INDEX_SHA1: &str = "f6ad102bcaa53b1a58358f16e376d548d44933ec";
const OBJECTS: &str = "https://resources.download.minecraft.net";
const MAX_METADATA: usize = 16 * 1024 * 1024;
const MAX_OBJECT: usize = 64 * 1024 * 1024;
static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

type Result<T> = std::result::Result<T, String>;

fn get(url: &str, limit: usize) -> Result<Vec<u8>> {
    let allowed = ["https://piston-meta.mojang.com/", "https://launchermeta.mojang.com/", "https://resources.download.minecraft.net/"];
    if !allowed.iter().any(|prefix| url.starts_with(prefix)) { return Err("untrusted asset URL".into()); }
    let response = ureq::get(url).timeout(std::time::Duration::from_secs(30)).call().map_err(|e| format!("download failed: {e}"))?;
    let mut bytes = Vec::new();
    response.into_reader().take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > limit { return Err("download exceeds size limit".into()); }
    Ok(bytes)
}

fn json(bytes: &[u8]) -> Result<Value> { serde_json::from_slice(bytes).map_err(|e| format!("invalid asset metadata: {e}")) }
fn hash(value: &str) -> Result<&str> {
    if value.len() == 40 && value.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) { Ok(value) }
    else { Err("invalid asset SHA-1".into()) }
}
fn verified(bytes: &[u8], expected: &str, size: Option<u64>) -> bool {
    size.is_none_or(|n| n == bytes.len() as u64) && crate::pack::sha1_hex(bytes) == expected
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("invalid asset path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".rustmc-{}-{}.tmp", std::process::id(), TEMP_ID.fetch_add(1, Ordering::Relaxed)));
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(temp);
    result
}

/// Syncs into the normal Minecraft asset layout. The fetch function takes a URL and byte limit.
fn sync_with<F>(dir: &Path, fetch: &F) -> Result<()>
where F: Fn(&str, usize) -> Result<Vec<u8>> + Sync {
    let manifest = json(&fetch(MANIFEST, MAX_METADATA)?)?;
    let version = manifest["versions"].as_array().and_then(|v| v.iter().find(|v| v["id"] == "1.8.9")).ok_or("1.8.9 missing from version manifest")?;
    let version_url = version["url"].as_str().ok_or("missing version URL")?;
    let version_hash = hash(version["sha1"].as_str().ok_or("missing version hash")?)?;
    let version_bytes = fetch(version_url, MAX_METADATA)?;
    if !verified(&version_bytes, version_hash, None) { return Err("version metadata checksum mismatch".into()); }
    let version = json(&version_bytes)?;
    let index = &version["assetIndex"];
    if index["id"] != "1.8" { return Err("unexpected asset index".into()); }
    let index_hash = hash(index["sha1"].as_str().ok_or("missing index hash")?)?;
    let index_size = index["size"].as_u64().ok_or("missing index size")?;
    if index_size > MAX_METADATA as u64 { return Err("asset index too large".into()); }
    let index_bytes = fetch(index["url"].as_str().ok_or("missing index URL")?, MAX_METADATA)?;
    if !verified(&index_bytes, index_hash, Some(index_size)) { return Err("asset index checksum mismatch".into()); }
    let data = json(&index_bytes)?;
    let entries = data["objects"].as_object().ok_or("missing asset objects")?;
    if entries.len() > 100_000 { return Err("too many asset objects".into()); }
    let mut objects = BTreeMap::new();
    for value in entries.values() {
        let h = hash(value["hash"].as_str().ok_or("missing asset hash")?)?.to_owned();
        let size = value["size"].as_u64().ok_or("missing asset size")?;
        if size > MAX_OBJECT as u64 { return Err("asset object too large".into()); }
        if objects.insert(h.clone(), size).is_some_and(|prior| prior != size) { return Err("conflicting asset sizes".into()); }
    }
    let tasks = Mutex::new(objects.into_iter().collect::<Vec<_>>());
    let errors = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..6 {
            scope.spawn(|| loop {
                let Some((h, size)) = tasks.lock().unwrap().pop() else { break };
                let path = dir.join("objects").join(&h[..2]).join(&h);
                if fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == size)
                    && fs::read(&path).is_ok_and(|b| verified(&b, &h, Some(size))) { continue; }
                let url = format!("{OBJECTS}/{}/{}", &h[..2], h);
                let result = fetch(&url, MAX_OBJECT).and_then(|bytes| {
                    if !verified(&bytes, &h, Some(size)) { return Err("asset checksum mismatch".into()); }
                    atomic_write(&path, &bytes)
                });
                if let Err(e) = result { errors.lock().unwrap().push(e); }
            });
        }
    });
    if let Some(error) = errors.into_inner().unwrap().into_iter().next() { return Err(error); }
    atomic_write(&dir.join("indexes/1.8.json"), &index_bytes)
}

fn cached_complete(dir: &Path) -> bool {
    let Ok(bytes) = fs::read(dir.join("indexes/1.8.json")) else { return false };
    if bytes.len() > MAX_METADATA || !verified(&bytes, INDEX_SHA1, None) { return false; }
    let Ok(index) = json(&bytes) else { return false };
    let Some(objects) = index["objects"].as_object() else { return false };
    if objects.len() > 100_000 { return false; }
    objects.values().all(|value| {
        let Some(h) = value["hash"].as_str().and_then(|h| hash(h).ok()) else { return false };
        let Some(size) = value["size"].as_u64().filter(|n| *n <= MAX_OBJECT as u64) else { return false };
        let path = dir.join("objects").join(&h[..2]).join(h);
        fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == size)
            && fs::read(path).is_ok_and(|b| verified(&b, h, Some(size)))
    })
}

pub(crate) fn start(dir: PathBuf, tx: std::sync::mpsc::Sender<Result<PathBuf>>) {
    let error_tx = tx.clone();
    if let Err(e) = std::thread::Builder::new().name("asset-sync".into()).spawn(move || {
        let result = if cached_complete(&dir) { Ok(()) } else { sync_with(&dir, &get) }.map(|()| dir);
        let _ = tx.send(result);
    }) { let _ = error_tx.send(Err(format!("could not start asset sync: {e}"))); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn sync_repairs_corruption_and_rejects_bad_hashes_without_publishing_index() {
        let dir = std::env::temp_dir().join(format!("rustmc-assets-test-{}", std::process::id()));
        let bytes = b"test asset".to_vec();
        let digest = crate::pack::sha1_hex(&bytes);
        let index = format!(r#"{{"objects":{{"minecraft/sounds/test.ogg":{{"hash":"{digest}","size":{}}}}}}}"#, bytes.len()).into_bytes();
        let index_hash = crate::pack::sha1_hex(&index);
        let version = format!(r#"{{"assetIndex":{{"id":"1.8","sha1":"{index_hash}","size":{},"url":"index"}}}}"#, index.len()).into_bytes();
        let version_hash = crate::pack::sha1_hex(&version);
        let manifest = format!(r#"{{"versions":[{{"id":"1.8.9","url":"version","sha1":"{version_hash}"}}]}}"#).into_bytes();
        let downloads = AtomicUsize::new(0);
        let fetch = |url: &str, _: usize| -> Result<Vec<u8>> {
            match url {
                MANIFEST => Ok(manifest.clone()), "version" => Ok(version.clone()), "index" => Ok(index.clone()),
                _ if url.ends_with(&digest) => { downloads.fetch_add(1, Ordering::Relaxed); Ok(bytes.clone()) },
                _ => Err("unexpected URL".into()),
            }
        };
        sync_with(&dir, &fetch).unwrap();
        let path = dir.join("objects").join(&digest[..2]).join(&digest);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        sync_with(&dir, &fetch).unwrap();
        assert_eq!(downloads.load(Ordering::Relaxed), 1, "verified cached object must not be fetched");
        fs::write(&path, b"bad asset!").unwrap();
        sync_with(&dir, &fetch).unwrap();
        assert_eq!(downloads.load(Ordering::Relaxed), 2);
        assert_eq!(fs::read(path).unwrap(), bytes);
        let old_index = fs::read(dir.join("indexes/1.8.json")).unwrap();
        let bad = |url: &str, limit| if url == "index" { Ok(b"broken".to_vec()) } else { fetch(url, limit) };
        assert!(sync_with(&dir, &bad).is_err());
        assert_eq!(fs::read(dir.join("indexes/1.8.json")).unwrap(), old_index);
        assert!(hash("../bad").is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}

impl crate::App {
    pub(crate) fn poll_asset_download(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some(rx) = &self.asset_download else { return };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => self.asset_download = None,
            Ok(result) => {
                self.asset_download = None;
                match result {
                    Ok(dir) => {
                        self.auto_assets_dir = None;
                        if self.sound.is_none() && std::env::var_os("RUSTMC_NO_SOUND").is_none() {
                            if let Some(read) = mc_sound::manager::index_reader(&dir, "1.8") {
                                let mut sound = mc_sound::SoundManager::new(std::sync::Arc::new(read));
                                let server = self.packs.applied.as_ref().map(|(_, p)| p.as_path());
                                if let Ok(packs) = mc_assets::repository::build_stack(&self.jar, &self.base_packs, &self.selected_pack_paths(), server) {
                                    let packs = std::sync::Arc::new(packs);
                                    let json = packs.read_all("sounds.json");
                                    let overlay: mc_sound::manager::Reader = std::sync::Arc::new(move |p: &str| p.strip_prefix("minecraft/").and_then(|r| packs.read(r)));
                                    sound.set_packs(overlay, &json);
                                }
                                self.sound = Some(sound);
                            }
                        }
                        println!("Minecraft assets are ready");
                    }
                    Err(e) => eprintln!("Minecraft asset download failed: {e}"),
                }
            }
        }
    }
}
