# Native WASM mods for rustmc

Status: design spec (WM0). An experimental subset exists; see [`crates/mc-mods/README.md`](../crates/mc-mods/README.md) for its actual implementation, security limits, and `rustmc:plugin@1.0.0` ABI. The milestones below remain goals. Target: rustmc 1.8.9 client, multiplayer only.

A WASM mod is a sandboxed, JVM-free plugin loaded by rustmc through an embedded WebAssembly runtime. One
`.wasm` runs unchanged on macOS, Windows and Linux. This is the "light mod" path (HUDs, utilities,
settings screens, overlays); heavy content mods stay on the Forge/HotSpot path (Wave 5). Both sit on one
Rust event bus (section 6).

Design pillars, in priority order: (1) the client cannot be crashed or frozen by a mod, (2) zero cost when
idle and bounded cost when active, (3) deterministic and testable, (4) stable across rustmc updates.

---

## 1. Runtime

**Decision: wasmtime, component model + WIT, Cranelift AOT with an on-disk cache, epoch interruption.**

| Choice | Pick | Why | Rejected |
|---|---|---|---|
| Engine | `wasmtime` | Best codegen (Cranelift), first-class component model and `wit-bindgen`/`bindgen!`, epoch interruption, `ResourceLimiter`, pooling allocator, NaN canonicalisation, builds on all three OSes without LLVM, strong security process. | `wasmer`: component model lags, three backends of uneven quality, LLVM backend is a build burden. `wasmi`/`wasm3`: interpreters, 10-50x slower; a per-frame HUD hook must not cost milliseconds. |
| ABI | Component model + WIT | Typed, versioned interfaces; bindings for Rust, C, Go, JS, C# via `wit-bindgen`; the host owns the contract. Imports are capability-gated by simply not linking them. | Core wasm + hand-rolled `ptr,len` ABI: every language needs a hand-written SDK, no type safety, versioning by convention. We use a raw byte format only in one place (the draw stream, 4.4). |
| Compile | Cranelift AOT at install, `Module::serialize` into `<gameDir>/mods-wasm/.cache/<sha256>-<wasmtime-ver>-<target>.cwasm` | No JIT stall at launch; cache keyed by content hash + engine version + CPU features; on mismatch, recompile in the background. Loading a `.cwasm` is mmap + relocations (sub-ms). | Compile every launch: 50-300 ms per mod. Winch (baseline compiler) is used only for hot-reload in dev. |
| Interruption | **Epoch interruption** | A watchdog thread bumps the engine epoch every 1 ms; each hook call sets `store.set_epoch_deadline(budget_ms)`. Overhead 1-3%. | Fuel: exact and deterministic but 10-30% slower. Fuel is used only in the test harness (section 9). |
| Memory | `StoreLimits`: 32 MiB linear memory default (manifest may request up to 128 MiB, shown in the prompt), 64k table elements, 1 instance per store, pooling allocator with guard pages, `max_wasm_stack` 512 KiB | A mod cannot OOM the process. A refused `memory.grow` returns -1 to the guest and counts as a strike. | Unlimited. |
| Determinism | NaN canonicalisation on; wasm threads, relaxed-SIMD and memory64 off; no clock or entropy imports except host `game-time` and a seeded `random`; plain SIMD128 allowed | Same inputs give the same outputs on every OS and CPU, which replay tests need. | n/a |

