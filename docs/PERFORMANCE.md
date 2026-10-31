# Performance work, 2026-10-30

This pass implements measured frame-pacing fixes and rendering/upload optimizations. It does not complete every proposed architecture change, the vanilla accuracy audit, or the remaining post-shader ports. Earlier percentage estimates were planning ranges, not measured promises.

## Main finding

Animated atlas updates copied the entire level-0 atlas into `rgba` after each changed sprite. A 1024x1024 atlas meant repeated 4 MiB allocations/copies on client ticks. Updates now copy only changed rows, and non-interpolated animation frames borrow their existing mip data. Animation counters, interpolation arithmetic, mip contents, and upload ordering are retained.

The old benchmark also paired a start-to-start interval with the following callback's work. Re-pairing the saved original samples gave 5.15 ms mean wall time and 5.07 ms preceding callback work in the slowest 1%, rather than the apparent 0.47 ms obtained from the following callback. Those samples do not establish an event-loop stall. Acquisition, submission and presentation now have separate timers; callback work is elapsed time and includes waits.

Benchmark mode previously skipped render-frame construction and game ticks. Both now run, with its scripted camera preserved. The saved original and final benchmarks therefore have different coverage; compare adjacent instrumented builds for smaller changes. The historical camera orbit is retained for continuity with earlier runs.

## Implemented

| Area | Change |
|---|---|
| Measurement | Same-frame interval/work attribution, full render-data/tick coverage, surface/submit/present/update/animation phases, outside-callback time |
| Presentation | Backend environment selection, supported present-mode override, frame-latency override; default remains 2 |
| Atlas | Region-only CPU synchronization and borrowed native animation mip chains |
| Workers | Packing, layer/index preparation and bounds on workers; stale-job cancellation before meshing/packing; reusable geometry state and mesh vectors |
| Uploads | Bounded completed-result backlog and per-frame section/byte budget; reusable staging buffers with ordered copies and asynchronous reclamation |
| GPU arenas | Rebuilds retain compatible allocations when the new geometry fits; capacities are released correctly on removal |
| Draw preparation | One section visibility pass with persistent page buckets; opaque page/slot order preserved |
| Transparency | Persistent sort/index buffers, distance keys calculated once, original stable tie behavior preserved |
| Visibility | Fixed-size section flood-fill scratch; dense traversal and cached membership; no bulk hash-set construction when a traversal completes |
| Meshing | Direct section-state reads; quad bounds and tint-presence cached per baked model alternative |
| GUI | Four vertices per indexed quad, shared growing index buffer, direct draw-command consumption, pooled draw vectors/texture strings, allocation-free transformed-quad append |
| Fonts | Width calculation streams UTF-16 units without a temporary vector, retaining formatting behavior |
| Entities | Reused world/hand/GUI geometry scratch; bit-identical matrix-input caching; repeated layer texture lookup and item face lighting cached |
| Lightmap | Identical lightmap bytes skip GPU uploads |
| Post processing | Borrow pass descriptions, cache bind groups by input view, avoid auxiliary-uniform name formatting |
| Reproduction | Alternating Windows benchmark runner with scenario controls, binary hashes, per-frame CSVs and summaries; static mock scene for captures |

Shared arenas, exact packed vertices, asynchronous meshing, multi-draw indirect, particle instancing and entity instancing were already present before this work.

## Measurements

RTX 3060, 1280x720, 64-chunk mock scene, release build, uncapped, 10-second measured runs. Raw artifacts are ignored under `tests/forge/build/tmp/performance/` and `tests/forge/build/tmp/bench-*`. Small deltas need repeated runs; the crowd workload varied substantially between repetitions.

| Build | Average FPS | 1% low | 0.1% low | Maximum frame |
|---|---:|---:|---:|---:|
| Saved original binary, old benchmark coverage | 2182.3 | 194.3 | 134.4 | 15.746 ms |
| Coverage/attribution repair, before optimizations | 1882.1 | 176.8 | 141.4 | 7.638 ms |
| First optimization batch, normal tick/render coverage | 2193.8 | 1107.3 | 824.0 | 1.445 ms |

Frame-latency 1/2/3 tests gave 2209.7/2193.8/2193.6 average FPS and 1114.1/1107.3/1110.0 1% lows. This is not evidence for changing the default to 1.

