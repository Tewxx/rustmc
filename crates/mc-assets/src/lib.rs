//! Resource loading for 1.8.9: layered packs, blockstates, block models, texture atlas.
//!
//! Nothing from Mojang is bundled. The base layer is the user's own `1.8.9.jar`.

pub mod atlas;
pub mod blockstate;
pub mod colormap;
pub mod model;
pub mod repository;

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

enum Layer {
    Zip(Mutex<zip::ZipArchive<BufReader<File>>>),
    Dir(PathBuf),
}

static DYNAMIC: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());

/// Registers an in-memory file under `path` (a `DynamicTexture`'s stand-in, e.g. the server list
/// icons). `AssetSource::read` serves `servers/...` paths from here ahead of every pack layer.
pub fn register_dynamic(path: &str, bytes: Vec<u8>) {
    let mut d = DYNAMIC.lock().unwrap();
    match d.iter_mut().find(|(p, _)| p == path) {
        Some(e) => e.1 = bytes,
        None => d.push((path.to_string(), bytes)),
    }
}

/// Forge mod jars: `domain:path` textures (`lunarforge:ui/icons/x.png`) come from `assets/<domain>/<path>` in these,
/// behind every resource pack layer.
static MOD_JARS: Mutex<Vec<Layer>> = Mutex::new(Vec::new());

/// Makes a mod jar's `assets/` folder readable through `AssetSource::read` with a `domain:` prefix.
pub fn register_mod_jar(path: &Path) -> std::io::Result<()> {
    let zip = zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    MOD_JARS.lock().unwrap().push(Layer::Zip(Mutex::new(zip)));
    Ok(())
}

/// Ordered resource pack stack. Later layers override earlier ones, like the
/// resource pack screen's top-of-list-wins order.
#[derive(Default)]
pub struct AssetSource {
    layers: Vec<Layer>,
}

impl AssetSource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a zip/jar or a folder pack on top of the stack.
    pub fn push(&mut self, path: &Path) -> std::io::Result<()> {
        let layer = if path.is_dir() {
            Layer::Dir(path.to_path_buf())
        } else {
            let zip = zip::ZipArchive::new(BufReader::new(File::open(path)?))
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Layer::Zip(Mutex::new(zip))
        };
        self.layers.push(layer);
        Ok(())
    }

    /// Reads `assets/minecraft/<path>` from the highest layer that has it.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        if path.starts_with("servers/") {
            if let Some((_, b)) = DYNAMIC.lock().unwrap().iter().find(|(p, _)| p == path) {
                return Some(b.clone());
            }
        }
        match path.split_once(':') {
            Some((domain, rest)) if domain != "minecraft" => {
                let full = format!("assets/{domain}/{rest}");
                self.read_root(&full).or_else(|| MOD_JARS.lock().unwrap().iter().rev().find_map(|l| read_layer(l, &full)))
            }
            _ => self.read_root(&format!("assets/minecraft/{path}")),
        }
    }

    /// Reads a file by its full path inside the layers (`pack.png`, `pack.mcmeta`).
    pub fn read_root(&self, full: &str) -> Option<Vec<u8>> {
        self.layers.iter().rev().find_map(|l| read_layer(l, full))
    }

    /// A root file from every layer above the default pack, highest first.
    pub fn read_over_base(&self, full: &str) -> Option<Vec<u8>> {
        self.layers.iter().skip(1).rev().find_map(|l| read_layer(l, full))
    }

    /// A root file of the lowest layer only (the default pack's `pack.png`).
    pub fn read_base(&self, full: &str) -> Option<Vec<u8>> {
        self.layers.first().and_then(|l| read_layer(l, full))
    }

    /// `IResourceManager.getAllResources`: every layer's copy of `assets/minecraft/<path>`,
    /// lowest layer first.
    pub fn read_all(&self, path: &str) -> Vec<Vec<u8>> {
        let full = format!("assets/minecraft/{path}");
        self.layers.iter().filter_map(|l| read_layer(l, &full)).collect()
    }

    /// Number of layers (jar included).
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn read_json(&self, path: &str) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.read(path)?).ok()
    }
}

fn read_layer(l: &Layer, full: &str) -> Option<Vec<u8>> {
    match l {
        Layer::Dir(d) => std::fs::read(d.join(full)).ok(),
        Layer::Zip(z) => {
            let mut z = z.lock().unwrap();
            let mut f = z.by_name(full).ok()?;
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf).ok()?;
            Some(buf)
        }
    }
}
