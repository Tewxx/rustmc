//! Bounded asynchronous section meshing. The main thread snapshots and owns the world;
//! workers only read immutable snapshots and never touch the GPU.
use crate::*;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub(crate) type Key = (i32, i32, i32);
struct Job { key: Key, revision: u64, current: Arc<AtomicU64>, world: World, smooth: bool, fancy: bool, scratch: mesh::ChunkMesh }
pub(crate) struct ReadyMesh { pub mesh: mesh::ChunkMesh, pub upload: mesh::PreparedSection }
struct Done { key: Key, revision: u64, mesh: Option<ReadyMesh> }

/// Sections uploaded per poll (`RenderGlobal.updateChunks` also bounds uploads per frame).
const UPLOAD_SECTIONS: usize = 4;
/// Arena bytes uploaded per poll.
const UPLOAD_BYTES: usize = 4 * 1024 * 1024;
/// Dispatch throttle while mesh jobs are already in flight; idle workers get a full refill.
const DISPATCH_BUSY: usize = 4;
/// Queue entries inspected when picking the nearest section. `schedule_meshes` appends batches in
/// distance order, so the nearest queued sections are at the front.
const NEAREST_WINDOW: usize = 64;

/// Squared block distance from the camera to a section's centre (`RenderChunk` distance).
fn section_distance_sq((x, sy, z): Key, center: [i32; 3]) -> i64 {
    let dx = (x as i64) * 16 + 8 - center[0] as i64;
    let dy = (sy as i64) * 16 + 8 - center[1] as i64;
    let dz = (z as i64) * 16 + 8 - center[2] as i64;
    dx * dx + dy * dy + dz * dz
}

fn camera_block(pos: [f32; 3]) -> [i32; 3] {
    [pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32]
}

/// Index of the queued key nearest to `center` among the first [`NEAREST_WINDOW`] entries.
fn nearest_index(order: &VecDeque<Key>, center: [i32; 3]) -> Option<usize> {
    let mut best: Option<(usize, i64)> = None;
    for (i, &key) in order.iter().take(NEAREST_WINDOW).enumerate() {
        let d = section_distance_sq(key, center);
        if best.is_none_or(|(_, best)| d < best) { best = Some((i, d)); }
    }
    best.map(|(i, _)| i)
}

