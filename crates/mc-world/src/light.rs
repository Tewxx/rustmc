//! Sky and block light for chunks that have none from disk.
//!
//! Sky light starts from `Chunk.generateSkylightMap` (straight down from 15, losing
//! `max(opacity, 1)` per block once it has hit anything), then both light types spread
//! like `World.checkLightFor`: a neighbour gets `light - max(1, opacity)`. The result is
//! vanilla's steady state; the update order that vanilla uses doesn't change it.

use crate::blocks;
use crate::chunk::{column_index, Chunk};
use crate::World;
use std::collections::{HashMap, VecDeque};

/// Which light array to read or write.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sky,
    Block,
}

/// Lights every chunk whose `light_valid` is false. Neighbouring loaded chunks take
/// part so light flows across borders.
pub fn relight(world: &mut World) {
    relight_impl(world, false);
}

fn relight_impl(world: &mut World, seed_all: bool) {
    let dirty: Vec<(i32, i32)> = world.chunks.iter().filter(|(_, c)| !c.light_valid).map(|(k, _)| *k).collect();
    if dirty.is_empty() {
        return;
    }
    // Per-chunk opacity/emission lookup tables, so the flood fill does no table lookups.
    let mut opacity: HashMap<(i32, i32), Box<[u8]>> = HashMap::new();
    for (&k, c) in &world.chunks {
        opacity.insert(k, opacity_map(c));
    }

    let mut queue = VecDeque::new();
    for &(cx, cz) in &dirty {
        let c = world.chunks.get_mut(&(cx, cz)).unwrap();
        let op = &opacity[&(cx, cz)];
        c.sky_light.fill(0);
        c.block_light.fill(0);
        for z in 0..16 {
            for x in 0..16 {
                // generateSkylightMap
                let mut k: i32 = 15;
                for y in (0..256).rev() {
                    let i = column_index(x, y, z);
                    let mut o = opacity_of(op[i]);
                    if o == 0 && k != 15 {
                        o = 1;
                    }
                    k -= o;
                    if k <= 0 {
                        break;
                    }
                    c.sky_light[i] = k as u8;
                }
                for y in 0..256 {
                    let i = column_index(x, y, z);
                    let e = emission_of(op[i]);
                    if e > 0 {
                        c.block_light[i] = e;
                        queue.push_back((cx * 16 + x as i32, y, cz * 16 + z as i32, Kind::Block));
                    }
                }
            }
        }
    }
    // Sky seeds: only cells next to something darker than they could light. Open air
    // above the terrain is already final, which skips most of the column volume.
    for &(cx, cz) in &dirty {
        for z in 0..16 {
            for x in 0..16 {
                for y in 0..256 {
                    let (wx, wz) = (cx * 16 + x, cz * 16 + z);
                    let s = get(world, Kind::Sky, wx, y, wz);
                    if s <= 1 {
                        continue;
                    }
                    let spreads = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
                        .iter()
                        .any(|&(dx, dy, dz)| (0..256).contains(&(y + dy)) && get(world, Kind::Sky, wx + dx, y + dy, wz + dz) + 1 < s);
                    if spreads || seed_all {
                        queue.push_back((wx, y, wz, Kind::Sky));
                    }
                }
            }
        }
    }

    while let Some((x, y, z, kind)) = queue.pop_front() {
        let light = get(world, kind, x, y, z);
        if light <= 1 {
            continue;
        }
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if !(0..256).contains(&ny) {
                continue;
            }
            let key = (nx >> 4, nz >> 4);
            let Some(op) = opacity.get(&key) else { continue };
            let i = column_index((nx & 15) as usize, ny, (nz & 15) as usize);
            let o = opacity_of(op[i]).max(1);
            let new = light as i32 - o;
            if new <= 0 {
                continue;
            }
            let c = world.chunks.get_mut(&key).unwrap();
            let slot = match kind {
                Kind::Sky => &mut c.sky_light[i],
                Kind::Block => &mut c.block_light[i],
            };
            if (*slot as i32) < new {
                *slot = new as u8;
                queue.push_back((nx, ny, nz, kind));
            }
        }
    }

    for k in dirty {
        world.chunks.get_mut(&k).unwrap().light_valid = true;
    }
}

/// Packs opacity (capped at 15) in the low nibble and emission in the high nibble.
fn opacity_map(c: &Chunk) -> Box<[u8]> {
    let mut out = vec![0u8; 65536].into_boxed_slice();
    for y in 0..256 {
        for z in 0..16 {
            for x in 0..16 {
                let (id, meta) = c.get(x, y, z);
                if id == 0 {
                    continue;
                }
                let p = blocks::props(id, meta);
                out[column_index(x, y, z)] = p.opacity.min(15) | (p.emission.min(15) << 4);
            }
        }
    }
    out
}

fn opacity_of(v: u8) -> i32 {
    (v & 15) as i32
}

fn emission_of(v: u8) -> u8 {
    v >> 4
}

fn get(world: &World, kind: Kind, x: i32, y: i32, z: i32) -> u8 {
    let Some(c) = world.chunks.get(&(x >> 4, z >> 4)) else { return 0 };
    let i = column_index((x & 15) as usize, y, (z & 15) as usize);
    match kind {
        Kind::Sky => c.sky_light[i],
        Kind::Block => c.block_light[i],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_sky_is_full_and_caves_are_dark() {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..64 {
                    c.set(x, y, z, 1, 0);
                }
            }
        }
        // A sealed one-block pocket.
        c.set(8, 30, 8, 0, 0);
        w.insert(c);
        relight(&mut w);
        assert_eq!(w.light(5, 64, 5), (15, 0));
        assert_eq!(w.light(8, 30, 8), (0, 0));
    }

    #[test]
    fn light_spreads_under_an_overhang() {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                c.set(x, 63, z, 1, 0);
            }
        }
        // Roof over x 0..8 at y 66; under it light comes in sideways and drops 1 per block.
        for x in 0..8 {
            for z in 0..16 {
                c.set(x, 66, z, 1, 0);
            }
        }
        w.insert(c);
        relight(&mut w);
        assert_eq!(w.light(8, 64, 5).0, 15);
        assert_eq!(w.light(7, 64, 5).0, 14);
        assert_eq!(w.light(5, 64, 5).0, 12);
    }

    #[test]
    fn edge_seeding_matches_seeding_everything() {
        let build = || {
            let mut w = World::default();
            for cx in -1..=1 {
                for cz in -1..=1 {
                    w.insert(crate::gen::generate(cx, cz));
                }
            }
            w
        };
        let (mut fast, mut full) = (build(), build());
        relight_impl(&mut fast, false);
        relight_impl(&mut full, true);
        for (k, c) in &full.chunks {
            let f = &fast.chunks[k];
            for i in 0..c.sky_light.len() {
                assert_eq!(f.sky_light[i], c.sky_light[i], "chunk {k:?} index {i}");
            }
        }
    }

    #[test]
    fn glowstone_emits() {
        let mut w = World::default();
        let mut c = Chunk::new(0, 0);
        c.set(8, 100, 8, 89, 0);
        w.insert(c);
        relight(&mut w);
        assert_eq!(w.light(9, 100, 8).1, 14);
        assert_eq!(w.light(12, 100, 8).1, 11);
    }
}
