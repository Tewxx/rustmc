# Forge API stubs: build, run, implement, discover

The mod-facing API (Minecraft 1.8.9 in SRG names, Forge/FML, LWJGL 2: about 4,400 classes; LaunchWrapper is the real jar, not stubbed) exists as
a generated jar. Each class is one of two kinds:

- **Stub** (default): every method calls into Rust. You implement methods one at a time; the rest log and return zero.
- **Real**: the Forge-patched vanilla bytecode, unchanged, for classes whose logic is pure Java (GUI widgets, Forge
  events, `Session`...). Listed in `tools/shadow/keep-real.txt`. Real code calls the stubbed classes for everything Rust
  owns (`Minecraft`, `FontRenderer`, `Tessellator`...), so you implement only those leaves.

A stub never overrides a method a real ancestor implements (so a stubbed `GuiMultiplayer` still gets `GuiScreen`'s real
`mouseClicked`). Individual methods of a real class can still be forced to Rust with `tools/shadow/stub-methods.txt`.

Code: `crates/mc-forge` (`stubs.rs` dispatch, `impls/` handlers, `game.rs` game-loop entry points, `runtime.rs` JVM,
`java/rustmc/forge/{Stub,Bus,ModLoader}.java` glue), generator `tools/shadow/StubJar.java`.

**Status.** Proven end to end, in tests and in the real client:
- **GUI mods.** `tests/account_manager.rs` loads the real AccountManager 1.8 jar, runs its `@Mod` lifecycle, lets it add
  its "Accounts" button to the server list through Forge's real `InitGuiEvent`, clicks it through the real LWJGL mouse
  path (`handleMouseInput`), opens the mod's screens (Add, Cracked), types into its real `GuiTextField` and draws it all
  into `Draw` commands. In the client, `rustmc --screenshot x.png --screen multiplayer` shows that button on the native
  multiplayer screen with the vanilla texture and font.
- **Mixin.** Everything runs under the real LaunchWrapper `LaunchClassLoader`; mods' `TweakClass` (MixinTweaker) is
  launched from their manifest and Mixin transforms the stub classes. `tests/mixin.rs` shows a `@Mixin` wrapping a
  Rust-implemented stub (HEAD and RETURN injections). `tools/shadow/corpus-mixins.sh` loads every target class of the real
  Meowtils and Raven mixin configs: **all 32 (Meowtils) and 13 (Raven) targets load with their mixins applied.**

**Not wired:** nothing *calls* the mixin-hooked game methods yet (Rust runs the loop, so a mixin on `runTick` or
`getMouseOver` is applied but never executed), coremods (`FMLCorePlugin` transformers, e.g. Lunar), GL matrix/scissor/blend
state and non-`minecraft` textures in mod draws, world events (only client/render ticks and screen events fire), field
mirroring of live player/world state into Java, and audio for UI clicks.

## Build the stub jar

    tools/shadow/stubs.sh tests/forge/build/stubs.jar

Needs the local inputs `tools/forgetest/env.sh` already uses (Prism libraries, `MCP-919`, JDK 8 under `.mc-tools`).
The output is derived from your own Minecraft/Forge jars, is gitignored, and must never be committed. It keeps every
class name, descriptor, field and annotation, so Mixin, ASM transformers and reflection see the real shape. Each method
and static initializer is replaced by `Stub.call("owner.name(desc)", this, args)`. Enums are copied unchanged
(their static initializers would otherwise crash). Third-party libraries inside the Forge jar stay real.
`STUB_KEEP=<regex>` keeps matching classes unstubbed, e.g. to run Forge's real `EventBus`.

## Class loading rules (learned the hard way)

