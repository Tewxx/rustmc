//! Block/item texture atlas (`TextureMap` + `TextureAtlasSprite`): uniform cells, vanilla's
//! mipmap generation (`TextureUtil.generateMipmapData`) and `.mcmeta` animations.
//!
//! Every sprite gets a square cell of the largest sprite size. A sprite smaller than the cell is
//! upscaled for level 0 only; its own mip chain lands on the levels whose size matches, so the
//! GPU's level selection samples exactly the texels vanilla uploads for each level.

use crate::AssetSource;
use std::collections::HashMap;

pub const MISSING: &str = "missingno";

#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub min_u: f32,
    pub min_v: f32,
    pub max_u: f32,
    pub max_v: f32,
}

impl Sprite {
    /// `getInterpolatedU`: `u` is in model texels (0–16).
    pub fn u(&self, u: f32) -> f32 {
        self.min_u + (self.max_u - self.min_u) * u / 16.0
    }

    pub fn v(&self, v: f32) -> f32 {
        self.min_v + (self.max_v - self.min_v) * v / 16.0
    }
}

/// One `AnimationFrame`: frame index and its time (-1 = the default frametime).
#[derive(Clone, Copy, Debug)]
struct Frame {
    index: usize,
    time: i32,
}

struct Animation {
    /// Cell origin in level-0 pixels.
    x: u32,
    y: u32,
    /// Native sprite size.
    size: u32,
    /// Per frame: the native mip chain (`framesTextureData`).
    frames: Vec<Vec<Vec<u32>>>,
    order: Vec<Frame>,
    frame_time: i32,
    interpolate: bool,
    frame_counter: usize,
    tick_counter: i32,
}

/// A region of one mip level to re-upload after an animation step.
pub struct Upload {
    pub level: u32,
    pub x: u32,
    pub y: u32,
    pub size: u32,
    pub rgba: Vec<u8>,
}

pub struct Atlas {
    pub width: u32,
    pub height: u32,
    /// Level 0 pixels.
    pub rgba: Vec<u8>,
    /// Every level including 0, each `(width >> i) × (height >> i)`.
    pub levels: Vec<Vec<u8>>,
    pub cell: u32,
    sprites: HashMap<String, Sprite>,
    /// Native first-frame pixels, for `ItemModelGenerator`.
    natives: HashMap<String, (u32, Vec<u32>)>,
    animations: Vec<Animation>,
}

impl Atlas {
    /// Builds an atlas from texture names like `blocks/stone` with vanilla's default 4 mip levels.
    pub fn build(assets: &AssetSource, names: impl IntoIterator<Item = String>) -> Atlas {
        Self::build_with_mipmaps(assets, names, 4)
    }

