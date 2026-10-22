//! The section payload of `S21PacketChunkData` / `S26PacketMapChunkBulk`
//! (`getExtractedData` and `Chunk.fillChunk`).
//!
//! Layout, for the sections set in `bitmask` in ascending order: all block arrays (4096
//! little-endian `u16` of `id << 4 | meta`), then all block-light nibble arrays, then (with sky
//! light) all sky-light arrays, then 256 biome bytes when the chunk is full.
//! Section indices are `y << 8 | z << 4 | x`; nibble `i` is the low half of byte `i >> 1` when
//! `i` is even.

use crate::buf::{Error, Result};

/// `S21PacketChunkData.func_180737_a`
pub fn data_size(sections: usize, sky_light: bool, full: bool) -> usize {
    sections * 2 * 4096 + sections * 2048 + if sky_light { sections * 2048 } else { 0 } + if full { 256 } else { 0 }
}

/// One chunk section as sent on the wire.
#[derive(Clone, Debug, PartialEq)]
pub struct WireSection {
    pub blocks: Box<[u16; 4096]>,
    /// Packed nibbles.
    pub block_light: Box<[u8; 2048]>,
    pub sky_light: Option<Box<[u8; 2048]>>,
}

impl WireSection {
    pub fn empty(sky: bool) -> Self {
        Self { blocks: Box::new([0; 4096]), block_light: Box::new([0; 2048]), sky_light: sky.then(|| Box::new([0; 2048])) }
    }
}

/// Decoded payload: `sections[i]` is `Some` for each bit set in the mask.
#[derive(Clone, Debug, PartialEq)]
pub struct WireChunk {
    pub sections: [Option<WireSection>; 16],
    pub biomes: Option<Box<[u8; 256]>>,
}

/// `getExtractedData` over sections already filtered the way vanilla does (a full chunk skips
/// empty sections; a partial update includes only masked ones). Returns `(bitmask, data)`.
pub fn encode(sections: &[Option<WireSection>; 16], biomes: Option<&[u8; 256]>, sky_light: bool) -> (u16, Vec<u8>) {
    let mut mask = 0u16;
    let mut present = Vec::new();
    for (i, s) in sections.iter().enumerate() {
        if let Some(s) = s {
            mask |= 1 << i;
            present.push(s);
        }
    }
    let mut out = Vec::with_capacity(data_size(present.len(), sky_light, biomes.is_some()));
    for s in &present {
        for &b in s.blocks.iter() {
            out.extend_from_slice(&b.to_le_bytes());
        }
    }
    for s in &present {
        out.extend_from_slice(&s.block_light[..]);
    }
    if sky_light {
        for s in &present {
            match &s.sky_light {
                Some(l) => out.extend_from_slice(&l[..]),
                None => out.extend_from_slice(&[0u8; 2048]),
            }
        }
    }
    if let Some(b) = biomes {
        out.extend_from_slice(b);
    }
    (mask, out)
}

/// Parses a payload as `Chunk.fillChunk` does. `full` means biomes follow.
pub fn decode(mask: u16, data: &[u8], sky_light: bool, full: bool) -> Result<WireChunk> {
    if data.len() < data_size(mask.count_ones() as usize, sky_light, full) {
        return Err(Error::Eof);
    }
    let mut sections: [Option<WireSection>; 16] = Default::default();
    let mut p = 0;
    for (i, slot) in sections.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            let mut s = WireSection::empty(sky_light);
            for (k, b) in s.blocks.iter_mut().enumerate() {
                *b = u16::from_le_bytes([data[p + k * 2], data[p + k * 2 + 1]]);
            }
            p += 8192;
            *slot = Some(s);
        }
    }
    for s in sections.iter_mut().flatten() {
        s.block_light.copy_from_slice(&data[p..p + 2048]);
        p += 2048;
    }
    if sky_light {
        for s in sections.iter_mut().flatten() {
            s.sky_light.as_mut().unwrap().copy_from_slice(&data[p..p + 2048]);
            p += 2048;
        }
    }
    let biomes = full.then(|| {
        let mut b = Box::new([0u8; 256]);
        b.copy_from_slice(&data[p..p + 256]);
        b
    });
    Ok(WireChunk { sections, biomes })
}

/// Nibble `i` (0–4095) of a packed array.
#[inline]
pub fn nibble(arr: &[u8; 2048], i: usize) -> u8 {
    (arr[i >> 1] >> ((i & 1) * 4)) & 15
}

#[inline]
pub fn set_nibble(arr: &mut [u8; 2048], i: usize, v: u8) {
    let sh = (i & 1) * 4;
    arr[i >> 1] = (arr[i >> 1] & !(15 << sh)) | ((v & 15) << sh);
}
