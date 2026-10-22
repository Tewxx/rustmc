# rustmc — handoff

A Minecraft 1.8.9 client in Rust. Goal: replace PvP clients (Lunar etc.) with something that **looks and
feels 1:1 vanilla** but runs with **much higher FPS and steadier frame times**.

Reference source: `../MCP-919/src/minecraft/` (decompiled 1.8.9, MCP names). It is a reference only —
never copy it into this repo. Assets load at runtime from the user's `1.8.9.jar`.

---

## Current state (works today)

| Area | Status | Where |
|---|---|---|
| Core math (`MathHelper`, AABB, Vec3, BlockPos, EnumFacing, NBT) | Bit-exact ports, tested | `crates/mc-core` |
| Assets: layered packs, blockstates, models, baking, atlas, colormaps | Done | `crates/mc-assets` |
| World: 1.8 chunks, Anvil loader, demo terrain, ~80-block id table | Done | `crates/mc-world` |
| Sky/block light (vanilla steady state, edge-seeded BFS) | Done, single-threaded | `mc-world/src/light.rs` |
| Smooth lighting + AO (`BlockModelRenderer` port), lightmap | Done | `mc-render/src/ao.rs`, `lightmap.rs` |
| Chunk meshing | Done, **single-threaded, full rebuild only** | `mc-render/src/mesh.rs` |
| wgpu world renderer | One draw per chunk, CPU frustum culling | `mc-render/src/gpu.rs` |
| GUI: font, HUD, F3, chat, title/pause/options/video/controls | Done | `crates/mc-gui`, `mc-render/src/gui_gpu.rs` |
| Client: state machine, input, settings (`options.txt`) | Done (fly camera, no player) | `crates/mc-client/src/main.rs` |

