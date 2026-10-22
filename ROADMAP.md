# rustmc — roadmap to a 1:1 Minecraft 1.8.9 in Rust

Goal: everything 1:1 with vanilla 1.8.9 (behaviour, visuals, protocol), multithreaded where vanilla's
semantics allow it. Reference: `../MCP-919/src/minecraft/` (read-only, never copied in).

## Playability priority (latest user steering)

The user also rejects the current visuals. Prioritize visible parity and a same-world Java/Rust
image baseline. The new item-transform and sky-value golden checks validate individual components;
they do not establish visual parity. Demo terrain, missing fluid/special rendering, mipmaps,
animations, entity lighting and GUI details remain visible gaps.

The user rejected foundation-only work while the app remained a terrain viewer. A local playable
milestone now exists: grounded player physics, creative flight, Steve/hand rendering, F5 views,
creative catalogue/search/hotbar, break/place/pick and local commands. Launch `Play-RustMC.cmd`.
See `PLAYING.md` for controls and explicit limits. This is not full physics, item or server parity.

Before returning to the foundation schedule below, validate movement against actual Java traces,
make player/inventory/world edits persistent, and expand real block/item interactions. The local
controller and commands must eventually move through the authoritative server/protocol path.
Do not use this milestone as evidence that S3/S5/S7 or the integrated server are complete.

## Original audit (2026-10-21, before the registry and playable milestones)

- ~7k lines of Rust vs ~280k lines of Java reference (~3%). Tests: 32 passing.
- What exists is a **terrain viewer**: assets/models/atlas, AO + smooth lighting, chunk meshing, wgpu
  renderer, GUI screens, fly camera. Quality of what's there is good (bit-exact math, tested).
- The **foundations a full game needs are missing or provisional**:
  - Blocks are an ~80-entry `id/meta → blockstate string` table (`mc-world/src/blocks.rs`). No
    `Block`/`IBlockState` model, properties, `getActualState`, materials, hardness, bounding boxes.
  - `World` is a `HashMap<(i32,i32), Chunk>` with no heightmap, no tick, no entities, no updates.
  - Lighting is a "steady state" BFS, not vanilla's incremental `World.checkLightFor`.
  - Terrain is a sine-wave demo, not `ChunkProviderGenerate`.
  - No server, no protocol, no player, no physics, no items/inventory.
  - Not a git repository.
- HANDOFF.md prioritises performance (P0–P5) next. **That order is wrong for the 1:1 goal**: tuning
  vertex formats and draw submission for a renderer whose data model is about to change is wasted work.
  The P0 benchmark and the P1 threading ideas are kept, but scheduled where the data they depend on exists.

## Architecture decision (do this shape from now on)

Vanilla singleplayer = an **integrated server thread** + a **client thread** talking packets through a
local `NetworkManager`. We mirror that exactly:

```
mc-core      math, Java Random, NBT, BlockPos, AABB         (exists)
mc-data      Block/Item/Biome/Entity registries + states   (new)  ← single source of truth
mc-world     chunk storage, light engine, Anvil IO          (rework)
mc-gen       worldgen, bit-exact per seed                   (new)
mc-protocol  protocol 47 codecs, compression, encryption    (new)
mc-server    MinecraftServer/WorldServer tick, entities AI  (new)
mc-client    WorldClient, EntityPlayerSP, input, screens    (rework)
mc-render    meshing, entities, sky, particles (wgpu)       (exists, grows)
mc-gui       screens, font, HUD                             (exists, grows)
tools/       Java golden-data extractors (run against MCP)  (new)
```

Why: it is the only way the same code paths serve singleplayer and multiplayer, it is how vanilla gets
its behaviour (client prediction vs server authority), and it gives the first, safe thread split.

**Multithreading rules (1:1 constraint).** Vanilla tick logic is order-dependent (block updates, light
updates, entity order, `Random` call order). Parallelising it changes results. So:
- Parallel: chunk IO, worldgen noise/terrain phase, client meshing, rendering, packet encode/decode,
  compression, server↔client split, asset loading.
- Serial, in vanilla order: server world tick, scheduled/random block ticks, entity ticks, population
  (decoration) — anything that consumes a shared `Random` or reads results of earlier updates.