The subsequent upload/network/cache batch measured 2200.9 average / 1322.9 1% low in one quiet run, 2042.4 / 917.1 under `MOCK_STREAM=4`, and 2142.8 / 1106.6 under `MOCK_EDITS=200`. Its streaming 0.1% low was worse than the first batch in that single run, so this is not a universal performance claim.

Two alternating comparisons of entity-transform caching gave roughly 146-162 FPS with 1,000 entities for both builds; overall improvement was small relative to run variation. A release headless test with 200 zombies measured 0.777 ms CPU encoding with reused instancing and 0.723 ms with transform caching. This excludes simulation and presentation and is a microbenchmark, not a whole-game FPS result.

Final dense-membership comparisons used two alternating 10-second runs per build, saved under `tests/forge/build/tmp/bench-20261030-193806-51763001`:

| Scenario | Average FPS, previous / final | 1% low, previous / final | 0.1% low, previous / final |
|---|---|---|---|
| Quiet, run 1 | 2211.7 / 2173.4 | 1336.0 / 1395.7 | 750.4 / 1103.9 |
| Quiet, run 2 | 2219.1 / 2203.2 | 1341.0 / 1421.3 | 750.6 / 1043.5 |
| Stream 4, run 1 | 2028.0 / 2057.8 | 905.8 / 1194.1 | 517.3 / 938.5 |
| Stream 4, run 2 | 2025.1 / 2054.3 | 876.9 / 1178.8 | 508.3 / 945.7 |

Final quiet maximum frame times were 1.163 and 4.140 ms. The latter contained an isolated delay outside measured callback work (maximum callback work was 0.902 ms), so occasional external scheduling gaps remain. Final streaming maxima were 1.664/1.673 ms. No frames exceeded 16.7 ms in these runs. Relative to the saved original binary, final quiet 1% lows are about seven times higher, with the benchmark-coverage caveat above; average FPS is approximately unchanged.

## Validation

- Required `mc-render`, `mc-client`, `mc-gui` tests and additional `mc-assets` tests pass with `RUSTMC_REQUIRE_GPU=1`; `mc-data` tests also passed after geometry-reset support was added.
- Indexed GUI and expanded GUI paths produce identical pixels, including blending, transformed geometry, index-buffer growth and panorama rendering.
- Cached/instanced entity geometry matches the existing expanded path on the GPU; cached and uncached player transforms also match.
- Final versus saved-original full-client static HUD, glass and inventory (`net-inv`) captures have zero differing pixels at 1280x720.
- Atlas tests cover interpolated/native frames, unchanged pixels, and agreement between both CPU copies.
- Worker tests cover snapshot equivalence, invalidation, unload/reload and stale results; network tests cover dimension-ordered chunk preparation and the Forge bypass.
- These are regression checks against the prior Rust output and existing Java fixtures, not proof of complete vanilla pixel parity. No fresh vanilla/OptiFine FPS comparison or live Forge-mod regression run was performed in this pass. Forge connections retain their existing main-thread chunk handling because their handshake can override dimensions.

## Controls

| Environment variable | Use |
|---|---|
| `WGPU_BACKEND` | Select wgpu backend, e.g. `vulkan` or `dx12` |
| `RUSTMC_PRESENT_MODE` | Prefer a supported `immediate`, `mailbox` or `fifo` mode; Windows/Vulkan defaults to Mailbox, other backends to Immediate; normal vsync settings still apply |
| `RUSTMC_FRAME_LATENCY` | Desired frame latency, clamped to 1-3; default 2 |
| `RUSTMC_NO_UPLOAD_POOL=1` | Use queue writes instead of pooled mesh/index staging |
| `RUSTMC_NO_GUI_INDEXING=1` | Use expanded GUI triangles for comparisons |
| `RUSTMC_NO_TRANSFORM_CACHE=1` | Disable entity transform caching for comparisons |
| `RUSTMC_NO_ITEM_INSTANCING=1` | Keep held items in the expanded vertex path |
| `RUSTMC_NO_ENTITY_CULLING=1` | Disable conservative player/humanoid frustum culling |
| `RUSTMC_NO_SKY_INDEXING=1` | Use expanded sky/cloud triangles for comparisons |
| `RUSTMC_RENDER_TIMING=1` | Print encoding/submission and arena statistics on exit |
| `MOCK_STATIC=1` | Stop the default mock player's movement/swing packets for repeatable captures |

Build and run from PowerShell:

