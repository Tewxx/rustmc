//! `--bench <seconds>`: flies a fixed, time-based camera path uncapped (vsync off, no frame limit)
//! and reports frame-time statistics. The path depends only on elapsed time, so runs are comparable
//! across builds even though frame counts differ.

use glam::Vec3;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

/// Frames in the first second are dropped: pipeline warm-up and the first uploads skew them.
const WARMUP_S: f64 = 1.0;
/// Radius of the circle the camera flies around the start point, in blocks.
const ORBIT: f32 = 24.0;

pub struct Bench {
    duration_s: f64,
    origin: Vec3,
    start: Option<Instant>,
    last: Option<Instant>,
    /// Per measured frame: (interval since the previous frame, CPU time in update + draw), in ms.
    samples: Vec<(f64, f64)>,
}

pub struct Pose {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl Bench {
    pub fn new(duration_s: f64, origin: Vec3) -> Self {
        Self { duration_s, origin, start: None, last: None, samples: Vec::with_capacity(1 << 16) }
    }

    /// Camera pose for this frame: two full yaw sweeps while orbiting and bobbing in height.
    pub fn pose(&mut self, now: Instant) -> Pose {
        let start = *self.start.get_or_insert(now);
        let t = ((now - start).as_secs_f64() / self.duration_s.max(1e-3)) as f32;
        let a = t * std::f32::consts::TAU;
        Pose {
            pos: self.origin + Vec3::new(ORBIT * a.cos() - ORBIT, 4.0 * (2.0 * a).sin(), ORBIT * a.sin()),
            yaw: -45.0 + 720.0 * t,
            pitch: 15.0 + 20.0 * (3.0 * a).sin(),
        }
    }

    /// Records a finished frame. Returns true once the run is over.
    pub fn frame(&mut self, frame_start: Instant, cpu_ms: f64) -> bool {
        let start = *self.start.get_or_insert(frame_start);
        if let Some(last) = self.last {
            if (frame_start - start).as_secs_f64() >= WARMUP_S {
                self.samples.push(((frame_start - last).as_secs_f64() * 1e3, cpu_ms));
            }
        }
        self.last = Some(frame_start);
        (frame_start - start).as_secs_f64() >= self.duration_s + WARMUP_S
    }

    /// Prints the one-line summary and writes per-frame samples to `csv`.
    pub fn finish(&self, csv: &Path, gpu: &str, chunks: usize) {
        if self.samples.is_empty() {
            eprintln!("bench: no frames recorded");
            return;
        }
        let s = Stats::of(&self.samples);
        println!(
            "bench: {:.0} s, {} frames, avg {:.1} fps, 1% low {:.1} fps, 0.1% low {:.1} fps, frame {:.3} ms (p99 {:.3}), cpu {:.3} ms (p99 {:.3}), {chunks} chunks, {gpu}",
            self.duration_s, s.frames, s.avg_fps, s.low1_fps, s.low01_fps, s.avg_frame_ms, s.p99_frame_ms, s.avg_cpu_ms, s.p99_cpu_ms,
        );
        let write = || -> std::io::Result<()> {
            let mut f = std::io::BufWriter::new(std::fs::File::create(csv)?);
            writeln!(f, "frame_ms,cpu_ms")?;
            for (frame, cpu) in &self.samples {
                writeln!(f, "{frame:.4},{cpu:.4}")?;
            }
            f.flush()
        };
        match write() {
            Ok(()) => println!("bench: wrote {}", csv.display()),
            Err(e) => eprintln!("bench: failed to write {}: {e}", csv.display()),
        }
    }
}

struct Stats {
    frames: usize,
    avg_fps: f64,
    low1_fps: f64,
    low01_fps: f64,
    avg_frame_ms: f64,
    p99_frame_ms: f64,
    avg_cpu_ms: f64,
    p99_cpu_ms: f64,
}

impl Stats {
    /// "x% low" is the average FPS over the slowest x% of frames (at least one frame).
    fn of(samples: &[(f64, f64)]) -> Stats {
        let mut frame: Vec<f64> = samples.iter().map(|s| s.0).collect();
        let mut cpu: Vec<f64> = samples.iter().map(|s| s.1).collect();
        frame.sort_by(|a, b| b.total_cmp(a));
        cpu.sort_by(|a, b| b.total_cmp(a));
        let n = frame.len();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let slowest = |pct: f64| 1e3 / mean(&frame[..((n as f64 * pct).ceil() as usize).clamp(1, n)]);
        let p99 = |v: &[f64]| v[((n as f64 * 0.01) as usize).min(n - 1)];
        let avg_frame_ms = mean(&frame);
        Stats {
            frames: n,
            avg_fps: 1e3 / avg_frame_ms,
            low1_fps: slowest(0.01),
            low01_fps: slowest(0.001),
            avg_frame_ms,
            p99_frame_ms: p99(&frame),
            avg_cpu_ms: mean(&cpu),
            p99_cpu_ms: p99(&cpu),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lows_average_the_slowest_frames() {
        // 999 frames at 1 ms and one at 11 ms.
        let mut v = vec![(1.0, 0.5); 999];
        v.push((11.0, 0.5));
        let s = Stats::of(&v);
        assert_eq!(s.frames, 1000);
        assert!((s.avg_fps - 1e3 / 1.01).abs() < 1e-9);
        assert!((s.low01_fps - 1e3 / 11.0).abs() < 1e-9);
        assert!((s.low1_fps - 1e3 / 2.0).abs() < 1e-9);
    }

    #[test]
    fn path_is_a_function_of_time() {
        let mut b = Bench::new(10.0, Vec3::ZERO);
        let t0 = Instant::now();
        let p0 = b.pose(t0);
        assert_eq!(p0.pos, Vec3::ZERO);
        let p = b.pose(t0 + std::time::Duration::from_secs(5));
        assert!((p.yaw - 315.0).abs() < 1e-3);
    }
}
