# rustmc — roadmap to a 1:1 Minecraft 1.8.9 in Rust

Goal: everything 1:1 with vanilla 1.8.9 (behaviour, visuals, protocol), multithreaded where vanilla's
semantics allow it. Reference: `../MCP-919/src/minecraft/` (read-only, never copied in).

## Where we are (audit, 2026-10-21)

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

Each segment is sized to be done and reviewed on its own. Do them in order unless marked parallel.

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

**S1b — `getActualState`.** Hand-port every override (fences, walls, panes, stairs shape, redstone
wire, doors, vines, fire, grass/mycelium/podzol snowy, tripwire, flower pots, …) on `StateId` +
a `BlockAccess` trait. Golden: extractor builds a fake `IBlockAccess` and dumps actual states for
every block over a set of neighbour configurations.

**S1c — state → model + tints, renderer switch.** Port `BlockModelShapes` state mappers (blockstate
file, ignored properties, per-variant names) and block colors (`colorMultiplier`/`getRenderColor`).
Renderer bakes every state; culling uses `is_opaque_cube(fancy_graphics)` from settings. Delete the
`mc-world/src/blocks.rs` table. Golden: dump `BlockModelShapes` variant locations for every state.

**S1d — bounds.** `setBlockBoundsBasedOnState`, `addCollisionBoxesToList`, `getSelectedBoundingBox`,
`collisionRayTrace` per state/neighbours. Golden: same fake-world harness as S1b.

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

**S0, then S1.** S1 (the block registry) is the single biggest blocker: world storage, lighting,
worldgen, physics (collision boxes), the renderer and the protocol all read block states. Until it
exists, every other system would be built on the provisional `blocks.rs` table and rewritten later.