## How "1:1" is verified (applies to every segment)

Every segment ships with **golden tests generated from the real Java code**, not hand-written expectations:
- `tools/extract/`: small Java programs compiled against the MCP classes that dump data to JSON/binary
  (registries, state↔meta tables, bounding boxes, generated chunks for a seed, physics traces, packets).
- Rust tests load the dumps and assert exact equality (bit-exact floats where vanilla uses them).
- Visual work uses the existing `--screenshot` tool plus a pixel-diff against vanilla screenshots taken
  at the same camera.
- A segment is done only when its golden tests pass. No "close enough".

---

## Segments

Each segment is sized to be done and reviewed on its own. Segment numbers are names, not the
execution order; the order is below.

### Execution order (decided 2026-10-21): client track first

Two tracks reach full 1:1 in the end. The **client track** comes first because it reaches a usable
product sooner (our client playing on real servers, the original PvP goal) and because a real
vanilla/Spigot server then acts as a free 1:1 oracle for everything the client shows.

1. **Client track:** S1b–S1d → **SJ** → S3 → S5 → S7 (player physics only) → S8 → S9 → S11 → S12 → S13.
   Milestone M1: rustmc joins a local Spigot 1.8.8 server and plays, movement bit-exact.
2. **Server track:** **SH** → S4 → S6 → S7 (rest of the entities) → S10.
   Milestone M2: a vanilla 1.8.9 client plays on the rustmc server in a generated world.
3. M3: singleplayer through the integrated server reaches parity. M4: the S12 performance targets.

Two cross-cutting segments, added to the original list:

**SJ — Java semantics layer** (`mc-core::java`), before anything ticks. Vanilla depends on Java
implementation details that porting the logic does not reproduce: `HashMap`/`HashSet` iteration
order (redstone wire updates neighbours in `HashSet` order), `String.hashCode` (seeds),
`float`/`double` and overflow semantics, `StrictMath`, which `Random` instance is drawn from and when.
Port `HashMap`/`HashSet`/`LinkedHashMap` with identical hashing and iteration, plus `BlockPos.hashCode`
etc., each golden-tested against JDK 8.

**SH — differential harness.** Grow `tools/extract` from static dumps into behaviour checks: run the
real 1.8.9 server headless from the MCP classes, apply scripted actions, record per-tick world/entity
state and packet streams, and replay the same script against rustmc. Later: cross-play (vanilla client
↔ rustmc server, rustmc client ↔ vanilla server).

**Scope of "1:1".** Game logic, physics, worldgen and protocol: bit-exact, including vanilla's bugs
(many mechanics depend on them). Rendering: visually identical within a per-scene screenshot
tolerance (GL fixed-function vs wgpu and driver differences rule out guaranteed bit-identical pixels).
Excluded: dead services (Twitch, Realms, Mojang login → Microsoft auth instead), JVM/LWJGL quirks,
mod loaders.

### S0 — Ground rules & tooling  *(DONE 2026-10-21)*

Delivered: git repo; `mc-core::random::JavaRandom` and `mc-core::strict_math::log` (fdlibm), bit-exact
vs JDK 8; `tools/extract/extract.ps1` → `golden/blocks.json` (all 198 blocks, every state, meta↔state,
static properties); `mc-world/tests/golden_blocks.rs`; `rustmc --bench <s>`.
Baseline (RTX 3060, radius 8, 10 s): avg 3118 fps, 1% low 1293, 0.1% low 923, CPU 0.27 ms/frame.
GPU timestamp queries and a tracy/puffin feature were left for S12, when GPU cost is being tuned.

Findings the golden test surfaced (fixed in `blocks.rs`):
- Unregistered id/meta pairs read as **air** (`ExtendedBlockStorage.get` via `BLOCK_STATE_IDS`), not as
  the block; the table used to render e.g. grass meta 1–15 and acacia/dark-oak variant bits 2–3.
- Grass is `CUTOUT_MIPPED` (side overlay), not solid.

Original scope:
- `git init`, commit current state, `.gitignore` for `target/`, shots.
- `tools/extract/`: build script that compiles extractors against `MCP-919` classes; first extractor
  dumps `Block.blockRegistry` (ids, names, every state, meta↔state) as a smoke test.
