//! Player skins (`SkinManager` + `ImageBufferDownload`): the `textures` profile property, a
//! background download from the texture server, legacy 64×32 conversion, and the default
//! Steve/Alex choice from the profile UUID (`DefaultPlayerSkin`).
use mc_protocol::Uuid;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

/// Decoded skin ready for upload.
pub struct Skin {
    pub uuid: Uuid,
    pub rgba: Vec<u8>,
    pub slim: bool,
}

pub struct Skins {
    tx: Sender<Skin>,
    rx: Receiver<Skin>,
    /// Atlas cell and model per player with a downloaded skin.
    pub loaded: HashMap<Uuid, (u32, bool)>,
    /// Retained pixels per player, for re-uploading after a resource-pack reload recreates
    /// the GPU textures.
    rgba: HashMap<Uuid, Vec<u8>>,
    requested: HashMap<Uuid, String>,
    next_cell: u32,
}

impl Default for Skins {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Skins { tx, rx, loaded: HashMap::new(), rgba: HashMap::new(), requested: HashMap::new(), next_cell: 0 }
    }
}

/// `UUID.hashCode() & 1`: Alex for odd hashes.
pub fn default_slim(u: Uuid) -> bool {
    let hilo = u.0 ^ u.1;
    (((hilo >> 32) as i32) ^ (hilo as i32)) & 1 == 1
}

fn b64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        buf = buf << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// The skin URL and slim flag inside a base64 `textures` property value.
pub fn parse_textures(value: &str) -> Option<(String, bool)> {
    let json: serde_json::Value = serde_json::from_slice(&b64(value)?).ok()?;
    let skin = &json["textures"]["SKIN"];
    let url = skin["url"].as_str()?.to_string();
    let slim = skin["metadata"]["model"].as_str() == Some("slim");
    Some((url, slim))
}

/// `ImageBufferDownload.parseUserSkin`: 64×64 RGBA with legacy skins expanded and the
/// opacity rules applied.
pub fn convert(img: &image::RgbaImage) -> Option<Vec<u8>> {
    let (w, h) = img.dimensions();
    if w != 64 || (h != 32 && h != 64) {
        return None;
    }
    let mut out = image::RgbaImage::new(64, 64);
    for y in 0..h {
        for x in 0..64 {
            out.put_pixel(x, y, *img.get_pixel(x, y));
        }
    }
    if h == 32 {
        // drawImage(dx1, dy1, dx2, dy2, sx1, sy1, sx2, sy2): dx1 > dx2 mirrors horizontally.
        let copies: [[u32; 8]; 12] = [
            [24, 48, 20, 52, 4, 16, 8, 20],
            [28, 48, 24, 52, 8, 16, 12, 20],
            [20, 52, 16, 64, 8, 20, 12, 32],
            [24, 52, 20, 64, 4, 20, 8, 32],
            [28, 52, 24, 64, 0, 20, 4, 32],
            [32, 52, 28, 64, 12, 20, 16, 32],
            [40, 48, 36, 52, 44, 16, 48, 20],
            [44, 48, 40, 52, 48, 16, 52, 20],
            [36, 52, 32, 64, 48, 20, 52, 32],
            [40, 52, 36, 64, 44, 20, 48, 32],
            [44, 52, 40, 64, 40, 20, 44, 32],
            [48, 52, 44, 64, 52, 20, 56, 32],
        ];
        for [dx1, dy1, dx2, _dy2, sx1, sy1, sx2, sy2] in copies {
            for sy in sy1..sy2 {
                for (i, sx) in (sx1..sx2).enumerate() {
                    let dx = dx1 - 1 - i as u32; // dx1 > dx2: mirrored
                    let _ = dx2;
                    out.put_pixel(dx, dy1 + (sy - sy1), *img.get_pixel(sx, sy));
                }
            }
        }
    }
    let opaque = |o: &mut image::RgbaImage, x0, y0, x1, y1| {
        for y in y0..y1 {
            for x in x0..x1 {
                o.get_pixel_mut(x, y).0[3] = 255;
            }
        }
    };
    // setAreaTransparent: only when the area has no transparent pixel already.
    let has_alpha = (0..32).any(|y| (32..64).any(|x| out.get_pixel(x, y).0[3] < 128));
    if !has_alpha {
        for y in 0..32 {
            for x in 32..64 {
                out.get_pixel_mut(x, y).0[3] = 0;
            }
        }
    }
    opaque(&mut out, 0, 0, 32, 16);
    opaque(&mut out, 0, 16, 64, 32);
    opaque(&mut out, 16, 48, 48, 64);
    Some(out.into_raw())
}

