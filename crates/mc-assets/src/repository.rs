//! `ResourcePackRepository`: the `resourcepacks/` folder scan, `pack.mcmeta`, and the
//! asset stack (jar, selected packs bottom to top, server pack on top).

use crate::AssetSource;
use std::io::Read;
use std::path::{Path, PathBuf};

/// `PackMetadataSection.getPackFormat` of 1.8.9: the only format this client accepts.
pub const PACK_FORMAT: i32 = 1;

/// `ResourcePackRepository.Entry`.
#[derive(Clone, Debug)]
pub struct PackEntry {
    /// `getResourcePackName`: the file or folder name.
    pub name: String,
    pub path: PathBuf,
    /// `func_183027_f`; 0 when `pack.mcmeta` has no usable `pack` section.
    pub format: i32,
    /// The `description` chat component as JSON; `None` for an invalid `pack` section.
    pub description: Option<serde_json::Value>,
    /// `pack.png` bytes, if any.
    pub icon: Option<Vec<u8>>,
}

impl PackEntry {
    /// `func_183027_f() == 1`.
    pub fn compatible(&self) -> bool {
        self.format == PACK_FORMAT
    }
}

/// `PackMetadataSectionSerializer`: `(pack_format, description)` of the `pack` section.
/// `Err` is an unparsable file (the pack is dropped); `Ok(None)` a missing/invalid section.
#[allow(clippy::result_unit_err)]
pub fn parse_meta(bytes: &[u8]) -> Result<Option<(i32, serde_json::Value)>, ()> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    if !v.is_object() {
        return Err(());
    }
    let Some(p) = v.get("pack").and_then(|p| p.as_object()) else { return Ok(None) };
    let (Some(f), Some(d)) = (p.get("pack_format").and_then(|f| f.as_i64()), p.get("description")) else { return Ok(None) };
    Ok(Some((f as i32, d.clone())))
}

fn read_in_pack(path: &Path, name: &str) -> Option<Vec<u8>> {
    if path.is_dir() {
        return std::fs::read(path.join(name)).ok();
    }
    let mut z = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path).ok()?)).ok()?;
    let mut f = z.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// `Entry.updateResourcePack`; `None` when vanilla's exception drops the pack.
pub fn load_entry(path: &Path) -> Option<PackEntry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let meta = read_in_pack(path, "pack.mcmeta")?;
    let (format, description) = match parse_meta(&meta).ok()? {
        Some((f, d)) => (f, Some(d)),
        None => (0, None),
    };
    Some(PackEntry { name, path: path.to_path_buf(), format, description, icon: read_in_pack(path, "pack.png") })
}

/// `getResourcePackFiles` + `updateRepositoryEntriesAll`: `.zip` files and folders that
/// hold a `pack.mcmeta`. Sorted by name (vanilla leaves it to the file system).
pub fn scan(dir: &Path) -> Vec<PackEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            (p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".zip"))) || (p.is_dir() && p.join("pack.mcmeta").is_file())
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files.iter().filter_map(|p| load_entry(p)).collect()
}

/// `fixDirResourcepacks`
pub fn ensure_dir(dir: &Path) {
    if !dir.is_dir() {
        let _ = std::fs::remove_file(dir);
        let _ = std::fs::create_dir_all(dir);
    }
}

/// The `ResourcePackRepository` constructor: keeps the `selected` names that exist and are
/// compatible (or were accepted as incompatible before); returns them in order and the names
/// vanilla removes from the settings.
pub fn restore_selection(all: &[PackEntry], selected: &[String], incompatible: &[String]) -> (Vec<PackEntry>, Vec<String>) {
    let (mut out, mut removed) = (Vec::new(), Vec::new());
    for name in selected {
        for e in all.iter().filter(|e| &e.name == name) {
            if e.compatible() || incompatible.contains(&e.name) {
                out.push(e.clone());
                break;
            }
            removed.push(e.name.clone());
        }
    }
    (out, removed)
}

