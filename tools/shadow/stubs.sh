#!/bin/bash
# Local-only: no-op stub jar of the whole mod-facing API (MC 1.8.9 SRG + Forge/FML + LaunchWrapper + LWJGL2).
# Third-party libs inside the Forge jar (repackage/, paulscode...) stay real. Override with STUB_KEEP=<regex>.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
[ "$#" = 1 ] || { echo "usage: $0 NEW_OUTPUT.jar" >&2; exit 2; }
ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
CP="$ASM:$PL/lzma/lzma/0.0.1/lzma-0.0.1.jar:$FORGE_JAR"
# Forge classes reference notch names; shadow.forge makes ShadowJar remap them to SRG alongside vanilla.
SHADOW="$FT_BUILD/shadow-forge.jar"
[ -f "$SHADOW" ] || SHADOW_JAVA_OPTS=-Dshadow.forge=true "$HERE/build.sh" "$SHADOW"
# Kept as-is: third-party libs in the Forge jar, plus tools/shadow/keep-real.txt (and $STUB_KEEP).
REAL=$(grep -v '^#' "$HERE/keep-real.txt" | grep . | paste -sd'|' -)
# STUB_MIXIN_SCANS="a.json b.json" (tools/modscan output) also keeps real the classes those mods' mixins need bodies in.
if [ -n "${STUB_MIXIN_SCANS:-}" ]; then REAL="$REAL|$(python3 -I "$HERE/mixin-keep.py" $STUB_MIXIN_SCANS | paste -sd'|' -)"; fi
KEEP="fml/repackage/|^(?!net/minecraft/|net/minecraftforge/|org/lwjgl/)|$REAL${STUB_KEEP:+|$STUB_KEEP}"
mkdir -p "$FT_BUILD/shadow-tools"
"$FT_JAVAC" -nowarn -cp "$CP" -d "$FT_BUILD/shadow-tools" "$FT_REPO/tools/forgespec/ForgeSpec.java" "$FT_REPO/tools/forgespec/EventScan.java" "$HERE/StubJar.java"
"$FT_JAVA" -cp "$FT_BUILD/shadow-tools:$CP" StubJar "$1" "$KEEP" "$HERE/stub-methods.txt" "$JDK8/jre/lib/rt.jar" $(ft_libs | tr ':' ' ') -- \
  "$SHADOW" "$FORGE_JAR" \
  "$PL/org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar" "$PL/org/lwjgl/lwjgl/lwjgl_util/2.9.4-nightly-20150209/lwjgl_util-2.9.4-nightly-20150209.jar"
# Stub.call target: compiled separately (a stubbed Stub would recurse) and added to the same jar.
STUBC="$FT_BUILD/stub-class"; rm -rf "$STUBC"; mkdir -p "$STUBC"

# The glue needs Forge and LaunchWrapper types, so it compiles against the stub jar itself. LaunchWrapper is not
# stubbed: it is the real class loader that Mixin and the stubs run under.
"$FT_JAVAC" -nowarn -cp "$1:$PL/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar:$ASM" -d "$STUBC" "$FT_REPO"/crates/mc-forge/java/rustmc/*/*.java
"$JDK8/bin/jar" uf "$1" -C "$STUBC" rustmc
