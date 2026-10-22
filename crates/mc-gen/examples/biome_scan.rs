fn main() {
    let seed: i64 = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(12345);
    let mut g = mc_gen::Generator::new(seed);
    let mut first = std::collections::BTreeMap::new();
    let r = 1024;
    let v = g.biomes_for_generation(-r, -r, 2 * r, 2 * r);
    for (i, b) in v.iter().enumerate() {
        let x = (i as i32 % (2 * r) - r) * 4;
        let z = (i as i32 / (2 * r) - r) * 4;
        first.entry(*b).or_insert((x >> 4, z >> 4, 0)).2 += 1;
    }
    for (b, (cx, cz, n)) in first {
        println!("{b:3} chunk {cx},{cz} count {n}");
    }
}
