//! Decodes every file given on stdin (one path per line) and reports failures and total time.
use std::io::BufRead;
fn main() {
    let t = std::time::Instant::now();
    let (mut ok, mut bad, mut frames) = (0, 0, 0usize);
    for line in std::io::stdin().lock().lines() {
        let p = line.unwrap();
        match mc_sound::vorbis::decode(&std::fs::read(&p).unwrap()) {
            Ok(d) => {
                ok += 1;
                frames += d.samples.len() / d.channels.max(1);
            }
            Err(e) => {
                bad += 1;
                println!("{p}: {e}");
            }
        }
    }
    println!("{ok} ok, {bad} failed, {frames} frames in {:?}", t.elapsed());
}
