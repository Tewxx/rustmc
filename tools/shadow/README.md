# Experimental shadow jar

`tools/shadow/build.sh /path/to/new.jar` uses the local Minecraft 1.8.9 client, Forge 11.15.1.2318 universal jar, MCP mappings and JDK 8 configured by `tools/forgetest/env.sh`. It checks patch checksums, applies client binpatches, and maps class and member references from obfuscated names to SRG. It refuses to overwrite the output. The output contains locally supplied game code; do not commit or redistribute it.

This is a first spike, not a Forge runtime. It does not apply access transformers, nativise methods, or provide a state bridge. Hierarchy resolution, transformed bytecode, resources and full class coverage require comparison against the real Forge loader. On the local inputs it applies 825 patches and produces 2,534 classes; basic SRG names have been inspected, but no Forge boot from this jar has been verified.

`tools/shadow/check-diff.sh` exercises a preliminary method comparison against the controlled coremod transformer. `MethodDiff` compares individual class files by normalized ASM method text and access flags, ignoring debug metadata and stack map frames. It detects the controlled changed method and no change for an identical class. This is not yet connected to the actual Forge transformer chain and is not a complete semantic equivalence or hook-detection guarantee.

`tools/shadow/check-mixin-diff.sh` runs the embedded LaunchWrapper probe with Mixin class export enabled, then compares the actual transformed class to its original. It checks that both the modified method and injected handler are detected. This still uses a controlled test class and standalone tweaker, not the full Forge transformer chain.
