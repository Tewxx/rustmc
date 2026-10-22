//! `decode <in.ogg> <out.f32>`: raw interleaved f32 for comparing against a reference decoder.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = mc_sound::vorbis::decode(&std::fs::read(&a[1]).unwrap()).unwrap();
    let bytes: Vec<u8> = d.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&a[2], bytes).unwrap();
    println!("{} ch {} Hz {} frames", d.channels, d.rate, d.samples.len() / d.channels);
}