**Isolation and failure policy.** One `Store` + `Instance` per mod; nothing is shared between mods except via
host-mediated events. A trap (panic, unreachable, OOB, stack overflow, epoch timeout, OOM) in any hook:
1. unwinds only that call; the host never holds a lock across a guest call;
2. the mod gets a strike and the hook output is discarded (draw list dropped, event passes through unmodified);
3. a **trap** disables the mod for the session: store dropped, toast ("Mod *Keystrokes* crashed and was
   disabled", click-through to the log, "Restart mod" button), crash report written to
   `<gameDir>/mods-wasm/crashes/<id>-<ts>.txt` (wasm backtrace, mod/API version, last 50 log lines);
4. **budget overruns** (soft failures) skip that frame's output; 3 strikes within 60 s disable the mod;
5. a mod disabled twice in one launch is switched off in `state.toml` until the user re-enables it.

Every host-to-guest call is also wrapped in `catch_unwind`, so a host-side panic in an imported function cannot
kill the client either.

---

## 2. Packaging

`<name>.rmod` is a zip, read in place (never extracted, so zip-slip cannot happen; entry names with `..`,
absolute paths or symlinks are rejected).

```
keystrokes.rmod
  mod.toml              manifest (required)
  mod.wasm              component (path set by `entry`)
  assets/               optional resource-pack layer (pack.mcmeta synthesised)
    assets/keystrokes/textures/gui/key.png
    assets/keystrokes/lang/en_US.lang
    assets/keystrokes/sounds.json + sounds/*.ogg
  config.default.toml   optional defaults
  SIGNATURE             optional detached ed25519 over the canonical archive hash (M4)
```

```toml
# mod.toml
[mod]
id          = "keystrokes"        # [a-z0-9_]{2,32}, unique, immutable
name        = "Keystrokes HUD"
version     = "1.2.0"             # semver of the mod
authors     = ["Benjy"]
description = "WASD/mouse/CPS overlay"
license     = "MIT"
entry       = "mod.wasm"
side        = "client"            # only value in v1

[api]
version = "1.0"                   # API the mod was built against
min     = "1.0"                   # lowest host API it can run on

[capabilities]                    # everything absent is denied
hud            = true             # 2D overlay
world_render   = false            # world-space primitives
screens        = true             # own GUI screens
input_read     = true             # key/mouse events and state
keybinds       = true             # register keybinds in Controls
config         = true             # own config dir only
chat_read      = false
chat_send      = false            # FLAGGED
packets_read   = false            # sensitive
packets_modify = false            # FLAGGED
input_inject   = false            # FLAGGED
clipboard      = false            # sensitive
network        = []               # exact https hosts, e.g. ["api.example.com"]; sensitive
memory_mib     = 32

[dependencies]
rustmc  = ">=0.9"                 # host version range
libbase = { version = "^1.0", optional = true }

[load]
after    = ["libbase"]            # ordering hints
before   = []
priority = 0                      # tiebreak, lower first
```

Rules: unknown keys warn; an unknown *capability* is a hard error (an old host never silently grants a power it
does not understand). Duplicate `id`: highest version wins, the loser is listed as "Shadowed". Dependencies must
form a DAG; unmet or cyclic required deps disable the mod with an explanation.

**Location.** `<gameDir>/mods-wasm/*.rmod`, separate from Forge's `<gameDir>/mods/*.jar`. Per-mod data lives in
`<gameDir>/mods-wasm/config/<id>/`, the only filesystem location a mod can touch (through the `config`
interface; no WASI filesystem is linked). Enabled state and granted capabilities: `mods-wasm/state.toml`. Under
Prism, `gameDir` is the instance's `.minecraft` (`mc-client/src/launch.rs`), so mods are per instance for free.

**Enable/disable UI.** New `GuiScreenMods` (title screen and Options, "Mods..."), built from `mc-gui` widgets
and the list layout already used by `resource_packs.rs` / `multiplayer.rs`. Per row: icon, name, version, API
badge, status (Active / Disabled / Crashed / Incompatible / Blocked), toggle, Permissions, Config. Toggling is
live (hot load/unload, section 7); only mods carrying an asset layer trigger the shared resource reload that
`resource_pack.rs`/`client_packs.rs` already do.

---

## 3. Threading and frame model

Goal: the main (tick + render) thread never waits on a mod unless a hook is semantically synchronous, and
then only for a bounded time and only if someone subscribed.

```
main thread:  [input][net + tick @20TPS][world pass][HUD/GUI draw][present]
                         | publish Snapshot (Arc)     ^ takes latest finished DrawList per mod
                         v                            |
mod thread:   hook queue --> run hooks (per-hook epoch budget) --> DrawList double buffer
```

* **Mod worker thread.** One OS thread (`rustmc-mods`, lowered priority) runs all async hooks of all mods in
  load order. wasmtime `Store`s are `Send`, so every mod's store lives there. Each hook has its own budget; the
  thread cannot be monopolised by one mod because the epoch deadline interrupts it. (Per-mod threads are a later
  option; nothing in the design depends on one thread.)
* **Async hooks (default):** `on-tick`, `on-frame`, `render-hud`, `render-world`, `render-screen`, and all
  notifications (entity spawn/update/remove, block change, join/leave, screen open/close, task results). The
  main thread only enqueues a few words plus an `Arc` clone and never blocks. If the worker is behind, frame
  hooks are coalesced (skipped), never queued.
* **Output of render hooks is a `DrawList`** (packed command stream, 4.4), produced into a per-mod double
  buffer. When building a frame the main thread takes the latest *completed* list per mod (atomic swap,
  wait-free) and appends it to `GuiBatch` / the world-pass buffer. Latency: at most one frame. If a mod misses its
  deadline its previous list is reused for up to 3 frames, then dropped, so a hitching mod flickers; it never
  stalls the game.
* **Synchronous hooks (opt-in, cancellable):** `on-packet-send`, `on-packet-receive`, `on-chat-send`,
  `on-chat-receive`, `on-key`, `on-mouse`. They run on the main thread at the exact event site (before
  `handle_packet` in `net_handler.rs::process_net`; before `Net::send`), with a hard budget of **0.5 ms per
  event shared across subscribers**. With no subscriber the cost is one relaxed atomic load per event site.
  Subscribers register a *filter* (packet ids, chat prefix) so the host crosses into wasm only on matches.
  Timeout means the event passes through unmodified plus a strike.

| Hook | Runs on | Default budget |
|---|---|---|
| `on-tick` | worker, after each client tick | 5 ms |
| `render-hud`, `render-screen` | worker, once per frame (coalesced) | 2 ms |
| `render-world` | worker | 2 ms |
| sync hooks (packet/chat/input) | main | 0.5 ms |
| `on-load` / `on-unload` | worker | 500 ms / 100 ms |
| task-result callbacks | worker | 2 ms |

  The host keeps a per-mod EMA of hook time, shown in the Mods screen and F3 (`Mods: 0.12 ms`). A mod above
  50% of budget for 5 s gets a "slow mod" toast, not a disable.
* **Background work.** No threads inside wasm. `http.get`/`post` (capability) and `tasks.after-ms` run on a
  2-thread host pool; results arrive as `on-task-result`. CPU-heavy mod work must be split across ticks.
* **Reading game state without locks.** At the end of each client tick (`tick.rs::run_tick`) the host builds an
  immutable `Snapshot` and publishes it with an `ArcSwap`. The worker pins the latest `Arc<Snapshot>` for a hook,
  so reads never contend with the main thread. Frame hooks also get `frame-ctx` (partial ticks, camera) to
  interpolate. Snapshot contents:
  * player pos/prev/motion/rotation/health/food/xp/slot/flags, 46 inventory stacks (NBT lazily fetched);
  * entities within 128 blocks as struct-of-arrays, **built only if some mod subscribed entities**;
  * tab list, scoreboard sidebar, server info, dimension/time/weather;
  * blocks go through a **read-only chunk view** (`Arc` of the chunk map; M2 must confirm `mc-world` chunk
    storage can be shared copy-on-write, otherwise the snapshot carries a 5x5-chunk window), not through the snapshot.
  Query results cross the boundary as flat `record`s (the canonical ABI lowers them with plain copies); bulk
  results return `list<record>`.
* **Idle cost.** No subscribed mod means only the player block is snapshotted, no queue traffic, no draw
  lists. Snapshot build budgets: 50 us (player only), 200 us (with entities); enforced in CI.

---

## 4. API surface

One WIT package, `rustmc:mod@1.0.0`. A mod's `world` imports only the interfaces its capabilities allow, and
the host links only granted ones. The core follows; the full `.wit` lives in the SDK repo.

### 4.1 Core types and lifecycle

```wit
package rustmc:mod@1.0.0;

interface types {
  record vec3 { x: f64, y: f64, z: f64 }
  record block-pos { x: s32, y: s32, z: s32 }
  record item-stack { id: s16, damage: s16, count: u8, has-nbt: bool }
  record frame-ctx {
    partial-ticks: f32, fps: u32,
    gui-width: u32, gui-height: u32, gui-scale: u32,   // scaled units, like vanilla ScaledResolution
    mouse-x: s32, mouse-y: s32,
    cam: vec3, cam-yaw: f32, cam-pitch: f32, fov: f32,
  }
  type argb = u32;
  enum log-level { trace, debug, info, warn, error }
}

interface lifecycle {            // exported by the mod
  use types.{frame-ctx};
  on-load:   func(host-api: string, features: list<string>) -> result<_, string>;
  on-unload: func();
  on-tick:   func();
  on-frame:  func(ctx: frame-ctx);
}
```

### 4.2 Events (exported by the mod; each interface is optional)

```wit
interface events {
  use types.{block-pos, vec3};

  enum screen-kind { chat, inventory, container, pause, options, controls, multiplayer, other, mod-screen }
  record key-event   { code: s32, down: bool, repeat: bool, modifiers: u8, char: u32 }  // LWJGL codes, mc-client/src/keymap.rs
  record mouse-event { x: s32, y: s32, button: s8, down: bool, wheel: s32 }
  record entity-ref  { id: s32, kind: u8, pos: vec3 }
  variant verdict<T> { pass, cancel, replace(T) }

  // synchronous, main thread, filtered, 0.5 ms
  on-key:            func(e: key-event) -> bool;                  // true = consume (needs input_inject)
  on-mouse:          func(e: mouse-event) -> bool;
  on-chat-send:      func(msg: string) -> verdict<string>;       // cancel/replace needs chat_send
  on-chat-receive:   func(json: string, plain: string, position: u8) -> verdict<string>;
  on-packet-send:    func(id: u8, body: list<u8>) -> verdict<list<u8>>;
  on-packet-receive: func(id: u8, body: list<u8>) -> verdict<list<u8>>;

  // asynchronous notifications, worker thread
  on-keybind:        func(id: string, down: bool);
  on-world-join:     func(server: string, dimension: s32);
  on-world-leave:    func(reason: string);
  on-screen-open:    func(kind: screen-kind, title: string);
  on-screen-close:   func(kind: screen-kind);
  on-entity-spawn:   func(e: entity-ref);
  on-entity-update:  func(e: entity-ref);        // coalesced to once per tick
  on-entity-remove:  func(id: s32);
  on-block-change:   func(pos: block-pos, old-state: u16, new-state: u16);
  on-task-result:    func(id: u32, ok: bool, data: list<u8>);
}

interface render {                // exported by the mod
  use types.{frame-ctx};
  render-hud:    func(ctx: frame-ctx);                      // emits via ui-draw.submit
  render-world:  func(ctx: frame-ctx);                      // emits via world-draw.submit
  render-screen: func(ctx: frame-ctx, kind: events.screen-kind);   // overlay on vanilla screens
}
```

Packet bodies are the payload bytes in the wire format `mc-protocol` already parses
(`crates/mc-protocol/src/{buf,packets}.rs`). `replace` bytes are re-parsed by the same decoder and
**rejected if they fail to parse or grow by more than 4 KiB**. Receive hooks cover an allow-list of play-state
ids (chat, entity, block, scoreboard, title...); never handshake/login/Forge-handshake states, keep-alive,
resource-pack responses.

### 4.3 Queries and actions (imported by the mod)

```wit
interface game {                  // queries; read the pinned snapshot, no locks
  use types.{vec3, block-pos, item-stack};
  record player {
    pos: vec3, prev-pos: vec3, motion: vec3, yaw: f32, pitch: f32, on-ground: bool,
    health: f32, food: s32, saturation: f32, xp-level: s32, selected-slot: u8,
    sprinting: bool, sneaking: bool, flying: bool, gamemode: u8,
  }
  record entity { id: s32, kind: u8, pos: vec3, prev-pos: vec3, yaw: f32, pitch: f32, flags: u8, name: option<string>, health: option<f32> }
  record tab-entry { name: string, display: option<string>, ping: s32, gamemode: u8, uuid: string }
  record server-info { address: string, motd: string, ping-ms: s32, players: u32, max-players: u32, brand: option<string> }

  player:        func() -> player;
  inventory:     func() -> list<item-stack>;                 // 46 slots
  entities-near: func(radius: f32, kinds: list<u8>) -> list<entity>;
  block-state:   func(p: block-pos) -> option<u16>;          // none = chunk not loaded
  block-name:    func(state: u16) -> string;
  tab-list:      func() -> list<tab-entry>;
  sidebar:       func() -> option<tuple<string, list<tuple<string, s32>>>>;
  server:        func() -> option<server-info>;
  game-time:     func() -> u64;                              // ticks; the only clock
  option:        func(key: string) -> option<string>;        // read-only vanilla options (fov, guiScale...)
}

interface actions {
  send-chat:     func(msg: string);                          // cap chat_send; 5/s; vanilla 100-char limit
  send-command:  func(cmd: string);
  send-packet:   func(id: u8, body: list<u8>) -> result<_, string>;   // cap packets_modify; allow-list below
  play-sound:    func(name: string, volume: f32, pitch: f32);// local only
  open-screen:   func(id: string);                           // one of this mod's registered screens
  close-screen:  func();
  set-key-state: func(keybind: string, down: bool);          // cap input_inject, FLAGGED; expires after 1 tick unless renewed
  toast:         func(title: string, body: string);
}
```

`send-packet` allow-list in v1: plugin messages on channels the mod registered (same registry FG1 builds for
`REGISTER`), and client-settings. The host owns movement, digging, block placement, entity-use, window-click
and keep-alive; those are not sendable.

### 4.4 2D rendering (immediate mode over a packed draw stream)

A command stream avoids one boundary crossing per primitive. The SDK encodes calls into a little-endian byte
stream (op byte + fixed operands; strings interned in a table at the head) and calls `submit` once per hook.
The host decodes directly into `GuiBatch` with bounds checks.

```wit
interface ui-draw {
  // ops (format v1; byte layout in docs/DRAW_STREAM.md):
  //  0 fill-rect(l,t,r,b,argb)             -> GuiBatch::rect
  //  1 gradient-rect(l,t,r,b,argb,argb)    -> GuiBatch::gradient
  //  2 textured-rect(tex,x,y,u,v,w,h)      -> GuiBatch::textured_rect / textured_rect_uv
  //  3 text(str,x,y,argb,shadow)           -> FontRenderer::draw / draw_shadow
  //  4 item-icon(id,damage,count,x,y,decor)-> GUI item renderer (mc-render/src/items.rs)
  //  5 push-clip(x,y,w,h)  6 pop-clip      -> scissor
  //  7 push-transform(tx,ty,scale)  8 pop
  //  9 bind-texture(handle)
  submit:      func(stream: list<u8>);
  text-width:  func(s: string) -> s32;               // FontRenderer::width
  load-texture: func(path: string) -> option<u32>;   // "modid:textures/gui/key.png" from the mod's layer, or a vanilla path
}
```

Limits per list: 16k commands, 256 KiB, nesting depth 16; overflow drops that list (strike, no disable).
Coordinates are vanilla scaled-GUI units and clamped to the screen; colours are ARGB; text goes through the
same `FontRenderer` (section codes, glyph fallback) so it is pixel-identical to vanilla. No custom shaders, raw GL
or pixel readback.

### 4.5 World-space rendering (cap `world_render`, deliberately limited)

```wit
interface world-draw {
  // same stream mechanism; ops: line(a,b,argb,width), box(min,max,argb,fill), billboard-text(pos,str,argb,scale),
  // billboard-quad(pos,w,h,tex,uv)
  submit: func(stream: list<u8>, depth-test: bool);
}
```

Coordinates are absolute world f64; the host subtracts the *current* camera when consuming (as the entity/tile
renderers in `mc-render/src/{tile,objects}.rs` do), so a one-frame-late list does not swim when the camera
moves. Drawn in a new pass in `mc-render/src/gpu.rs` after translucent terrain and entities, before the HUD, as
one instanced line/quad batch sharing the GUI vertex format. Caps: 4096 primitives per mod per frame, 64 textures.
Not offered in v1: custom meshes or shaders, altering chunk/entity/model rendering, render-to-texture.

### 4.6 Screens, keybinds, config, HTTP, logging

```wit
interface screens {               // cap screens
  record widget { id: u32, x: s32, y: s32, w: s32, h: s32, kind: widget-kind, label: string }
  variant widget-kind {
    button(bool),                        // enabled
    toggle(bool),                        // value
    slider(slider-spec),                 // value 0..1, steps
    text-field(text-spec),
    label,
  }
  record slider-spec { value: f32, steps: u32 }
  record text-spec   { text: string, max-len: u32, placeholder: string }
  variant value { b(bool), f(f32), s(string), click }
  register: func(id: string, title: string);
  // exported by the mod for each registered id:
  //   init:      func(id: string, w: s32, h: s32) -> list<widget>;
  //   on-widget: func(id: string, widget: u32, v: value);
  //   draw:      func(id: string, ctx: frame-ctx);       // optional extra drawing via ui-draw
  //   on-close:  func(id: string);
}
```

Widgets are **host-owned**: the mod describes them and the host instantiates the real `mc-gui` widgets
(`widgets::Button`, sliders, `text_field.rs`), so look, click sound, focus, clipboard, tab and key repeat match
vanilla, and results return as `on-widget`. The host owns screen lifetime (Escape, resize, disconnect).

```wit
interface keybinds {               // cap keybinds
  register: func(id: string, default-code: s32, category: string, name-key: string);
  is-down:  func(id: string) -> bool;           // this tick's state; edges via events.on-keybind
}
interface config {                 // cap config; private dir only
  get:    func(key: string) -> option<string>;
  set:    func(key: string, value: string);     // debounced atomic write, <=64 KiB, <=256 keys
  schema: func(json: string);                   // optional: host generates a settings screen
}
interface http {                   // cap network, host allow-list, https only
  get:  func(url: string) -> u32;               // task id; result via on-task-result
  post: func(url: string, body: list<u8>) -> u32;   // 1 MiB cap, 10 s timeout, 4 req/s, no off-list redirects
}
interface log { log: func(level: types.log-level, msg: string); }   // 50 lines/s, tagged [mod:id] in latest.log
```

### 4.7 Where each piece lands

| API | Existing Rust it calls |
|---|---|
| tick, snapshot | `mc-client/src/tick.rs::run_tick`, `player.rs`, `entities.rs`, `mc-entity` |
| packet hooks | receive: `mc-client/src/net_handler.rs::{process_net, handle_packet}`; send: `net.rs::Net::send(PlayServerbound)`; types: `mc-protocol::packets` |
| chat | `mc-client/src/chat.rs` (`parse`/`format`/`unformatted`), `mc-gui::hud::Hud::{add_chat, set_action_bar}` |
| 2D draw | `mc-gui::batch::GuiBatch::{rect, gradient, textured_rect_uv}`, `mc-gui::font::FontRenderer::{draw, draw_shadow, width}`, `mc-render::gui_gpu::GuiRenderer::register_rgba` (mod textures) |
| item icons | `mc-render/src/items.rs`, `mc-client/src/icon.rs` |
| HUD insertion | `mc-gui::hud::Hud::render` (after vanilla HUD, before chat/tab) |
| widgets, screens | `mc-gui::{widgets, text_field, settings}`, glue in `mc-client/src/screens_glue.rs` |
| input, keybinds | `mc-client/src/{input,keymap}.rs`, `mc-gui::keys` (Controls list), `options.txt` via `mc-gui::settings` |
| scoreboard, tab | `mc-client/src/scoreboard.rs`, `mc-gui::hud::{Sidebar, TabList}` |
| server info | `mc-gui::server_data`, `mc-client/src/server.rs` |
| world, blocks | `mc-world` chunks, `mc-data::block` |
| sound | `mc-sound`, `mc-client/src/sound_glue.rs` |
| asset layer | `mc-assets::repository` pack stack, `mc-client/src/{resource_pack,client_packs,pack}.rs` |
| world pass | `mc-render/src/gpu.rs` (new pass), camera from `mc-client/src/render_frame.rs` |
| HTTP | host `ureq`/`reqwest` on the task pool (same stack as `auth.rs`) |

---

## 5. Capabilities and safety

* **No ambient authority.** A component can only call what its `world` imports, and the host links only
  granted interfaces. No WASI is linked (no clock, random, env, fs, sockets, stdio); `log`, `game-time` and a
  seeded `random` replace the useful parts. Instantiating a mod that imports an ungranted interface fails with
  "requires capability X".
* **Declared in `mod.toml`, shown on install and on every update that adds one** (`GuiScreenModPermissions`):
  * *Normal*: HUD, own screens, input state, keybinds, private config, read chat, read game state.
  * *Sensitive (yellow)*: **network** (hosts listed), **clipboard**, **read packets**.
  * *Flagged (red, default off, separate confirmation)*: **modify/send packets**, **send chat/commands**,
    **inject input**, **cancel/replace chat or packets**. Text: "This mod can act for you or change what the
    game sends to servers. Many servers prohibit automation and modified clients; you are responsible for
    following server rules."
* **Denied by default:** everything beyond "Normal" until granted; filesystem outside
  `mods-wasm/config/<id>/`; process spawn; native code; other mods' data; the access token, session and saved
  server passwords; arbitrary plugin channels; login/handshake packet states.
* **Grants are stored per mod** with the manifest hash they were granted for; a changed capability list
  re-prompts. "Revoke" unlinks immediately (mod reloads).
* **Server policy courtesy.** A user-editable per-server deny list (and optionally a `rustmc:policy` plugin
  channel a server may announce) disables flagged capabilities while connected. This is a convenience, not a
  security boundary. The platform is general: it ships no gameplay-automation features and the SDK has no
  helpers for them.
* **Rate and size limits** on all outbound actions (chat 5/s, http 4/s, packets 20/s) and on every string/list
  crossing the boundary (default 1 MiB), enforced host-side; violations count as strikes.
* **Supply chain:** optional ed25519 signature (M4) shows "Verified: <fingerprint>" or "Unsigned" in the prompt.
  No mod auto-update.

---

## 6. Interop with Forge; loader policy

* **One event bus.** The Rust client owns a single typed `EventBus` (tick, packets in/out, chat, render-hud,
  screens, keys). WASM mods subscribe to it; the Forge bridge (FG2/FG3: HotSpot with Rust-backed stand-in
  `net.minecraft` classes) is another subscriber translating the same events into Forge `Event`s. Neither side
  polls the other.
* **Ordering** within one event: Rust core handlers, then Forge subscribers in Forge priority order, then WASM
  mods in load order (`[load]` hints topologically, ties by `priority`, then id). Cancellation by an earlier
  stage is visible to later ones only if they declare `receive-cancelled`. HUD order: vanilla HUD, Forge
  overlays, WASM `render-hud`, then chat/tab/screens.
* **No cross-calls in v1.** A WASM mod cannot call Java mod code, nor the reverse. The supported bridge is
  messages: named channels on the bus (`bus.publish(channel, bytes)` / `on-bus`) with the usual size limits.
  A typed bridge waits for demand.
* **Coexistence.** rmods never appear in the FML mod list, never touch registry sync, and are invisible to
  servers unless they use plugin channels, so a Forge server handshake is unaffected. The Mods screen has two
  tabs, "Forge" and "Native".
* **Loader policy: OptiFine is never loaded, by any loader.** A shared `ModScanner` runs before both the Forge
  loader and the `.rmod` loader and refuses a candidate (jar, rmod, directory or classpath entry) if any of:
  (a) the file name matches `(?i)optifine|optifabric|of_hd`; (b) manifest id / modid / name /
  `Implementation-Title` matches the same; (c) the archive contains `optifine/` or `net/optifine/` entries,
  `OptiFineTweaker`, `OptiFineForgeTweaker`, `OptiFineClassTransformer`, or those strings in `mcmod.info`,
  `mod.toml`, `MANIFEST.MF` or `META-INF/*.json`; (d) any dependency names it. Refused files appear as "Blocked"
  with the reason; they are never opened beyond reading the central directory and manifest. The check applies
  to dependencies and on hot reload. There is no override flag.

---

## 7. Tooling and SDK

**Crate `rustmc-mod-sdk`** (WIT files vendored under `wit/`):
* `wit-bindgen` bindings plus wrappers: `Hud` (draw-stream builder: `rect`, `text`, `item`...), typed
  `Config`, `Keybind`, `Verdict`.
* proc-macros: `#[rustmc::mod_main]`, `#[on_tick]`, `#[on_render_hud]`, `#[on_key]`, `#[on_mouse]`,
  `#[on_chat_receive]`, `#[on_packet(receive, ids = [...])]` (writes the subscription filter into a custom
  section the host reads without instantiating), `#[screen("id")]`, `#[on_widget("id")]`, `#[config]` (schema).
* The SDK panic hook logs message + location via `log` before trapping.
* **`cargo rustmc`**: `new` (template), `build` (cargo for `wasm32-wasip2`/`wasm32-unknown-unknown` +
  `wasm-tools component new`; validates the manifest and checks that capabilities in `mod.toml` cover the imports
  actually used; undeclared use is an error), `pack` (.rmod), `run` (build, copy to a dev instance, launch rustmc
  with `--dev-mods <dir>`), `test`, `sign`.
* **Hot reload** (dev and the user-visible "Reload" button): the host watches `--dev-mods` with the `notify`
  crate; on change it compiles in the background (Winch in dev), instantiates a fresh store, calls `on-load`,
  and swaps at a tick boundary; the old store gets `on-unload` and is dropped. Config and keybind registrations
  persist by id. If compile or `on-load` fails, the old instance keeps running and the error is toasted and
  printed by `cargo rustmc run`.
* **Other languages:** anything with a WIT toolchain targets the same `.wit`: C/C++ (`wit-bindgen c` +
  wasi-sdk), Zig (via the C bindings), Go (TinyGo + `wit-bindgen-go`), AssemblyScript (`jco`-style
  componentize), C# (`componentize-dotnet`), JS/Python (`componentize-js/py`; documented as too heavy for
  per-frame hooks). The draw stream is specified byte-for-byte in `docs/DRAW_STREAM.md` so non-Rust SDKs write
  their own encoders; the Rust SDK is the reference.