    /// Missing or broken textures map to the magenta/black checker, as in vanilla.
    pub fn build_with_mipmaps(assets: &AssetSource, names: impl IntoIterator<Item = String>, mip_levels: u32) -> Atlas {
        // (name, native size, frames as ARGB, metadata)
        let mut loaded: Vec<(String, u32, Vec<Vec<u32>>, Option<(Vec<Frame>, i32, bool)>)> = Vec::new();
        let missing = missing_texture();
        loaded.push((MISSING.to_string(), 16, vec![missing], None));
        let mut seen = std::collections::HashSet::new();
        for name in names {
            if !seen.insert(name.clone()) || name == MISSING {
                continue;
            }
            let Some(img) = assets
                .read(&format!("textures/{name}.png"))
                .and_then(|b| image::load_from_memory(&b).ok())
                .map(|i| i.to_rgba8())
            else {
                continue;
            };
            let w = img.width();
            if w == 0 || !w.is_power_of_two() {
                continue;
            }
            let meta = assets
                .read_json(&format!("textures/{name}.png.mcmeta"))
                .and_then(|j| j.get("animation").cloned());
            let argb: Vec<u32> = img.pixels().map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect();
            let count = (img.height() / w).max(1) as usize;
            if meta.is_none() && img.height() != w {
                // "broken aspect ratio and not an animation": vanilla skips the sprite.
                continue;
            }
            let frame = |i: usize| argb[i * (w * w) as usize..(i + 1) * (w * w) as usize].to_vec();
            match meta {
                None => loaded.push((name, w, vec![frame(0)], None)),
                Some(a) => {
                    let frame_time = a.get("frametime").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
                    let interpolate = a.get("interpolate").and_then(|v| v.as_bool()).unwrap_or(false);
                    let mut order = Vec::new();
                    if let Some(list) = a.get("frames").and_then(|v| v.as_array()) {
                        for f in list {
                            if let Some(i) = f.as_i64() {
                                order.push(Frame { index: i as usize, time: -1 });
                            } else if let Some(i) = f.get("index").and_then(|v| v.as_i64()) {
                                let time = f.get("time").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
                                order.push(Frame { index: i as usize, time });
                            }
                        }
                    }
                    if order.iter().any(|f| f.index >= count) {
                        continue;
                    }
                    if order.is_empty() {
                        order = (0..count).map(|index| Frame { index, time: -1 }).collect();
                    }
                    let frames = (0..count).map(frame).collect();
                    loaded.push((name, w, frames, Some((order, frame_time, interpolate))));
                }
            }
        }

        let cell = loaded.iter().map(|l| l.1).max().unwrap_or(16).max(16);
        let cols = (loaded.len() as f32).sqrt().ceil() as u32;
        let rows = (loaded.len() as u32).div_ceil(cols);
        let width = (cols * cell).next_power_of_two();
        let height = (rows * cell).next_power_of_two();
        // Total levels so a 16-texel sprite reaches vanilla's smallest level.
        let extra = cell.trailing_zeros().saturating_sub(4);
        let level_count = (1 + extra + mip_levels).min(cell.trailing_zeros() + 1);
        let mut levels: Vec<Vec<u8>> = (0..level_count).map(|l| vec![0u8; ((width >> l) * (height >> l) * 4) as usize]).collect();
        let mut sprites = HashMap::new();
        let mut natives = HashMap::new();
        let mut animations = Vec::new();
        for (i, (name, size, frames, meta)) in loaded.into_iter().enumerate() {
            let (x, y) = ((i as u32 % cols) * cell, (i as u32 / cols) * cell);
            let mips: Vec<Vec<Vec<u32>>> = frames.iter().map(|f| mip_chain(f, size, mip_levels)).collect();
            let first = meta.as_ref().map_or(0, |m| m.0[0].index);
            write_cell(&mut levels, width, x, y, cell, size, &mips[first]);
            let f = 0.009999999776482582f64;
            sprites.insert(
                name.clone(),
                Sprite {
                    min_u: x as f32 / width as f32 + (f / width as f64) as f32,
                    min_v: y as f32 / height as f32 + (f / height as f64) as f32,
                    max_u: (x + cell) as f32 / width as f32 - (f / width as f64) as f32,
                    max_v: (y + cell) as f32 / height as f32 - (f / height as f64) as f32,
                },
            );
            natives.insert(name, (size, frames[first].clone()));
            if let Some((order, frame_time, interpolate)) = meta {
                if frames.len() > 1 {
                    animations.push(Animation {
                        x,
                        y,
                        size,
                        frames: mips,
                        order,
                        frame_time,
                        interpolate,
                        frame_counter: 0,
                        tick_counter: 0,
                    });
                }
            }
        }
        let rgba = levels[0].clone();
        Atlas { width, height, rgba, levels, cell, sprites, natives, animations }
    }

    pub fn sprite(&self, name: &str) -> Sprite {
        self.sprites.get(name).or_else(|| self.sprites.get(MISSING)).copied().unwrap()
    }

    /// The sprite whose UV rectangle contains `(u, v)`.
    pub fn sprite_at(&self, u: f32, v: f32) -> Option<Sprite> {
        self.sprites.values().find(|s| (s.min_u..=s.max_u).contains(&u) && (s.min_v..=s.max_v).contains(&v)).copied()
    }

    pub fn has(&self, name: &str) -> bool {
        self.sprites.contains_key(name)
    }

    /// Native (size, ARGB pixels) of a sprite's first frame.
    pub fn native(&self, name: &str) -> Option<(u32, &[u32])> {
        self.natives.get(name).or_else(|| self.natives.get(MISSING)).map(|(s, p)| (*s, p.as_slice()))
    }

