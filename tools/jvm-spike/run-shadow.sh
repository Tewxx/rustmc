#!/bin/bash
# Headless controlled FML/Mixin probe: never launches Minecraft's game entrypoint.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
"$HERE/build.sh" >/dev/null
"$FT_HERE/build.sh" ftshadow ftbus ftlifecycle ftlifecycle2 >/dev/null
SHADOW=${1:-$FT_BUILD/shadow-probe.jar}
[ -f "$SHADOW" ] || { echo "missing generated shadow jar: $SHADOW (run tools/shadow/build.sh first)" >&2; exit 1; }
SHADOW=$(cd "$(dirname "$SHADOW")" && pwd)/$(basename "$SHADOW")
VANILLA="$PL/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar"
[ -f "$VANILLA" ] || { echo "missing vanilla input: $VANILLA" >&2; exit 1; }
case "$(uname -s)" in
  Darwin) JVM="$JDK8/jre/lib/server/libjvm.dylib";;
  Linux) JVM="$JDK8/jre/lib/amd64/server/libjvm.so"; [ -f "$JVM" ] || JVM="$JDK8/jre/lib/aarch64/server/libjvm.so";;
  *) echo 'Unsupported JDK library path' >&2; exit 1;;
esac
D=$(mktemp -d "${TMPDIR:-/tmp}/rustmc-shadow-spike.XXXXXX")
echo "probe directory: $D"
CP="$FT_BUILD/jvm-spike/spike.jar:$FT_BUILD/mods/ftshadow.jar:$FORGE_JAR:$VANILLA:$SHADOW:$PL/org/spongepowered/mixin/0.8.7/mixin-0.8.7.jar:$(ft_libs)"
ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
mkdir -p "$D/shadow-tools"
"$FT_JAVAC" -cp "$ASM" -d "$D/shadow-tools" "$FT_REPO/tools/shadow/MethodDiff.java"
unzip -p "$SHADOW" net/minecraft/util/MathHelper.class > "$D/pristine.class"
[ -s "$D/pristine.class" ] || { echo 'shadow MathHelper missing' >&2; exit 1; }
for mode in control hooked graph-control graph-hooked; do
  mkdir -p "$D/$mode/mods"
  cp "$FT_BUILD/mods/ftcommon.jar" "$FT_BUILD/mods/ftbus.jar" "$FT_BUILD/mods/ftlifecycle.jar" "$FT_BUILD/mods/ftlifecycle2.jar" "$D/$mode/mods/"
  case "$mode" in
    control) flag=0; arg=shadow-control; graph=0;;
    hooked) flag=1; arg=shadow; graph=0;;
    graph-control) flag=0; arg=shadow-control; graph=1;;
    graph-hooked) flag=1; arg=shadow; graph=1;;
  esac
  if ! (cd "$D/$mode" && RUSTMC_SHADOW_JAR="$SHADOW" RUSTMC_SHADOW_MIXIN="$flag" RUSTMC_SHADOW_GRAPH="$graph" \
      CARGO_TARGET_DIR="$FT_BUILD/jvm-spike/cargo-target" cargo run --quiet --manifest-path "$FT_REPO/Cargo.toml" --release -p jvm-spike -- "$JVM" "$CP" bridgeprobe/ShadowRun 100000 "$arg") > "$D/$mode.log" 2>&1; then
    tail -60 "$D/$mode.log" >&2; exit 1
  fi
  cmp "$FT_REPO/tests/forge/oracle/bus-client.jsonl" "$D/$mode/ft-trace/bus.jsonl" || { echo "event bus trace differs from Forge oracle" >&2; exit 1; }
  cmp "$FT_REPO/tests/forge/oracle/lifecycle-client-subset.jsonl" "$D/$mode/ft-trace/lifecycle.jsonl" || { echo "lifecycle trace differs from Forge oracle" >&2; exit 1; }
  if grep -Eq 'SEVERE:|unsupported headless client call|shadow missing available class|Unable to read a class file correctly|An unexpected exception' "$D/$mode.log"; then
    echo "FML lifecycle reported an error; see $D/$mode.log" >&2; exit 1
  fi
  cmp "$D/pristine.class" "$D/$mode/selected-MathHelper.class"
  "$FT_JAVA" -cp "$D/shadow-tools:$ASM" MethodDiff "$D/pristine.class" "$D/$mode/final-MathHelper.class" > "$D/$mode.diff"
  if [ "$flag" = 0 ]; then
    [ ! -s "$D/$mode.diff" ] || { echo 'unchanged control differs:' >&2; cat "$D/$mode.diff" >&2; exit 1; }
  else
    grep -Fxq 'func_76128_c(D)I' "$D/$mode.diff" || { echo 'missing modified target in method diff' >&2; exit 1; }
    grep -q '^handler\$.*afterFloor' "$D/$mode.diff" || { echo 'missing injected handler in method diff' >&2; exit 1; }
  fi
  if [ "$graph" = 1 ]; then
    for cls in net.minecraft.init.Bootstrap net.minecraft.init.Blocks net.minecraft.block.Block net.minecraft.util.MathHelper \
      'net.minecraft.block.Block$5' 'net.minecraft.block.Block$6' \
      'net.minecraft.world.biome.BiomeGenBase$FlowerEntry' 'net.minecraft.world.biome.BiomeGenHills$EmeraldGenerator'; do
      grep -Fxq "$cls" "$D/$mode/shadow-selected.txt" || { echo "shadow graph did not select $cls" >&2; exit 1; }
    done
    count=$(wc -l < "$D/$mode/shadow-selected.txt" | tr -d ' ')
    [ "$count" -ge 400 ] || { echo "shadow graph selected too few classes: $count" >&2; exit 1; }
    missing=0
    [ ! -f "$D/$mode/shadow-missing.txt" ] || missing=$(wc -l < "$D/$mode/shadow-missing.txt" | tr -d ' ')
    echo "$mode selected $count shadow classes; $missing unresolved lookups with no input bytes"
  fi
  grep 'rust_forge_movement_view=passed' "$D/$mode.log"
  grep 'rust_forge_tick_bridge=passed' "$D/$mode.log"
  grep 'rust_forge_event_bridge=passed' "$D/$mode.log"
  grep 'shadow_mode=' "$D/$mode.log"
done
echo 'lifecycle and event bus traces match the Forge oracle; selected bytes match the shadow; final chain diff finds target and injection; control is unchanged'