### Sample mod: Keystrokes + CPS HUD with a settings screen

```toml
# mod.toml
[mod]
id = "keystrokes"
name = "Keystrokes HUD"
version = "1.0.0"
authors = ["Benjy"]
entry = "mod.wasm"
side = "client"
[api]
version = "1.0"
min = "1.0"
[capabilities]
hud = true
screens = true
input_read = true
keybinds = true
config = true
```

```rust
// src/lib.rs   (Cargo.toml: crate-type = ["cdylib"]; deps: rustmc-mod-sdk = "1", serde)
use rustmc_mod_sdk::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[config(file = "keystrokes.toml")]
struct Cfg { enabled: bool, x: i32, y: i32, scale: f32, show_cps: bool }
impl Default for Cfg {
    fn default() -> Self { Self { enabled: true, x: 4, y: 4, scale: 1.0, show_cps: true } }
}

#[derive(Default)]
struct State {
    cfg: Cfg,
    lmb: Vec<u64>, rmb: Vec<u64>,     // click times in game ticks (host game-time, no wall clock)
    down: [bool; 6],                  // W A S D LMB RMB
}

const KEYS: [i32; 4] = [key::W, key::A, key::S, key::D];

#[rustmc::mod_main]
fn main() -> Result<Mod<State>, String> {
    keybind::register("toggle", key::NONE, "key.categories.keystrokes", "key.keystrokes.toggle");
    keybind::register("settings", key::NONE, "key.categories.keystrokes", "key.keystrokes.settings");
    screen::register("settings", "Keystrokes");
    Ok(Mod::new(State { cfg: Cfg::load(), ..Default::default() }))
}

#[on_keybind]
fn keybind(st: &mut State, id: &str, down: bool) {
    if !down { return; }
    match id {
        "toggle"   => { st.cfg.enabled ^= true; st.cfg.save(); }
        "settings" => screen::open("settings"),
        _ => {}
    }
}

#[on_mouse]
fn mouse(st: &mut State, e: MouseEvent) -> bool {
    if e.down {
        let now = game::time();
        match e.button { 0 => st.lmb.push(now), 1 => st.rmb.push(now), _ => {} }
    }
    false                              // observe only, never consume
}

#[on_tick]
fn tick(st: &mut State) {
    let now = game::time();
    st.lmb.retain(|t| now - t < 20);   // 1 s window = 20 ticks
    st.rmb.retain(|t| now - t < 20);
    for (i, k) in KEYS.iter().enumerate() { st.down[i] = input::key_down(*k); }
    st.down[4] = input::mouse_down(0);
    st.down[5] = input::mouse_down(1);
}

#[on_render_hud]
fn hud(st: &mut State, _ctx: &FrameCtx, ui: &mut Hud) {
    if !st.cfg.enabled { return; }
    let (x, y) = (st.cfg.x, st.cfg.y);
    ui.push_scale(st.cfg.scale);
    let mut cell = |cx: i32, cy: i32, w: i32, label: &str, on: bool| {
        ui.rect(x + cx, y + cy, x + cx + w, y + cy + 20, if on { 0x90FFFFFF } else { 0x90000000 });
        ui.text_centered(label, x + cx + w / 2, y + cy + 6, if on { 0xFF000000 } else { 0xFFFFFFFF }, false);
    };
    cell(22, 0,  20, "W", st.down[0]);
    cell(0,  22, 20, "A", st.down[1]);
    cell(22, 22, 20, "S", st.down[2]);
    cell(44, 22, 20, "D", st.down[3]);
    if st.cfg.show_cps {
        cell(0,  44, 31, &format!("{} CPS", st.lmb.len()), st.down[4]);
        cell(33, 44, 31, &format!("{} CPS", st.rmb.len()), st.down[5]);
    }
    ui.pop();
}

// Host builds real vanilla Buttons/sliders from this description.
#[screen("settings")]
fn settings(st: &mut State, w: i32, h: i32) -> Vec<Widget> {
    let cx = w / 2 - 100;
    vec![
        Widget::toggle(1, cx, h / 4,       200, "Enabled",  st.cfg.enabled),
        Widget::toggle(2, cx, h / 4 + 24,  200, "Show CPS", st.cfg.show_cps),
        Widget::slider(3, cx, h / 4 + 48,  200, "Scale", (st.cfg.scale - 0.5) / 1.5, 30),
        Widget::slider(4, cx, h / 4 + 72,  200, "X", st.cfg.x as f32 / w as f32, 100),
        Widget::slider(5, cx, h / 4 + 96,  200, "Y", st.cfg.y as f32 / h as f32, 100),
        Widget::done(6, cx, h / 4 + 128, 200),
    ]
}

#[on_widget("settings")]
fn on_widget(st: &mut State, id: u32, v: Value, (w, h): (i32, i32)) {
    match (id, v) {
        (1, Value::Bool(b))  => st.cfg.enabled = b,
        (2, Value::Bool(b))  => st.cfg.show_cps = b,
        (3, Value::Float(f)) => st.cfg.scale = 0.5 + f * 1.5,
        (4, Value::Float(f)) => st.cfg.x = (f * w as f32) as i32,
        (5, Value::Float(f)) => st.cfg.y = (f * h as f32) as i32,
        _ => {}
    }
    st.cfg.save();                     // debounced by the host
}
```