    /// `TextureMap.updateAnimations` for one client tick; returns the regions to upload.
    pub fn tick(&mut self) -> Vec<Upload> {
        let mut out = Vec::new();
        let (width, cell, levels) = (self.width, self.cell, self.levels.len() as u32);
        for a in &mut self.animations {
            a.tick_counter += 1;
            let time = |a: &Animation, c: usize| {
                let f = a.order[c];
                if f.time == -1 {
                    a.frame_time
                } else {
                    f.time
                }
            };
            let mut data: Option<Vec<Vec<u32>>> = None;
            if a.tick_counter >= time(a, a.frame_counter) {
                let i = a.order[a.frame_counter].index;
                a.frame_counter = (a.frame_counter + 1) % a.order.len();
                a.tick_counter = 0;
                let k = a.order[a.frame_counter].index;
                if i != k && k < a.frames.len() {
                    data = Some(a.frames[k].clone());
                }
            } else if a.interpolate {
                let d0 = 1.0 - a.tick_counter as f64 / time(a, a.frame_counter) as f64;
                let i = a.order[a.frame_counter].index;
                let k = a.order[(a.frame_counter + 1) % a.order.len()].index;
                if i != k && k < a.frames.len() {
                    let (fa, fb) = (&a.frames[i], &a.frames[k]);
                    data = Some(
                        fa.iter()
                            .zip(fb)
                            .map(|(la, lb)| {
                                la.iter()
                                    .zip(lb)
                                    .map(|(&j1, &k1)| {
                                        let c = |s: u32| {
                                            ((((j1 >> s) & 255) as f64 * d0 + ((k1 >> s) & 255) as f64 * (1.0 - d0)) as i32 as u32) & 255
                                        };
                                        (j1 & 0xFF00_0000) | c(16) << 16 | c(8) << 8 | c(0)
                                    })
                                    .collect()
                            })
                            .collect(),
                    );
                }
            }
            if let Some(chain) = data {
                let mut tmp: Vec<Vec<u8>> = (0..levels).map(|_| Vec::new()).collect();
                for (l, buf) in tmp.iter_mut().enumerate() {
                    let s = cell >> l;
                    *buf = vec![0; (s * s * 4) as usize];
                }
                // Render the cell into a scratch cell-sized buffer per level, then upload.
                let mut cell_levels: Vec<Vec<u8>> = tmp;
                write_cell_local(&mut cell_levels, cell, a.size, &chain);
                for (l, buf) in cell_levels.into_iter().enumerate() {
                    out.push(Upload { level: l as u32, x: a.x >> l, y: a.y >> l, size: cell >> l, rgba: buf });
                }
            }
        }
        // Keep the CPU copy in sync for anything reading it later (GUI registers level 0).
        for u in &out {
            let w = width >> u.level;
            let dst = &mut self.levels[u.level as usize];
            for row in 0..u.size {
                let s = (row * u.size * 4) as usize;
                let d = (((u.y + row) * w + u.x) * 4) as usize;
                dst[d..d + (u.size * 4) as usize].copy_from_slice(&u.rgba[s..s + (u.size * 4) as usize]);
            }
            if u.level == 0 {
                self.rgba = self.levels[0].clone();
            }
        }
        out
    }
}

/// `TextureUtil.generateMipmapData` for one frame, native size, `levels` extra levels.
fn mip_chain(level0: &[u32], size: u32, levels: u32) -> Vec<Vec<u32>> {
    let mut out = vec![level0.to_vec()];
    // The alpha-aware path is chosen once, from level 0.
    let transparent = level0.iter().any(|&p| p >> 24 == 0);
    let mut s = size;
    for _ in 0..levels {
        if s <= 1 {
            break;
        }
        let prev = out.last().unwrap();
        let n = s >> 1;
        let mut next = vec![0u32; (n * n) as usize];
        for y in 0..n {
            for x in 0..n {
                let k = |dx: u32, dy: u32| prev[((y * 2 + dy) * s + x * 2 + dx) as usize];
                next[(y * n + x) as usize] = blend_colors(k(0, 0), k(1, 0), k(0, 1), k(1, 1), transparent);
            }
        }
        out.push(next);
        s = n;
    }
    out
}

