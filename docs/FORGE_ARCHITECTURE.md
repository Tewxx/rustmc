# Forge architecture: Rust core, JVM at the edges

Goal: every Forge 1.8.9 mod works — plain `@Mod`s, GUI/HUD mods, Mixin mods, ASM coremods — while the game
itself stays Rust and stays fast. We never fall back to "run real Minecraft". Inputs: `docs/FORGE_COMPAT.md`
(what real mods touch), `docs/forge-spec/` (the Forge contract), `tests/forge/` (differential tests).

## The core problem

Mixins and coremods patch *bytecode of specific vanilla methods*, often at specific instructions
(`INVOKE List.size` inside `EntityRenderer.getMouseOver`). Rust-backed stub classes have no such
instructions, so a stub-only design breaks every serious mod. Running all of vanilla in the JVM throws away
the point of rustmc. The answer is to decide **per method**, from the installed mod set, where code runs.

## 1. Shadow Minecraft: real bytecode, Rust state

At setup (once per jar/mod-set change, cached):
1. Take the user's own `1.8.9.jar` (or Prism's client jar), apply Forge's `binpatches.pkg.lzma` from the Forge
   universal jar → Forge-patched vanilla, then remap notch→SRG with the MCP mappings. This is byte-for-byte
   the class set real mods are compiled and mixed against (same names, descriptors, instructions, and
   Forge's own event-post call sites). Nothing Mojang/Forge is redistributed; it's built locally.
2. **Nativisation pass** (our own ASM pass, runs *before* any mod transformer): leaf subsystem methods whose
   work Rust owns — world block/entity storage and queries, chunk access, lighting, networking I/O, sound,
   texture upload, GL — get their bodies replaced by `native` calls into Rust. Everything else keeps its
   vanilla body, so mixin injection points still exist.
3. **Conflict rule:** if a loaded mod targets a method we planned to nativise, that method keeps its
   vanilla body (its own callees stay native). Decided per mod set; logged.

Result: a "shadow" class graph whose objects are thin views over Rust state, with vanilla control flow
available wherever mods need it.

## 2. Per-method hosting: Java only where a mod hooks

After the full transformer chain (Forge ATs, coremod `IClassTransformer`s, Mixin) runs, diff each class
against its pristine shadow. A method is **hooked** if any transformer changed it.

- Rust drives the game loop (tick, input, render, network) exactly as today.
- At every point where vanilla would call a method, Rust has a **dispatch slot**. Unhooked → Rust runs its
  own implementation, no JVM crossing. Hooked → Rust calls the Java method via a cached JNI method ID; the
  mod's injected code runs inside vanilla's body; inner calls land back in Rust via the native leaves.
- Slots are resolved once at load into a table, so the hot path is one indexed branch. A vanilla install
  with no mods never starts a JVM.

Example: Meowtils injects into `Minecraft.runTick` and `EntityRenderer.getMouseOver`. Those two run as Java
(with their injections); `World.getEntitiesWithinAABBExcludingEntity` inside `getMouseOver` is native, so
the expensive part stays Rust. Everything else stays Rust.

## 3. State bridge: fields mods can see

Mods read and write real fields (`thePlayer.posX`, `gameSettings.keyBindAttack`), often by reflection, so
fields need real JVM storage.

- **Static field-access analysis** of every loaded mod (+ the hooked methods' bodies) lists exactly which
  fields can be touched (`tools/modscan` already extracts this). Only those fields are mirrored.
- **Sync points:** Rust → Java copy of mirrored fields before any Java dispatch in a phase (once per tick /
  frame, batched, only dirty objects); Java → Rust diff after. Reflective access to unlisted fields is
  covered by mirroring the full field set of the classes mods reflect on (`setAccessible` targets are
  statically visible too); a generous default set (player, world, settings, current screen) is always mirrored.
- Object identity: one Java object per Rust entity/chunk/screen, created lazily, kept in a slab keyed by Rust
  id, released on removal — so `==` and maps keyed by entities behave.

## 4. Events: one bus, posted only when someone listens

Rust core posts every Forge event at the same point Forge does (`docs/forge-spec/events.json` lists fire
sites). Posting to Java happens only if a Java subscriber exists for that event class (bitset check) —
otherwise zero cost. Cancel/result flow back into Rust. WASM mods (`docs/WASM_MODS.md`) share the bus.
When a hooked method runs in Java, Forge's own post sites inside it fire naturally; Rust suppresses its
duplicate for that call.

## 5. Rendering: GL through Rust

Mods draw through `GlStateManager`, `Tessellator`/`WorldRenderer`, `FontRenderer`, raw `GL11`, display lists,
FBOs and sometimes GL20 shaders.

- An LWJGL2-compatible Java API surface (`org.lwjgl.opengl.GL11/12/13/14/15/20/ARB*/EXT*`, `Display`,
  `Keyboard`, `Mouse`) implemented as natives into a Rust **GL emulation layer**: it records fixed-function
  state + immediate-mode geometry into batches and maps them to wgpu pipelines (state-keyed pipeline cache,
  per-frame vertex arena). This is the same draw layer U1 builds for vanilla's GUI, so mod UIs and vanilla UIs
  go through one path and one batch.
- Display lists → recorded command buffers replayed by Rust. FBOs → wgpu render targets.
- GL20 shaders: GLSL 1.x upgraded by a small preprocessor, then translated with naga's GLSL frontend to WGSL;
  unsupported constructs report as missing capabilities rather than crashing.

## 6. Loading, Forge itself, networking

- Real `LaunchClassLoader` + tweaker chain (FMLTweaker, MixinTweaker, coremod plugins) inside the embedded
  JVM, with the shadow jar as "minecraft". Real FML/Forge classes load on top (they are written against
  the patched vanilla we reproduce), so lifecycle, `@Mod`, EventBus, Configuration, registries, keybinds and
  the `FML|HS` handshake behave like Forge — then Rust replaces only their I/O leaves.
- Networking: Rust owns the socket and codec. `NetworkManager.channelRead0`/`sendPacket` are dispatch slots:
  hooked → Rust materialises the Java `Packet` object (generated codecs Rust ↔ Java) and calls in. Custom
  payload channels (`FMLProxyPacket`, `SimpleNetworkWrapper`) bridge to the same path.

## 7. Safety

Mods run in-process, so: OptiFine is never loaded (`docs/FORGE_COMPAT.md` signals); a Java 8
`SecurityManager` denies process spawning, native library loading outside an allowlist, and sockets except
Mojang/Microsoft auth hosts unless the user grants them per mod; mods see a `Session` facade, never the
real access token; update/download-and-run code paths are refused.

## 8. Performance budget

- No mods: no JVM. Mods with no hooks: JVM starts lazily; per-frame cost = events with subscribers only.
- Every JNI crossing is counted per slot in the profiler; the bench gains a `forge` scenario.
- Targets: plain HUD mod adds < 0.1 ms/frame; a 30-mixin client adds < 1 ms/frame at 20 TPS + 240 fps.

## 9. Build order (each step ends with `tests/forge` cases passing against golden traces)

1. **Spike (de-risk first):** build the shadow jar (binpatches + SRG), boot an embedded HotSpot from Rust
   with LaunchClassLoader + Mixin, load FT1's own mixin test mod, detect hooked methods by diffing, and call
   one hooked method from Rust with natives stubbed. Measure JNI dispatch cost.
2. Lifecycle + EventBus + tick/input/chat events + field mirror for player/world/settings.
3. GL emulation layer + GUI/HUD (`GuiScreen`, `FontRenderer`, `RenderGameOverlayEvent`).
4. Hooked-method dispatch across the client loop (runTick, clickMouse, getMouseOver, orientCamera, renderers).
5. Network dispatch + custom payloads + `FML|HS`.
6. Coremods/ASM transformers, Forge-class patching (Lunar), GL20 shader translation.
7. Corpus smoke tier (user-approved, sandboxed, offline-mode test server only).
