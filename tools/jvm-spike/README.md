# JNI invocation spike

Build a controlled target first with `tools/forgetest/build.sh ftcommon ftcore`, then run:

```
cargo run --release -p jvm-spike -- /path/to/libjvm /path/to/tests/forge/build/dev/ftcore.jar ft/core/Target
```

The program loads the specified JVM, finds a static `value()I` method and calls its cached method ID 100,000 times. It reports the last result and elapsed time per call. One local JDK 8/macOS ARM64 run returned 1 at roughly 196 ns per call; this is a single microbenchmark, not a frame-time or portability claim.

This does not boot LaunchClassLoader, Forge or Mixin, diff transformed methods, host Minecraft methods or bridge game state. Those remain required parts of FG2.

`tools/jvm-spike/run-mixin.sh` builds a controlled LaunchWrapper/Mixin harness and checks that the embedded JVM returns 2 after injection (the original method returns 1), then calls the transformed instance method 100,000 times through a cached JNI method ID. One local run measured about 216 ns per call; this is a microbenchmark, not a game-frame measurement. It succeeds locally on JDK 8/macOS ARM64, although Mixin reports a bootstrap timing warning and an unknown side. It does not launch FML, use the shadow jar, diff methods, or exercise a Minecraft class.

## Headless FML + shadow probe

Build a shadow jar locally and run the controlled probe:

```sh
mkdir -p tests/forge/build
tools/shadow/build.sh tests/forge/build/shadow-probe.jar
tools/jvm-spike/run-shadow.sh tests/forge/build/shadow-probe.jar
```

The probe embeds JDK 8, runs the real FML client tweaker and LaunchClassLoader,
and loads the project's `ftshadow` coremod and Mixin. A headless entrypoint replaces
the Minecraft game entrypoint; no game window, third-party mod or account is used.
The selector inserts the local shadow `MathHelper` bytes after Forge's patcher and
before the Mixin transformers. A final transformer saves the bytes passed to class
definition. The script compares the selected bytes to the shadow jar, then compares
per-method normalized ASM output before and after the full transformer chain. It
requires an unchanged no-Mixin control and detects `func_76128_c(D)I` plus the
injected handler. Rust registers a controlled JNI native, invokes the hooked
method, checks every callback, and measures both control and hooked dispatch after
10,000 warmup calls. Logs and locally derived class bytes stay in the printed
probe directory and must not be redistributed.

FML's remapper expects the original obfuscated jar to resolve patch inputs. Shadow
alone fails; placing shadow ahead of the original jar causes a double-patch checksum
failure. This experiment puts the original first and substitutes **only the test
class** from shadow. The test coremod registers the selector before Mixin
bootstrap; the probe verifies the public transformer list places it after Forge’s
patcher and before Mixin. This
is an experiment, not a production loader or whole-graph solution. Substituting
all shadow classes at this position would discard any earlier transformations,
and classes loaded before the selector is registered would bypass it. Forge reports
a signature warning in this environment, and Mixin reports
initialization during a tweak constructor; both remain to be resolved. The probe
does not start full FML mod lifecycle, instantiate the Minecraft client, nativise
world subsystems, synchronize game fields, or demonstrate arbitrary-mod behavior.
A single local warm run measured about 320 ns/control and 347 ns/hooked dispatch;
these are noisy microbenchmarks and do not establish game or cross-platform cost.
