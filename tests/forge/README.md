# Forge compatibility test work in progress

`tools/forgetest/build.sh` builds the mods listed in `mods/ORDER` against the local MCP 1.8.9 classes and Forge universal jar, then maps their Minecraft references to SRG names. Configure the input paths with the environment variables in `tools/forgetest/env.sh`. Build products are ignored under `tests/forge/build`.

`tools/forgetest/run-forge.sh --twice` runs a local offline Forge server and a Forge client and compares their client traces before copying them to `golden/forge`. The client opens a window; do not run it when an uninterrupted desktop is required. Use `FT_PORT` to choose an unused server port. These scripts currently assume the Prism library layout and macOS ARM64 LWJGL natives.

This is an incomplete suite: the recorded traces are not yet validated as deterministic, there is no rustmc trace runner or Mixin test mod, and the tests do not establish Forge compatibility. The Forge client and server artifacts, JDK 8, MCP classes and Prism libraries must already be installed locally.
