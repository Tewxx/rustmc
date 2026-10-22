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
    if let Some(biomes) = level.get("Biomes").and_then(Tag::as_bytes) {
        if biomes.len() == 256 { chunk.biomes.copy_from_slice(biomes); }
    }
    if let Some(tiles) = level.get("TileEntities").and_then(Tag::as_list) {
        for tile in tiles.iter().filter_map(Tag::as_compound) {
            let coords = ["x", "y", "z"].map(|k| tile.get(k).and_then(Tag::as_i64));
            if let [Some(x), Some(y), Some(z)] = coords {
                chunk.tile_entities.insert(mc_core::BlockPos::new(x as i32, y as i32, z as i32), tile.clone());
            }
        }
    }
    chunk.terrain_populated = level.get("TerrainPopulated").and_then(Tag::as_i64).unwrap_or(0) != 0;
    chunk.light_populated = level.get("LightPopulated").and_then(Tag::as_i64).unwrap_or(0) != 0;
    chunk.inhabited_time = level.get("InhabitedTime").and_then(Tag::as_i64).unwrap_or(0);
    if let Some(sections) = level.get("Sections").and_then(Tag::as_list) {
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
            let mut section = crate::chunk::Section::empty();
            for i in 0..4096.min(blocks.len()) {
                let id = blocks[i] as u16 | ((nibble(add, i) as u16) << 8);
                section.blocks[i] = id << 4 | nibble(data, i) as u16;
            }
            section.recount();
            let sky = s.get("SkyLight").and_then(Tag::as_bytes);
            let block = s.get("BlockLight").and_then(Tag::as_bytes);
            for i in 0..4096 {
                section.sky_light[i] = nibble(sky, i);
                section.block_light[i] = nibble(block, i);
            }
            chunk.sections[y as usize] = Some(section);
        }
    }
    match level.get("HeightMap").and_then(Tag::as_ints) {
        Some(h) if h.len() == 256 => chunk.height_map.copy_from_slice(h),
        _ => chunk.generate_height_map(),
    }
    chunk.height_map_minimum = chunk.height_map.iter().copied().min().unwrap_or(0);
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;
    use mc_core::BlockPos;
    use mc_data::{BlockAccess, StateId};

    #[test]
    fn saved_biomes_and_pot_contents_reach_world_block_access() {
        let pot = Tag::Compound([
            ("id".into(), Tag::String("FlowerPot".into())),
            ("x".into(), Tag::Int(-1)), ("y".into(), Tag::Int(70)), ("z".into(), Tag::Int(-1)),
            ("Item".into(), Tag::String("minecraft:tallgrass".into())), ("Data".into(), Tag::Int(2)),
        ].into());
        let mut biomes = vec![1; 256];
        biomes[255] = 6;
        let root = Tag::Compound([("Level".into(), Tag::Compound([
            ("Biomes".into(), Tag::ByteArray(biomes)),
            ("TileEntities".into(), Tag::List(10, vec![pot])),
        ].into()))].into());
        let mut chunk = parse_chunk(&root, -1, -1).unwrap();
        chunk.set(15, 70, 15, 140, 0);
        let mut world = World::default(); world.insert(chunk);
        let pos = BlockPos::new(-1, 70, -1);
        assert_eq!(world.biome_id(pos), 6);
        assert_eq!(world.flower_pot(pos), Some((31, 2)));
        assert_eq!(world.state(pos).actual_state(&world, pos).value("contents"), Some("fern"));
        assert_eq!(world.state(BlockPos::new(-1, -1, -1)), StateId::AIR);
    }

    #[test]
    fn saved_moving_piston_data_reaches_collision_geometry() {
        use mc_data::geometry::BlockGeometry;
        let piston=Tag::Compound([
            ("id".into(),Tag::String("Piston".into())),
            ("x".into(),Tag::Int(15)),("y".into(),Tag::Int(70)),("z".into(),Tag::Int(0)),
            ("blockId".into(),Tag::Int(1)),("blockData".into(),Tag::Int(0)),
            ("facing".into(),Tag::Int(5)),("progress".into(),Tag::Float(0.5)),("extending".into(),Tag::Byte(1)),
        ].into());
        let root=Tag::Compound([("Level".into(),Tag::Compound([
            ("TileEntities".into(),Tag::List(10,vec![piston])),
        ].into()))].into());
        let mut chunk=parse_chunk(&root,0,0).unwrap();chunk.set(15,70,0,36,5);
        let mut world=World::default();world.insert(chunk);
        let pos=BlockPos::new(15,70,0);let state=world.state(pos);
        let carried=world.moving_piston(pos).unwrap();
        assert_eq!(carried.state.block().id,1);
        let mut geometry=BlockGeometry::new();
        assert_eq!(geometry.update_bounds(state,&world,pos).0,[-0.5,0.,0.,0.5,1.,1.]);
        let collision=geometry.collision_box(state,&world,pos).unwrap();
        assert_eq!([collision.min_x,collision.max_x],[15.,15.5]);
    }
}
