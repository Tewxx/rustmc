#!/bin/sh
# Idempotent dev setup (macOS/Linux): vanilla 1.8.9 jar + assets, JDK 8, cargo dev tools.
# Usage: tools/setup.sh [--no-cargo-tools]
# Layout (inside the repo, all git-ignored):
#   MCP-919/jars/versions/1.8.9/1.8.9.jar   vanilla client jar (SHA-1 checked)
#   MCP-919/.minecraft/assets               sound/lang assets (index "1.8")
#   .mc-tools/jdk8                          JDK 8 for tools/extract and tools/oracle
# MCP-919 (decompiled 1.8.9 sources + compiled bin/minecraft) is cloned from Marcelektro/MCP-919.
set -eu
REPO=$(cd "$(dirname "$0")/.." && pwd)
ROOT=$REPO
MCP=$ROOT/MCP-919
TOOLS=$REPO/.mc-tools
J=$MCP/jars/versions/1.8.9
A=$MCP/.minecraft/assets
META=https://piston-meta.mojang.com/mc/game/version_manifest_v2.json
CURL="curl -sfL"
command -v jq >/dev/null || { echo "need jq (brew install jq)"; exit 1; }

[ -d "$MCP/bin/minecraft" ] || { rm -rf "$TOOLS/mcp-clone"; mkdir -p "$TOOLS"; git clone --depth 1 https://github.com/Marcelektro/MCP-919 "$TOOLS/mcp-clone" && rsync -a --exclude .git --ignore-existing "$TOOLS/mcp-clone/" "$MCP/" && rm -rf "$TOOLS/mcp-clone"; }
mkdir -p "$J" "$A/indexes" "$A/objects" "$TOOLS"
VURL=$($CURL $META | jq -r '.versions[]|select(.id=="1.8.9").url')
$CURL "$VURL" -o "$J/1.8.9.json"

SHA=$(jq -r .downloads.client.sha1 "$J/1.8.9.json")
if ! echo "$SHA  $J/1.8.9.jar" | shasum -c - >/dev/null 2>&1; then
  echo "downloading 1.8.9.jar"
  $CURL "$(jq -r .downloads.client.url "$J/1.8.9.json")" -o "$J/1.8.9.jar"
  echo "$SHA  $J/1.8.9.jar" | shasum -c -
fi

$CURL "$(jq -r .assetIndex.url "$J/1.8.9.json")" -o "$A/indexes/1.8.json"
echo "syncing assets"
jq -r '.objects[].hash' "$A/indexes/1.8.json" | sort -u | while read -r h; do
  p=$(echo "$h" | cut -c1-2)
  [ -s "$A/objects/$p/$h" ] || echo "$h"
done | A="$A" xargs -P 16 -n 1 sh -c 'p=$(echo $0 | cut -c1-2); mkdir -p "$A/objects/$p"; curl -sfL "https://resources.download.minecraft.net/$p/$0" -o "$A/objects/$p/$0"'

if [ ! -x "$TOOLS/jdk8/Contents/Home/bin/javac" ] && [ ! -x "$TOOLS/jdk8/bin/javac" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) OS=macos ARCH=aarch64 ;; Darwin-x86_64) OS=macos ARCH=x64 ;;
    Linux-x86_64) OS=linux ARCH=x64 ;;     Linux-aarch64) OS=linux ARCH=aarch64 ;;
    *) echo "unsupported platform for JDK 8 download"; exit 1 ;;
  esac
  echo "downloading JDK 8 (Zulu)"
  mkdir -p "$TOOLS/jdk8"
  URL=$($CURL "https://api.azul.com/metadata/v1/zulu/packages/?java_version=8&os=$OS&arch=$ARCH&archive_type=tar.gz&java_package_type=jdk&latest=true&release_status=ga" | jq -r '.[0].download_url')
  $CURL "$URL" | tar -xz -C "$TOOLS/jdk8" --strip-components=1
fi

if [ "${1:-}" != "--no-cargo-tools" ]; then
  # nextest: faster test runner; samply: sampling profiler for the FPS work
  for t in cargo-nextest samply; do command -v "$t" >/dev/null || cargo install --locked "$t"; done
fi

echo "done. Run: cargo run --release --bin rustmc -- --jar $J/1.8.9.jar"