```powershell
cargo build --release --bin rustmc --config 'profile.release.package.wasmtime.debug=false' --config 'profile.dev.package.wasmtime.debug=false'
cargo build --release -p mc-protocol --example mock_server
./tools/bench-windows.ps1 -Seconds 20 -Repeats 3
# Optional comparison against an explicitly saved executable:
./tools/bench-windows.ps1 -Seconds 20 -Repeats 3 -Reference tests/forge/build/tmp/performance/rustmc-batch1.exe
```

The runner sets the repository TMP/TEMP directory, runs the client from `tests/forge/build/game`, restores environment overrides and stops only the mock process it started. It runs without the embedded Forge runtime.

## Remaining work

| Candidate | State / requirement |
|---|---|
| Smaller/reusable neighborhood snapshots | Section block/light arrays now share immutable storage and detach on writes; neighborhood metadata still copies 3x3 columns with three vertical sections |
| Full-cube mask meshing and incremental mesh patches | Not implemented; retain special side-render rules, geometry state order, AO/light samples, quad order and model selection |
| Lighting parallelism/locality | Not implemented; preserve ordered world mutations and lighting results before introducing asynchronous commits |
| GPU frustum/occlusion culling and persistent draw bundles | Not implemented; profile larger worlds first, retain draw order and prove conservative visibility without readback stalls |
| GPU-resident entity model geometry and broader render culling | Per-quad instancing now includes held items. Conservative CPU frustum culling covers players, zombies, pig zombies and skeletons; other types and occlusion remain outstanding |
| Static sky/cloud buffers | Shared quad vertices and cached index topology are implemented; vertex transforms stay on the CPU to preserve arithmetic |
| Text/layout/item/sign/banner caches | Only width and transient-storage work completed; cache invalidation and dynamic/obfuscated text need dedicated coverage |
| Resource/pipeline prewarming | Not implemented beyond existing startup setup; measure first-use stalls and retain resource-pack/mod invalidation |
| Dedicated render thread / parallel entity preparation | Not implemented; atlas copies explained the principal stalls; preserve Forge callback order, frame ownership and input latency |
| General greedy/GPU meshing | Research only: merging changes per-vertex fog, AO/light interpolation and atlas filtering unless explicitly reconstructed |
| Opaque front-to-back reordering | Not applied because equal-depth/custom-model output must retain ordering semantics |
| PGO / native CPU tuning | Not applied; needs a representative profile and math-fixture validation before claiming gains |
| Remaining post shaders and accuracy audit | Still outstanding; programs other than blit/blur retain their existing copy fallback |

The project remains uncommitted. Do not infer completion of these remaining items from the frame-pacing improvement.

## Follow-up targeting 2x FPS

Saved baseline: `tests/forge/build/tmp/performance-2x/rustmc-before.exe`, from the completed first pass with normal benchmark coverage. This is a like-for-like baseline; the original benchmark-coverage caveat does not apply to comparisons against this executable.

- Held item quads join the existing ordered entity instance stream, allowing adjacent body/item/body geometry to share draws. Their CPU clip positions, face culling, texture UVs, tint and glint arithmetic are preserved. The four clip W values travel with each packed quad, avoiding a CPU/GPU transform-math change. The device-limit fallback expands these records correctly.
- Player pose updates no longer clone six mesh parts to copy only pivots/rotations; humanoid headwear updates also avoid their unnecessary mesh clone.
- Conservative frustum tests skip entire off-screen players and common humanoid mobs before pose generation. Swept bounds include limbs/armor and resource-pack held-item extents/transforms; unknown/nonfinite bounds retain rendering. Other entity types retain their current path. This is not occlusion culling and does not alter ticking or Forge callback order.
- Sky/cloud quads use four vertices instead of six. Indices persist until sunrise, stars or cloud-face topology changes. CPU transforms, fog arithmetic and primitive order are unchanged.
- Chunk snapshot clones share block/sky-light/block-light arrays; writes detach each affected array independently. This retains immutable worker snapshots while reducing main-thread copying.
- Entity-run storage is reused, and avatar vertex buffers grow instead of silently truncating dense scenes at the old fixed vertex count.

GPU tests compare packed items, mirrored/near-camera items, off-screen entity removal, and indexed skies/clouds against the expanded paths. Sky checks include day, night, sunrise/weather, fast/fancy/no clouds, cloud-height transitions and cached topology. Bounds tests cover animated player geometry and oversized, mirrored, sheared and projective held-item transforms. Additional world tests cover buffer sharing and independent block/light mutations. Workspace/all-target compilation also passes.