- `golden/` directory for dumps (gitignored if large, regenerated by script).
- Port `java.util.Random` bit-exact into `mc-core` (needed by gen, ticks, particles) with tests.
- `--bench` harness from HANDOFF P0 (cheap now, guards against regressions later).
- **Done when:** repo is versioned, one golden dump is produced and asserted by a Rust test.

### S1 — Block registry & states  *(foundation for everything; split into S1a–S1d)*

**S1a — static registry (DONE 2026-10-21).** New crate `mc-data`: `Block` (all 198, every static
property, material, sound, default bounds), `StateId` (all 7806 states in vanilla's `getValidStates`
order, property get/with/cycle, `Display` like `BlockStateBase.toString`), `Material` (all 35).
Tables are generated by `cargo run -p gen-data` from `golden/{blocks,state_ids,materials}.json`; the
generator asserts vanilla's layout invariants, and `mc-data/tests/golden.rs` re-checks every state,
meta, storage id (`id<<4|meta` via `BLOCK_STATE_IDS`, last state wins) and network id
(`id + (meta<<12)`) through the public API. `mc-world` light/AO properties now come from the
registry for every block; output is pixel-identical on the demo terrain.

**S1b — `getActualState` (DONE 2026-10-21).** `mc-data/src/actual_state.rs`: all 19 overriding
classes (52 blocks) hand-ported on `StateId::actual_state` + a `BlockAccess` trait (block states,
flower-pot and comparator tile entities). The registry gained `class_chain` (for `instanceof`),
fire encouragement/flammability, diode/stem fields and `ids::*` constants. Golden:
`golden/actual_states.json`, 104,000 random 3×3×3 scenes evaluated by the real code on a fake
`IBlockAccess` (`tools/extract/.../SceneWorld.java`), with neighbours biased per class towards the
blocks its logic inspects so rare branches are well sampled; `mc-data/tests/actual_state.rs` replays
them all. Not yet wired into rendering (S1c).

**S1c — state → model + tints, renderer switch (IMPLEMENTED 2026-10-21).** `mc-data/src/model_location.rs`
ports `BlockModelShapes` state mappers; `golden/models.json` checks all 7806 states, including built-in
renderer exclusions. `color.rs` ports block color methods and exact 3x3 biome blending; `biome.rs`
implements climate/altitude and swamp/mesa/roofed-forest/mutation color rules using the scalar Java
simplex noise path. Generated climate fields cover all 61 biomes. `golden/colors.json` checks every
state in mixed-biome scenes; `golden/biomes.json` checks 12,200 color/temperature samples against Java
(temperature bits exact). Tests use the real jar's colormaps, including ARGB values and f32→f64 climate.
Renderer resolves actual states before model lookup, indexes a dense state array, shares geometry for
identical model locations, honors graphics-dependent opacity/layers and remeshes on Fancy/Fast
changes. Deleted the partial `mc-world/src/blocks.rs` table. Saved biome and tile-entity NBT now reaches
the world accessors (pots/comparators). Missing variants and unresolved weighted alternatives follow
`ModelBakery` fallback behavior. Regular JSON models for every mapped state bake with real textures.
Regression checks cover chunk-border fences, snowy grass, invalid metadata and fast-leaf occlusion.
Rendered demo screenshots were inspected; full all-block vanilla visual parity remains open for S9
plant offsets, translucent/fluid layers and special renderers. Bounds/side culling landed in S1d below.

**S1d — bounds (IMPLEMENTED 2026-10-21).** `mc-data/src/geometry/` ports state-dependent raw bounds,
`getSelectedBoundingBox`, `getCollisionBoundingBox`, ordered `addCollisionBoxesToList`, per-block
`collisionRayTrace`, and all `shouldSideBeRendered` overrides. The caller owns `BlockGeometry` to
preserve singleton-bound mutations without shared mutable registry data: stair ray-pass persistence,
torch ray bounds, inverted vine bounds, compound collision order, zero-height snow boxes, lily pad
boat exclusions and floating-point coordinate conversion all follow Java. `BlockAccess::moving_piston`
supplies carried state/facing/lastProgress/extending; saved Anvil piston NBT is wired through.
`GeometryWorld.java` runs the real methods without chunk IO or ticks; `golden/geometry.json` replays
37,506 ordered scenes covering all 7806 states, 487,578 axis/oblique/tied/inside/zero/near-parallel rays,
112,518 collision-list probes and 300 moving pistons. Every coordinate is compared by float bits,
including bounds left by each call. Renderer uses the same geometry context and side rules; real-jar
regressions cover glass, mixed-material slabs, inset faces and existing connected models. Full
vanilla visual comparison is still open. World collision enumeration and voxel ray traversal will be
added with world/player logic; the per-block APIs are ready for them.

