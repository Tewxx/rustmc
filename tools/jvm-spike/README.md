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
It boots vanilla registries and runs the real FML lifecycle with a small headless
client delegate and the project's `ftcommon`, `ftbus`, `ftlifecycle`, and `ftlifecycle2`
test mods. Both modes must match the 19-line Forge client oracle fixture in
`tests/forge/oracle/lifecycle-client-subset.jsonl`; its mod list is narrowed to
the four test mods and Forge built-ins present in this run. The 35 event-bus
records also match `tests/forge/oracle/bus-client.jsonl`, covering priorities,
cancellation, results, registration, inheritance, and error cases. Unsupported client
delegate calls and event-dispatch errors fail the probe.
The first two runs insert only the local shadow `MathHelper` bytes after Forge's
patcher and before the Mixin transformers. Two more runs select every available
`net.minecraft` class from the shadow jar in the same position. They check that
hundreds of classes, including Forge-added nested classes, were selected and fail
if an input class has no matching shadow entry. Unresolved lookups with no input
bytes are counted separately in the probe directory. A final transformer saves the bytes passed to class
definition. The script compares the selected bytes to the shadow jar, then compares
per-method normalized ASM output before and after the full transformer chain. It
requires an unchanged no-Mixin control and detects `func_76128_c(D)I` plus the
injected handler. The reusable `mc-forge` tick bridge posts START/END/START and checks that our
test mod observes that exact sequence. Rust also posts a controlled event through the Forge bus and checks listener order,
payload changes, and cancellation on the returned result. It also registers a
controlled JNI native, invokes the hooked method, checks every callback, and
measures both control and hooked dispatch after
10,000 warmup calls. Logs and locally derived class bytes stay in the printed
probe directory and must not be redistributed.

FML's remapper expects the original obfuscated jar to resolve patch inputs. Shadow
alone fails; placing shadow ahead of the original jar causes a double-patch checksum
failure. This experiment puts the original first and substitutes either the test
class or the available Minecraft class graph from shadow. The test coremod registers the selector before Mixin
bootstrap; the probe verifies the public transformer list places it after Forge’s
patcher and before Mixin. This
is an experiment, not a production loader or whole-graph solution. The graph run may discard transformations registered before the selector, and
classes loaded before the selector is registered bypass it. It does not prove
complete class coverage or correctness with other coremods. Forge reports
a signature warning in this environment, and Mixin reports
initialization during a tweak constructor; both remain to be resolved. The probe does not instantiate the Minecraft client, nativise world subsystems,
synchronize game fields, or demonstrate arbitrary-mod behavior. The lifecycle
uses vanilla registry bootstrap and a stubbed client delegate, so it does not
prove Rust ownership of game state or support for mods that need client services.
A single local warm run measured about 320 ns/control and 347 ns/hooked dispatch;
these are noisy microbenchmarks and do not establish game or cross-platform cost.

The event-bus oracle comparison is strict. A repeated graph run once emitted an
extra EV-05 instance-handler record: Forge dispatched the instance handler before
a same-priority static handler that then threw `IncompatibleClassChangeError`.
Another ordering throws first. The probe preserves its raw logs and trace in the
printed directory; repeated real-client oracle sessions are needed to determine
whether both orders are valid before changing the comparison.
