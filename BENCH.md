# Benchmarks

The default build is multiplayer-only, so `--bench` needs a world from a server: run it with `--connect` against the
mock server below (the commands in this file). Offline generated-terrain runs (`--radius N`) exist only in a
`--features singleplayer` build.

`rustmc --bench <s>` flies a fixed time-based camera path uncapped (vsync off) and prints avg FPS, 1%/0.1% lows,
frame/cpu percentiles (p50/p99/p99.9/max), hitch counts (>16.7 ms, >33 ms) and, once wired, per-phase CPU ms.
`bench.csv` gets one row per frame (`frame_ms,cpu_ms,net_ms,mesh_ms,world_ms,entities_ms,gui_ms,present_ms`).
Note `cpu_ms` is the whole `update + draw_frame`, so it **includes time blocked in `get_current_texture` / present**.

## Asynchronous mesh workers

Client remeshing uses a bounded persistent worker pool. Each job captures three vertical sections from each of the 3×3 neighboring columns, their biomes, heightmaps and nearby tile entities; revisions discard results invalidated by later changes or unloads. The main thread creates at most four snapshots per frame and performs GPU uploads. The current AO code can read two blocks past a face, so an 18³ snapshot would not preserve the current results without changing its sampling semantics. This is a larger snapshot than the R1 target, and incremental lighting still runs on the main thread.

A headless CPU-only synthetic terrain test (`RUSTMC_REQUIRE_GOLDEN=1 cargo test --release -p mc-client mesh_worker_headless_bench -- --ignored --nocapture`) meshed 169 sections twice on this shared Mac. The old blocking parallel call took 23.51 / 92.79 ms; the new queue took 56.73 / 95.15 ms wall time, with worst individual polls of 0.11 / 0.51 ms. The queue test sleeps 1 ms between polls, and both runs were affected by concurrent builds; these are not comparable frame rates or reliable throughput estimates. The test confirms matching index counts, not pixel parity. No before/after FPS, frame-time, full image-parity, or cross-platform GPU measurements have been made. The baseline below predates this change.

## Instanced particles

Particles retain their CPU-generated, sorted corners and per-corner UVs, but upload one 100-byte instance per quad; the vertex shader applies the camera transform. The previous path uploaded six 60-byte vertices per quad. `RUSTMC_NO_PARTICLE_INSTANCING=1` selects the old path for comparisons. Entity models still use CPU-expanded vertices.

On this shared Mac, two headless Metal release runs of `cargo test --release -p mc-render particle_headless_bench -- --ignored --nocapture` used 4,000 synthetic quads. Mean CPU encoding time was 0.482–0.999 ms for the old path and 0.154–0.517 ms for instancing; particle vertex uploads were 1,440,000 and 400,000 bytes/frame respectively. This excludes particle simulation, sorting, GPU wait and presentation, and concurrent activity makes the timings noisy. The off-screen pixel test compares both paths for overlapping particles, mixed world geometry and a 1,500-particle growth case; full-game visual parity and cross-platform performance remain unverified.

## Busy scenarios (mock server, `crates/mc-protocol/examples/mock_server.rs`)

```
cargo run --release -p mc-protocol --example mock_server -- 25599        # prefix with env vars below
rustmc --jar MCP-919/jars/versions/1.8.9/1.8.9.jar --connect 127.0.0.1:25599 --bench 10
```

| Env | Effect |
|---|---|
| `MOCK_CROWD=N` | N entities (alternating default-skin players and zombies) in a disc of radius ~30 around spawn, moving at 20 Hz, players swinging |
| `MOCK_STREAM=C` | C chunk columns/s (17 hilly 2-section chunks each) are unloaded then reloaded on x = 4..16 (chunk churn: net + decode + light + mesh + upload) |
| `MOCK_EDITS=N` | N random stone/air `BlockChange`s per tick (20 Hz) within 12 blocks of spawn |