pub(crate) struct MeshWorkers {
    tx: mpsc::SyncSender<Job>,
    rx: mpsc::Receiver<Done>,
    revisions: HashMap<Key, (u64, Arc<AtomicU64>)>,
    pending: HashMap<Key, u64>,
    order: VecDeque<Key>,
    queued: HashSet<Key>,
    next_revision: u64,
    in_flight: usize,
    capacity: usize,
    ready: VecDeque<Done>,
    recycled: Vec<mesh::ChunkMesh>,
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
                let mut geometry = BlockGeometry::new();
                loop {
                    let job = match rx.lock().unwrap().recv() { Ok(job) => job, Err(_) => break };
                    let (x, sy, z) = job.key;
                    let mesh = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if job.current.load(Relaxed) != job.revision { return None; }
                        geometry.reset();
                        let mesh = mesh::build_section_reusing(&job.world, &models, x, sy, z, job.smooth, job.fancy, &mut geometry, job.scratch);
                        if job.current.load(Relaxed) != job.revision { return None; }
                        let upload = mesh::PreparedSection::new(&mesh, sy);
                        Some(ReadyMesh { mesh, upload })
                    })).ok().flatten();
                    if tx.send(Done { key: job.key, revision: job.revision, mesh }).is_err() { break; }
                }
            }).expect("spawn mesh worker");
        }
        Self { tx, rx: done_rx, revisions: HashMap::new(), pending: HashMap::new(), order: VecDeque::new(), queued: HashSet::new(), next_revision: 0, in_flight: 0, capacity, ready: VecDeque::new(), recycled: Vec::new() }
    }

    pub(crate) fn invalidate(&mut self, key: Key, schedule: bool) {
        // A job that is queued but not yet dispatched snapshots the world when a worker picks it
        // up, so a repeated dirty mark before then is already covered: keep one queued revision
        // instead of restarting the same section. In-flight jobs still get a new revision so their
        // stale result is dropped and the section is meshed again.
        if schedule && self.pending.contains_key(&key) {
            return;
        }
        self.next_revision = self.next_revision.wrapping_add(1);
        let rev = self.next_revision;
        if schedule {
            let entry = self.revisions.entry(key).or_insert_with(|| (rev, Arc::new(AtomicU64::new(rev))));
            entry.0 = rev; entry.1.store(rev, Relaxed);
            self.pending.insert(key, rev);
            if self.queued.insert(key) { self.order.push_back(key); }
        } else {
            if let Some((_, current)) = self.revisions.remove(&key) { current.store(rev, Relaxed); }
            self.pending.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.next_revision = self.next_revision.wrapping_add(1);
        for (_, current) in self.revisions.values() { current.store(self.next_revision, Relaxed); }
        self.revisions.clear(); self.pending.clear(); self.order.clear(); self.queued.clear();
    }

    fn recycle(&mut self, mut mesh: mesh::ChunkMesh) {
        mesh.vertices.clear(); mesh.indices.clear();
        if self.recycled.len() < self.capacity { self.recycled.push(mesh); }
    }

    /// Drop queued keys whose revision was withdrawn while they waited (an unload or a section
    /// that became empty), so they cannot occupy the dispatch window. `pending` and `order` only
    /// differ by those tombstones.
    fn compact_order(&mut self) {
        if self.order.len() == self.pending.len() { return; }
        let pending = &self.pending;
        self.order.retain(|key| pending.contains_key(key));
        self.queued = self.order.iter().copied().collect();
    }

    /// Pop the queued section nearest to the camera. `RenderGlobal.setupTerrain` fills
    /// `chunksToUpdate` from a BFS outward from the camera's chunk, so dispatch follows distance.
    fn take_nearest(&mut self, center: [i32; 3]) -> Option<Key> {
        let i = nearest_index(&self.order, center)?;
        let key = self.order.remove(i)?;
        self.queued.remove(&key);
        Some(key)
    }

    pub(crate) fn poll(&mut self, world: &World, center: [i32; 3], smooth: bool, fancy: bool) -> Vec<(Key, ReadyMesh)> {
        let mut ready = Vec::new();
        while let Ok(done) = self.rx.try_recv() {
            self.in_flight -= 1;
            self.ready.push_back(done);
        }
        let mut bytes = 0;
        while let Some(done) = self.ready.pop_front() {
            if self.revisions.get(&done.key).is_some_and(|(rev, _)| *rev == done.revision) && world.is_chunk_loaded(done.key.0, done.key.2) {
                if let Some(mesh) = done.mesh {
                    bytes += mesh.upload.upload_bytes(&mesh.mesh);
                    ready.push((done.key, mesh));
                    if bytes >= UPLOAD_BYTES || ready.len() >= UPLOAD_SECTIONS { break; }
                }
                else { eprintln!("mesh worker failed for {:?}", done.key); }
            } else if let Some(mesh) = done.mesh { self.recycle(mesh.mesh); }
        }
        self.compact_order();
        let mut dispatched = 0;
        // `RenderGlobal.updateChunks` drains `chunksToUpdate` until its frame deadline. Hold the
        // four-at-a-time throttle while workers are busy, but refill the whole pool in one poll
        // when every worker is idle (a fresh queue after a teleport or a big edit burst).
        let limit = if self.in_flight == 0 { self.capacity } else { DISPATCH_BUSY };
        while self.in_flight + self.ready.len() < self.capacity && dispatched < limit {
            let Some(key) = self.take_nearest(center) else { break };
            let Some(revision) = self.pending.remove(&key) else { continue };
            if !world.is_chunk_loaded(key.0, key.2) { continue; }
            let scratch = self.recycled.pop().unwrap_or_default();
            let current = self.revisions[&key].1.clone();
            let job = Job { key, revision, current, world: world.mesh_snapshot(key.0, key.1, key.2), smooth, fancy, scratch };
            match self.tx.try_send(job) {
                Ok(()) => { self.in_flight += 1; dispatched += 1; },
                Err(mpsc::TrySendError::Full(job)) => {
                    self.pending.insert(job.key, job.revision);
                    self.queued.insert(job.key);
                    self.order.push_front(job.key);
                    self.recycle(job.scratch);
                    break;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => { eprintln!("mesh workers disconnected"); break; }
            }
        }
        ready
    }
}

impl App {
    pub(crate) fn schedule_meshes(&mut self, keys: Vec<Key>) {
        if self.renderer.is_none() { return; }
        // Dirty marks are section keys; overlapping block/light ranges can repeat one. Collapse
        // them before scheduling so one section never occupies the queue twice.
        let mut seen = HashSet::with_capacity(keys.len());
        let keys: Vec<Key> = keys.into_iter().filter(|key| seen.insert(*key)).collect();
        let center = camera_block(self.camera.pos.to_array());
        let (mut nonempty, empty): (Vec<_>, Vec<_>) = keys.into_iter().partition(|&(x, sy, z)|
            self.world.chunks.get(&(x, z)).and_then(|c| c.sections[sy as usize].as_ref()).is_some());
        if let Some(pool) = &mut self.mesh_workers { for &key in &empty { pool.invalidate(key, false); } }
        if let Some(renderer) = &mut self.renderer {
            for &(x, sy, z) in &nonempty { renderer.invalidate_section_visibility(x, sy, z); }
            let mesh = mesh::ChunkMesh::default();
            for (x, sy, z) in empty { renderer.upload_section(x, sy, z, &mesh); }
        }
        if !nonempty.is_empty() {
            // Append in camera-distance order so `poll` always finds the nearest work at the front
            // of its queue (`RenderGlobal.setupTerrain`'s BFS from the camera's chunk).
            nonempty.sort_unstable_by_key(|&key| section_distance_sq(key, center));
            let pool = self.mesh_workers.get_or_insert_with(|| MeshWorkers::new(&self.models));
            for key in nonempty { pool.invalidate(key, true); }
        }
    }

    pub(crate) fn poll_meshes(&mut self) {
        let _z = crate::bench::zone(crate::bench::Phase::Mesh);
        let Some(pool) = &mut self.mesh_workers else { return };
        let center = camera_block(self.camera.pos.to_array());
        let ready = pool.poll(&self.world, center, self.ui.settings.ambient_occlusion != 0, self.ui.settings.fancy_graphics);
        if let Some(renderer) = &mut self.renderer {
            for ((x, sy, z), mesh) in ready {
                renderer.upload_prepared_section(x, sy, z, &mesh.mesh, mesh.upload);
                pool.recycle(mesh.mesh);
            }
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

    /// Terrain columns with opaque sections, missing upper sections (so light falls back to the
    /// height map) and one tile entity per column, like a real chunk after `handleChunkData`.
    fn bench_world(radius: i32) -> World {
        let mut world = World::default();
        for cx in -radius..=radius {
            for cz in -radius..=radius {
                let mut c = Chunk::new(cx, cz);
                for x in 0..16 {
                    for z in 0..16 {
                        c.set(x, 32, z, 1, 0);
                        c.set(x, 31, z, 3, 0);
                        if (x + z) % 5 == 0 { c.set(x, 33, z, 17, 0); }
                    }
                }
                c.biomes.fill(((cx + cz) & 3) as u8 + 1);
                c.tile_entities.insert(BlockPos::new(cx * 16 + 3, 33, cz * 16 + 3), [
                    ("id".into(), mc_core::nbt::Tag::String("FlowerPot".into())),
                    ("Item".into(), mc_core::nbt::Tag::String("minecraft:tallgrass".into())),
                    ("Data".into(), mc_core::nbt::Tag::Int(2)),
                ].into());
                c.generate_height_map();
                world.insert(c);
            }
        }
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
            let ready = workers.poll(&world, [0, 40, 0], true, true);
            worst = worst.max(t.elapsed());
            count += ready.len(); indices += ready.iter().map(|(_, m)| m.mesh.indices.len()).sum::<usize>();
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
    fn repeated_marks_while_queued_keep_one_revision() {
        let Some(models) = models() else { return };
        let mut workers = MeshWorkers::new(&models);
        let key = (0, 2, 0);
        workers.invalidate(key, true);
        let revision = workers.revisions[&key].0;
        for _ in 0..1000 {
            workers.invalidate(key, true);
        }
        assert_eq!(workers.order.len(), 1);
        assert_eq!(workers.pending.len(), 1);
        assert_eq!(workers.pending[&key], revision);
        assert_eq!(workers.revisions[&key].0, revision);

        // Once poll dispatches the job (it removes it from the queue and pending), a further
        // dirty mark must invalidate the in-flight result and schedule the section again.
        let _ = workers.order.pop_front();
        workers.queued.remove(&key);
        workers.pending.remove(&key);
        workers.invalidate(key, true);
        assert!(workers.revisions[&key].0 > revision);
        assert_eq!(workers.order.len(), 1);
        assert_eq!(workers.pending.len(), 1);
    }

    #[test]
    fn stale_results_are_discarded_after_edit_and_reload() {
        let Some(models) = models() else { return };
        let mut world = world();
        let key = (0, 2, 0);
        let mut workers = MeshWorkers::new(&models);
        workers.invalidate(key, true);
        assert!(workers.poll(&world, [0, 40, 0], true, true).is_empty());
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
            for (k, result) in workers.poll(&world, [0, 40, 0], true, true) {
                assert_eq!(k, key);
                assert!(got.is_none(), "stale result was returned");
                got = Some(result.mesh);
            }
            if workers.in_flight == 0 && workers.pending.is_empty() { break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        equal_mesh(&got.expect("worker timed out"), &expected);
        assert_eq!(workers.in_flight, 0);
    }

    /// A block in the neighbouring column or the section above must reach the mesher through the
    /// snapshot: both hide a face of a block in the meshed section.
    #[test]
    fn neighbour_blocks_cull_faces_from_a_snapshot() {
        let Some(models) = models() else { return };
        let build = |with_neighbours: bool| {
            let mut world = World::default();
            let mut c = Chunk::new(0, 0);
            c.set(15, 40, 8, 1, 0); // east face sits on the column border
            c.set(15, 47, 8, 1, 0); // top face sits on the section border
            if with_neighbours { c.set(15, 48, 8, 1, 0); }
            c.generate_height_map();
            world.insert(c);
            if with_neighbours {
                let mut east = Chunk::new(1, 0);
                east.set(0, 40, 8, 4, 0);
                east.generate_height_map();
                world.insert(east);
            }
            world
        };
        let models = models.mesh_snapshot();
        let mut geometry = BlockGeometry::new();
        let with = build(true);
        let without = build(false);
        let hidden = mesh::build_section(&with, &models, 0, 2, 0, true, true, &mut geometry);
        let exposed = mesh::build_section(&without, &models, 0, 2, 0, true, true, &mut geometry);
        assert!(!exposed.vertices.is_empty());
        assert!(hidden.indices.len() < exposed.indices.len(), "neighbour faces were not culled");

        for world in [&with, &without] {
            let snapshot = world.mesh_snapshot(0, 2, 0);
            let mut a = BlockGeometry::new(); let mut b = BlockGeometry::new();
            let full = mesh::build_section(world, &models, 0, 2, 0, true, true, &mut a);
            let shared = mesh::build_section(&snapshot, &models, 0, 2, 0, true, true, &mut b);
            equal_mesh(&full, &shared);
        }
    }

    #[test]
    fn dispatch_picks_the_nearest_queued_section() {
        let order = VecDeque::from([(4, 2, 0), (-3, 2, 0), (1, 2, 1), (0, 2, 0)]);
        assert_eq!(nearest_index(&order, [0, 40, 0]), Some(3));
        assert_eq!(nearest_index(&order, [24, 40, 24]), Some(2));

        // Vertical distance counts too: section 2 is below the camera, section 3 above it.
        let vertical = VecDeque::from([(0, 3, 0), (0, 2, 0)]);
        assert_eq!(nearest_index(&vertical, [8, 8, 8]), Some(1));
        assert_eq!(nearest_index(&vertical, [8, 72, 8]), Some(0));

        // Only the window is inspected; a far key cannot jump ahead of the batch in front.
        let mut windowed: VecDeque<Key> = (0..NEAREST_WINDOW as i32 + 1).map(|i| (100 + i, 2, 0)).collect();
        windowed.push_back((0, 2, 0));
        assert_eq!(nearest_index(&windowed, [8, 40, 8]), Some(0));
        assert_eq!(nearest_index(&VecDeque::new(), [0, 40, 0]), None);
    }

    #[test]
    fn withdrawn_sections_do_not_block_the_dispatch_window() {
        let Some(models) = models() else { return };
        let mut workers = MeshWorkers::new(&models);
        for key in [(0, 2, 0), (1, 2, 0), (2, 2, 0)] { workers.invalidate(key, true); }
        workers.invalidate((1, 2, 0), false);
        assert_eq!(workers.order.len(), 3);
        workers.compact_order();
        assert_eq!(workers.order, VecDeque::from([(0, 2, 0), (2, 2, 0)]));
        assert_eq!(workers.queued.len(), 2);
        assert!(!workers.queued.contains(&(1, 2, 0)));
        // Re-dirtying the withdrawn section queues it again.
        workers.invalidate((1, 2, 0), true);
        assert_eq!(workers.order.len(), 3);
    }

    /// Run with `cargo test --release -p mc-client mesh_snapshot_bench -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual CPU benchmark"]
    fn mesh_snapshot_bench() {
        let world = bench_world(8);
        let mut keys = Vec::new();
        for cx in -8..=8 { for cz in -8..=8 { for sy in 0..4 { keys.push((cx, sy, cz)); } } }
        // Warm the caches/tables the snapshot walks.
        for _ in 0..64 { for &(cx, sy, cz) in &keys { std::hint::black_box(world.mesh_snapshot(cx, sy, cz)); } }
        let rounds = 50;
        let t = std::time::Instant::now();
        for _ in 0..rounds { for &(cx, sy, cz) in &keys { std::hint::black_box(world.mesh_snapshot(cx, sy, cz)); } }
        let elapsed = t.elapsed();
        let per = elapsed.as_secs_f64() / (rounds * keys.len()) as f64;
        eprintln!("mesh_snapshot: {} columns, {} snapshot/round, {:.3} us/snapshot, {:.1} ms per {} dispatches",
            world.chunks.len(), keys.len(), per * 1e6, per * 16. * 1e3, 16);
    }
}