- Everything runs under `Launch.classLoader` (LaunchWrapper's `LaunchClassLoader`). Rust must never use `FindClass` /
  string class names from a thread with no Java frame: that resolves the system loader's *second copy* of every class.
  Use `runtime::class / new_object / call_static / static_field / alloc` (handlers may too).
- LaunchWrapper excludes the **tweaker's whole package** from the loader. The launcher classes therefore live in
  `rustmc.boot`, apart from `rustmc.forge` (Stub, Bus, ModLoader), which must be loaded by the LaunchClassLoader.
- `org.lwjgl.*` also stays in the system loader, which has its own copy of `Stub`; `runtime` binds both.
- Java runs on the thread that created the JVM: give that thread a big stack (the tests spawn a 64 MiB one; the client's main
  thread is fine).
- A corpus mod that shades Mixin must not have another Mixin on the class path (signer clash); mods that rely on the
  system one need `mixin-0.8.7.jar` in `RUSTMC_FORGE_CP`.

## Run it

    RUSTMC_FORGE_STUBS=tests/forge/build/stubs.jar RUSTMC_FORGE_CP=<minecraft libs> JAVA_HOME=<jdk8> rustmc ...

`RUSTMC_JVM` can point straight at a `libjvm` instead of `JAVA_HOME`. `RUSTMC_FORGE_CP` is the class path of Minecraft's
libraries (guava, log4j, gson, authlib...; `ft_libs` in `tools/forgetest/env.sh` prints it). Every jar in `mods-forge/`
is added to the class path and its `@Mod` classes run pre-init, init and post-init (`runtime::load_mods`). Without
`RUSTMC_FORGE_STUBS` no JVM starts. If startup fails the client prints why and continues.

## Mixin

`mods-forge/*.jar` with a `TweakClass` manifest entry (e.g. `org.spongepowered.asm.launch.MixinTweaker`) are launched by
`rustmc.boot.BootTweaker`, which also forces Mixin's side to CLIENT (no FML to tell it). A mixin applies to whatever the class
is in the jar:
- HEAD/RETURN/TAIL injections and accessors work on **stubs** (the injection wraps the Rust-dispatched body).
- INVOKE/FIELD/NEW/CONSTANT points, `@Redirect`, `@ModifyArg/Variable/Constant` and `@Overwrite` need the **real vanilla body**.
  `tools/modscan` finds them; build the jar with `STUB_MIXIN_SCANS=".mc-tools/scan/Mod.json" tools/shadow/stubs.sh OUT.jar`
  (`mixin-keep.py` turns the scans into `keep-real` entries). A class that Rust implements can be real too if its Rust methods are
  listed in `stub-methods.txt` (`Minecraft` and `FontRenderer` already are). Without that, a `required` mixin fails with
  "failed injection check (0/1)".
- A stub method whose real ancestor already implements it just calls super, so Mixin can still target it by name.

    tools/shadow/corpus-mixins.sh [JAR...]    # per mod: how many mixin target classes loaded, and why any failed
    RUSTMC_FORGE_TRACE=1 RUSTMC_JVM_OPTS="-Xcheck:jni"   # every stub call as it happens / JNI misuse checks

## Test against a real mod

    tools/shadow/test-mods.sh [MOD_JAR]      # default: .mc-tools/mod-corpus/AccountManager-1.8.jar

Builds the stub jar if missing, then runs `cargo test -p mc-forge` with the libraries and mod on the class path.
Read the `[forge] stubbed:` lines to see what the mod needed that Rust does not do yet.

## How the client uses it (`mc-client/src/forge_ui.rs`)

`forge_open_native` (a native screen opened: make its Java twin), `forge_click` (twin first), `forge_tick`
(`ClientTickEvent` START/END), `forge_frame_begin/end` (sync, `RenderTickEvent`, replay the twin's draw commands over
the native screen), `ModScreen` (a Java-opened screen as a client `Screen`: draw, mouse, keys, resize). Screens opt in
through `Screen::java_screen()` (`"main_menu"`, `"multiplayer"`, `"ingame_menu"`). Text widths come from the client font.

## What the client calls (`game.rs`)

All run on the JVM-attached thread via `runtime::with_env`:
`client_tick(start)`, `render_tick(start, partial)`, `open_screen(screen)` (what `Minecraft.displayGuiScreen` does),
`draw_screen(mx, my, partial)`, `mouse_click(x, y, button)`, `mouse_release`, `key_typed(ch, key_code)`, `resize`, `open_shadow(kind)`, `focus()`; plus `impls::set_display`,
`set_mouse`, `set_modifiers`, `set_session` (name/UUID only: mods never see the access token) and `take_draw()` for the recorded
`Draw::Text` / `Draw::Vertices` commands. `impls::set_text_width` plugs in the client's font metrics (default 6 px/char).

## Implement a method

1. Get its key (see Discovery). A key is `owner/Class.name(desc)`; leave off `(desc)` to match every overload.
2. Add a line to the matching file in `crates/mc-forge/src/impls/` (`minecraft.rs`, `gfx.rs`, `forge.rs`, `lwjgl.rs`, `math.rs`):

        register("net/minecraft/util/MathHelper.func_76128_c", |c| Ok(math::floor_double(c.f64(0)?).into()));

   - Arguments: `c.i32(n)`, `c.i64(n)`, `c.f32(n)`, `c.f64(n)`, `c.bool(n)`, `c.string(n)` (`Option<String>`), `c.obj(n)`
     (a raw Java object). `c.this` is the receiver (null for statics), `c.env` the JNI env.
   - Return: anything `Ret` converts from: numbers, `bool`, `&str`/`String`, `()`, or a `GlobalRef` for an object
     (`Ret::Obj(c.env.new_global_ref(&c.this)?)` returns `this`, as fluent builders do).
     Return `Ret::Null` or `()` for void and for null/0/false.
   - Errors from JNI (`?`) are logged as `[forge] stub handler <key> failed` and return null; the game keeps running.
   Java exceptions inside a handler print their stack trace and return null.
3. Or turn a class real: add a regex to `tools/shadow/keep-real.txt` and rebuild; only its Rust-owned leaves need handlers.
4. Test: `tools/shadow/test-mods.sh`; extend `tests/stubs_jvm.rs` (direct calls) or `tests/account_manager.rs` (a mod).

Keep handlers cheap: each call boxes its arguments and passes a string key. Fields are real Java fields, not
intercepted; to expose Rust state, write the field from Rust (`env.set_field`) at a sync point rather than in a getter.

## Discovery: what to implement and what its key is

**1. Ask the running game.** Every unimplemented method that gets called logs once (static initializers are skipped):

    [forge] stubbed: net/minecraft/client/Minecraft.func_71410_x()Lnet/minecraft/client/Minecraft;

`mc_forge::stubs::stubbed()` returns the sorted list. Run a mod, read the log, implement from the top.
Static initializers show as `Class.<clinit>()V`; those are normal and mean the class was first used.

**2. Scan a mod before running it.** `tools/modscan` lists the Minecraft/Forge members a jar references, ranked:

    tools/modscan/scan_all.sh .mc-tools/mod-corpus .mc-tools/scan
    python3 -I tools/modscan/report.py .mc-tools/scan/*.json --top 30

`docs/FORGE_COMPAT.md` holds the merged ranking for the four corpus mods (hottest: `Minecraft.func_71410_x`,
`FontRenderer.func_78256_a`, `GlStateManager` calls, `Gui.func_73734_a`, `ItemStack.func_77973_b`).

**3. Look a key up by name.**

    tools/shadow/stubkey.py MathHelper floor
    net/minecraft/util/MathHelper.func_76128_c  floor_double (D)I

First argument: substring of the class; second (optional): substring of the MCP method name. Output is
`<key without desc>  <mcp name> <desc>`. Mods and mixin refmaps use SRG names (`func_*`, `field_*`); the MCP name is
only there so you can read it. Without the second argument it lists every method of matching classes.

**4. Browse the full API.** `docs/forge-spec/api/api-vanilla-patched.json` and `api-forge-fml.json` list every class
with its fields and methods (`n` = SRG name, `mcp` = readable name, `d` = descriptor); `docs/forge-spec/events.md`
lists Forge events and where Forge fires them. Example:

    python3 -c "import json;[print(c['name'],m['n'],m.get('mcp'),m['d']) for c in json.load(open('docs/forge-spec/api/api-vanilla-patched.json')) if c['name'].endswith('/KeyBinding') for m in c['methods']]"

**5. Inspect the jar itself.** `$JDK8/bin/javap -p -cp tests/forge/build/stubs.jar net.minecraft.client.settings.KeyBinding`
shows exactly what a mod links against.

## Known limits

- 23 classes need Twitch, rcon or LWJGL-ES classes that are not on the class path, and 3 LWJGL `MappedObjectTransformer`
  classes need ASM 3; they are server or streaming code that mods do not call.
- Enum methods run real Java and never reach Rust.
- Real classes are only as good as the stubs under them: if a stub returns null where vanilla returns an object, the
  real code NPEs. The `[forge] stubbed` log and the Java stack trace show where.
- Event classes rely on `Bus.cancelable`/`hasResult` (annotation lookup) instead of Forge's load-time
  `EventSubscriptionTransformer`; `getListenerList()` is not implemented because `Bus` does not use it.
- Not wired: a real `LaunchClassLoader` (Mixin, coremods), field mirroring of live game state into Java objects, a GL
  state/texture backend, keyboard/mouse routing from the window, replaying `Draw` commands in the client UI.
