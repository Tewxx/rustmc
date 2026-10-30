#!/bin/bash
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
"$FT_HERE/build.sh" ftcommon ftcore >/dev/null
ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
LW="$PL/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
CP="$FT_BUILD/dev/ftcore.jar:$ASM:$LW"
mkdir -p "$FT_BUILD/shadow-tools"
"$FT_JAVAC" -cp "$CP" -d "$FT_BUILD/shadow-tools" "$FT_HERE/FTCoreCheck.java" "$HERE/MethodDiff.java"
BEFORE="$FT_BUILD/classes/ftcore/ft/core/Target.class"
AFTER="$FT_BUILD/shadow-tools/Target-transformed.class"
"$FT_JAVA" -cp "$FT_BUILD/shadow-tools:$CP" FTCoreCheck "$BEFORE" "$AFTER" >/dev/null
[ -z "$("$FT_JAVA" -cp "$FT_BUILD/shadow-tools:$ASM" MethodDiff "$BEFORE" "$BEFORE")" ]
DIFF=$("$FT_JAVA" -cp "$FT_BUILD/shadow-tools:$ASM" MethodDiff "$BEFORE" "$AFTER")
[ "$DIFF" = 'value()I' ] || { echo "unexpected diff: $DIFF" >&2; exit 1; }
echo 'method diff detects value()I'
