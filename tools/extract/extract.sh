#!/bin/sh
# Compiles the golden-data extractor against the MCP 1.8.9 classes and writes dumps to <repo>/golden.
# Usage: tools/extract/extract.sh [dump names...]
# Needs a JDK 8: $JAVA8_HOME, or the one tools/setup.sh installs in .mc-tools/jdk8.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
MCP=${MCP:-$REPO/MCP-919}
CLASSES=$MCP/bin/minecraft
[ -f "$CLASSES/net/minecraft/block/Block.class" ] || { echo "No compiled MCP classes in $CLASSES (run tools/setup.sh)" >&2; exit 1; }

J8=${JAVA8_HOME:-$REPO/.mc-tools/jdk8}
[ -d "$J8/Contents/Home" ] && J8=$J8/Contents/Home
[ -x "$J8/bin/javac" ] || { echo "JDK 8 not found; run tools/setup.sh or set JAVA8_HOME" >&2; exit 1; }

# The vanilla jar supplies resources Bootstrap reads (lang files); MCP classes come first so they win.
LIBS=$(find "$MCP/jars/libraries" -name '*.jar' ! -name '*natives*' | tr '\n' ':')
CP="$CLASSES:$MCP/jars/versions/1.8.9/1.8.9.jar:${LIBS%:}"

OUT=$HERE/out
rm -rf "$OUT"; mkdir -p "$OUT"
"$J8/bin/javac" -nowarn -encoding UTF-8 -cp "$CP" -d "$OUT" $(find "$HERE/src" -name '*.java')
exec "$J8/bin/java" -cp "$OUT:$CP" rustmc.extract.Main "$REPO/golden" "$@"
