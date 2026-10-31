//! `play <event> [assets dir]`: plays a vanilla sound event through the mixer and reports timing.
fn main() {
    let ev = std::env::args().nth(1).unwrap_or_else(|| "random.orb".into());
    let dir = std::path::PathBuf::from(
        std::env::args().nth(2).unwrap_or_else(|| "MCP-919/.minecraft/assets".into()),
    );
    let read = mc_sound::manager::index_reader(&dir, "1.8").expect("asset index");
    let mut s = mc_sound::SoundManager::new(std::sync::Arc::new(read));
    println!("{} events, output available: {}", s.event_count(), s.mixer.available);
    let t = std::time::Instant::now();
    s.play(&ev, None, 1.0, 1.0);
    println!("queued after {:?}, voices {}", t.elapsed(), s.mixer.voice_count());
    while s.mixer.voice_count() > 0 && t.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    println!("finished after {:?}", t.elapsed());
}
