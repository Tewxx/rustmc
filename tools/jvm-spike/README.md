# JNI invocation spike

Build a controlled target first with `tools/forgetest/build.sh ftcommon ftcore`, then run:

```
cargo run --release -p jvm-spike -- /path/to/libjvm /path/to/tests/forge/build/dev/ftcore.jar ft/core/Target
```

The program loads the specified JVM, finds a static `value()I` method and calls its cached method ID 100,000 times. It reports the last result and elapsed time per call. One local JDK 8/macOS ARM64 run returned 1 at roughly 196 ns per call; this is a single microbenchmark, not a frame-time or portability claim.

This does not boot LaunchClassLoader, Forge or Mixin, diff transformed methods, host Minecraft methods or bridge game state. Those remain required parts of FG2.

`tools/jvm-spike/run-mixin.sh` builds a controlled LaunchWrapper/Mixin harness and checks that the embedded JVM returns 2 after injection (the original method returns 1). It succeeds locally on JDK 8/macOS ARM64, although Mixin reports a bootstrap timing warning and an unknown side. It does not launch FML, use the shadow jar, diff methods, or exercise a Minecraft class.