Original scope:
- `mc-data`: every 1.8.9 block (ids 0–197) with its `BlockState` properties, exact
  `getStateFromMeta` / `getMetaFromState`, `getActualState` (fences, walls, panes, stairs shape,
  redstone wire, doors, vines, grass snowy, …), material, hardness, resistance, light opacity/emission,
  `isOpaqueCube`, `isFullCube`, render layer, render type, collision + selection boxes
  (`setBlockBoundsBasedOnState`, `addCollisionBoxesToList`), slipperiness, sound type.
- Blockstate-JSON variant key generated from properties (replaces the hand table in `blocks.rs`).
- Renderer switches to the registry; every block in the game renders (all models, not ~80).
- **Done when:** all states, metas, boxes and flags match the Java dump; screenshot of a test world
  with every block placed matches vanilla.

### S2 — Item registry  *(parallel with S3 after S1)*
- All items (256–431, records 2256–2267) and `ItemBlock`s: max stack, damage, item models, NBT on stacks.
- **Done when:** registry dump matches.

### S3 — Chunk & world storage, light engine
- `Chunk`/`ExtendedBlockStorage` exact: heightmap, `precipitationHeightMap`, skylight map
  (`generateSkylightMap`, `relightBlock`), `isModified`, tile-entity map, entity lists per section.
- Vanilla light engine port: `World.checkLightFor` incremental updates (not steady state), with its
  quirks; `checkLight` on populate.
- Storage designed for concurrent readers: chunks in a map of `Arc`s or a slot table; per-section dirty
  flags so the client mesher can snapshot sections while the world mutates.
- Anvil load **and save** (region files, NBT), level.dat.
- **Done when:** light after a scripted sequence of `setBlockState` calls matches a Java dump; loading
  then saving a vanilla world round-trips.

### S4 — World generation  *(big; split into four reviews)*
- 4a `GenLayer` biome stack + `WorldChunkManager`, `NoiseGeneratorOctaves/Perlin/Simplex`,
  `ChunkProviderGenerate.setBlocksInChunk` / `replaceBlocksForBiome`, superflat + amplified + large biomes
  + customized presets.
- 4b Carvers: `MapGenCaves`, `MapGenRavine`.
- 4c Structures: villages, strongholds, mineshafts, temples, ocean monuments (`MapGenStructure` + pieces).
- 4d Population: `BiomeDecorator`, all `WorldGen*` features (trees, ores, lakes, dungeons, flowers…),
  snow/ice, initial mob spawns. Nether (`ChunkProviderHell`) and End (`ChunkProviderEnd`).
- Threading: 4a/4b per chunk in parallel; 4c/4d serial in vanilla order.
- **Done when:** for several seeds, every generated chunk in a 32×32 area is byte-identical to Java
  (blocks, metas, biomes, heightmap, light).

### S5 — Protocol 47  *(parallel with S4)*
- `mc-protocol`: VarInt/strings/slots/metadata/chunk-data encodings; every Handshake/Status/Login/Play
  packet both directions; zlib compression threshold; AES/CFB8 encryption.
- Local in-memory connection (integrated server) + TCP connection, same `NetworkManager` API.
- **Done when:** every packet round-trips; captured byte streams from a Java client/server decode and
  re-encode identically.

### S6 — Server core
- `MinecraftServer` 20 TPS loop, `WorldServer.tick` (time, weather, scheduled block updates
  `NextTickListEntry`, random ticks, `PlayerManager` chunk tracking, `EntityTracker`), `PlayerList`,
  `NetHandlerPlayServer`, `ServerConfigurationManager`, commands framework, `server.properties`.
