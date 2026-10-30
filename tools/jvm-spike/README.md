# JNI invocation spike

Build a controlled target first with `tools/forgetest/build.sh ftcommon ftcore`, then run:

```
cargo run --release -p jvm-spike -- /path/to/libjvm /path/to/tests/forge/build/dev/ftcore.jar ft/core/Target
```

The program loads the specified JVM, finds a static `value()I` method and calls its cached method ID 100,000 times. It reports the last result and elapsed time per call. One local JDK 8/macOS ARM64 run returned 1 at roughly 196 ns per call; this is a single microbenchmark, not a frame-time or portability claim.

This does not boot LaunchClassLoader, Forge or Mixin, diff transformed methods, host Minecraft methods or bridge game state. Those remain required parts of FG2.

`build.sh` also builds an experimental LaunchWrapper/Mixin harness. The embedded run `spike/Run 1` currently fails: LaunchWrapper starts, but its test target returns 1 rather than the expected injected value 2. A real headless Forge server does apply the same mixin. This harness records a reproducible bootstrap gap; it is not a passing FG2 test.
