#!/bin/bash
# Headless controlled FML/Mixin probe: never launches Minecraft's game entrypoint.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
"$HERE/build.sh" >/dev/null
"$FT_HERE/build.sh" ftshadow >/dev/null
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
for mode in control hooked; do
  mkdir -p "$D/$mode"
  if [ "$mode" = control ]; then flag=0; arg=shadow-control; else flag=1; arg=shadow; fi
  if ! (cd "$D/$mode" && RUSTMC_SHADOW_JAR="$SHADOW" RUSTMC_SHADOW_MIXIN="$flag" \
      cargo run --quiet --manifest-path "$FT_REPO/Cargo.toml" --release -p jvm-spike -- "$JVM" "$CP" bridgeprobe/ShadowRun 100000 "$arg") > "$D/$mode.log" 2>&1; then
    tail -60 "$D/$mode.log" >&2; exit 1
  fi
  cmp "$D/pristine.class" "$D/$mode/selected-MathHelper.class"
  "$FT_JAVA" -cp "$D/shadow-tools:$ASM" MethodDiff "$D/pristine.class" "$D/$mode/final-MathHelper.class" > "$D/$mode.diff"
  if [ "$mode" = control ]; then
    [ ! -s "$D/$mode.diff" ] || { echo 'unchanged control differs:' >&2; cat "$D/$mode.diff" >&2; exit 1; }
  else
    grep -Fxq 'func_76128_c(D)I' "$D/$mode.diff" || { echo 'missing modified target in method diff' >&2; exit 1; }
    grep -q '^handler\$.*afterFloor' "$D/$mode.diff" || { echo 'missing injected handler in method diff' >&2; exit 1; }
  fi
  grep 'shadow_mode=' "$D/$mode.log"
done
echo 'selected bytes match the shadow; final chain diff finds target and injection; control is unchanged'
