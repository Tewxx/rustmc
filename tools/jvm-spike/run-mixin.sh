#!/bin/bash
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
"$HERE/build.sh" >/dev/null
case "$(uname -s)" in
  Darwin) JVM="$JDK8/jre/lib/server/libjvm.dylib";;
  Linux) JVM="$JDK8/jre/lib/amd64/server/libjvm.so"; [ -f "$JVM" ] || JVM="$JDK8/jre/lib/aarch64/server/libjvm.so";;
  *) echo 'Set up the JVM path for this platform before running this spike' >&2; exit 1;;
esac
CP="$FT_BUILD/jvm-spike/spike.jar:$FT_BUILD/mods/ftmixin.jar:$PL/org/spongepowered/mixin/0.8.7/mixin-0.8.7.jar:$(ft_libs)"
cargo run --manifest-path "$FT_REPO/Cargo.toml" --release -p jvm-spike -- "$JVM" "$CP" spike/Run 1
