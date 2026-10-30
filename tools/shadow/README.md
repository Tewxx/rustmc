# Experimental shadow jar

`tools/shadow/build.sh /path/to/new.jar` uses the local Minecraft 1.8.9 client, Forge 11.15.1.2318 universal jar, MCP mappings and JDK 8 configured by `tools/forgetest/env.sh`. It checks patch checksums, applies client binpatches, and maps class and member references from obfuscated names to SRG. It refuses to overwrite the output. The output contains locally supplied game code; do not commit or redistribute it.

This is a first spike, not a Forge runtime. It does not apply access transformers, nativise methods, or provide a state bridge. Hierarchy resolution, transformed bytecode, resources and full class coverage require comparison against the real Forge loader. On the local inputs it applies 825 patches and produces 2,534 classes; basic SRG names have been inspected, but no Forge boot from this jar has been verified.
