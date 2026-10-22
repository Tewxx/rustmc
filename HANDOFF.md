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
| Block registry: all 198 blocks, 7806 states, materials, sounds, `getActualState` (golden-tested) | Done | `crates/mc-data` |
| Assets: layered packs, blockstates, models, baking, atlas, colormaps | Done | `crates/mc-assets` |
| State → model mapping for all 7806 states; neighbor-derived models, block tints, 61 biome color profiles | Java-golden-tested; regular JSON models wired into meshing | `mc-data/{model_location,color,biome}.rs`, `mc-render/src/models.rs` |
| Block bounds, selection/collision boxes, per-block ray traces, side rendering | Ordered Java replay verified; includes mutable bounds and moving pistons | `mc-data/src/geometry/` |
| World: 1.8 chunks, Anvil loader, saved biomes/tile-entity NBT, demo terrain | Partial (no tick, entities, save or incremental light yet) | `crates/mc-world` |
| Sky/block light (vanilla steady state, edge-seeded BFS) | Done, single-threaded | `mc-world/src/light.rs` |
| Smooth lighting + AO (`BlockModelRenderer` port), lightmap | Done | `mc-render/src/ao.rs`, `lightmap.rs` |
| Chunk meshing | Done, **single-threaded, full rebuild only** | `mc-render/src/mesh.rs` |
| wgpu world renderer | One draw per chunk, CPU frustum culling | `mc-render/src/gpu.rs` |
| GUI: font, HUD, F3, chat, title/pause/options/video/controls | Done | `crates/mc-gui`, `mc-render/src/gui_gpu.rs` |
| Local player: 20 TPS movement, gravity, jump, sprint, sneak ledges, slab steps, collision, creative flight | Playable; full Java differential physics validation remains | `mc-client/src/player.rs` |
| Steve body, F5 front/rear views, first-person arm and held block/item rendering | Working local milestone; animation/lighting parity remains | `mc-render/src/avatar.rs` |
| Creative catalogue, tabs, search, stack/cursor operations, hotbar icons; local commands, break/place/pick | Working; special item behavior/NBT and complete placement rules remain | `mc-client/src/{inventory,commands,main}.rs` |
| Client: state machine, input, settings (`options.txt`) | Local playable creative world, death/respawn screen | `crates/mc-client/src/main.rs` |

## Session log — multiplayer, sound, integrated server (Oct 2026)

Commits cbf3425 … 5bc480a. New since the table above:

| Area | Status | Where |
|---|---|---|
| Multiplayer (`--connect host:port --name X`, `--online` for Microsoft auth) | Join, chunks, movement packets in vanilla order/thresholds, digging, C0A/C02/C08, C09 sync, PosLook replies, chat/action bar, death screen, block changes | `mc-client/src/{net,main,chat}.rs` |
| Online auth | Device-code MSA → XBL → XSTS → MC; refresh token cached in git-ignored `rustmc_account.json`. **Never tested with a real account** | `mc-client/src/auth.rs` |
| Remote entities | Other players (3-step interp, body clamp, limbs, held item, skins, name tags), item entities, picking/attack | `mc-client/src/entities.rs`, `mc-render/src/avatar.rs` |
| Containers | Window 0 + chest/hopper/dispenser/crafting/furnace, slotClick modes 0/1/2/-999, transactions, C0F/C0D, creative C10 | `mc-client/src/container.rs` |
| Sound | Pure-Rust Vorbis (matches ffmpeg), WinMM mixer, sounds.json, categories, music ticker, footsteps/dig/break/place, GUI clicks, sound options screen. **Not listened to by a human yet** | `crates/mc-sound`, `mc-client/src/music.rs` |
| Integrated server (singleplayer) | Scheduled tick list (time, priority, id; 1000/tick), updateLCG random ticks over chunks within 7, neighbour notifications; liquids flow; grass/mycelium, ice, snow, reeds, cactus, crops, farmland, leaf decay | `mc-client/src/server.rs`, `mc-logic/src/blocks.rs` |
| Inventory player preview | `drawEntityOnScreen` for survival, player container, creative inventory tab | `mc-render/src/avatar.rs::encode_gui` |

Testing: `cargo run -p mc-protocol --example mock_server -- 25599` (env `MOCK_CHEST=1`, `MOCK_SKIN=<png>`)
then `rustmc.exe --connect 127.0.0.1:25599 --screenshot shots/audit/x.png --screen net`.

**Next, in order:** saplings (needs `mc-gen::trees` usable outside population), vines, fire;
mob models/rendering for servers; mob/rain/fall/hurt sounds; multithreaded meshing;
settings parity audit (video options vs `GameSettings`); death score from metadata 18;
remaining commands; sneaking name-tag occlusion. User-only checks: app icon in Explorer/taskbar,
a real Microsoft login, listening to sound.

## User priority: playable game (latest steering)

Latest steering also rejects the visual appearance as far from 1:1. Do not call the playable
prototype visually accurate. A visual audit found and fixed discarded model display transforms,
inventory/held objects incorrectly using world-state models, animated item PNG strips squeezed
into icons, alpha blending on an sRGB surface, and skipped translucent model blocks. The client
now bakes actual item models, honors inherited display transforms, prefers a raw UNORM target,
and renders translucent faces in a separate sorted pass. Player geometry is drawn before that
pass; the first-person hand follows it. Steve's scale/root and arm rest angle now follow Java.

