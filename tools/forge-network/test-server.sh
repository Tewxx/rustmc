#!/bin/bash
# Exercise the Rust client handshake against a fresh, offline local Forge 1.8.9 server.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
INPUTS=${RUSTMC_INPUTS:-$(cd "$(git rev-parse --git-common-dir)/.." && pwd)}
SERVER=${FORGE_SERVER_DIR:-$INPUTS/.mc-tools/forge-server}
JDK=${JDK8:-$INPUTS/.mc-tools/jdk8}
[ -x "$JDK/bin/java" ] || JDK=$JDK/Contents/Home
VER=1.8.9-11.15.1.2318-1.8.9
for f in libraries minecraft_server.1.8.9.jar "forge-$VER-universal.jar"; do [ -e "$SERVER/$f" ] || { echo "missing $SERVER/$f" >&2; exit 1; }; done
PORT=${RUSTMC_FORGE_TEST_PORT:-$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')}
DIR=$(mktemp -d "${TMPDIR:-/tmp}/rustmc-forge-network.XXXXXX")
for f in libraries minecraft_server.1.8.9.jar "forge-$VER-universal.jar"; do ln -s "$SERVER/$f" "$DIR/$f"; done
printf 'eula=true\n' > "$DIR/eula.txt"
printf 'online-mode=false\nserver-ip=127.0.0.1\nserver-port=%s\nlevel-type=FLAT\nspawn-animals=false\nspawn-monsters=false\n' "$PORT" > "$DIR/server.properties"
mkfifo "$DIR/in"
(tail -f /dev/null > "$DIR/in") & holder=$!
(cd "$DIR" && exec "$JDK/bin/java" -Xmx768m -jar "forge-$VER-universal.jar" nogui < in > server.log 2>&1) & server=$!
cleanup() {
  if kill -0 "$server" 2>/dev/null; then echo stop > "$DIR/in" || true; fi
  kill "$holder" 2>/dev/null || true
  wait "$server" 2>/dev/null || true
  echo "Forge server log: $DIR/server.log"
}
trap cleanup EXIT
for i in $(seq 1 90); do
  grep -q 'Done (' "$DIR/server.log" 2>/dev/null && break
  kill -0 "$server" 2>/dev/null || { tail -30 "$DIR/server.log"; exit 1; }
  sleep 1
done
grep -q 'Done (' "$DIR/server.log" || { echo 'Forge server did not start' >&2; exit 1; }
RUSTMC_FORGE_TEST_PORT=$PORT RUSTMC_FORGE_TEST_JAR="$INPUTS/MCP-919/jars/versions/1.8.9/1.8.9.jar" cargo test -p mc-client real_forge_server -- --ignored --nocapture --test-threads=1
