#!/bin/bash
# Oracle runner: run the built test mods in REAL Forge 1.8.9 (11.15.1.2318) against a local offline-mode
# Forge dedicated server and record golden traces to $GOLDEN_DIR (default <main checkout>/golden/forge/<group>.jsonl).
#
# Launches Forge directly through LaunchWrapper (no Prism UI, no account) with the Prism library set; a real
# LWJGL2 window opens (macOS has no headless GL) but the session drives itself via tests/forge/mods/ftcommon
# and quits by itself. Nothing under $PRISM is modified. Everything runs in $FT_BUILD/oracle/.
#
# Usage: tools/forgetest/run-forge.sh [--no-build] [--twice] [--out DIR]
#   --twice   run two sessions and diff them (determinism check) before publishing the golden traces
#   --out DIR write traces to DIR instead of $GOLDEN_DIR
set -e
. "$(dirname "$0")/env.sh"
BUILD=1; TWICE=0; OUT="$GOLDEN_DIR"
while [ $# -gt 0 ]; do case "$1" in
  --no-build) BUILD=0;; --twice) TWICE=1;; --out) OUT="$2"; shift;; *) echo "unknown arg $1" >&2; exit 2;; esac; shift; done
if [ "$BUILD" = 1 ]; then "$FT_HERE/build.sh"; fi

O="$FT_BUILD/oracle"
CLIENT_VER_JAR="$PL/com/mojang/minecraft/1.8.9/minecraft-1.8.9-client.jar"
NATIVES_JAR="$PL/org/lwjgl/lwjgl/lwjgl-platform/2.9.4-nightly-20150209/lwjgl-platform-2.9.4-nightly-20150209-natives-osx-arm64.jar"
[ -f "$FORGE_SERVER_DIR/forge-$FORGE_VER-universal.jar" ] || { echo "Forge server not installed in $FORGE_SERVER_DIR (see tests/forge/README.md)" >&2; exit 1; }

run_session() { # $1 = trace dir
  TR="$1"; rm -rf "$O/server" "$O/client" "$TR"; mkdir -p "$O/server/mods" "$O/client/mods" "$O/client/config" "$TR/client" "$TR/server"
  # ---- server ----
  S="$O/server"
  for f in minecraft_server.1.8.9.jar libraries "forge-$FORGE_VER-universal.jar"; do ln -s "$FORGE_SERVER_DIR/$f" "$S/$f"; done
  for m in $(grep -v '^#' "$FT_REPO/tests/forge/mods/SERVER_MODS" | grep -v '^$'); do cp "$FT_BUILD/mods/$m.jar" "$S/mods/"; done
  echo "eula=true" > "$S/eula.txt"
  cat > "$S/server.properties" <<EOF
online-mode=false
server-ip=127.0.0.1
server-port=$FT_PORT
level-type=FLAT
level-seed=1
generate-structures=false
spawn-animals=false
spawn-monsters=false
spawn-npcs=false
difficulty=0
gamemode=0
force-gamemode=true
allow-flight=true
view-distance=4
motd=rustmc forge oracle
enable-command-block=false
max-players=2
EOF
  mkfifo "$S/stdin"
  ( cd "$S" && exec "$FT_JAVA" -Xmx768m -Dft.trace.dir="$TR/server" -jar "forge-$FORGE_VER-universal.jar" nogui < "$S/stdin" > "$S/server.log" 2>&1 ) &
  SPID=$!
  trap 'kill "$SPID" 2>/dev/null || true' EXIT
  exec 7<>"$S/stdin"   # keep the fifo open so the server never sees EOF
  for i in $(seq 1 120); do grep -q 'Done (' "$S/server.log" 2>/dev/null && break; kill -0 $SPID 2>/dev/null || { echo "server died:"; tail -30 "$S/server.log"; exec 7>&-; return 1; }; sleep 1; done
  grep -q 'Done (' "$S/server.log" || { echo "server did not start"; kill $SPID; exec 7>&-; return 1; }
  echo "[oracle] server up"
  # ---- client ----
  C="$O/client"
  for m in $(grep -v '^#' "$FT_REPO/tests/forge/mods/ORDER" | grep -v '^$'); do cp "$FT_BUILD/mods/$m.jar" "$C/mods/"; done
  [ -f "$FT_REPO/tests/forge/oracle-extra-mods.txt" ] && true
  mkdir -p "$C/natives"; (cd "$C/natives" && unzip -qo "$NATIVES_JAR" && rm -rf META-INF)
  cat > "$C/options.txt" <<EOF
pauseOnLostFocus:false
renderDistance:4
guiScale:2
fancyGraphics:false
particles:2
soundCategory_master:0.0
soundCategory_music:0.0
mouseSensitivity:0.0
viewBobbing:false
fov:0.0
gamma:1.0
EOF
  CP="$FORGE_JAR:$CLIENT_VER_JAR:$(ft_libs)"
  CLIENT_STATUS=0
  ( cd "$C" && perl -e 'alarm shift; exec @ARGV' ${FT_CLIENT_TIMEOUT:-300} "$FT_JAVA" -Xmx1g \
      -Djava.library.path="$C/natives" -Dorg.lwjgl.librarypath="$C/natives" -Dnet.java.games.input.librarypath="$C/natives" \
      -Dft.trace.dir="$TR/client" -Dft.timeout.s=${FT_CLIENT_TIMEOUT:-240} \
      -Dfml.ignoreInvalidMinecraftCertificates=true -Dfml.ignorePatchDiscrepancies=true \
      -cp "$CP" net.minecraft.launchwrapper.Launch --tweakClass net.minecraftforge.fml.common.launcher.FMLTweaker \
      --username FTBot --version 1.8.9 --gameDir "$C" --assetsDir "$PRISM/assets" --assetIndex 1.8 \
      --uuid 00000000000000000000000000000001 --accessToken 0 --userProperties '{}' --userType legacy \
      --server 127.0.0.1 --port "$FT_PORT" --width 854 --height 480 ) > "$C/client.log" 2>&1 || CLIENT_STATUS=$?
  echo "stop" >&7; exec 7>&-
  for i in $(seq 1 30); do kill -0 $SPID 2>/dev/null || break; sleep 1; done
  kill $SPID 2>/dev/null || true
  trap - EXIT
  if [ "$CLIENT_STATUS" -ne 0 ]; then echo "[oracle] client failed ($CLIENT_STATUS); see $C/client.log" >&2; return "$CLIENT_STATUS"; fi
  echo "[oracle] session done: $(ls "$TR/client" | wc -l | tr -d ' ') client trace files"
}

if [ $TWICE = 1 ]; then
  run_session "$O/trace-a"; run_session "$O/trace-b"
  if diff -r "$O/trace-a/client" "$O/trace-b/client" > "$O/twice.diff"; then echo "[oracle] two sessions identical"; else echo "[oracle] NONDETERMINISTIC, see $O/twice.diff"; exit 1; fi
  FINAL="$O/trace-b"
else
  run_session "$O/trace"; FINAL="$O/trace"
fi
mkdir -p "$OUT"
cp "$FINAL"/client/*.jsonl "$OUT/"
for f in "$OUT"/*.jsonl; do printf '%s: %s lines\n' "$(basename "$f")" "$(wc -l < "$f" | tr -d ' ')"; done
