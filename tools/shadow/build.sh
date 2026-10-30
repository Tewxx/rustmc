#!/bin/bash
# Build from locally supplied official artifacts; never distributes their contents.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
[ "$#" = 1 ] || { echo "usage: $0 NEW_OUTPUT.jar" >&2; exit 2; }
ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
LZMA="$PL/lzma/lzma/0.0.1/lzma-0.0.1.jar"
CP="$ASM:$LZMA:$FORGE_JAR"
mkdir -p "$FT_BUILD/shadow-tools"
"$FT_JAVAC" -nowarn -cp "$CP" -d "$FT_BUILD/shadow-tools" "$FT_REPO/tools/forgespec/ForgeSpec.java" "$FT_REPO/tools/forgespec/EventScan.java" "$HERE/ShadowJar.java"
"$FT_JAVA" -cp "$FT_BUILD/shadow-tools:$CP" ShadowJar "$PL/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar" "$FORGE_JAR" "$MCP/conf/joined.srg" "$MCP/conf/methods.csv" "$MCP/conf/fields.csv" "$1"
