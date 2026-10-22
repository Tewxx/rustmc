//! Byte-for-byte comparisons against vanilla dumps made by `tools/oracle/run.sh`.
//! Each test is skipped when its dump is missing (the oracle needs Java 8 and the MCP classes).

use mc_gen::Generator;
use std::path::PathBuf;

fn dump(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/oracle").join(name);
    match std::fs::read(&p) {
        Ok(v) => Some(v),
        Err(_) => {
            eprintln!("skipping: no oracle dump {}", p.display());
            None
        }
    }
}

fn be_i32(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// b.bin: `biomes 12345 -40 -40 80 80`
#[test]
fn biome_layers_match_vanilla() {
    let Some(d) = dump("b.bin") else { return };
    let mut g = Generator::new(12345);
    let gen = g.biomes_for_generation(-40, -40, 80, 80);
    let idx = g.biomes_at(-40, -40, 80, 80);
    let n = 80 * 80;
    let mut bad = 0;
    for i in 0..n {
        let want = be_i32(&d[i * 4..]);
        if gen[i] != want {
            if bad < 10 {
                eprintln!("genBiomes[{i}] ({}, {}) = {} want {}", i % 80, i / 80, gen[i], want);
            }
            bad += 1;
        }
    }
    for i in 0..n {
        let want = be_i32(&d[(n + i) * 4..]);
        if idx[i] != want {
            if bad < 20 {
                eprintln!("biomeIndex[{i}] ({}, {}) = {} want {}", i % 80, i / 80, idx[i], want);
            }
            bad += 1;
        }
    }
    assert_eq!(bad, 0, "{bad} biome mismatches");
}

const CHUNK_BYTES: usize = 8 + 65536 * 2 + 256;

fn compare_chunks(file: &str, seed: i64) {
    let Some(d) = dump(file) else { return };
    let mut g = Generator::new(seed);
    eprintln!("{file}: {} chunks", d.len() / CHUNK_BYTES);
    let mut total_bad = 0;
    for rec in d.chunks(CHUNK_BYTES) {
        let (cx, cz) = (be_i32(rec), be_i32(&rec[4..]));
        let c = g.provide_chunk(cx, cz);
        let mut bad = 0;
        for i in 0..65536usize {
            let want = u16::from_be_bytes([rec[8 + i * 2], rec[9 + i * 2]]);
            let (x, z, y) = (i & 15, (i >> 4) & 15, (i >> 8) as i32);
            let (id, meta) = c.get(x, y, z);
            let got = id << 4 | meta as u16;
            if got != want {
                if bad < 5 {
                    eprintln!("chunk {cx},{cz} block {x},{y},{z}: got {}:{} want {}:{}", got >> 4, got & 15, want >> 4, want & 15);
                }
                bad += 1;
            }
        }
        for i in 0..256 {
            if c.biomes[i] != rec[8 + 131072 + i] {
                if bad < 8 {
                    eprintln!("chunk {cx},{cz} biome {i}: got {} want {}", c.biomes[i], rec[8 + 131072 + i]);
                }
                bad += 1;
            }
        }
        total_bad += bad;
    }
    assert_eq!(total_bad, 0, "{total_bad} block/biome mismatches");
}

/// c.bin: `chunks 12345 -3 -3 3 3`
#[test]
fn provided_chunks_match_vanilla() {
    compare_chunks("c.bin", 12345);
}

/// c2.bin: `chunks -4172144997902289642 300 -520 340 -480` (wide, many biomes)
#[test]
fn provided_chunks_match_vanilla_far() {
    compare_chunks("c2.bin", -4172144997902289642);
}

/// c3.bin: `chunks 7 0 -245 24 -215` (mesa, bryce, plateaus)
#[test]
fn provided_chunks_match_vanilla_mesa() {
    compare_chunks("c3.bin", 7);
}

/// c4.bin: `chunks 1 160 -260 180 -240` (swamp)
#[test]
fn provided_chunks_match_vanilla_swamp() {
    compare_chunks("c4.bin", 1);
}