fn blend_colors(a: u32, b: u32, c: u32, d: u32, transparent: bool) -> u32 {
    let px = [a, b, c, d];
    if !transparent {
        let comp = |shift: u32| {
            let v: f32 = px.iter().map(|p| ((((p >> shift) & 255) as f32) / 255.0).powf_java(2.2)).sum();
            let f4 = ((v as f64) * 0.25).powf(0.45454545454545453) as f32;
            ((f4 as f64) * 255.0) as i32 as u32
        };
        return comp(24) << 24 | comp(16) << 16 | comp(8) << 8 | comp(0);
    }
    let (mut f, mut f1, mut f2, mut f3) = (0f32, 0f32, 0f32, 0f32);
    for &p in &px {
        if p >> 24 != 0 {
            f += ((p >> 24 & 255) as f32 / 255.0).powf_java(2.2);
            f1 += ((p >> 16 & 255) as f32 / 255.0).powf_java(2.2);
            f2 += ((p >> 8 & 255) as f32 / 255.0).powf_java(2.2);
            f3 += ((p & 255) as f32 / 255.0).powf_java(2.2);
        }
    }
    f /= 4.0;
    f1 /= 4.0;
    f2 /= 4.0;
    f3 /= 4.0;
    let back = |v: f32| ((v as f64).powf(0.45454545454545453) * 255.0) as i32;
    let mut i2 = back(f);
    if i2 < 96 {
        i2 = 0;
    }
    (i2 as u32) << 24 | (back(f1) as u32) << 16 | (back(f2) as u32) << 8 | back(f3) as u32
}

/// `(float)Math.pow((double)x, 2.2D)`
trait PowJava {
    fn powf_java(self, e: f64) -> f32;
}
impl PowJava for f32 {
    fn powf_java(self, e: f64) -> f32 {
        (self as f64).powf(e) as f32
    }
}

/// Fills one cell on every atlas level from a sprite's native mip chain.
fn write_cell(levels: &mut [Vec<u8>], width: u32, x: u32, y: u32, cell: u32, size: u32, chain: &[Vec<u32>]) {
    let mut local: Vec<Vec<u8>> = (0..levels.len()).map(|l| vec![0; (((cell >> l) * (cell >> l)) * 4) as usize]).collect();
    write_cell_local(&mut local, cell, size, chain);
    for (l, buf) in local.into_iter().enumerate() {
        let (w, s) = (width >> l, cell >> l);
        let (cx, cy) = (x >> l, y >> l);
        for row in 0..s {
            let d = (((cy + row) * w + cx) * 4) as usize;
            let src = (row * s * 4) as usize;
            levels[l][d..d + (s * 4) as usize].copy_from_slice(&buf[src..src + (s * 4) as usize]);
        }
    }
}

/// Cell-local pixels per level: upscale (nearest) while the level is larger than the native size,
/// then take the matching native mip; beyond the native chain the last level is repeated.
fn write_cell_local(levels: &mut [Vec<u8>], cell: u32, size: u32, chain: &[Vec<u32>]) {
    let up = cell.trailing_zeros() - size.trailing_zeros();
    for (l, buf) in levels.iter_mut().enumerate() {
        let s = cell >> l;
        let (src, ss) = if (l as u32) < up {
            (&chain[0], size)
        } else {
            let i = ((l as u32 - up) as usize).min(chain.len() - 1);
            (&chain[i], (size >> i).max(1))
        };
        for py in 0..s {
            for px in 0..s {
                let sx = (px as u64 * ss as u64 / s as u64) as u32;
                let sy = (py as u64 * ss as u64 / s as u64) as u32;
                let p = src[(sy.min(ss - 1) * ss + sx.min(ss - 1)) as usize];
                let o = ((py * s + px) * 4) as usize;
                buf[o] = (p >> 16) as u8;
                buf[o + 1] = (p >> 8) as u8;
                buf[o + 2] = p as u8;
                buf[o + 3] = (p >> 24) as u8;
            }
        }
    }
}

fn missing_texture() -> Vec<u32> {
    (0..256)
        .map(|i| {
            let (x, y) = (i % 16, i / 16);
            if (x < 8) ^ (y < 8) {
                0xFF00_0000
            } else {
                0xFFF8_00F8
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_mip_of_uniform_block_is_stable_and_alpha_path_drops_low_alpha() {
        let c = 0xFF80_4020;
        let b = blend_colors(c, c, c, c, false);
        assert!(((b >> 16) & 255).abs_diff(0x80) <= 1);
        // Alpha path: averaged alpha below 96 becomes fully transparent.
        assert_eq!(blend_colors(0x40FF_FFFF, 0, 0, 0, true) >> 24, 0);
        assert_eq!(blend_colors(0xFFFF_FFFF, 0, 0, 0, true) >> 24, 135);
    }
    #[test]
    fn chain_reaches_one_texel() {
        let chain = mip_chain(&vec![0xFFFF_FFFF; 256], 16, 4);
        assert_eq!(chain.iter().map(Vec::len).collect::<Vec<_>>(), vec![256, 64, 16, 4, 1]);
    }
}
