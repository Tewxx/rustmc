use mc_gen::Generator;
use mc_world::World;

#[test]
fn populate_counts() {
    let mut g = Generator::new(12345);
    let mut w = World::default();
    for cx in -4..=4 {
        for cz in -4..=4 {
            let mut c = g.provide_chunk(cx, cz);
            c.terrain_populated = true;
            w.insert(c);
        }
    }
    let before = count(&w);
    for cx in -4..4 {
        for cz in -4..4 {
            mc_gen::populate::populate(&mut g, &mut w, cx, cz);
        }
    }
    let after = count(&w);
    let mut diff: Vec<_> = after.iter().filter(|(k, v)| before.get(k) != Some(v)).map(|(k, v)| (*k, *before.get(k).unwrap_or(&0), *v)).collect();
    diff.sort();
    for (k, a, b) in &diff {
        println!("{:<28} {a:>7} -> {b:>7}", mc_data::Block::by_id(*k).map_or("?", |b| b.name));
    }
    let biomes: std::collections::BTreeSet<u8> = (-64..64).step_by(8).flat_map(|x| (-64..64).step_by(8).map(move |z| (x, z))).map(|(x, z)| w.biome_id(mc_core::BlockPos::new(x, 64, z))).collect();
    println!("biomes {biomes:?}");
}

fn count(w: &World) -> std::collections::HashMap<u16, usize> {
    let mut m = std::collections::HashMap::new();
    for c in w.chunks.values() {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    *m.entry(c.get(x, y, z).0).or_insert(0) += 1;
                }
            }
        }
    }
    m
}
