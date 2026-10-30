#!/bin/bash
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
"$FT_HERE/build.sh" ftcommon ftmixin
D="$FT_BUILD/jvm-spike"
mkdir -p "$D/classes"
CP="$FT_BUILD/dev/ftmixin.jar:$FT_BUILD/forge-dev.jar:$PL/org/spongepowered/mixin/0.8.7/mixin-0.8.7.jar:$(ft_libs)"
"$FT_JAVAC" -proc:none -source 1.8 -target 1.8 -cp "$CP" -d "$D/classes" "$HERE"/java/spike/*.java "$HERE"/java/bridgeprobe/*.java "$HERE"/java/spikecapture/*.java "$HERE"/java/driverapp/*.java "$FT_REPO"/crates/mc-forge/java/rustmc/forge/*.java
"$JDK8/bin/jar" cf "$D/spike.jar" -C "$D/classes" .
printf '%s\n' "$D/spike.jar:$FT_BUILD/mods/ftmixin.jar:$CP"