The flat sky is replaced by overworld sky planes, biome/time sky and fog colors, sunrise fan,
sun/moon and fast/fancy cloud geometry from the Java paths. Runtime time is still fixed to noon;
stars, weather effects and non-overworld skies remain unimplemented. `item_transforms.json`
checks all 1,595 JSON models against Java parent resolution; `sky.json` checks 1,080 overworld
biome/time/partial-tick samples. These validate components, not a full rendered scene.

**Next visual work:** establish same-world, same-camera Java/Rust image comparisons. Terrain is
still the sine-wave demo unless an Anvil world is supplied. Fluids, special block renderers,
texture mipmaps/animations, full player/item lighting and several GUI details still differ.
Do not hide those gaps behind screenshot smoke tests or unrelated foundation work.

The user's existing `target/release/rustmc.exe` session can lock that executable during builds.
Do not kill their session or discard its edits. Build an alternate executable with
`cargo rustc --release --offline -p mc-client --bin rustmc -- -o target/release/rustmc-updated.exe`;
`Play-RustMC.cmd` prefers that alternate file. Normal `cargo build` can fail with Windows error 5
while the old window is open. The active window does not automatically receive code changes.

The user explicitly rejected further foundation-only progress while the app remained a spectator
viewer. The current checkpoint adds a real local player, visible hand/body, a creative inventory,
world editing and commands. **Keep playability first.** Read `PLAYING.md` for controls and limits;
double-click `Play-RustMC.cmd` or launch `rustmc --play` to enter directly.

Tests exercise ground/wall/ceiling collisions, jump, slab steps, sneak edges, flight, selection,
menu-to-hotbar operations, command mutation/error atomicity, break/place/pick and preventing placement
inside the player. The creative catalogue is extracted from Java (`DumpCreative`, `creative.tsv`)
and its icon assets are checked against the actual jar. It is metadata, not a complete Item/NBT port.
Editing blocks now invalidates light in neighbouring chunks and receives light from unchanged
boundary sources, fixing stale light after border edits.

Current limits: local simulation only; no packets or integrated server. Edits/inventory are session-only.
Crafting, container interaction, item-use behavior, drops, mobs, fluids, redstone and full survival are
still missing. Special block renderers and several placement rules remain incomplete. Do not call
this full parity or claim the new controller has Java golden validation. Next work should validate
player traces against Java and extend persistent gameplay and real block/item interaction before
returning to unrelated foundations.

Run: `cargo run --release --bin rustmc -- --radius 8`
Screenshot any screen headless: `rustmc --screenshot out.png --screen title|pause|options|video|controls|hud|debug|chat`
Tests: `cargo test --release --workspace --offline` (77 tests). Golden/real-jar tests skip when their
inputs are missing; the local verification used all inputs. New dumps: `models.json` (7806 mappings),
`colors.json` (7806 states with mixed-biome neighborhoods), `biomes.json` (61 profiles, 200 samples each).
`geometry.json` replays 37,506 scenes covering every state, 487,578 rays and 112,518 masked collision
list queries, including 300 moving-piston tile-entity scenes.
Benchmark: `rustmc --radius 8 --bench 10` → one-line summary + `bench.csv` (vsync/limit overridden in memory only).
Golden data: `powershell -ExecutionPolicy Bypass -File tools/extract/extract.ps1` (needs a JDK 8 and MCP's
compiled `bin/minecraft`; writes `golden/`, which is gitignored because it's derived from Mojang code).

Registry tables: after changing the extractor, rerun it and then `cargo run --release -p gen-data`, which
rewrites `crates/mc-data/src/generated/` (committed; never edit by hand).

**The plan now lives in `ROADMAP.md`** (segments S0–S13). The performance sections below are scheduled
there as S8/S12; read the roadmap first.

Earlier checkpoint (2026-10-21): **S1d implemented.** Geometry APIs live on an explicit
`BlockGeometry` owned by the serial caller. Keep the same instance across calls: vanilla stores
mutable bounds on singleton blocks, and a stair ray trace permanently changes later bounds until
other operations replace them. Raw bounds can be inverted (vines); `Bounds` preserves that while
world AABBs normalize like Java. Collision lists retain Java order, strict mask intersections and
f32 coordinate arithmetic, including at large coordinates. Saved piston NBT now feeds moving-box
geometry. `mesh::build_with_geometry` applies `setBlockBoundsBasedOnState` and `shouldSideBeRendered`,
with one persistent context on the client. Regression checks cover adjacent glass, matching slab
halves across materials, and visible inset faces beneath solid blocks. `shots/s1d-final.png` is a
render smoke check. The playable checkpoint above now consumes these geometry APIs for the local
player's collision, block selection and placement.

Previous checkpoint (S1c): removed the partial
`mc-world/src/blocks.rs` table; lighting/AO use registry properties. Model lookup is a dense state
array and shares baked geometry across ignored properties. Fancy/Fast changes now remesh; solid
layers skip alpha testing (needed for fast leaves). Absent variants show the missing cube, and
unresolved weighted alternatives are skipped in vanilla order. Anvil biomes and flower pots now
reach actual-state/tint logic. `shots/s1c-final.png` is a render smoke check, **not** a vanilla
pixel-parity claim. Plant position offsets, fluids, translucency,
tile-entity rendering and the all-block vanilla screenshot comparison remain outstanding.

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
