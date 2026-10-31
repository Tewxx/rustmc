# Controlled live bridge fixture

This jar contains only local assertions and Mixin probes. It does not contact a
server, inspect accounts, load a third-party mod, or inject another process.

Compile all Java sources against the generated stubs, the exact Forge runtime
classpath, and a supplied Mixin jar, with `javac -proc:none`. Copy the JSON config
to the classes root and package using `manifest.txt`. Put the resulting jar on
the JVM classpath and in `runtime::start`'s `mod_jars` argument so LaunchWrapper
loads its Mixin tweaker. Add the same Mixin jar to the runtime classpath.

On Windows, `./crates/mc-forge/tests/mods/liveprobe/build.ps1` builds the fixture
and writes `tests/forge/build/liveprobe-runtime.json`. Use that manifest's
`classpath` for the Mixin-enabled test: it replaces the old ASM 5 runtime entry
with a coherent installed ASM 9.5 set (`asm`, `asm-tree`, `asm-commons`,
`asm-util`, `asm-analysis`). `-AsmVersion` selects another locally installed
complete set, and `-MixinJar` selects the supplied Mixin jar. This does not
change the generator's ASM 5 classpath or download anything.

`ft.liveprobe.Probe` exposes these static methods to the Rust JVM integration test:

- `assertGraph(int localId, int otherId, double localX, float health, int itemId,
  int count, int damage)` checks fields and ordinary API getters against the
  published snapshot, including world entity lookup, lists, health and inventory.
- `remember(int otherId)` then `assertStable(int otherId)` verifies that a second
  snapshot preserves player/world/entity/inventory/list object identities.
- `mutateAndAct(int otherId)` sets yaw `137.5`, pitch `-23.25`, motion
  `(0.125, 0.25, -0.375)`, on-ground false, sprint true, selected slot 7, movement
  `(-0.5, 0.75, true, true)`, and calls the actual swing/chat/attack/send-queue APIs.
  The encoded chat packet carries `liveprobe-packet` and the chat action carries
  `liveprobe-chat`. A second encoded packet is C04 position `(1.25, 64.5, -2.75,
  true)`. Rust should validate both returned state and queued actions; unchanged
  snapshot fields are expected to be omitted by `Live.changes()`.
- `assertRemoved(int id)` checks removal after a snapshot omits the remembered
  remote entity; `assertDisconnected()` checks a disconnected snapshot.
- `resetTrace()`, `clearTrace()`, `body()`, `trace()`, `setCancelTick(boolean)` /
  `setCancelWalking(boolean)` let Rust assert the actual generated-class hook order. The host
  body invokes `body()` through JNI. Normal traces are `tick:head,body,tick:return`
  and `walk:head,body,walk:return`. A HEAD cancellation must prevent the host body.

These are bounded tests of the exposed bridge. Passing them does not establish
complete vanilla, Forge, agent, or injected-client compatibility.
