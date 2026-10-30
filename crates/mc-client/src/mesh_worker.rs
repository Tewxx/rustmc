//! Bounded asynchronous section meshing. The main thread snapshots and owns the world;
//! workers only read immutable snapshots and never touch the GPU.
use crate::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Mutex};

pub(crate) type Key = (i32, i32, i32);
struct Job { key: Key, revision: u64, world: World, smooth: bool, fancy: bool }
struct Done { key: Key, revision: u64, mesh: Option<mesh::ChunkMesh> }

pub(crate) struct MeshWorkers {
    tx: mpsc::SyncSender<Job>,
    rx: mpsc::Receiver<Done>,
    revisions: HashMap<Key, u64>,
    pending: HashMap<Key, u64>,
    order: VecDeque<Key>,
    queued: HashSet<Key>,
    next_revision: u64,
    in_flight: usize,
    capacity: usize,
}

impl MeshWorkers {
    pub(crate) fn new(models: &BlockModels) -> Self {
        let count = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
        let capacity = count * 2;
        let (tx, rx) = mpsc::sync_channel::<Job>(capacity);
        let rx = Arc::new(Mutex::new(rx));
        let (done_tx, done_rx) = mpsc::channel();
        let models = Arc::new(models.mesh_snapshot());
        for i in 0..count {
            let (rx, tx, models) = (rx.clone(), done_tx.clone(), models.clone());
            std::thread::Builder::new().name(format!("section-mesh-{i}")).spawn(move || {
                loop {
                    let job = match rx.lock().unwrap().recv() { Ok(job) => job, Err(_) => break };
                    let (x, sy, z) = job.key;
                    let mut geometry = BlockGeometry::new();
                    let mesh = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                        mesh::build_section(&job.world, &models, x, sy, z, job.smooth, job.fancy, &mut geometry))).ok();
                    if tx.send(Done { key: job.key, revision: job.revision, mesh }).is_err() { break; }
                }
            }).expect("spawn mesh worker");
        }
        Self { tx, rx: done_rx, revisions: HashMap::new(), pending: HashMap::new(), order: VecDeque::new(), queued: HashSet::new(), next_revision: 0, in_flight: 0, capacity }
    }

    pub(crate) fn invalidate(&mut self, key: Key, schedule: bool) {
        self.next_revision = self.next_revision.wrapping_add(1);
        let rev = self.next_revision;
        if schedule {
            self.revisions.insert(key, rev);
            self.pending.insert(key, rev);
            if self.queued.insert(key) { self.order.push_back(key); }
        } else {
            self.revisions.remove(&key);
            self.pending.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.next_revision = self.next_revision.wrapping_add(1);
        self.revisions.clear(); self.pending.clear(); self.order.clear(); self.queued.clear();
    }

    pub(crate) fn poll(&mut self, world: &World, smooth: bool, fancy: bool) -> Vec<(Key, mesh::ChunkMesh)> {
        let mut ready = Vec::new();
        while let Ok(done) = self.rx.try_recv() {
            self.in_flight -= 1;
            if self.revisions.get(&done.key) == Some(&done.revision) && world.is_chunk_loaded(done.key.0, done.key.2) {
                if let Some(mesh) = done.mesh { ready.push((done.key, mesh)); }
                else { eprintln!("mesh worker failed for {:?}", done.key); }
            }
        }
        let mut dispatched = 0;
        let mut examined = 0;
        while self.in_flight < self.capacity && dispatched < 4 && examined < 128 {
            let Some(key) = self.order.pop_front() else { break };
            self.queued.remove(&key);
            examined += 1;
            let Some(revision) = self.pending.remove(&key) else { continue };
            if !world.is_chunk_loaded(key.0, key.2) { continue; }
            let job = Job { key, revision, world: world.mesh_snapshot(key.0, key.1, key.2), smooth, fancy };
            match self.tx.try_send(job) {
                Ok(()) => { self.in_flight += 1; dispatched += 1; },
                Err(mpsc::TrySendError::Full(_)) => { self.pending.insert(key, revision); self.queued.insert(key); self.order.push_front(key); break; }
                Err(mpsc::TrySendError::Disconnected(_)) => { eprintln!("mesh workers disconnected"); break; }
            }
        }
        ready
    }
}