The bench camera ignores server position, so `MOCK_STREAM` churns chunks inside the orbit instead of following the player.
The mock server only sends 8x8 = 64 chunks, so connected runs are cheap on world geometry; `--radius 8/16` offline
(generated terrain, `--features singleplayer` build only) were the world-heavy cases.

## Baseline (macOS, Apple M2, wgpu Metal, release, 10 s runs)

**Caveat: the machine was shared (load average 10-20 from other agents' cargo builds) during every run, and the
numbers swing 1.5-2x between identical runs.** Two passes are shown (A / B); treat avg FPS as +-50%. Detail columns are pass B.
The quiet-machine numbers are likely at the better end (A), so re-run on an idle Mac before trusting small deltas.

| Scenario | avg fps A / B | 1% low | 0.1% low | frame ms p50 / p99 / max | frames >16.7 ms |
|---|---|---|---|---|---|
| offline `--radius 8` (289 chunks, ~0.7-0.85 M quads) | 400 / 226 | 51.5 | 43.6 | 3.1 / 17.0 / 24.0 | 26 |
| offline `--radius 16` (1089 chunks, 1.9-3.4 M quads) | 208 / 118 | 51.8 | 37.4 | 8.1 / 16.4 / 28.2 | 9 |
| connected, plain mock (64 chunks) | 378 / 265 | 47.0 | 44.0 | 2.3 / 18.4 / 22.8 | 46 |
| `MOCK_MOBS=1` | 162 / 359 | 45.6 | 42.5 | 2.2 / 19.8 / 24.0 | 71 |
| `MOCK_PARTICLES=1` | 397 / 355 | 46.4 | 42.2 | 2.3 / 19.2 / 24.2 | 64 |
| `MOCK_CROWD=200` | 357 / 361 | 47.2 | 42.7 | 2.3 / 18.2 / 23.9 | 54 |
| `MOCK_CROWD=1000` | 421 / 368 | 48.6 | 44.7 | 2.3 / 18.7 / 22.7 | 51 |
| `MOCK_STREAM=4` | 404 / 315 | 48.4 | 43.7 | 2.4 / 19.2 / 23.4 | 49 |
| `MOCK_STREAM=16` | 289 / 270 | 43.4 | 34.5 | 2.4 / 20.8 / 30.1 | 84 |
| `MOCK_EDITS=20` | 246 / 325 | 48.3 | 44.1 | 2.3 / 19.5 / 23.1 | 57 |
| `MOCK_EDITS=200` | 240 / 328 | 46.4 | 43.3 | 2.2 / 19.8 / 23.5 | 55 |

No >33 ms frames in any steady-state run (earlier runs under heavier load had 3). Startup meshing blocks the main
thread: 0.24-0.46 s at radius 8, 0.84-1.4 s at radius 16 (`meshed N chunks` line).
Per-phase CPU columns are empty until main.rs calls the zone hooks (below).

Observations worth knowing:
- Every connected run has a recurring ~16-24 ms frame about 5x/s (p99 ~18 ms vs p50 ~2.3 ms), also present offline at radius 8.
  The samply profile of the plain connected run shows the main thread spending **67% inside `Surface::get_current_texture`
  -> `wgpu_hal::metal::Surface::acquire_texture`** (waiting on a CAMetalLayer drawable), so this is present/drawable throttling, not game logic.
- Quad count for the same radius differs between runs (r8: 853k, 694k; r16: 1.89M, 2.43M, 3.38M): meshing/lighting looks nondeterministic
  (probably sections meshed before neighbours/light are final). Worth a look by whoever owns meshing.

## CPU profile (samply, offline `--radius 16`, main thread, last 9 s, ~8400 samples)

`samply record --unstable-presymbolicate --save-only` then symbolicated offline (profile in scratch only).

| Inclusive | Function |
|---|---|
| 96% | `mc_render::gpu::Renderer::render` |
| 72% | `Renderer::encode` (self 7%) |
| 21% | `Queue::write_buffer` (per-frame uniform/vertex uploads) |
| 18% | `core::slice::sort::stable::driftsort` (a per-frame sort in encode; 12% self + 4.5% quicksort) |
| 17% | `wgpu RenderPass::drop` -> `render_pass_end` (replaying thousands of recorded draws) |
| 15% | `wgpu_hal::DynDevice::create_buffer` (per-frame buffer creation) |
| ~20% (self) | blocked: `semaphore_timedwait_trap` 10% + `mach_msg2_trap` 10% (drawable/present wait) |
| 7% self | `memmove` |

Plus `set_bind_group`, `set_index_buffer`, `Queue::submit` visible at ~1-2% self each. Mesh worker threads are idle in steady state.

## Suspected problems vs the numbers

- **One buffer + bind group + draw per section: confirmed.** At radius 16 the main thread is 72% in `encode` plus 17% in `render_pass_end`; cost scales with radius (4.7-8.5 ms/frame vs 2.5-4.4 at radius 8).
- **Per-frame GUI/dynamic buffer allocation: confirmed as a cost.** `create_buffer` 15% and `write_buffer` 21% of main-thread time every frame (cannot attribute to GUI vs other buffers without phase zones).
- **New: a per-frame sort is 18% of the frame** at radius 16 (likely sorting section draws / translucent geometry).
- **Blocking `mesh_parallel` on the main thread: confirmed for startup/remesh only** (0.8-1.4 s freeze at radius 16). Steady-state edit/stream scenarios show only mild hitch growth (stream16: 84 vs 46 hitches, p99.9 28 vs 22 ms); inconclusive under the machine noise.
- **CPU-side clip-space transforms for entities/particles: not confirmed at these scales.** 1000 crowd entities and 35 particle types are not measurably slower than the plain scene.
- **Single-threaded lighting: not measured separately** (startup only; offline startup is dominated by `mc_gen` terrain generation in the profile).

## Profiler feature

`cargo build --release -p mc-client --features profile` enables `puffin` 0.19 + `puffin_http` 0.16 (server on 127.0.0.1:8585, view with `puffin_viewer`).
Without the feature, `bench::zone`, `profile_init` and `profile_frame` cost only the phase timer / nothing.

## main.rs hooks still to add (main.rs was off-limits)

```rust
// fn main(), after arg parsing:
bench::profile_init();
// WindowEvent::RedrawRequested, right after `self.last_frame = now;`:
bench::profile_frame();
// fn update(): replace `self.process_net();` with
{ let _z = bench::zone(bench::Phase::Net); self.process_net(); }
// edit-path remesh (the `mesh_parallel(...)` + `r.upload_section` loop near "Remesh only the columns"), and remesh():
let _z = bench::zone(bench::Phase::Mesh);   // as first line of that block
// fn draw_frame(): wrap `let sr = self.build_gui();`
let sr = { let _z = bench::zone(bench::Phase::Gui); self.build_gui() };
// and around `r.render(...)`:
let _z = bench::zone(bench::Phase::World);  // = whole render incl. present until mc-render can report sub-phases
```

World vs entities vs present split needs the same zones inside `mc_render::gpu::Renderer::render` (needs a callback or moving `zone` into mc-render).

## Headless mob instancing (Metal)

The manual `particle_headless_bench` also measures 200 identical unarmored zombies. One release-mode run measured mean CPU preparation/encode of 1.270 ms with legacy vertices and 0.783 ms with mob instances. The measurement excludes simulation, GPU wait and presentation; it is a synthetic workload on a shared machine, not an FPS result. Mob model quads use 180-byte instances instead of six 60-byte vertices. Adjacent mob batches coalesce; held items and the additive eye pass retain their ordering. Enchanted armor falls back to legacy geometry because its glint relies on depth equality. Player models also use instances, while GUI previews, nonliving entities and tile entities still use the legacy path. Set `RUSTMC_NO_ENTITY_INSTANCING=1` for comparison. An offscreen Metal test compares pixels for a hurt zombie, overlapping spider eye layers, enchanted armor fallback, downloaded player skin, leather armor, and particles; other GPUs and full in-game scenes remain unverified.