/// `NBTUtil.readGameProfileFromNBT` as far as the skull renderers need it: the profile id
/// (`EntityPlayer.getUUID`: the offline id when only a name is stored) and the `textures`
/// property value.
pub fn skull_profile(c: &mc_core::nbt::Compound) -> Option<(Uuid, Option<String>)> {
    use mc_core::nbt::Tag;
    let name = c.get("Name").and_then(Tag::as_str).unwrap_or("");
    let id = c.get("Id").and_then(Tag::as_str).and_then(Uuid::parse);
    if id.is_none() && name.is_empty() {
        return None;
    }
    let uuid = id.unwrap_or_else(|| Uuid::offline_player(name));
    let textures = c
        .get("Properties")
        .and_then(Tag::as_compound)
        .and_then(|p| p.get("textures"))
        .and_then(Tag::as_list)
        .and_then(|l| l.first())
        .and_then(Tag::as_compound)
        .and_then(|t| t.get("Value"))
        .and_then(Tag::as_str)
        .map(str::to_owned);
    Some((uuid, textures))
}

/// `TileEntityItemStackRenderer`'s profile for a skull stack: a `SkullOwner` compound, or a
/// bare name (resolved through the session server in vanilla; here only its offline id).
pub fn item_skull_profile(tag: &mc_core::nbt::Compound) -> Option<(Uuid, Option<String>)> {
    use mc_core::nbt::Tag;
    match tag.get("SkullOwner")? {
        Tag::Compound(c) => skull_profile(c),
        Tag::String(name) if !name.is_empty() => Some((Uuid::offline_player(name), None)),
        _ => None,
    }
}

/// Profiles whose skins a renderer asked for since the last tick (renderers only borrow the
/// client immutably, so they queue here and `Skins::request_wanted` starts the downloads).
static WANTED: std::sync::Mutex<Vec<(Uuid, String)>> = std::sync::Mutex::new(Vec::new());
/// Skins also registered as GUI textures (`rustmc:skin/<uuid>`), for skull item icons.
static GUI_SKINS: std::sync::Mutex<Vec<Uuid>> = std::sync::Mutex::new(Vec::new());

pub fn want(uuid: Uuid, textures: &str) {
    let mut w = WANTED.lock().unwrap();
    if !w.iter().any(|(u, _)| *u == uuid) {
        w.push((uuid, textures.to_owned()));
    }
}

pub fn gui_path(uuid: Uuid) -> String {
    format!("rustmc:skin/{uuid}")
}

/// The GUI texture for a skull item's owner, once the skin has arrived (Steve until then,
/// like the `ThreadDownloadImageData` fallback).
pub fn gui_skin(profile: &(Uuid, Option<String>)) -> Option<String> {
    let (uuid, textures) = profile;
    let t = textures.as_deref()?;
    if GUI_SKINS.lock().unwrap().contains(uuid) {
        return Some(gui_path(*uuid));
    }
    want(*uuid, t);
    None
}

pub fn mark_gui_skin(uuid: Uuid) {
    let mut g = GUI_SKINS.lock().unwrap();
    if !g.contains(&uuid) {
        g.push(uuid);
    }
}

impl Skins {
    /// Starts the downloads renderers queued with [`want`].
    pub fn request_wanted(&mut self) {
        let wanted = std::mem::take(&mut *WANTED.lock().unwrap());
        for (uuid, textures) in wanted {
            self.request(uuid, &textures);
        }
    }