The first item-batching-only comparison, Vulkan/Immediate with two alternating 10-second runs, measured 225.7-226.1 FPS versus 158.3-160.2 with 1,000 entities (about 42%). Quiet FPS was effectively unchanged. Its interrupted streaming run included a 323 ms queue-submission stall; this is retained in the raw logs rather than discarded as a success. The complete follow-up streaming comparison did not reproduce it.

The complete follow-up comparison used two alternating 10-second runs per build, Vulkan/Immediate for both executables, with no concurrent builds. Artifacts: `tests/forge/build/tmp/bench-20261030-204530-22113930`. Values below are the means of the two run summaries:

| Scenario | Baseline / optimized average FPS | Change | Baseline / optimized 1% low |
|---|---:|---:|---:|
| Quiet | 1932.7 / 2233.0 | +15.5% | 680.1 / 1271.7 |
| 1,000 entities | 146.6 / 1083.5 | 7.39x | 100.3 / 235.6 |
| Stream 4 | 1763.4 / 2047.5 | +16.1% | 423.4 / 934.2 |
| 200 block edits/s | 1853.9 / 2139.2 | +15.4% | 614.5 / 926.9 |

Crowd results depend strongly on which entities are actually in the scripted camera's view; this is not a 7x claim for a frame where every entity is visible. The 2x target is exceeded in this crowded workload, including its 1% lows, and is not achieved for every scene. Quiet results varied between benchmark sessions, so small deltas should be treated cautiously. Final streaming maxima were 1.59/5.71 ms. One edit run contained a 37.53 ms frame in the timed update phase, so isolated hitches remain; its 0.1% low was 295 FPS versus 348 for that baseline repetition.

Backend probes on this RTX 3060 measured about 2,211-2,215 FPS with Vulkan versus 1,723-1,727 with DX12, so the backend remains Vulkan. A separate idle Immediate/Mailbox/Mailbox/Immediate confirmation used the same optimized binary and ten seconds per scenario. Quiet average FPS was 1981.7/2543.5/2558.5/2022.8; streaming average FPS was 2049.0/2442.8/2392.9/2086.5. Mailbox averaged +27.4% quiet and +16.9% streaming, with quiet 1% lows of 1420-1446 versus 463-536. Raw folders: `bench-20261030-204903-c6739891`, `bench-20261030-204926-a086be65`, `bench-20261030-204950-bf1e124d`, `bench-20261030-205014-e84d88f6`.

Windows/Vulkan now prefers supported Mailbox for uncapped rendering, followed by Immediate and Fifo. Other backends retain the prior preference. Explicit present-mode overrides and vsync's Fifo selection are retained; maximum frame latency remains 2. These presentation gains are separate from the code comparison table above and are not multiplied into a blanket FPS claim.

The delivered renderer build (SHA256 `4DF73B4F894AD0A57CE50EFE81E17CAC1026B80B843E7389A6D8D157A76110AA`, saved as `performance-2x/rustmc-delivery.exe`) was checked with unset backend/presentation overrides. Its log confirms Vulkan/Mailbox. Two ten-second runs per workload measured 2551-2576 FPS quiet, 1008-1032 with 1,000 entities, 2179-2232 streaming, and 2185-2277 under edits; raw folder `bench-20261030-205520-5e9d2dd4`.

Longer twenty-second runs with the same defaults measured 2460.5/2532.1 FPS quiet (1% lows 919.3/1092.2) and 2389.8/2469.4 streaming (1% lows 1072.7/1257.7), under `bench-20261030-205820-cc2178e4`. Quiet maximum frames were 3.16/5.75 ms and streaming 1.70/1.99 ms; none exceeded 16.7 ms. The longer camera path and scheduling gaps affect the tail figures, so the short-run gains do not guarantee every future run's lows.

Final required/additional regressions passed: 191 tests, 6 intentionally ignored, with GPU availability required. Full-client HUD, glass and inventory comparisons against the saved baseline each have zero changed pixels at 1280x720. The user subsequently requested default IGN `<your IGN>`; performance fixtures explicitly pass `--name Player` so benchmark/capture identity remains comparable. Both launch scripts now prefer the canonical Cargo release executable over the old `rustmc-updated` fallback.