impl App {
    pub(crate) fn schedule_meshes(&mut self, keys: Vec<Key>) {
        if self.renderer.is_none() { return; }
        let cx = (self.camera.pos.x.floor() as i32) >> 4;
        let cz = (self.camera.pos.z.floor() as i32) >> 4;
        let (mut nonempty, empty): (Vec<_>, Vec<_>) = keys.into_iter().partition(|&(x, sy, z)|
            self.world.chunks.get(&(x, z)).and_then(|c| c.sections[sy as usize].as_ref()).is_some());
        if let Some(pool) = &mut self.mesh_workers { for &key in &empty { pool.invalidate(key, false); } }
        if let Some(renderer) = &mut self.renderer {
            for &(x, sy, z) in &nonempty { renderer.invalidate_section_visibility(x, sy, z); }
            let mesh = mesh::ChunkMesh::default();
            for (x, sy, z) in empty { renderer.upload_section(x, sy, z, &mesh); }
        }
        if !nonempty.is_empty() {
            nonempty.sort_by_key(|&(x, _, z)| (x as i64 - cx as i64).abs() + (z as i64 - cz as i64).abs());
            let pool = self.mesh_workers.get_or_insert_with(|| MeshWorkers::new(&self.models));
            for key in nonempty { pool.invalidate(key, true); }
        }
    }

