//! `--bench <seconds>`: flies a fixed, time-based camera path uncapped (vsync off, no frame limit)
//! and reports frame-time statistics. The path depends only on elapsed time, so runs are comparable
//! across builds even though frame counts differ.

use glam::Vec3;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

/// CPU phases a frame can be split into. `main.rs` wraps the matching code in `let _z = zone(Phase::X);`;
/// time is summed per frame and drained by [`Bench::frame`]. Unwired phases simply report nothing.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Phase {
    Net,
    Mesh,
    World,
    Entities,
    Gui,
    Acquire,
    Submit,
    Present,
    Update,
    Animation,
}
const PHASES: [&str; 10] = ["net", "mesh", "world", "entities", "gui", "acquire", "submit", "present", "update", "animation"];
static ACC_NS: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];

pub fn record(phase: Phase, elapsed: std::time::Duration) {
    ACC_NS[phase as usize].fetch_add(elapsed.as_nanos() as u64, Relaxed);
}

/// Times a scope into `phase` (and, with `--features profile`, opens a puffin scope).
#[allow(dead_code)]
pub struct Zone {
    phase: Phase,
    t: Instant,
    #[cfg(feature = "profile")]
    _scope: puffin::ProfilerScope,
}

#[allow(dead_code)]
pub fn zone(phase: Phase) -> Zone {
    Zone {
        phase,
        t: Instant::now(),
        #[cfg(feature = "profile")]
        _scope: {
            static IDS: [std::sync::OnceLock<puffin::ScopeId>; 10] = [const { std::sync::OnceLock::new() }; 10];
            let id = *IDS[phase as usize].get_or_init(|| puffin::ThreadProfiler::call(|tp| tp.register_named_scope(PHASES[phase as usize], "frame", file!(), line!())));
            puffin::ProfilerScope::new(id, "")
        },
    }
}

impl Drop for Zone {
    fn drop(&mut self) {
        record(self.phase, self.t.elapsed());
    }
}

/// Starts the puffin HTTP server (view with `puffin_viewer`) and marks frame boundaries. No-op without
/// the `profile` feature. Call once at startup, and `profile_frame()` once per frame.
#[allow(dead_code)]
pub fn profile_init() {
    #[cfg(feature = "profile")]
    {
        puffin::set_scopes_on(true);
        match puffin_http::Server::new("127.0.0.1:8585") {
            // Leaked on purpose: the server must live as long as the process.
            Ok(s) => std::mem::forget(s),
            Err(e) => eprintln!("profile: puffin server failed: {e}"),
        }
        println!("profile: puffin server on 127.0.0.1:8585 (run puffin_viewer)");
    }
}

#[allow(dead_code)]
pub fn profile_frame() {
    #[cfg(feature = "profile")]
    puffin::GlobalProfiler::lock().new_frame();
}

/// Frames in the first second are dropped: pipeline warm-up and the first uploads skew them.
const WARMUP_S: f64 = 1.0;
/// Radius of the circle the camera flies around the start point, in blocks.
const ORBIT: f32 = 24.0;

pub struct Bench {
    duration_s: f64,
    origin: Vec3,
    start: Option<Instant>,
    last: Option<(Instant, f64, [f64; 10])>,
    /// Callback-start interval paired with the work that began at that interval's start.
    samples: Vec<(f64, f64)>,
    /// Per measured frame: ms per [`Phase`] (all zero until main.rs wires the zones).
    phases: Vec<[f64; 10]>,
}

pub struct Pose {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl Bench {
    pub fn new(duration_s: f64, origin: Vec3) -> Self {
        Self { duration_s, origin, start: None, last: None, samples: Vec::with_capacity(1 << 16), phases: Vec::with_capacity(1 << 16) }
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
        let mut ph = [0.0; 10];
        for (p, a) in ph.iter_mut().zip(&ACC_NS) {
            *p = a.swap(0, Relaxed) as f64 / 1e6;
        }
        if let Some((last, last_cpu, last_ph)) = self.last {
            if (last - start).as_secs_f64() >= WARMUP_S {
                self.samples.push(((frame_start - last).as_secs_f64() * 1e3, last_cpu));
                self.phases.push(last_ph);
            }
        }
        self.last = Some((frame_start, cpu_ms, ph));
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
        println!(
            "bench: frame ms p50 {:.3} p99 {:.3} p99.9 {:.3} max {:.3}; cpu ms p50 {:.3} p99 {:.3} p99.9 {:.3} max {:.3}; hitches >16.7 ms: {}, >33 ms: {}",
            s.frame_pct[0], s.frame_pct[1], s.frame_pct[2], s.frame_pct[3], s.cpu_pct[0], s.cpu_pct[1], s.cpu_pct[2], s.cpu_pct[3], s.over16, s.over33,
        );
        let n = self.phases.len() as f64;
        let line: Vec<String> = PHASES
            .iter()
            .enumerate()
            .filter_map(|(i, name)| {
                let v: Vec<f64> = self.phases.iter().map(|p| p[i]).collect();
                let sum: f64 = v.iter().sum();
                let max = v.iter().cloned().fold(0.0, f64::max);
                (sum > 0.0).then(|| format!("{name} {:.3} (max {max:.2})", sum / n))
            })
            .collect();
        if !line.is_empty() {
            println!("bench: cpu phases avg ms: {}", line.join(", "));
        }
        let write = || -> std::io::Result<()> {
            let mut f = std::io::BufWriter::new(std::fs::File::create(csv)?);
            writeln!(f, "frame_ms,cpu_ms,{},outside_ms", PHASES.map(|n| format!("{n}_ms")).join(","))?;
            for ((frame, cpu), ph) in self.samples.iter().zip(&self.phases) {
                writeln!(f, "{frame:.4},{cpu:.4},{},{:.4}", ph.map(|v| format!("{v:.4}")).join(","), (frame - cpu).max(0.0))?;
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
    /// p50, p99, p99.9, max.
    frame_pct: [f64; 4],
    cpu_pct: [f64; 4],
    over16: usize,
    over33: usize,
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
        // Sorted descending, so the p-th percentile sits at index (1 - p) * n.
        let pct = |v: &[f64]| [0.5, 0.01, 0.001, 0.0].map(|top| v[((n as f64 * top) as usize).min(n - 1)]);
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
            frame_pct: pct(&frame),
            cpu_pct: pct(&cpu),
            over16: frame.iter().filter(|&&f| f > 16.7).count(),
            over33: frame.iter().filter(|&&f| f > 33.0).count(),
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

    #[test]
    fn frame_intervals_are_paired_with_the_work_that_preceded_them() {
        let t = Instant::now();
        let mut b = Bench::new(10.0, Vec3::ZERO);
        b.start = Some(t - std::time::Duration::from_secs(2));
        b.frame(t, 4.0);
        b.frame(t + std::time::Duration::from_millis(5), 0.2);
        b.frame(t + std::time::Duration::from_millis(6), 0.3);
        assert_eq!(b.samples, [(5.0, 4.0), (1.0, 0.2)]);
    }
}