---

## 8. Versioning and stability

* **API version** `MAJOR.MINOR` of the WIT package (`rustmc:mod@1.x`). Within a major, changes are additive
  only: new functions, new interfaces, new enum/variant cases at the end. Records crossing the boundary are
  frozen; extension comes as new `*-v2` records or new interfaces. Components import by interface version
  (`rustmc:mod/game@1.0.0`) and the wasmtime linker registers `@1.0.0` and `@1.1.0` side by side.
* **Compat shims.** The host keeps every shipped interface version as a thin adapter over the newest internals.
  A **golden suite of compiled mods per API version** (`tests/mods/api-1.0/*.rmod`) runs in CI against every
  host build; a host change that breaks one fails CI.
* **Major bumps** (`2.0`) may remove things only after deprecation; the host supports current and previous
  major concurrently for at least 12 months / 4 rustmc minor releases.
* **Deprecation.** Items are annotated `@deprecated(since=1.3, replacement=...)` in WIT; the host warns once
  per mod in the log, shows a "Uses deprecated API" badge, and removes only at the next major.
* **Feature detection.** `on-load(host-api, features)` passes the host API version and a feature list
  (`"world-draw.billboard"`, `"screens.slider"`, `"entities.health"`); mods branch on features, not versions.
  `[api] min` blocks too-old hosts with a clear message; a mod built against a newer minor still runs if every
  import it uses exists (checked at instantiate).
