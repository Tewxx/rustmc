# Forge compatibility test work in progress

`tools/forgetest/build.sh` builds the mods listed in `mods/ORDER` against the local MCP 1.8.9 classes and Forge universal jar, then maps their Minecraft references to SRG names. Configure the input paths with the environment variables in `tools/forgetest/env.sh`. Build products are ignored under `tests/forge/build`.

`tools/forgetest/run-forge.sh --twice` runs a local offline Forge server and a Forge client and compares their client traces before copying them to `golden/forge`. The client opens a window; do not run it when an uninterrupted desktop is required. Use `FT_PORT` to choose an unused server port. These scripts currently assume the Prism library layout and macOS ARM64 LWJGL natives.

This is an incomplete suite: the recorded traces are not yet validated as deterministic, there is no rustmc trace runner or Mixin test mod, and the tests do not establish Forge compatibility. The Forge client and server artifacts, JDK 8, MCP classes and Prism libraries must already be installed locally.

`tools/forgetest/run-rustmc.sh` lists trace IDs in the source as not implemented and exits nonzero. It is a progress inventory, not a compatibility test: it neither runs mods nor compares traces. To add a test, add a mod directory to `mods/ORDER`, use `ft.Trace` with a stable ID, and add it to `SERVER_MODS` only if its server side is needed. Build and validate its traces in real Forge before treating them as an oracle.

The intended future corpus smoke tier checks load, GUI and HUD behavior for user-selected jars. Third-party mods are not run by this suite; `.mc-tools/mod-corpus/` contains the local scan inputs described in `docs/FORGE_COMPAT.md`.

`tools/forgetest/check-core.sh` builds the coremod and checks its transformer against a controlled class without opening a window. A headless Forge dedicated-server run recorded `CORE-01` value 2, confirming loading and transformation on that side; client loading and broader ordering remain unverified.
