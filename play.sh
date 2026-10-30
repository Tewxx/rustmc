#!/bin/sh
# macOS/Linux equivalent of Play-RustMC.cmd.
cd "$(dirname "$0")" || exit 1
JAR=""
for j in ../MCP-919/jars/versions/1.8.9/1.8.9.jar MCP-919/jars/versions/1.8.9/1.8.9.jar; do
  if [ -f "$j" ]; then JAR="$j"; break; fi
done
if [ -x target/release/rustmc-updated ]; then
  BIN=target/release/rustmc-updated
else
  [ -x target/release/rustmc ] || cargo build --release --offline --bin rustmc || exit 1
  BIN=target/release/rustmc
fi
# Opens the title screen (singleplayer is a build feature, off by default).
if [ -n "$JAR" ]; then exec "$BIN" --jar "$JAR" "$@"; fi
exec "$BIN" "$@"