Run: `cargo run --release --bin rustmc -- --radius 8`
Screenshot any screen headless: `rustmc --screenshot out.png --screen title|pause|options|video|controls|hud|debug|chat`
Tests: `cargo test --release` (41 passing; `mc-render/tests/vanilla_jar.rs` runs against the real jar,
`mc-world/tests/golden_blocks.rs` against `golden/blocks.json`; both skip when their input is missing).
Benchmark: `rustmc --radius 8 --bench 10` → one-line summary + `bench.csv` (vsync/limit overridden in memory only).
Golden data: `powershell -ExecutionPolicy Bypass -File tools/extract/extract.ps1` (needs a JDK 8 and MCP's
compiled `bin/minecraft`; writes `golden/`, which is gitignored because it's derived from Mojang code).

**The plan now lives in `ROADMAP.md`** (segments S0–S13). The performance sections below are scheduled
there as S8/S12; read the roadmap first.

Measured (RTX 3060, radius 8 = 289 chunks, 732k quads):
- Startup: light **1.66 s**, mesh **2.6 s** (both single-threaded) — too slow.
- FPS: not benchmarked against Lunar yet (see Phase P0).

---

## NEXT: Performance (priority)

Order matters: measure first, then fix the CPU side (threads), then cut per-frame GPU/driver cost.
Every change must keep output **pixel-identical** (use the screenshot tool to diff before/after).

### P0 — Benchmark harness (do first)
- Add `--bench <seconds>`: fixed camera path (scripted yaw sweep + flight), uncapped, vsync off.
  Log avg FPS, 1% low, 0.1% low, CPU frame time, GPU frame time (wgpu timestamp queries).
- Print a one-line summary + write `bench.csv`. Run it before/after each phase below.
- Add a `tracy` (or `puffin`) profiler feature flag for zone timing.
- Done when: we have baseline numbers and can compare to Lunar at the same render distance.

### P1 — Multithreaded meshing + lighting
- Add `rayon`. Mesh chunks in parallel: `mesh::build` only reads `World` + `BlockModels` → already safe
  to share by `&`. Use `par_iter` over chunk keys; upload on the main thread.
- **Section-level meshing** (16×16×16) instead of whole columns: smaller rebuilds, better culling.
- **Fast block access in the mesher:** build a padded 18×18×18 snapshot of block ids/light per section
  before meshing (no HashMap lookups per neighbour — this is most of today's 2.6 s).
- Lighting: parallelize the column fill per chunk; run the BFS per region with a work queue, or move to
  section-batched propagation. Keep `edge_seeding_matches_seeding_everything` test green.
- **Background worker pool** with a priority queue (closest-to-camera first) so meshing never blocks a
  frame; results come back over a channel and upload incrementally (cap uploads/frame).
- Dirty tracking: `World::set_block` marks the section + neighbours dirty → rebuild only those
  (needed anyway for block breaking/placing).
- Done when: radius 8 loads in < 300 ms, block edits rebuild in < 2 ms, no frame hitch on load.

### P2 — GPU-side draw submission
- **One vertex/index arena** (a few large buffers, sub-allocated per section) instead of a buffer +
  bind group per chunk. Chunk origin moves into a per-draw instance attribute / storage buffer.
- **Compact vertex** (today 28 bytes): pack position as u16 ×3 relative to section (1/2048 block
  precision), UV as u16 ×2 (atlas index), color u8×4, light u8×2 → ~16–20 bytes. Halves bandwidth.
- **`multi_draw_indexed_indirect`** (wgpu `MULTI_DRAW_INDIRECT` feature, fallback to a loop):
  one call per render layer for all visible sections.
- **Per-face buckets**: split each section mesh by facing (6 buckets + "any"); skip buckets facing away
  from the camera (big win: ~40–50% fewer triangles).
- Done when: per-frame CPU time for world submission < 0.1 ms at radius 16.

### P3 — Culling
- **Cave culling**: port `VisGraph` / `RenderChunk.setCompiledChunk` visibility (which faces of a
  section connect) and the BFS in `RenderGlobal.setupTerrain`. Huge underground and in hilly terrain.
- **GPU frustum + occlusion culling** (compute pass writing the indirect buffer; optional Hi-Z occlusion
  from last frame's depth).
- Done when: underground / behind-hill views draw a small fraction of sections.

### P4 — Frame pacing & latency (the "feel" part of performance)
- Uncapped mode: `PresentMode::Immediate`, `desired_maximum_frame_latency: 1`.
- Read input as late as possible before encoding; raw mouse (`DeviceEvent::MouseMotion`, already used).
- Replace `thread::sleep` FPS limiter with a precise spin-sleep limiter.
- Avoid per-frame allocations: reuse the GUI vertex buffer (`gui_gpu.rs` creates one per frame today),
  reuse `GuiBatch` vectors, and keep a persistent staging belt for uploads.
- Done when: 0.1% lows are within ~25% of average FPS in the bench.

### P5 — Busy-scene performance (do once entities exist)
- Instanced entity rendering (all models from a data table in one storage buffer).
- Batched nametags and text (one atlas, one draw).
- GPU particles.
- This is where Java clients drop hardest — expect the largest wins here.

**Realistic targets** (vs Lunar, same scene/settings): 2–4× average FPS, much higher 1% lows, lower
input latency. 10× is not realistic: above ~2000 FPS, present/compositor overhead dominates.

---

## Then: look 1:1 (needed before fair FPS comparisons)

- **Sky**: port `RenderGlobal.renderSky` (sky dome, sunrise colors, sun/moon, stars) and clouds.
- **Fog**: port `EntityRenderer.updateFogColor` + `setupFog` (fog color blended toward sky by render
  distance, linear fog start/end from the far plane). Today's fog is an approximation.
- **Mipmaps**: vanilla default 4 levels with its alpha-preserving downsample (`TextureUtil`).
- **Water/lava + translucent layer** (`BlockFluidRenderer`, back-to-front section sorting).
- Full block table (stairs, slabs, fences, panes, doors… with state from metadata + neighbours).
- Day/night: `lightmap::build` already takes sun brightness; drive it from world time.

## Then: feel 1:1 (PvP-critical)

- Player entity + **bit-exact movement physics** (`Entity.moveEntity`, `EntityLivingBase`,
  `EntityPlayerSP`), collision boxes for every block, 20 TPS tick + interpolation.
- Replay-diff harness vs real vanilla (must be 0-bit difference).
- First-person hand/held item, view bobbing, FOV changes (sprint).

## Then: multiplayer

- Protocol 47, encryption, compression, Microsoft auth; test on local Spigot 1.8.8 + Grim before
  any public server. Client brand `vanilla`, no extra packets.

---

## Gotchas learned so far

- Colors: vanilla does all color math in **raw (non-linear) space**. Textures are `Rgba8Unorm`, and the
  shaders convert to linear only at the very end when the swapchain is sRGB. Don't switch to `*Srgb`.
- AO depends on vanilla's **stored vertex order** after model rotation (`ModelRotation.rotateVertex`).
- `World::light` returns `(15, 0)` outside loaded chunks / outside y 0..256, like `ChunkCache`.
- Chunk draw order comes from a `HashMap` → 1-pixel depth ties can vary between runs. Sort draws when
  doing P2.
- Bash heredocs containing `'` break in this environment's Git Bash; use the Write/Edit tools.
- Chunks store raw `id << 4 | meta`, but only pairs some state's `getMetaFromState` produces resolve;
  every other pair is air. Check `golden/blocks.json` `states[].meta`, not `state_from_meta`.
- Anything drawing from `Random.nextGaussian` needs `strict_math::log`, not `f64::ln` (ulp differences).
- Golden dumps reflect a headless bootstrap: graphics-dependent blocks (leaves) report fast-graphics values.
