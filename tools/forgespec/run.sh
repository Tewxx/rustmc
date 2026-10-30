#!/bin/bash
# Regenerates the raw Forge 1.8.9 API/event dumps. Read-only on all inputs.
# Usage: tools/forgespec/run.sh [outDir]   (default: golden/forge-spec under the main repo)
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
MAIN="${RUSTMC_MAIN:-$(cd "$HERE/../.." && pwd)}"
LIB="${PRISM_LIBS:-/Volumes/SSD/PrismLauncher/libraries}"
JDK="${JDK8:-$MAIN/.mc-tools/jdk8/Contents/Home}"
OUT="${1:-$MAIN/golden/forge-spec}"
FORGE="$LIB/net/minecraftforge/forge/1.8.9-11.15.1.2318-1.8.9/forge-1.8.9-11.15.1.2318-1.8.9-universal.jar"
VAN="$LIB/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar"
LW="$LIB/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
MX="$LIB/org/spongepowered/mixin/0.8.7/mixin-0.8.7.jar"
ASM="$LIB/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
LZMA="$LIB/lzma/lzma/0.0.1/lzma-0.0.1.jar"
CP="$ASM:$LZMA:$FORGE"
BUILD="${TMPDIR:-/tmp}/forgespec-build"
mkdir -p "$BUILD" "$OUT"
"$JDK/bin/javac" -nowarn -cp "$CP" -d "$BUILD" "$HERE"/*.java
"$JDK/bin/java" -cp "$BUILD:$CP" ForgeSpec "$OUT" "$FORGE" "$VAN" \
  "$MAIN/MCP-919/conf/joined.srg" "$MAIN/MCP-919/conf/methods.csv" "$MAIN/MCP-919/conf/fields.csv" "$LW" "$MX"

# ---- second stage: patch hook index, event catalogue, copy API dumps into docs/forge-spec ----
REPO="$(cd "$HERE/../.." && pwd)"
DOCS="$REPO/docs/forge-spec"
FSRC="${FORGE_SRC:-$MAIN/.mc-tools/forge-src}"   # git clone --depth 1 -b 1.8.9 https://github.com/MinecraftForge/MinecraftForge
mkdir -p "$DOCS/api"
python3 -I "$HERE/hookindex.py" "$FSRC" "$DOCS/hooks-index.json"
python3 -I "$HERE/enrich.py" "$OUT/events.generated.json" "$DOCS/hooks-index.json" "$DOCS"
for f in api-forge-fml api-vanilla-patched api-launchwrapper api-mixin vanilla-patch-diff access-transformers summary; do cp "$OUT/$f.json" "$DOCS/api/"; done