    /// `TileEntitySkullRenderer`'s texture for a player skull: the downloaded skin cell, or
    /// whether the default skin is Alex (`DefaultPlayerSkin.getDefaultSkin(uuid)` when the
    /// profile has no skin; Steve while one is still downloading).
    pub fn skull_skin(&self, profile: Option<&(Uuid, Option<String>)>) -> (Option<u32>, bool) {
        let Some((uuid, textures)) = profile else { return (None, false) };
        match textures {
            Some(t) => {
                if let Some(&(cell, _)) = self.loaded.get(uuid) {
                    return (Some(cell), false);
                }
                want(*uuid, t);
                (None, false)
            }
            None => (None, default_slim(*uuid)),
        }
    }

    /// Starts a download for a profile's `textures` property (once per UUID and URL).
    pub fn request(&mut self, uuid: Uuid, textures: &str) {
        let Some((url, slim)) = parse_textures(textures) else { return };
        if self.requested.get(&uuid) == Some(&url) {
            return;
        }
        self.requested.insert(uuid, url.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // The texture server is plain http in 1.8 profiles; upgrade like modern launchers.
            let url = if url.starts_with("http://127.0.0.1") { url } else { url.replacen("http://", "https://", 1) };
            let Ok(resp) = ureq::get(&url).timeout(std::time::Duration::from_secs(15)).call() else { return };
            let mut bytes = Vec::new();
            if std::io::Read::read_to_end(&mut resp.into_reader(), &mut bytes).is_err() {
                return;
            }
            let Ok(img) = image::load_from_memory(&bytes) else { return };
            if let Some(rgba) = convert(&img.to_rgba8()) {
                let _ = tx.send(Skin { uuid, rgba, slim });
            }
        });
    }

    /// Finished downloads: assigns atlas cells and returns what to upload.
    pub fn poll(&mut self) -> Vec<(Uuid, u32, Vec<u8>)> {
        let mut up = Vec::new();
        for s in self.rx.try_iter() {
            let cell = match self.loaded.get(&s.uuid) {
                Some(&(c, _)) => c,
                None => {
                    let c = self.next_cell % 1024;
                    self.next_cell += 1;
                    c
                }
            };
            self.loaded.insert(s.uuid, (cell, s.slim));
            self.rgba.insert(s.uuid, s.rgba.clone());
            up.push((s.uuid, cell, s.rgba));
        }
        up
    }

    /// Every decoded skin, for re-upload after the renderer is recreated.
    pub fn cells(&self) -> Vec<(Uuid, u32, Vec<u8>)> {
        self.loaded
            .iter()
            .filter_map(|(uuid, (cell, _))| self.rgba.get(uuid).map(|rgba| (*uuid, *cell, rgba.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_property_and_defaults() {
        // {"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/abc","metadata":{"model":"slim"}}}}
        let json = r#"{"textures":{"SKIN":{"url":"http://textures.minecraft.net/texture/abc","metadata":{"model":"slim"}}}}"#;
        let enc = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let b = json.as_bytes();
            let mut s = String::new();
            for c in b.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                for i in 0..4 {
                    if i <= c.len() {
                        s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                    } else {
                        s.push('=');
                    }
                }
            }
            s
        };
        assert_eq!(parse_textures(&enc), Some(("http://textures.minecraft.net/texture/abc".into(), true)));
        // Notch's UUID 069a79f4-44e9-4726-a5be-fca90e38aaf5 → Steve (even hash).
        let notch = Uuid(0x069a79f444e94726, 0xa5befca90e38aaf5);
        let hilo = notch.0 ^ notch.1;
        assert_eq!(default_slim(notch), (((hilo >> 32) as i32 ^ hilo as i32) & 1) == 1);
    }

    #[test]
    fn legacy_skin_expands_mirrored_limbs() {
        let mut img = image::RgbaImage::new(64, 32);
        img.put_pixel(4, 20, image::Rgba([1, 2, 3, 255])); // right leg front, leftmost column
        let out = convert(&img).unwrap();
        // Copied to the left leg front (20..24, 52..64) mirrored: x 4 → 23.
        let px = |x: usize, y: usize| &out[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(px(23, 52), &[1, 2, 3, 255]);
    }
}