* **Engine drift.** wasmtime is pinned per rustmc release and upgraded as an ordinary dependency bump behind the
  golden-mod suite; the `.cwasm` cache key includes the wasmtime version.

---

## 9. Testing

* **Headless harness** (`mc-mods-test`; no window, GPU or network): loads a `.rmod`, replays a *recording*
  (sequence of snapshots, input events, raw `mc-protocol` packets, `frame-ctx`), and captures everything the mod
  emitted: decoded draw lists as text op lists, action calls, chat, packet verdicts, config writes, logs. Tests
  assert on that transcript with snapshot files ("after 3 W-down ticks the cell rect is 0x90FFFFFF"). It runs the
  same host code as the game with a null `GuiBatch` backend and **fuel metering instead of epochs**, so runs are
  bit-reproducible and can assert "hook used < N fuel".
* **Fixtures:** `rustmc --record-mod-session out.rec` captures real sessions (names anonymised); the existing
  mock server (`mc-protocol/examples/mock_server.rs`) is the default scenario source.
* **Pixel tests:** sample-mod draw lists go through the real GUI path and `--screenshot`, diffed against a reference.
* **Fuzzing the host boundary** (`cargo-fuzz`, nightly CI): (1) draw-stream decoder; (2) packet `replace`
  re-decode; (3) widget-list validation; (4) manifest/zip parser; (5) structure-aware fuzz of every exported hook
  using `wasm-smith` modules with host imports returning random data; (6) config/HTTP size limits. Targets must
  be crash- and leak-free under ASAN and produce bounded output.
