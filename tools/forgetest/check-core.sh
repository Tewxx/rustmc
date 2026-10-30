#!/bin/bash
set -euo pipefail
. "$(dirname "$0")/env.sh"
"$FT_HERE/build.sh" ftcommon ftcore
ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
LW="$PL/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
CP="$FT_BUILD/dev/ftcore.jar:$ASM:$LW"
"$FT_JAVAC" -cp "$CP" -d "$FT_BUILD/tools" "$FT_HERE/FTCoreCheck.java"
"$FT_JAVA" -cp "$FT_BUILD/tools:$CP" FTCoreCheck "$FT_BUILD/classes/ftcore/ft/core/Target.class"