    pub(crate) fn poll_meshes(&mut self) {
        let Some(pool) = &mut self.mesh_workers else { return };
        let ready = pool.poll(&self.world, self.ui.settings.ambient_occlusion != 0, self.ui.settings.fancy_graphics);
        if let Some(renderer) = &mut self.renderer {
            for ((x, sy, z), mesh) in ready { renderer.upload_section(x, sy, z, &mesh); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_world::chunk::Chunk;

    fn models() -> Option<BlockModels> {
        let Some(jar) = crate::find_jar() else {
            assert_ne!(std::env::var("RUSTMC_REQUIRE_GOLDEN").as_deref(), Ok("1"), "mesh tests need the vanilla jar");
            return None;
        };
        let mut assets = AssetSource::new(); assets.push(&jar).unwrap();
        Some(BlockModels::load(&assets))
    }

    fn world() -> World {
        let mut world = World::default();
        for cx in -1..=1 { for cz in -1..=1 {
            let mut c = Chunk::new(cx, cz);
            for &(x,y,z,id,meta) in &[(0,31,0,1,0),(15,32,15,2,0),(0,32,15,9,0),
                (15,33,0,53,0),(8,34,8,20,0),(0,47,0,85,0),(15,48,15,9,1)] {
                c.set(x,y,z,id,meta);
            }
            if cx == 0 && cz == 0 {
                c.set(2, 33, 2, 140, 0);
                c.tile_entities.insert(BlockPos::new(2, 33, 2), [
                    ("id".into(), mc_core::nbt::Tag::String("FlowerPot".into())),
                    ("Item".into(), mc_core::nbt::Tag::String("minecraft:tallgrass".into())),
                    ("Data".into(), mc_core::nbt::Tag::Int(2)),
                ].into());
            }
            c.biomes.fill(if (cx + cz) % 2 == 0 { 1 } else { 2 });
            c.generate_height_map();
            world.insert(c);
        }}
        world
    }

    fn equal_mesh(a: &mesh::ChunkMesh, b: &mesh::ChunkMesh) {
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.visibility, b.visibility);
        let bits = |m: &mesh::ChunkMesh| m.vertices.iter().map(|v| (v.pos.map(f32::to_bits), v.uv.map(f32::to_bits), v.color, v.light)).collect::<Vec<_>>();
        assert_eq!(bits(a), bits(b));
    }

    #[test]
    fn section_snapshot_matches_full_world_and_is_immutable() {
        let Some(models) = models() else { return };
        let mut world = world();
        for key in [(0, 1, 0), (0, 2, 0), (0, 3, 0), (-1, 2, -1)] {
            let snapshot = world.mesh_snapshot(key.0, key.1, key.2);
            let mut a = BlockGeometry::new(); let mut b = BlockGeometry::new();
            let full = mesh::build_section(&world, &models, key.0, key.1, key.2, true, true, &mut a);
            let compact = mesh::build_section(&snapshot, &models.mesh_snapshot(), key.0, key.1, key.2, true, true, &mut b);
            equal_mesh(&full, &compact);
        }
        let snapshot = world.mesh_snapshot(0, 2, 0);
        let before = snapshot.get(15, 32, 15);
        world.chunks.get_mut(&(0, 0)).unwrap().set(15, 32, 15, 4, 0);
        assert_eq!(snapshot.get(15, 32, 15), before);
        assert_ne!(world.get(15, 32, 15), before);
    }

    /// Run with `cargo test --release -p mc-client mesh_worker_headless_bench -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual headless CPU benchmark"]
    fn mesh_worker_headless_bench() {
        let models = models().expect("benchmark needs the vanilla jar");
        let mut world = World::default();
        let mut keys = Vec::new();
        for cx in -6..=6 { for cz in -6..=6 {
            let mut c = Chunk::new(cx, cz);
            for x in 0..16 { for z in 0..16 {
                for y in 32..38 { c.set(x, y, z, 1, 0); }
                c.set(x, 38, z, 2, 0);
                if (x + z + (cx * cx + cz * cz) as usize) % 11 == 0 { c.set(x, 39, z, 31, 1); }
            }}
            c.generate_height_map(); world.insert(c); keys.push((cx, 2, cz));
        }}
        let t = std::time::Instant::now();
        let baseline = crate::world_load::mesh_parallel(&world, &models, &keys, true, true);
        let blocking = t.elapsed();
        let expected: usize = baseline.iter().map(|(_, m)| m.indices.len()).sum();
        let mut workers = MeshWorkers::new(&models);
        for &key in &keys { workers.invalidate(key, true); }
        let start = std::time::Instant::now(); let mut worst = std::time::Duration::ZERO;
        let mut count = 0; let mut indices = 0;
        while count < keys.len() && start.elapsed() < std::time::Duration::from_secs(60) {
            let t = std::time::Instant::now();
            let ready = workers.poll(&world, true, true);
            worst = worst.max(t.elapsed());
            count += ready.len(); indices += ready.iter().map(|(_, m)| m.indices.len()).sum::<usize>();
            if count < keys.len() { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        let elapsed = start.elapsed();
        assert_eq!(count, keys.len()); assert_eq!(indices, expected);
        eprintln!("headless mesh CPU: {} sections, blocking {:.2} ms, asynchronous wall {:.2} ms, worst poll {:.2} ms (no GPU/present)",
            count, blocking.as_secs_f64()*1000., elapsed.as_secs_f64()*1000., worst.as_secs_f64()*1000.);
    }

    #[test]
    fn repeated_unload_reload_coalesces_queue_entries() {
        let Some(models) = models() else { return };
        let mut workers = MeshWorkers::new(&models);
        let key = (0, 2, 0);
        for _ in 0..10_000 {
            workers.invalidate(key, true);
            workers.invalidate(key, false);
        }
        workers.invalidate(key, true);
        assert_eq!(workers.order.len(), 1);
        assert_eq!(workers.pending.len(), 1);
        workers.clear();
        assert!(workers.order.is_empty() && workers.queued.is_empty() && workers.pending.is_empty());
    }

    #[test]
    fn stale_results_are_discarded_after_edit_and_reload() {
        let Some(models) = models() else { return };
        let mut world = world();
        let key = (0, 2, 0);
        let mut workers = MeshWorkers::new(&models);
        workers.invalidate(key, true);
        assert!(workers.poll(&world, true, true).is_empty());
        world.remove(0, 0);
        workers.invalidate(key, false);
        let mut replacement = Chunk::new(0, 0);
        replacement.set(4, 32, 4, 4, 0);
        replacement.generate_height_map();
        world.insert(replacement);
        workers.invalidate(key, true);
        let mut geometry = BlockGeometry::new();
        let expected = mesh::build_section(&world, &models, 0, 2, 0, true, true, &mut geometry);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut got = None;
        while std::time::Instant::now() < end {
            for (k, result) in workers.poll(&world, true, true) {
                assert_eq!(k, key);
                assert!(got.is_none(), "stale result was returned");
                got = Some(result);
            }
            if workers.in_flight == 0 && workers.pending.is_empty() { break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        equal_mesh(&got.expect("worker timed out"), &expected);
        assert_eq!(workers.in_flight, 0);
    }
}