* **Misbehaviour mods:** infinite loop, deep recursion, `memory.grow` bomb, panic in each hook, enormous draw
  lists, trap in `on-load`, 1000 log lines/s, calls beyond granted capabilities. Each must end with a
  disabled/limited mod, a toast, an intact frame loop, and a crash report.
* **Perf budget tests** (CI, via `mc-client/src/bench.rs` busy scene): (a) idle: no mods, and one mod with no
  subscriptions, within 0.5% of baseline frame time; (b) the sample mod adds < 0.15 ms main-thread and
  < 1 ms worker time per frame; (c) 20 trivially-subscribing mods add < 1 ms per tick; (d) snapshot build 50 us /
  200 us; (e) sync-hook dispatch with no subscriber < 20 ns. A regression over 10% fails CI.

---

## 10. Milestones

**M0: runtime, tick/HUD events, 2D draw (HUD mods).**
Scope: new crate `mc-mods` behind a default-on `wasm-mods` feature; wasmtime engine with epoch watchdog,
pooling allocator, limiter, `.cwasm` cache; `.rmod` loader and manifest parser; capability gating (hud, config,
log); worker thread and snapshot (player + server info); `lifecycle`, `on-tick`, `render-hud`, `ui-draw` ops
0-3 and 5-9; `game.player`, `game-time`, `log`; strike/disable/toast/crash-report path; `GuiScreenMods`
(toggle + status); OptiFine blocker in the shared scanner; harness skeleton.
Acceptance: a HUD-only version of the sample renders on a live connection; the loop/recursion/memory/trap
misbehaviour mods leave frame rate intact with a toast; idle perf test passes; one `.rmod` runs unchanged on
macOS, Windows and Linux CI; a cache hit loads in < 5 ms per mod; an OptiFine jar and rmod are refused with a
listed reason.

