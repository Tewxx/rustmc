#!/bin/bash
# Run an isolated offline vanilla server. Each start creates a fresh, retained world.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
INPUTS=${RUSTMC_INPUTS:-$(cd "$(git rev-parse --git-common-dir)/.." && pwd)}
JAVA_HOME_DIR=${JDK8:-$INPUTS/.mc-tools/jdk8}
[ -x "$JAVA_HOME_DIR/bin/java" ] || JAVA_HOME_DIR="$JAVA_HOME_DIR/Contents/Home"
JAVA=$JAVA_HOME_DIR/bin/java
JAR=${VISUAL_SERVER_JAR:-$INPUTS/.mc-tools/server/server.jar}
PORT=${PORT:-25871}
mkdir -p "$ROOT/shots/server"
state="$ROOT/shots/server/active"
case "${1:-}" in
start)
  world=${2:-}; [[ $world == grid || $world == terrain ]] || { echo 'expected grid or terrain' >&2; exit 2; }
  [ ! -e "$state" ] || { echo "server state exists: $state; stop it first" >&2; exit 1; }
  [ -f "$JAR" ] && [ -x "$JAVA" ] || { echo 'server jar or JDK 8 missing' >&2; exit 1; }
  D=$(mktemp -d "$ROOT/shots/server/${world}.XXXXXX")
  if [ "$world" = grid ]; then TYPE=FLAT; SETTINGS='3;minecraft:bedrock,60*minecraft:stone;1;'; else TYPE=DEFAULT; SETTINGS=''; fi
  cat > "$D/server.properties" <<PROPS
online-mode=false
server-ip=127.0.0.1
server-port=$PORT
level-name=world
level-seed=1234
level-type=$TYPE
generator-settings=$SETTINGS
generate-structures=false
gamemode=1
force-gamemode=true
difficulty=0
view-distance=16
spawn-protection=0
spawn-monsters=false
spawn-animals=false
spawn-npcs=false
allow-flight=true
max-players=2
PROPS
  python3 - "$D/ops.json" <<'PY'
import hashlib, uuid, json, sys
h=bytearray(hashlib.md5(b'OfflinePlayer:Player').digest()); h[6]=h[6]&15|48; h[8]=h[8]&63|128
with open(sys.argv[1], 'w') as f: json.dump([{'uuid':str(uuid.UUID(bytes=bytes(h))),'name':'Player','level':4}],f)
PY
  echo eula=true > "$D/eula.txt"
  mkfifo "$D/cmd.fifo"
  (tail -f /dev/null > "$D/cmd.fifo") & holder=$!
  (cd "$D" && exec "$JAVA" -Xmx2G -jar "$JAR" nogui < cmd.fifo > server.log 2>&1) & pid=$!
  printf '%s\n%s\n%s\n' "$D" "$pid" "$holder" > "$state"
  for _ in $(seq 1 120); do
    grep -q 'Done (' "$D/server.log" 2>/dev/null && { echo "server ready: $D"; exit 0; }
    kill -0 "$pid" 2>/dev/null || { echo "server exited; see $D/server.log" >&2; kill "$holder" 2>/dev/null || true; rm "$state"; exit 1; }
    sleep 1
  done
  echo "server timed out; see $D/server.log" >&2
  "$ROOT/tools/visual/server.sh" stop || true
  exit 1;;
stop)
  [ -f "$state" ] || { echo 'no managed server'; exit 0; }
  { read -r D; read -r pid; read -r holder; } < "$state"
  if kill -0 "$pid" 2>/dev/null; then
    echo stop > "$D/cmd.fifo"
    for _ in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then echo "server still running (pid $pid); state retained" >&2; exit 1; fi
  fi
  kill "$holder" 2>/dev/null || true
  rm "$state"
  echo "server stopped; world retained at $D";;
*) echo "usage: $0 start grid|terrain | stop" >&2; exit 2;;
esac