/// `Minecraft.refreshResources` order: default pack, `--pack`s, the selected packs from the
/// bottom of the list up, then the server pack.
pub fn build_stack(jar: &Path, base: &[PathBuf], selected: &[PathBuf], server: Option<&Path>) -> std::io::Result<AssetSource> {
    let mut assets = AssetSource::new();
    assets.push(jar)?;
    for p in base.iter().chain(selected) {
        if let Err(e) = assets.push(p) {
            eprintln!("skipping pack {}: {e}", p.display());
        }
    }
    if let Some(s) = server {
        assets.push(s)?;
    }
    Ok(assets)
}

/// Resolves option names against `dir` into pack paths (unknown or incompatible ones drop out,
/// as in the constructor).
pub fn selected_paths(dir: &Path, selected: &[String], incompatible: &[String]) -> Vec<PathBuf> {
    restore_selection(&scan(dir), selected, incompatible).0.into_iter().map(|e| e.path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rustmc-repo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zip_pack(path: &Path, files: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (n, b) in files {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn meta_and_compatibility() {
        let ok = parse_meta(br#"{"pack":{"pack_format":1,"description":"hi"}}"#).unwrap().unwrap();
        assert_eq!(ok.0, 1);
        assert!(parse_meta(br#"{"x":1}"#).unwrap().is_none());
        assert!(parse_meta(b"not json").is_err());
    }

    #[test]
    fn scan_and_selection() {
        let d = tmp("scan");
        zip_pack(&d.join("a.zip"), &[("pack.mcmeta", br#"{"pack":{"pack_format":1,"description":"A"}}"#)]);
        zip_pack(&d.join("new.zip"), &[("pack.mcmeta", br#"{"pack":{"pack_format":2,"description":"N"}}"#)]);
        zip_pack(&d.join("nometa.zip"), &[("x", b"")]);
        std::fs::write(d.join("notes.txt"), "").unwrap();
        std::fs::create_dir(d.join("folder")).unwrap();
        std::fs::write(d.join("folder/pack.mcmeta"), r#"{"pack":{"pack_format":1,"description":"F"}}"#).unwrap();
        std::fs::create_dir(d.join("empty")).unwrap();
        let all = scan(&d);
        let names: Vec<_> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.zip", "folder", "new.zip"]);
        assert!(all[0].compatible() && !all[2].compatible());
        // Incompatible packs stay selected only when previously accepted.
        let sel = ["new.zip".to_string(), "a.zip".to_string(), "gone.zip".to_string()];
        let (kept, removed) = restore_selection(&all, &sel, &[]);
        assert_eq!(kept.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.zip"]);
        assert_eq!(removed, ["new.zip"]);
        let (kept, removed) = restore_selection(&all, &sel, &["new.zip".to_string()]);
        assert_eq!(kept.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["new.zip", "a.zip"]);
        assert!(removed.is_empty());
    }

    #[test]
    fn stack_order_top_wins() {
        let d = tmp("stack");
        zip_pack(&d.join("jar.zip"), &[("assets/minecraft/t.txt", b"jar"), ("assets/minecraft/j.txt", b"j")]);
        zip_pack(&d.join("low.zip"), &[("assets/minecraft/t.txt", b"low")]);
        std::fs::create_dir_all(d.join("high/assets/minecraft")).unwrap();
        std::fs::write(d.join("high/assets/minecraft/t.txt"), "high").unwrap();
        zip_pack(&d.join("srv.zip"), &[("assets/minecraft/s.txt", b"srv")]);
        let s = build_stack(&d.join("jar.zip"), &[], &[d.join("low.zip"), d.join("high")], Some(&d.join("srv.zip"))).unwrap();
        assert_eq!(s.read("t.txt").unwrap(), b"high");
        assert_eq!(s.read("j.txt").unwrap(), b"j");
        assert_eq!(s.read("s.txt").unwrap(), b"srv");
        let all: Vec<_> = s.read_all("t.txt");
        assert_eq!(all, vec![b"jar".to_vec(), b"low".to_vec(), b"high".to_vec()]);
    }
}