**M1: input, keybinds, screens, config.**
Scope: `on-mouse`/`on-key` (sync, filtered), keybinds registered into Controls and saved in `options.txt`,
`screens` with the host-owned widget kit, `config` with atomic writes and auto schema screen, `ui-draw`
item-icon and textures plus the asset layer for mod textures and lang (via the existing pack stack), hot reload
via `--dev-mods`, permission prompt screen.
Acceptance: the full sample mod works; its keybinds appear in Controls; widgets match vanilla `Button` in a
screenshot diff; reload swaps a mod in < 200 ms without a main-thread hitch above 1 ms; a failed reload keeps the
old instance.

**M2: packets, chat, world queries.**
Scope: sync packet and chat hooks with filters and verdicts, `actions` (chat, command, allowed packets, sound,
toast), entity/block/tab/scoreboard queries (SoA entity snapshot, chunk read view), world/screen/entity
notifications, `http`, rate limits, flagged-capability prompt UI, `EventBus` extracted so FG1/FG2 can attach,
bus channels.
Acceptance: recorded-session replays pass in the harness; fuzz targets 1-4 run 1 h clean; no-subscriber dispatch
< 20 ns; undecodable `replace` bytes are rejected and logged; a Forge-handshake server still joins with an
rmod loaded.

**M3: world-space rendering and assets.**
Scope: `world-draw` pass (lines, boxes, billboard text/quads, depth on/off), per-mod primitive caps, texture,
sound and lang asset layers with the shared reload (`sounds.json` merged).
Acceptance: a sample overlay (billboard labels and a box at fixed world coordinates) is stable under camera
motion at 144 fps with one-frame-late lists; the pass costs < 0.2 ms for 1000 primitives on the reference Metal
machine; a screenshot-parity test exists per primitive.

**M4: SDK polish and signing.**
Scope: `rustmc-mod-sdk` 1.0 with proc-macros; `cargo rustmc new/build/pack/run/test/sign`; WIT repo and
binding guides for C, Go, AssemblyScript, Zig; `DRAW_STREAM.md`; ed25519 signing and fingerprint prompt; API
1.0 golden-mod suite frozen; perf CI gates enabled.
Acceptance: a Rust mod and a TinyGo mod each build from the docs in under 10 minutes and pass the harness;
unsigned or tampered archives are labelled; API 1.0 is declared stable.

---

## Open questions

1. Is up to one frame of latency on HUD/world overlays acceptable, or does some mod class need a synchronous HUD path (costing the main thread up to 2 ms)?
2. `mods-wasm/` separate directory, or `.rmod` in `mods/` beside Forge jars (simpler for users; the scanner would split by extension)?
3. Is a server-announced policy channel (`rustmc:policy`) wanted, or only a local deny list?
4. Keep `send-packet` limited to plugin channels and settings in v1, or plan a reviewed capability for more?
5. Signing: self-managed author keys only, or a rustmc-hosted verified-author registry?
6. Is 32 MiB and SIMD128 the right baseline for HUD-class mods?
