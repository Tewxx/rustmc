# Visual comparison

This harness compares vanilla 1.8.9 and rustmc at scripted camera positions in the
same offline server world. It is an investigation tool, not a parity test: a low
pixel error alone cannot establish correct gameplay, animation or rendering.

Fresh vanilla captures require Linux, `xvfb-run`, Java 8, the MCP-919 workspace,
Prism 1.8.9 libraries and Linux LWJGL natives. `vanilla.sh` opens the reference
client on a private Xvfb display and refuses to run on macOS. Set `RUSTMC_INPUTS`
to the checkout containing `.mc-tools` and `MCP-919`, `PRISM_LIBS` to Prism's
libraries, and `JDK8` or `LWJGL_NATIVES` if needed. Build the release rustmc
binary, then run `tools/visual/all.sh`. `PORT` selects the loopback port.
Each full run writes a fresh `shots/run.*` directory. The server script creates a fresh world under `shots/server` and leaves it there;
`tools/visual/server.sh stop` stops only the server it started. The server jar
must be supplied at `.mc-tools/server/server.jar` or via `VISUAL_SERVER_JAR`.

`shots/` is ignored. For existing pairs of captures, `tools/visual/diff.py` works
without starting any client. It reports missing captures and rejects differing
image dimensions rather than resizing them. Record build revisions, input world,
settings, camera and capture provenance before treating its output as evidence.
The saved interrupted-run images were not reproduced against the current code,
so they do not establish current visual parity.
