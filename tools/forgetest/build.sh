#!/bin/bash
# Build every Forge 1.8.9 test mod under tests/forge/mods/ into reobfuscated (SRG) jars: $FT_BUILD/mods/<mod>.jar
#
# Route (documented in tests/forge/README.md): plain javac against the MCP-named vanilla classes
# (MCP-919/bin/minecraft) + the Forge universal jar, then tools/forgetest/Reobf.java maps MCP -> SRG
# member names using MCP-919/conf/{joined.srg,methods.csv,fields.csv}. Output has the same shape as a
# ForgeGradle reobfJar jar (SRG names, mcmod.info, manifest attributes). No Gradle, no network.
#
# Usage: tools/forgetest/build.sh [mod-dir-name ...]   (default: all, in tests/forge/mods/ORDER)
set -eo pipefail
. "$(dirname "$0")/env.sh"
MODS_SRC="$FT_REPO/tests/forge/mods"
mkdir -p "$FT_BUILD/tools" "$FT_BUILD/dev" "$FT_BUILD/mods" "$FT_BUILD/classes"

ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
if [ ! -f "$FT_BUILD/tools/Reobf.class" ] || [ "$FT_HERE/Reobf.java" -nt "$FT_BUILD/tools/Reobf.class" ]; then
  "$FT_JAVAC" -nowarn -cp "$ASM" -d "$FT_BUILD/tools" "$FT_HERE/Reobf.java"
fi

# The Forge universal jar references notch (obfuscated) MC names; make an MCP-named copy for javac.
FORGE_DEV="$FT_BUILD/forge-dev.jar"
VANILLA="$PL/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar"
if [ ! -f "$FORGE_DEV" ] || [ "$FT_HERE/Reobf.java" -nt "$FORGE_DEV" ]; then
  "$FT_JAVA" -cp "$FT_BUILD/tools:$ASM" Reobf deobf "$MCP/conf/joined.srg" "$MCP/conf/methods.csv" "$MCP/conf/fields.csv" "$FORGE_JAR" "$FORGE_DEV" "$VANILLA" "$ASM"
fi
BASECP="$MCP/bin/minecraft:$FORGE_DEV:$(ft_libs)"
if [ $# -gt 0 ]; then
  LIST="$*"
else
  rm -f "$FT_BUILD"/dev/*.jar "$FT_BUILD"/mods/*.jar
  LIST=$(grep -v '^#' "$MODS_SRC/ORDER" | grep -v '^$')
fi

for m in $LIST; do
  d="$MODS_SRC/$m"
  [ -d "$d/src" ] || { echo "no such mod: $m" >&2; exit 1; }
  echo "== $m"
  cls="$FT_BUILD/classes/$m"; rm -rf "$cls"; mkdir -p "$cls"
  # earlier mods (MCP-named dev jars) are on the compile classpath, so mods may depend on ftcommon
  DEVCP=""
  for j in "$FT_BUILD"/dev/*.jar; do [ -f "$j" ] && [ "$j" != "$FT_BUILD/dev/$m.jar" ] && DEVCP="$DEVCP:$j"; done
  EXTRA=""
  [ -f "$d/compile-classpath.txt" ] && EXTRA=":$(sed "s#@PL@#$PL#g" "$d/compile-classpath.txt" | tr '\n' ':')"
  find "$d/src" -name '*.java' > "$FT_BUILD/classes/$m.files"
  "$FT_JAVAC" -nowarn -g -source 1.8 -target 1.8 -encoding UTF-8 -cp "$BASECP$DEVCP$EXTRA" -d "$cls" @"$FT_BUILD/classes/$m.files"
  [ -n "$(find "$cls" -name '*.class' | head -1)" ] || { echo "compile failed: $m" >&2; exit 1; }
  [ -d "$d/res" ] && cp -R "$d/res/." "$cls/"
  mf="$FT_BUILD/classes/$m.mf"; echo "Manifest-Version: 1.0" > "$mf"
  [ -f "$d/manifest.txt" ] && cat "$d/manifest.txt" >> "$mf"
  echo >> "$mf"
  (cd "$cls" && "$JDK8/bin/jar" cfm "$FT_BUILD/dev/$m.jar" "$mf" .)
  "$FT_JAVA" -cp "$FT_BUILD/tools:$ASM" Reobf "$MCP/conf/joined.srg" "$MCP/conf/methods.csv" "$MCP/conf/fields.csv" \
     "$FT_BUILD/dev/$m.jar" "$FT_BUILD/mods/$m.jar" "$MCP/bin/minecraft" "$FORGE_DEV" "$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
done
echo "built: $(ls "$FT_BUILD/mods" | tr '\n' ' ')"
