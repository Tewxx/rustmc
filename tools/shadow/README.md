# Experimental shadow jar

On Windows, `./tools/shadow/stubs.ps1` generates the Forge-patched SRG shadow jar,
the API stub jar, and the Java bridge classes using locally installed JDK 8,
Prism Launcher libraries, and MCP mappings. It defaults to `%APPDATA%/PrismLauncher`
and `MCP-919` inside or beside this repository; `-PrismDir`, `-McpDir`, and `-Jdk8`
override these locations. `-ValidateOnly` checks all inputs without building.
The exact 46-library runtime classpath comes from `tools/forgetest/env.sh`, rather
than collecting incompatible jar versions recursively. `-MixinScans @('a.json')`
adds the existing scanner's required real method bodies.

The default output is the git-ignored `tests/forge/build/stubs.jar`. The script
also writes `tests/forge/build/runtime.json` with `stubsJar`, `shadowJar`,
`javaHome`, `libjvm`, `classpath`, and the individual `libraries`. It sets the
corresponding `RUSTMC_STUBS_JAR`, `RUSTMC_FORGE_STUBS`, `RUSTMC_FORGE_CP`, and
`RUSTMC_JVM` environment variables in the calling PowerShell session. A later
session can restore them from that manifest before running JVM tests:

```powershell
$runtime = Get-Content tests/forge/build/runtime.json -Raw | ConvertFrom-Json
$env:RUSTMC_STUBS_JAR = $runtime.stubsJar
$env:RUSTMC_FORGE_STUBS = $runtime.stubsJar
$env:RUSTMC_FORGE_CP = $runtime.classpath
$env:RUSTMC_JVM = $runtime.libjvm
cargo test -p mc-forge --test stubs_jvm -- --nocapture
```

Generation and class presence do not prove that a mod or injected client works.
The generator may report explicitly skipped classes; inspect that output before
claiming complete API coverage. Jars are generated in unique staging directories
and the destination is replaced only after glue compilation succeeds. The cached
shadow jar is invalidated when its source generators, mappings, or input jars change.

`tools/shadow/build.sh /path/to/new.jar` uses the local Minecraft 1.8.9 client, Forge 11.15.1.2318 universal jar, MCP mappings and JDK 8 configured by `tools/forgetest/env.sh`. It checks patch checksums, applies client binpatches, and maps class and member references from obfuscated names to SRG. It refuses to overwrite the output. The output contains locally supplied game code; do not commit or redistribute it.

This is a first spike, not a Forge runtime. It does not apply access transformers, nativise methods, or provide a state bridge. Hierarchy resolution, transformed bytecode, resources and full class coverage require comparison against the real Forge loader. On the local inputs it applies 825 patches and produces 2,534 classes; basic SRG names have been inspected, but no Forge boot from this jar has been verified.

`tools/shadow/check-diff.sh` exercises a preliminary method comparison against the controlled coremod transformer. `MethodDiff` compares individual class files by normalized ASM method text and access flags, ignoring debug metadata and stack map frames. It detects the controlled changed method and no change for an identical class. This is not yet connected to the actual Forge transformer chain and is not a complete semantic equivalence or hook-detection guarantee.

`tools/shadow/check-mixin-diff.sh` runs the embedded LaunchWrapper probe with Mixin class export enabled, then compares the actual transformed class to its original. It checks that both the modified method and injected handler are detected. This still uses a controlled test class and standalone tweaker, not the full Forge transformer chain.

`tools/jvm-spike/run-shadow.sh` now tests one actual shadow class against the real
FML/LaunchWrapper/Mixin transformer chain and a Rust JNI callback, with selected
and final byte captures and a no-Mixin control. It still uses the original jar for
FML's patch/remapping lookups and a probe-only selector for one class; it does not
establish that the rest of the shadow graph can load or that the production bridge
can safely host arbitrary Forge methods. See the JVM spike README for the evidence
and limitations.
