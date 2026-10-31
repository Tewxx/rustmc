# Native component host (experimental)

The client loads `.rmod` files from the game directory’s `mods-wasm/` on a worker thread; bounded tick and frame messages feed its latest completed HUD to the GUI. World resets invalidate queued output. Frame requests may be dropped when the queue is busy; if the bounded queue fills on a tick, all native mods are disabled for that session and stale output is removed rather than silently losing ordered tick events. The current host runs `rustmc:plugin@1.0.0` components with lifecycle, `game.player`,
`game.game-time`, `ui-draw.submit`, logging, and observe-only keyboard/mouse input for mods declaring `input_read`. Input runs asynchronously on the worker; event consumption and cancellation are not supported. It links no WASI; components built with
`wasm32-wasip2` commonly import WASI and are rejected. The `accounts` interface is reserved
only and is not linked. Hot reload, input cancellation, screens, network, configuration, mod permissions UI and the
rest of `docs/WASM_MODS.md` are not yet implemented. This ABI is experimental.

Build a Rust guest using wit-bindgen 0.36 against `wit/rustmc.wit`, targeting
`wasm32-unknown-unknown`, and convert it into a component:

```
cargo build --release --target wasm32-unknown-unknown
cargo run -p mc-mods --example componentize -- path/to/core.wasm path/to/mod.wasm
```

A `.rmod` is a zip containing `mod.toml` and the component. The supported capability is
`hud`; unsupported capabilities fail closed. No Java, filesystem, network or credentials
are exposed. Fuel bounds each guest call and a trap disables that instance.

`ui-draw.submit` accepts at most 256 KiB and 16,384 commands. The stream begins with ASCII
`RMD1`, followed by operations with little-endian operands:

* `0`: `left:i32, top:i32, right:i32, bottom:i32, argb:u32`.
* `3`: `x:i32, y:i32, argb:u32, shadow:u8, length:u16, UTF-8 text[length]`.

Malformed or unsupported commands invalidate the entire list. Coordinates use scaled GUI
units; text uses the game's font. Only rectangles and text are implemented so far.
