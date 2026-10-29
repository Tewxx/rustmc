# rustmc — work waves (Phases 0–3)

Integration branch: `stabilize`. Each task runs in its own git worktree and branch; the orchestrator
merges in wave order. **Stabilise what exists before adding anything new.**

## Rules for every task
- Stay inside the files you own. Anything you need changed elsewhere goes in your final report.
- Expected values come from the Java reference (`MCP-919/src/minecraft`, compiled `MCP-919/bin/minecraft`)
  or golden dumps, never hand-written guesses. Never copy Java source into this repo.
- Rendering changes: screenshot before/after (`rustmc --screenshot out.png --screen <name>`).
- Report test failures verbatim. No "close enough".
- Shared untracked inputs live in the main checkout, not in worktrees:
  `/Volumes/SSD/projects/rustmc/MCP-919` (jar, assets, MCP classes), `/Volumes/SSD/projects/rustmc/.mc-tools/jdk8`,
  `/Volumes/SSD/projects/rustmc/golden` (once generated). Symlink them into your worktree if a test needs them.
- Commit on your branch with clear messages. Do not push.

## Wave 1 — foundations (parallel, now)
| Id | Task | Owns |
|---|---|---|
| A1 | Split `mc-client/src/main.rs` into modules, move-only | `mc-client/src/` |
| G1 | Hanging test, golden data on macOS, missing golden fails | `tools/extract`, `crates/*/tests/`, non-client `src` fixes |
| S1 | Cross-platform audio output via `cpal` | `crates/mc-sound` |
| C1 | CI (macOS/Windows/Linux), platform assumptions outside mc-client, release builds | `.github/`, `tools/`, `mc-protocol/tests`, `Play-RustMC.cmd` |
| P1 | Prism launch-argument module (not wired in) | new `mc-client/src/launch.rs` |
| B1 | Busy-scene benchmarks + profiler feature + baseline | `mc-client/src/bench.rs`, `mc-protocol/examples/mock_server.rs` |

## Wave 2 — stabilise existing features (after A1)
| Id | Task |
|---|---|
| F1 | Movement replay harness vs Java `EntityPlayerSP`; fix divergences (bit-exact) |
| F2 | Serverbound packet audit vs `NetHandlerPlayClient`/`PlayerControllerMP`; Spigot 1.8.8 + Grim session |
| F3 | Same-world same-camera vanilla vs rustmc screenshot diff; fix largest gaps |
| F4 | Containers/inventory/items audit vs Java (slotClick golden sequences) |
| F5 | Prism wrapper mode wired in, `dirs` per-OS paths, importable instance zip |
| F6 | Robustness: malformed packets, disconnects, chunk unload, pack reload; no panics in the net path |

## Wave 3 — performance (parallel with Wave 2)
R1 mesh/light worker pool + 18³ snapshots · R2 GPU arena/compact vertex/multi-draw-indirect ·
R3 `VisGraph` cave culling · R4 GPU instanced entities/particles · R5 frame pacing, buffer reuse.
Then profile on Metal; native Metal only where wgpu overhead shows.

## Wave 4 — missing client parity
V1 signs/skulls/banners/beacons · V2 maps/item frames/world border/lightning/minecarts/boats ·
V3 weather/stars/day-night/mipmaps/animated textures · V4 remaining GUI screens + pack sounds ·
U1 GUI draw layer mirroring vanilla's GL path (`GlStateManager`/`Tessellator`/`FontRenderer`) — the
seam Forge mod UIs will later draw through.
