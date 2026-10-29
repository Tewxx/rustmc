//! `play <event>`: plays a vanilla sound event through the mixer and reports timing.
fn main() {
    let ev = std::env::args().nth(1).unwrap_or_else(|| "random.orb".into());
    let dir = std::env::var_os("APPDATA")
        .map(|a| std::path::PathBuf::from(a).join(".minecraft"))
        .or_else(|| std::env::var_os("HOME").map(|h| {
            let h = std::path::PathBuf::from(h);
            if cfg!(target_os = "macos") { h.join("Library/Application Support/minecraft") } else { h.join(".minecraft") }
        }))
        .expect("no APPDATA/HOME")
        .join("assets");
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
