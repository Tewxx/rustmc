#!/bin/bash
# Capture the reference client only inside an isolated Xvfb display (Linux).
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
if [ "${VISUAL_IN_XVFB:-}" != 1 ]; then
  [ "$(uname -s)" = Linux ] && command -v xvfb-run >/dev/null || {
    echo 'Vanilla capture needs Linux with xvfb-run; no visible window will be opened.' >&2; exit 1;
  }
  exec xvfb-run -a -s '-screen 0 1280x720x24' env VISUAL_IN_XVFB=1 "$0" "$@"
fi
INPUTS=${RUSTMC_INPUTS:-$(cd "$(git rev-parse --git-common-dir)/.." && pwd)}
MCP=${MCP:-$INPUTS/MCP-919}
JAVA_HOME_DIR=${JDK8:-$INPUTS/.mc-tools/jdk8}
[ -x "$JAVA_HOME_DIR/bin/java" ] || JAVA_HOME_DIR="$JAVA_HOME_DIR/Contents/Home"
PRISM=${PRISM_LIBS:-$INPUTS/.mc-tools/libraries}
LW=$PRISM/org/lwjgl/lwjgl
SHOTS=${VISUAL_SHOTS:-$ROOT/shots}
NAT=$SHOTS/natives
OUT=$SHOTS/vanilla
NATIVES=${LWJGL_NATIVES:-$LW/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-linux.jar}
[ -f "$NATIVES" ] || { echo "missing Linux LWJGL natives: $NATIVES" >&2; exit 1; }
mkdir -p "$NAT" "$SHOTS/harness" "$OUT"
(cd "$NAT" && unzip -oq "$NATIVES")
CP=$MCP/bin/minecraft:$(find "$MCP/jars/libraries" -name '*.jar' | grep -v 'natives\|/lwjgl/' | tr '\n' ':')$LW/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar:$LW/lwjgl_util/2.9.4-nightly-20150209/lwjgl_util-2.9.4-nightly-20150209.jar:$MCP/jars/versions/1.8.9/1.8.9.jar
"$JAVA_HOME_DIR/bin/javac" -nowarn -cp "$CP" -d "$SHOTS/harness" "$ROOT/tools/visual/VisualHarness.java"
exec "$JAVA_HOME_DIR/bin/java" -Djava.library.path="$NAT" \
  -Dharness.world="${WORLD:-}" -Dharness.port="${PORT:-25871}" -Dharness.assets="$MCP/jars/assets" \
  -cp "$SHOTS/harness:$CP" VisualHarness "$OUT" "$ROOT/${1:-tools/visual/scenes.txt}" "${W:-1280}" "${H:-720}" "${2:-}"
