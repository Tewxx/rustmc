# Forge compatibility test work in progress

`tools/forgetest/build.sh` builds the mods listed in `mods/ORDER` against the local MCP 1.8.9 classes and Forge universal jar, then maps their Minecraft references to SRG names. Configure the input paths with the environment variables in `tools/forgetest/env.sh`. Build products are ignored under `tests/forge/build`.

`tools/forgetest/run-forge.sh --twice` runs a local offline Forge server and a Forge client and compares their client traces before copying them to `golden/forge`. The client opens a window; do not run it when an uninterrupted desktop is required. Use `FT_PORT` to choose an unused server port. These scripts currently assume the Prism library layout and macOS ARM64 LWJGL natives.

This is an incomplete suite: the recorded traces are not yet validated as deterministic, there is no rustmc trace runner or Mixin test mod, and the tests do not establish Forge compatibility. The Forge client and server artifacts, JDK 8, MCP classes and Prism libraries must already be installed locally.

`tools/forgetest/run-rustmc.sh [--no-build]` is the rustmc side: it boots the built jars on rustmc's Forge API (`crates/mc-forge/tests/forge_traces.rs`), loads every mod in `mods/ORDER`, drives 20 client ticks and frames, writes the mods' `ft.Trace` groups to `tests/forge/build/rustmc-trace/`, and diffs each group whose trace was recorded in real Forge against `oracle/`. It exits nonzero on any difference. A group with no recorded trace is not compared. To add a test, add a mod directory to `mods/ORDER`, use `ft.Trace` with a stable ID, and add it to `SERVER_MODS` only if its server side is needed. Build and validate its traces in real Forge before treating them as an oracle.

The runner drives no world or player, so mod scripts that wait for `ScriptEvent` steps (`ftclient`, `ftgui`, `ftchat`, `ftnet`) only emit their pre-init/init lines, and the recorded `lifecycle` trace is a four-mod subset whose mod list cannot match a full run. The `bus` group is the oracle-verified one; it still differs at `EV-05`: the two recorded sessions disagree with each other (one has an `instance` handler line, the other none), so that case needs a fresh real-Forge recording before it can gate anything.

The intended future corpus smoke tier checks load, GUI and HUD behavior for user-selected jars. Third-party mods are not run by this suite; `.mc-tools/mod-corpus/` contains the local scan inputs described in `docs/FORGE_COMPAT.md`.

`tools/forgetest/check-core.sh` builds the coremod and checks its transformer against a controlled class without opening a window. A headless Forge dedicated-server run recorded `CORE-01` value 2, confirming loading and transformation on that side; client loading and broader ordering remain unverified.

`ftmixin` is an opt-in experimental probe (`tools/forgetest/build.sh ftcommon ftmixin`). A local headless Forge server with Mixin 0.8.7 and ASM 5 first on its classpath recorded `MIXIN-01` value 2. Forge's ASM 5 mod scanner warns that newer classes inside the Mixin jar cannot be read. The default oracle does not load this probe until its Mixin classpath is configured. Its own-class injection and empty refmap do not test injection into a Minecraft class or mapped selectors.
