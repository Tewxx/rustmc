//! Run compilation and guest calls off the game thread. Frame/tick queues are bounded.
use crate::{draw::{self, Draw}, FrameCtx, KeyEvent, MouseEvent, ModInstance, Package, Snapshot};
use std::{path::Path, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc::{self, SyncSender}}, thread};

enum Input { Key(KeyEvent), Mouse(MouseEvent) }
enum Event { Tick(u64, Snapshot), Input(u64, Input, std::time::Instant, bool, Option<bool>), Frame }
enum Work { Tick(Snapshot), Input(Input, std::time::Instant, bool, Option<bool>), Frame(FrameCtx) }
struct Output { generation: u64, revision: u64, commands: Vec<Draw> }

pub struct ModWorker {
    tx: SyncSender<Event>,
    generation: Arc<AtomicU64>,
    output: Arc<Mutex<Output>>,
    overflow: Arc<AtomicBool>,
    frame_pending: Arc<AtomicBool>,
    frame_slot: Arc<Mutex<Option<(u64, FrameCtx)>>>,
    last_revision: u64,
    stale_frames: u8,
}
impl ModWorker {
    /// Start only if a package exists. All archive reads, compilation and guest calls run in the worker.
    pub fn start(dir: &Path) -> Option<Self> {
        let mut paths: Vec<_> = std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rmod"))).collect();
        if paths.is_empty() { return None; }
        paths.sort();
        let (tx, rx) = mpsc::sync_channel(256);
        let generation = Arc::new(AtomicU64::new(0));
        let output = Arc::new(Mutex::new(Output { generation: 0, revision: 0, commands: Vec::new() }));
        let overflow = Arc::new(AtomicBool::new(false));
        let worker_overflow = overflow.clone();
        let frame_pending = Arc::new(AtomicBool::new(false));
        let frame_slot = Arc::new(Mutex::new(None::<(u64, FrameCtx)>));
        let worker_frame_pending = frame_pending.clone();
        let worker_frame_slot = frame_slot.clone();
        let worker_generation = generation.clone();
        let worker_output = output.clone();
        thread::Builder::new().name("rustmc-mods".into()).spawn(move || {
            let mut ids = std::collections::HashSet::new();
            let mut mods = Vec::new();
            for path in paths {
                let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let p = Package::read(&path)?;
                    if !ids.insert(p.id.clone()) { anyhow::bail!("duplicate mod id {}", p.id); }
                    ModInstance::load(&p)
                }));
                match loaded { Ok(Ok(m)) => mods.push(m), Ok(Err(e)) => eprintln!("mod {}: {e:#}", path.display()), Err(_) => eprintln!("mod {}: host panic", path.display()) }
            }
            let mut active_generation = 0;
            while let Ok(event) = rx.recv() {
                if worker_overflow.load(Ordering::Acquire) {
                    eprintln!("WASM mods disabled: ordered event queue overflow");
                    break;
                }
                let event = match event {
                    Event::Tick(g, snapshot) => Some((g, Work::Tick(snapshot))),
                    Event::Input(g, input, at, screen, using) => Some((g, Work::Input(input, at, screen, using))),
                    Event::Frame => {
                        let frame = worker_frame_slot.lock().ok().and_then(|mut f| f.take());
                        worker_frame_pending.store(false, Ordering::Release);
                        frame.map(|(g, f)| (g, Work::Frame(f)))
                    }
                };
                let Some((event_generation, work)) = event else { continue; };
                if event_generation != worker_generation.load(Ordering::Acquire) { continue; }
                if event_generation != active_generation {
                    for m in &mut mods { m.reset_snapshot(); }
                    active_generation = event_generation;
                }
                match work {
                  Work::Tick(snapshot) => { mods.retain_mut(|m| {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.tick(snapshot.clone()))) {
                            Ok(Ok(())) => true, Ok(Err(e)) => { eprintln!("WASM tick: {e:#}"); false }, Err(_) => { eprintln!("WASM tick: host panic"); false }
                        }
                    }); },
                  Work::Input(input, at, screen, using) => { mods.retain_mut(|m| {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &input {
                            Input::Key(e) => m.key(e.clone(), at, screen, using),
                            Input::Mouse(e) => m.mouse(e.clone(), at, screen, using),
                        }));
                        match result { Ok(Ok(())) => true, Ok(Err(e)) => { eprintln!("WASM input: {e:#}"); false }, Err(_) => { eprintln!("WASM input: host panic"); false } }
                    }); },
                  Work::Frame(frame) => {
                        let gen = event_generation;
                        if gen != worker_generation.load(Ordering::Acquire) { continue; }
                        let mut commands = Vec::new();
                        mods.retain_mut(|m| {
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.render_hud(frame.clone()))) {
                                Ok(Ok(Some(bytes))) => { match draw::decode(&bytes) { Ok(mut d) if commands.len() + d.len() <= 16_384 => commands.append(&mut d), Ok(_) => eprintln!("WASM HUD command limit exceeded"), Err(e) => eprintln!("invalid WASM HUD: {e:#}") }; true },
                                Ok(Ok(None)) => true,
                                Ok(Err(e)) => { eprintln!("WASM HUD: {e:#}"); false }, Err(_) => { eprintln!("WASM HUD: host panic"); false }
                            }
                        });
                        if gen == worker_generation.load(Ordering::Acquire) {
                            if let Ok(mut out) = worker_output.lock() { out.generation = gen; out.revision = out.revision.wrapping_add(1); out.commands = commands; }
                        }
                  }
                }
            }
        }).ok()?;
        Some(Self { tx, generation, output, overflow, frame_pending, frame_slot, last_revision: 0, stale_frames: 0 })
    }
    fn ordered(&self, event: Event) {
        if matches!(self.tx.try_send(event), Err(mpsc::TrySendError::Full(_))) && !self.overflow.swap(true, Ordering::AcqRel) {
            // Losing an ordered event corrupts guest state; retire the worker instead.
            self.clear();
        }
    }
    pub fn tick(&self, snapshot: Snapshot) { self.ordered(Event::Tick(self.generation.load(Ordering::Acquire), snapshot)); }
    pub fn key(&self, event: KeyEvent, screen: bool, using: Option<bool>) {
        self.ordered(Event::Input(self.generation.load(Ordering::Acquire), Input::Key(event), std::time::Instant::now(), screen, using));
    }
    pub fn mouse(&self, event: MouseEvent, screen: bool, using: Option<bool>) {
        self.ordered(Event::Input(self.generation.load(Ordering::Acquire), Input::Mouse(event), std::time::Instant::now(), screen, using));
    }
    pub fn frame(&self, frame: FrameCtx) {
        let Ok(mut slot) = self.frame_slot.try_lock() else { return };
        *slot = Some((self.generation.load(Ordering::Acquire), frame));
        if !self.frame_pending.swap(true, Ordering::AcqRel) && self.tx.try_send(Event::Frame).is_err() {
            self.frame_pending.store(false, Ordering::Release);
        }
    }
    /// Invalidate queued output when the world changes; old in-flight calls cannot republish it.
    pub fn clear(&self) { self.generation.fetch_add(1, Ordering::AcqRel); }
    /// Never blocks rendering; stale output expires after three frames without a completed call.
    pub fn latest(&mut self) -> Vec<Draw> {
        let Ok(out) = self.output.try_lock() else { return Vec::new() };
        if out.generation != self.generation.load(Ordering::Acquire) { return Vec::new(); }
        if out.revision != self.last_revision { self.last_revision = out.revision; self.stale_frames = 0; }
        else { self.stale_frames = self.stale_frames.saturating_add(1); }
        if self.stale_frames > 3 { Vec::new() } else { out.commands.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn frames_coalesce_without_filling_the_tick_queue() {
        let (tx, rx) = mpsc::sync_channel(4);
        let worker = ModWorker { tx, generation: Arc::new(AtomicU64::new(0)), output: Arc::new(Mutex::new(Output { generation: 0, revision: 0, commands: vec![] })), overflow: Arc::new(AtomicBool::new(false)), frame_pending: Arc::new(AtomicBool::new(false)), frame_slot: Arc::new(Mutex::new(None)), last_revision: 0, stale_frames: 0 };
        let mut frame = FrameCtx { partial_ticks: 0., fps: 1, gui_width: 1, gui_height: 1, gui_scale: 1, mouse_x: 0, mouse_y: 0, cam: crate::rustmc::plugin::types::Vec3 { x: 0., y: 0., z: 0. }, cam_yaw: 0., cam_pitch: 0., fov: 70. };
        worker.frame(frame.clone()); frame.fps = 2; worker.frame(frame);
        assert!(matches!(rx.try_recv(), Ok(Event::Frame)));
        assert!(rx.try_recv().is_err());
        assert_eq!(worker.frame_slot.lock().unwrap().as_ref().unwrap().1.fps, 2);
    }

    #[test]
    fn tick_queue_overflow_invalidates_output_and_disables_worker() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let mut worker = ModWorker { tx, generation: Arc::new(AtomicU64::new(0)), output: Arc::new(Mutex::new(Output { generation: 0, revision: 1, commands: vec![Draw::Rect { left: 0, top: 0, right: 1, bottom: 1, argb: 0 }] })), overflow: Arc::new(AtomicBool::new(false)), frame_pending: Arc::new(AtomicBool::new(false)), frame_slot: Arc::new(Mutex::new(None)), last_revision: 0, stale_frames: 0 };
        assert_eq!(worker.latest().len(), 1);
        worker.tick(Snapshot::default());
        worker.tick(Snapshot::default());
        assert!(worker.overflow.load(Ordering::Acquire));
        assert!(worker.latest().is_empty());
    }

    #[test]
    fn worker_runs_package_and_invalidates_world_output() {
        let dir = std::env::temp_dir().join(format!("rustmc-mod-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join("test.rmod")).unwrap();
        let mut z = zip::ZipWriter::new(file);
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("mod.toml", o).unwrap();
        z.write_all(b"[mod]\nid='test'\nname='Test'\nversion='1.0'\nentry='mod.wasm'\nside='client'\n[api]\nversion='1.0'\nmin='1.0'\n[capabilities]\nhud=true\n").unwrap();
        z.start_file("mod.wasm", o).unwrap(); z.write_all(include_bytes!("../tests/data/guest.wasm")).unwrap(); z.finish().unwrap();
        let mut worker = ModWorker::start(&dir).unwrap();
        let frame = FrameCtx { partial_ticks: 0., fps: 60, gui_width: 854, gui_height: 480, gui_scale: 1, mouse_x: 0, mouse_y: 0, cam: crate::rustmc::plugin::types::Vec3 { x: 0., y: 0., z: 0. }, cam_yaw: 0., cam_pitch: 0., fov: 70. };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            worker.frame(frame.clone());
            if !worker.latest().is_empty() { break; }
            assert!(std::time::Instant::now() < deadline, "worker failed to produce a HUD");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        worker.clear();
        assert!(worker.latest().is_empty(), "old world output must be discarded immediately");
        drop(worker);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
