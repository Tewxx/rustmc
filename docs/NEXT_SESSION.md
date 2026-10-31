# Next session: Forge API, renderer rewrite, Meowtils

Paste "Continue from docs/NEXT_SESSION.md" into a new chat opened in this folder.

## 2026-10-31 Forge API breadth
- Real vanilla registries: `tools/shadow/keep-real.txt` now keeps `net/minecraft/block/`, `item/`, `init/Blocks|Items`,
  `creativetab/`, `potion/`, `enchantment/`, `entity/ai/attributes/`, `world/biome/BiomeGen*`, FML's `GameData`/
  `FMLControlledNamespacedRegistry`/`PersistentRegistryManager`/`RegistryDelegate`/`GameRegistry` and pure utils
  (`MathHelper`, `Cartesian`, `MapPopulator`, `IntegerCache`, `Team`, `Profiler`...). `ModLoader.registries` runs
  `Block.registerBlocks`, `BlockFire.init`, `Item.registerItems` before any mod; `Bootstrap.isRegistered` answers true.
  So `Blocks.*`/`Items.*` are the real objects, and `Live.stack` builds stacks with the real Forge constructor (Forge's
  `getItem` reads the registry delegate) plus their NBT (`CompressedStreamTools`, reparsed only when bytes change).
- World bridge: the client mirrors block sections and biome columns into Java once per tick (`forge_sync_world`:
  copy-on-write `SectionData::same` pointer compare, so a steady world costs one compare per section).
  `World.getBlockState`/`isAirBlock`/`getBiomeGenForCoords` answer from it (`Live.blockState`/`biome`).
- `Minecraft.getResourceManager` is `rustmc.forge.Resources`: client packs > mod jars > vanilla jar, backed by
  `impls::set_resources` (the client hands it its own pack stack, rebuilt on resource reload).
