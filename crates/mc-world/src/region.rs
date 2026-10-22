//! Anvil (`.mca`) reader for the 1.8 chunk format (`Blocks`/`Add`/`Data` arrays).

use crate::chunk::Chunk;
use mc_core::nbt::{self, Tag};
use std::io::Read;
use std::path::Path;

#[derive(Debug)]
pub enum RegionError {
    Io(std::io::Error),
    Nbt(nbt::NbtError),
    Format(&'static str),
}

impl std::fmt::Display for RegionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegionError::Io(e) => write!(f, "{e}"),
            RegionError::Nbt(e) => write!(f, "{e}"),
            RegionError::Format(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for RegionError {}

impl From<std::io::Error> for RegionError {
    fn from(e: std::io::Error) -> Self {
        RegionError::Io(e)
    }
}

/// Loads chunk `(cx, cz)` from `<world>/region`. `Ok(None)` if it was never generated.
pub fn load_chunk(world: &Path, cx: i32, cz: i32) -> Result<Option<Chunk>, RegionError> {
    let path = world.join("region").join(format!("r.{}.{}.mca", cx >> 5, cz >> 5));
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let idx = (((cx & 31) + (cz & 31) * 32) * 4) as usize;
    let header = data.get(idx..idx + 4).ok_or(RegionError::Format("truncated region header"))?;
    let offset = (u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize) * 4096;
    if offset == 0 {
        return Ok(None);
    }
    let head = data.get(offset..offset + 5).ok_or(RegionError::Format("chunk offset past end of file"))?;
    let len = u32::from_be_bytes(head[0..4].try_into().unwrap()) as usize;
    let body = data.get(offset + 5..offset + 4 + len).ok_or(RegionError::Format("chunk length past end of file"))?;
    let mut raw = Vec::new();
    match head[4] {
        1 => flate2::read::GzDecoder::new(body).read_to_end(&mut raw)?,
        2 => flate2::read::ZlibDecoder::new(body).read_to_end(&mut raw)?,
        _ => return Err(RegionError::Format("unknown chunk compression")),
    };
    let (_, root) = nbt::read(&raw).map_err(RegionError::Nbt)?;
    parse_chunk(&root, cx, cz).map(Some)
}

fn parse_chunk(root: &Tag, cx: i32, cz: i32) -> Result<Chunk, RegionError> {
    let level = root.as_compound().and_then(|c| c.get("Level")).and_then(Tag::as_compound).ok_or(RegionError::Format("missing Level"))?;
    let mut chunk = Chunk::new(cx, cz);
    let Some(sections) = level.get("Sections").and_then(Tag::as_list) else {
        return Ok(chunk);
    };
    // Saved light is only trusted when the chunk was fully lit (`LightPopulated`).
    let light_populated = level.get("LightPopulated").and_then(Tag::as_i64).unwrap_or(0) != 0;
    let mut has_section = [false; 16];
    for s in sections.iter().filter_map(Tag::as_compound) {
        let y = s.get("Y").and_then(Tag::as_i64).unwrap_or(-1);
        if !(0..16).contains(&y) {
            continue;
        }
        let blocks = s.get("Blocks").and_then(Tag::as_bytes).ok_or(RegionError::Format(
            "section has no Blocks array (this world is from 1.13+; only 1.8-format worlds load)",
        ))?;
        let data = s.get("Data").and_then(Tag::as_bytes);
        let add = s.get("Add").and_then(Tag::as_bytes);
        let nibble = |arr: Option<&[u8]>, i: usize| -> u8 {
            arr.and_then(|a| a.get(i >> 1)).map_or(0, |b| if i & 1 == 0 { b & 15 } else { b >> 4 })
        };
        for i in 0..4096.min(blocks.len()) {
            let id = blocks[i] as u16 | ((nibble(add, i) as u16) << 8);
            if id == 0 {
                continue;
            }
            let (lx, ly, lz) = (i & 15, (i >> 8) as i32, (i >> 4) & 15);
            chunk.set(lx, y as i32 * 16 + ly, lz, id, nibble(data, i));
        }
        has_section[y as usize] = true;
        let sky = s.get("SkyLight").and_then(Tag::as_bytes);
        let block = s.get("BlockLight").and_then(Tag::as_bytes);
        let base = (y as usize) << 12;
        for i in 0..4096 {
            chunk.sky_light[base + i] = nibble(sky, i);
            chunk.block_light[base + i] = nibble(block, i);
        }
    }
    if light_populated {
        // Missing sections read as full sky light above the terrain and 0 below
        // (`Chunk.getLightFor` uses `canSeeSky`), approximated per column here.
        for z in 0..16 {
            for x in 0..16 {
                let top = (0..256).rev().find(|&y| crate::blocks::props(chunk.get(x, y, z).0, chunk.get(x, y, z).1).opacity > 0);
                let height = top.map_or(0, |y| y + 1);
                for y in 0..256 {
                    if !has_section[(y >> 4) as usize] {
                        let i = crate::chunk::column_index(x, y, z);
                        chunk.sky_light[i] = if y >= height { 15 } else { 0 };
                    }
                }
            }
        }
    }
    chunk.light_valid = light_populated;
    Ok(chunk)
}