- Block behaviours in sub-reviews: (a) physics blocks (fluids, falling blocks, fire, farmland, crops,
  grass/mycelium spread, leaves decay), (b) redstone (wire, torches, repeaters, comparators, pistons,
  hoppers, droppers/dispensers), (c) interactive blocks (doors, buttons, levers, beds, chests…).
- **Done when:** golden tick traces (block states after N ticks from a seeded start) match Java for each
  behaviour group; a vanilla 1.8.9 client can connect to our server and play.

### S7 — Entities, player & physics  *(PvP-critical)*
- `Entity.moveEntity` (collision, step-up, sneaking edge logic), `EntityLivingBase` (gravity, friction,
  jump, knockback, damage, potion effects), `EntityPlayer`, `EntityPlayerSP` + `PlayerControllerMP`
  (prediction, position packets, block breaking timing, reach), `MovementInput`.
- Replay-diff harness: recorded input sequences → per-tick position/motion traces; must be 0-bit.
- Then the rest of the entity zoo in groups: items/XP/projectiles/TNT/falling blocks; passive mobs;
  hostile mobs + AI tasks + pathfinding; villagers; boss mobs; minecarts/boats; hanging entities.
- **Done when:** player traces match exactly; each mob group's tick trace matches.

### S8 — Client rework on top of the server
- `WorldClient` fed by packets (chunk data, block change, multi-block change, entities); client tick.
- Meshing moves to section level with a **rayon worker pool**, padded 18³ snapshots, priority by camera
  distance, incremental uploads, dirty-section rebuild (HANDOFF P1 — lands here).
- Port `RenderGlobal` visibility (`VisGraph`, `setupTerrain` BFS) — vanilla culling is part of 1:1 look.
- **Done when:** singleplayer runs through the integrated server; breaking/placing a block rebuilds in
  < 2 ms; no frame hitch while chunks stream in.

### S9 — Rendering completeness  *(split by feature)*
- Fluids (`BlockFluidRenderer`) + translucent layer with sorting; all special block renderers.
- Sky, sun/moon/stars, clouds (fast/fancy), fog (`setupFog`, `updateFogColor`), weather (rain/snow),
  day/night lightmap, mipmaps (`TextureUtil`), animated textures, render distance & view bobbing.
- Entity models + `RenderManager` renderers, tile-entity renderers (chests, signs, skulls, banners,
  beacons…), item rendering (GUI, first person, dropped, item frames), armour layers, enchant glint.
- Particles (`EffectRenderer`, all particle types), block-break cracks, selection box.
- **Done when:** screenshot diffs vs vanilla are pixel-identical for a fixed set of scenes.

### S10 — Gameplay systems
- Inventory & containers (`Container*`, slot click logic exact), crafting/furnace/brewing/enchanting/anvil
  recipes, combat & damage calc, hunger, XP, potions, enchantments, achievements & stats, scoreboard,
  villages/trading, game modes, death/respawn, dimensions/portals.
- **Done when:** golden tests per system (e.g. every recipe, slot-click sequences) match.

### S11 — GUI completeness & audio
- Every `Gui*` screen (inventories, world select/create, multiplayer list, resource packs, language,
  sound/skin options, chat options, stats, achievements…), `SoundManager` (paulscode → Rust audio), music.
- **Done when:** screenshot diffs match for every screen.

### S12 — Performance pass  *(HANDOFF P2–P5)*
- Vertex arena + compact vertex, multi-draw-indirect, per-face buckets, GPU culling, frame pacing,
  instanced entities, batched text, GPU particles. Must stay pixel-identical (S9 diffs are the guard).
- **Done when:** `--bench` beats Lunar at equal settings on average FPS and 1% lows.

### S13 — Online multiplayer
- Microsoft/Xbox auth, session server join, server list ping; tested against local Spigot 1.8.8 + Grim
  before any public server; brand `vanilla`, no extra packets.

---

## Next action

Follow the execution order above. Current position: **SJ** (Java collection/hash/string/overflow
semantics, golden-tested against JDK 8), then S3 and S5. S1c and S1d mappings/tints/geometry are in
place; the client is still a terrain viewer, with no protocol/player/gameplay implementation yet.