- `Minecraft.renderGlobal` (renderInfos/viewFrustum sized from the renderer's drawn/total sections) and
  `renderManager` exist from startup; renderer constructors set vanilla fields and RenderPlayer's six layers;
  addLayer/removeLayer, ModelBase texture offsets, LayerBipedArmor.initArmor.
- Every `ClientAction` is wired except `Ping`; `RenderGlobal.loadRenderers` remeshes everything. Vanilla F2
  screenshots added (`save_screenshot`: local-time name, `_n` suffix, clickable chat link, PNG encode off-thread).
- Live corpus: lunarforge and Meowtils both reach 0 unimplemented stubs and 0 Java exceptions
  (`tests/forge/build/lunar-run3.log`, `meow-run3.log`). Static gap sweep over 7 jars (+ Patcher, OverflowAnimations,
  PolyTime, NotSoEssential, EntityCulling, scan only: `tests/forge/build/scan-wide`) is `gaps-wide.json`; the
  remainder was split into `impls/gap_*.rs` worker tasks (`dsagent/tasks_rustmc_gaps.txt`).

## 2026-10-30 performance update
- Latest user preference: default offline IGN is `<your IGN>` (`args::DEFAULT_NAME`, also used by `App`). Explicit `--name` and authenticated launcher identities remain supported. Both launch scripts now prefer `target/release/rustmc` over stale alternate binaries. Performance/capture fixtures pass `--name Player` explicitly.
- Rebuilt after the IGN change: `target/release/rustmc.exe` and the compatibility alias `rustmc-updated.exe` both have SHA256 `71CFCB5E6CFB4994F1E0041B6202836C09798A4AFF1F86F20B653141B2FCEAED`. Launcher help and a mock-server connection verify the default name. Client tests pass and the three captures still match with explicit `--name Player`.
- Delivery validation: 191 tests passed, 6 ignored, GPU required; HUD/glass/inventory captures match the saved baseline pixel-for-pixel at 1280x720. Default Vulkan/Mailbox twenty-second runs measured 2461-2532 FPS quiet (1% 919-1092) and 2390-2469 streaming (1% 1073-1258), with no frames over 16.7 ms. Raw results in `bench-20261030-205820-cc2178e4`; tail metrics still vary. See `docs/PERFORMANCE.md` for the exact comparison conditions and limitations.
- Follow-up toward the user's 2x target: shared copy-on-write section arrays, held-item instance batching with exact CPU clip coordinates, removal of per-pose mesh clones, conservative player/humanoid frustum culling, indexed sky/clouds with cached topology, pooled entity-run storage, and growing avatar vertex buffers are implemented. Other entity types/occlusion and larger architecture candidates remain listed in `docs/PERFORMANCE.md`.
- Like-for-like two-run Vulkan/Immediate means: 1,000 entities 146.6 -> 1083.5 FPS (1% 100.3 -> 235.6); quiet 1932.7 -> 2233.0; stream-4 1763.4 -> 2047.5; edits-200 1853.9 -> 2139.2. The 2x target is exceeded in the crowded workload, not universally. Raw logs/binaries are under `tests/forge/build/tmp/performance-2x` and `bench-20261030-204530-22113930`. One 37.53 ms timed-update hitch remains under edits.
- Idle alternating presentation tests on the same optimized binary favored Mailbox (+27% quiet, +17% streaming), so Windows/Vulkan now prefers Mailbox when supported. Other backends and explicit overrides retain their existing selection; vsync still chooses Fifo, latency stays 2. DX12 tested slower than Vulkan. New comparisons can disable item instancing, entity culling or sky indexing with `RUSTMC_NO_ITEM_INSTANCING`, `RUSTMC_NO_ENTITY_CULLING`, `RUSTMC_NO_SKY_INDEXING`.
- Current user instructions prohibit committing or pushing. The older workflow notes below do not override that.
- Read `docs/PERFORMANCE.md` and the updated `BENCH.md` first. The practical optimization batch is implemented; larger architecture changes and shader ports remain listed explicitly.
- The earlier event-loop diagnosis was based on mispaired frame intervals and callback times. Correct attribution accounts for nearly the whole slow frame inside the preceding callback. Animated atlas updates repeatedly copied the full 4 MiB atlas; region-only synchronization removed the main periodic spikes.
- Benchmark mode now runs ordinary ticks and render-data construction while retaining its scripted camera; old benchmark coverage was incomplete. Surface acquisition/submission/presentation and outside-callback time are recorded separately.
- Worker-side upload preparation, cancellation and storage reuse; bounded uploads and pooled staging; arena range reuse; single-pass visibility/page bucketing; stable allocation-free transparency sorting; dense visibility masks; cached quad bounds; indexed/pooled GUI; font/entity scratch/caches; identical lightmap upload suppression; post bind-group/descriptor caching are implemented.
- Final quiet RTX 3060 runs measured 2173-2203 average FPS, 1396-1421 1% lows and 1044-1104 0.1% lows, versus the saved original's 2182/194/134 (with the benchmark-coverage caveat). Stream-4 lows measured 1179-1194 / 939-946. Dense cached membership removed the traversal-completion allocation spike. One isolated 4.14 ms outside-work scheduling gap remains. Raw logs/CSVs and saved comparison binaries are in `tests/forge/build/tmp/performance` and `tests/forge/build/tmp/bench-*`.
- Required tests plus atlas tests pass with `RUSTMC_REQUIRE_GPU=1`; mc-data tests passed too. Final static HUD/glass/inventory regression captures and GPU GUI/entity comparisons match exactly. This is not a full vanilla/OptiFine performance/parity certification, and no new live Forge-mod run was performed.
- `tools/bench-windows.ps1` runs alternating repeated scenarios and saves binary hashes and results. The mock server supports `MOCK_STATIC=1` for captures. New A/B controls: `RUSTMC_NO_UPLOAD_POOL`, `RUSTMC_NO_GUI_INDEXING`, `RUSTMC_NO_TRANSFORM_CACHE`, plus `RUSTMC_FRAME_LATENCY` and `RUSTMC_PRESENT_MODE`; `WGPU_BACKEND` is honored. Default latency remains 2.

## How to work
- Claude is the controller: split bulk work into tasks for DeepSeek workers, run them, review every diff, build and
  test before accepting. Hard or cross-cutting pieces Claude does itself.
- Runner: `%USERPROFILE%\Downloads\dsagent` (read its `README.md`; `dsagent.py`, `mktasks.py`). The API key is in its
  `.env`: never print, copy or commit it. `python dsagent.py --ping` checks it.
- Model `deepseek-flash` (V4.1-Flash), `reasoning_effort` `high`, `max` for hard tasks. Never `deepseek-v4-pro`.
- The runner was written for another repo; adapt task prompts to rustmc: one file per task, SRG names,
  `MCP-919/src/minecraft` as reference only (never copied into the repo).
- Commit locally in logical pieces once they build and pass. Don't push unless asked.

## State (uncommitted on `main`)
- Live bridge, hooks and events work and their JVM test passes (`crates/mc-forge/tests/live.rs`): player/world/
  entities/inventory/settings/view/key bindings/tab list mirrored into Java; `runTick`, `runGameLoop`, `startGame`,
  `loadWorld`, the three clicks and `onUpdateWalkingPlayer` run Mixin HEAD/cancel/RETURN around the Rust body; chat,
  overlay, key, mouse, player-tick, living-update, attack and world-last events; `RenderWorldLastEvent` draws project
  on the CPU. In the client: `startGame` after `load_mods`, `runGameLoop` around each frame, clicks around `interact`.
- Meowtils 2.0.1 (unmodified jar, user's real config copied to the harness dir) initialises and ticks:
  `RUSTMC_STUBS_JAR=$PWD/tests/forge/build/stubs-meowtils.jar RUSTMC_CORPUS_JAR=<jar> cargo test -p mc-forge --test corpus_live`
  writes `tests/forge/build/Meowtils-2.0.1-stubbed.txt` (empty: every stub it reaches is implemented).
- World and screen transitions enter Java. `Minecraft.loadWorld(WorldClient)` is now a Rust-owned continuation (it
  was still real vanilla bytecode, so the client's call could not reach it) and `forge_publish` fires it on join,
  respawn and when leaving a world (a reset while already out of one is not a change), after `Live.sync` installs the
  world. Native screens open and close through `Minecraft.displayGuiScreen` instead of calling `open_screen` directly,
  so Mixins on it run; a refused open no longer leaves a stale Java screen for `forge_sync` to mirror. `LifecycleMixin`
  in the liveprobe pins both paths (`live.rs`). Limitation: a HEAD handler on `loadWorld` sees the new world already in
  `theWorld` and `WorldEvent.Load` already posted, because the mirror publishes before the call; the body is the Rust
  continuation, not vanilla's. Firing it before the mirror instead would need `Live.sync` split into world/entity steps.
- Rebuilt `stubs.jar`/`stubs-meowtils.jar` from the current glue. The older Meowtils jar embedded a `Tab` class without
  today's fixes and threw `ClassCastException: Character cannot be cast to String` in `TeamUtil.isTeam`; the rebuilt
  jar runs the same corpus with 0 stubs and no exception.
- Real-client smoke with the rebuilt binary: `rustmc-updated.exe --connect 127.0.0.1:25599 --screenshot
  meow-gui-updated.png --screen mod` plus `RUSTMC_SHOT_KEYS=24` renders the Meowtils click GUI over the world
  (`tests/forge/build/game/meow-gui-updated.png`). The known `ClickGUI.applyBlur` shader NPE still logs.
- Done this session: AWT not headless off macOS (Swing log window); harness JVM tmpdir under the build dir (this
  host denies the test binary writes to `%TEMP%`); permanent JVM thread attach (`with_env`); `impls/misc.rs` (I18n
  with the client's language table, `ItemStack` constructors, GL no-ops, `isUsingItem`); `impls/texture.rs`
  (`DynamicTexture`/`TextureManager` dynamic locations -> GUI textures `rustmc/gl/<id>`, uploaded per frame in
  `client_packs.rs`); vanilla `GameSettings` key bindings with mod-held keys fed back (`forge_held`); local chat from
  `addChatMessage`; `EnumTypeAdapterFactory` real; unchanged snapshots are not re-published.
- Workers: `%USERPROFILE%/Downloads/dsagent/rustmc_agent.py` wraps dsagent for this repo (owned files, `check` tool
  = cargo check or javac of the glue, MCP-919 read-only). Task files `tasks_rustmc*.txt`, logs `run_rustmc*.log`.
- Renderer baseline (Windows, RTX 3060, mock server 64 chunks, `--bench 10`): 2414 fps avg, 1% low 223, 0.1% low 150.
  The worst 1% of frames have normal CPU time (0.38 ms) but 4.5 ms wall time: the spikes are outside
  update/draw/present (event-loop or OS pacing), so look there before meshing.

## Build/test gotchas
- MSVC fails on wasmtime's C helper under cargo (D8050): use
  `--config 'profile.dev.package.wasmtime.debug=false'` (or add it to `Cargo.toml`). Run cargo outside the sandbox.
- Java changes need `tools/shadow/stubs.ps1` (Meowtils: `-OutputJar tests/forge/build/stubs-meowtils.jar
  -MixinScans @("tests/forge/build/scan/Meowtils-2.0.1.json")`, then rerun without arguments to restore runtime.json), then `crates/mc-forge/tests/mods/liveprobe/build.ps1`, then
  `RUSTMC_REQUIRE_FORGE=1 cargo test -p mc-forge`.
- Architecture: `docs/FORGE_STUBS.md`. Stub classes dispatch to `stubs::register("class.srgMethod", ...)`, and
  unimplemented ones log `[forge] stubbed`. Real classes are listed in `tools/shadow/keep-real.txt`.
- Under the Windows sandbox the test process cannot write the session `%TEMP%`: the `mc-client` tests that use
  `std::env::temp_dir()` (`pack`, `asset_download`, `wasm_mods`) fail there. Point `TMP`/`TEMP` at an existing
  workspace directory for the test run, e.g. `target/testtmp`.

## Running Meowtils in the client
On Windows, double-click `Play-Meowtils.cmd` (or pass normal client arguments to it). `tools/launch-meowtils.ps1` reads the existing runtime metadata and game classpath, sets Java/Forge/TMP/TEMP, and launches the latest release from the game directory. Its preflight and argument forwarding were checked with `--help`; that check does not start mods. The existing mod folder contains the approved Meowtils and lunarforge JARs.

`tests/forge/build/game` (git-ignored) holds `mods-forge/Meowtils-2.0.1.jar` (a copy), the user's `meowtils/meowtils.json`
and `forge_cp.txt` (runtime libraries, Guava 21). From that folder, with the mock server on 25599:
`RUSTMC_FORGE_STUBS=<stubs-meowtils.jar> RUSTMC_FORGE_CP=$(cat forge_cp.txt) JAVA_HOME=<jdk8> RUSTMC_JVM_OPTS=-Djava.io.tmpdir=<game>/tmp
RUSTMC_SHOT_KEYS=24 rustmc.exe --jar <1.8.9.jar> --connect 127.0.0.1:25599 --screenshot meow-gui.png --screen mod`
renders its click GUI (`meow-gui.png`). Built exes may not write under %TEMP% on this PC. Open issues seen there: the
GUI blur NPEs (`ClickGUI.applyBlur`: EntityRenderer shaders), GlStateManager blend/alpha state (`func_179147_l`,
`func_179112_b`, `func_179084_k`, `func_179141_d`, `func_179118_c`) and `World.getPlayerEntityByUUID` are stubbed,
"Failed to load sounds" NPE. Auto-building the stub jar from a scan of `mods-forge/` is not done (general mod support).

## 2026-10-31 (later): world queries, commands, smoke
- Light is mirrored per section (`Live.light`, packed `sky<<4|block`, sent only when the copy-on-write arrays change)
  with skylightSubtracted. World getLightFor / getLightFromNeighborsFor / getCombinedLight / getLight /
  getLightBrightness / canSeeSky, Chunk light getters and Entity/EntityFX brightness follow vanilla over it.
- `World.rayTraceBlocks` (all three overloads) is vanilla's DDA in `Live.rayTrace`, with the real Block collision code.
  `getChunkFromBlockCoords` / `getChunkFromChunkCoords` give one Chunk object per column (`Live.chunk`).
- `/` chat runs mod client commands first (ClientCommandHandler order); a handled command is not sent.
- `Minecraft.timer` and `entityRenderer` exist from startup (Meowtils hooks read them before the first sync).
- Do not register Rust handlers on classes that mods mixin into (ItemRenderer broke Meowtils): mixin-keep skips them.
- Smoke: release client + mock server (port 25565) + both mods: no exceptions; Meowtils click GUI renders.
- Still open: tile entities are not mirrored (getTileEntity null), ModelRenderer draws no geometry, Ping action,
  the blur post-shader is accepted but not visible, and the second worker sweep over `gaps-wide.json`.

## Left, in order
1. Renderer: find the frame-pacing spikes first, then GPU-driven culling and fewer draws; benchmark a large world
   (`--features singleplayer --radius 16`) against vanilla Java on the same machine; keep visual parity (`tools/visual`).
2. Clear the remaining Meowtils client issues listed under "Running Meowtils in the client" (ClickGUI blur shaders,
   GlStateManager blend/alpha, `World.getPlayerEntityByUUID`, sounds) and add click/HUD captures to the harness.
3. Missing game pieces from `HANDOFF.md`/`ROADMAP.md` (signs/skulls/banners, maps; the mob-appearance particle needs
   a guardian model), via workers.

## Boundary
General Forge mod support only. No injector/cheat-client work: no window-class spoofing for Slinky
(`docs/SLINKY_COMPAT.md`), no vanilla-obfuscated remapping for injected payloads, no faked Netty `packet_handler`
pipeline, and no testing of aura/reach/ESP modules.
