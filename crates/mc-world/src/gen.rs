//! Tiny deterministic terrain for when no 1.8 world is available. Not vanilla worldgen.

use crate::chunk::Chunk;

fn hash(x: i32, z: i32, salt: i64) -> u64 {
    let mut h = (x as i64).wrapping_mul(341873128712) ^ (z as i64).wrapping_mul(132897987541) ^ salt;
    h ^= h >> 13;
    h = h.wrapping_mul(0x5DEECE66D);
    (h ^ (h >> 17)) as u64
}

fn height(x: i32, z: i32) -> i32 {
    let (fx, fz) = (x as f64, z as f64);
    let h = 66.0 + 5.0 * (fx * 0.07).sin() * (fz * 0.05).cos() + 3.0 * ((fx + fz) * 0.11).sin() + 2.0 * (fz * 0.19 - fx * 0.03).cos();
    h as i32
}

pub fn generate(cx: i32, cz: i32) -> Chunk {
    let mut c = Chunk::new(cx, cz);
    for lx in 0..16 {
        for lz in 0..16 {
            let (x, z) = (cx * 16 + lx as i32, cz * 16 + lz as i32);
            let h = height(x, z);
            c.set(lx, 0, lz, 7, 0);
            for y in 1..h {
                let r = hash(x, z, y as i64) % 200;
                let (id, meta) = if y < h - 4 {
                    match r {
                        0..=2 => (16, 0),
                        3 => (15, 0),
                        4..=9 => (1, [1u8, 3, 5][(r % 3) as usize]),
                        _ => (1, 0),
                    }
                } else {
                    (3, 0)
                };
                c.set(lx, y, lz, id, meta);
            }
            c.set(lx, h, lz, 2, 0);
            match hash(x, z, 99) % 100 {
                0..=11 => c.set(lx, h + 1, lz, 31, 1),
                12 => c.set(lx, h + 1, lz, 37, 0),
                13 => c.set(lx, h + 1, lz, 38, (hash(x, z, 7) % 9) as u8),
                14 => c.set(lx, h + 1, lz, 31, 2),
                _ => {}
            }
        }
    }
    // Trees, kept inside the chunk so leaves never cross a border.
    for t in 0..3 {
        let r = hash(cx, cz, 1000 + t);
        let (lx, lz) = (2 + (r % 12) as usize, 2 + ((r >> 8) % 12) as usize);
        let wood = if (r >> 16) % 3 == 0 { 2 } else { 0 };
        let base = height(cx * 16 + lx as i32, cz * 16 + lz as i32) + 1;
        let top = base + 4 + ((r >> 20) % 2) as i32;
        for y in top - 3..=top + 1 {
            let rad: i32 = if y >= top { 1 } else { 2 };
            for dx in -rad..=rad {
                for dz in -rad..=rad {
                    if rad == 2 && dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    let (x, z) = (lx as i32 + dx, lz as i32 + dz);
                    if (0..16).contains(&x) && (0..16).contains(&z) && c.get(x as usize, y, z as usize).0 == 0 {
                        c.set(x as usize, y, z as usize, 18, wood);
                    }
                }
            }
        }
        for y in base..top {
            c.set(lx, y, lz, 17, wood);
        }
        c.set(lx, base - 1, lz, 3, 0);
    }
    c
}
