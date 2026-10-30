#!/bin/bash
# Render every scene in scenes.txt with rustmc at the camera the vanilla harness recorded.
# Usage: run_rustmc.sh [scenes.txt] [filter]
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
INPUTS=${RUSTMC_INPUTS:-$(cd "$(git rev-parse --git-common-dir)/.." && pwd)}
WORLD=${WORLD:-}
FILTER=${2:-}
SHOTS=${VISUAL_SHOTS:-$ROOT/shots}
OUT=$SHOTS/rustmc
mkdir -p "$OUT"
cd "$OUT"
grep -v '^#' "$ROOT/${1:-tools/visual/scenes.txt}" | grep -v '^\s*$' | while read -r name world x y z yaw pitch time gui; do
  [ -n "$WORLD" ] && [ "$world" != "$WORLD" ] && continue
  [ -n "$FILTER" ] && [[ "$name" != *"$FILTER"* ]] && continue
  cam="$SHOTS/vanilla/screenshots/$name.cam"
  [ -f "$cam" ] || { echo "no vanilla camera for $name" >&2; exit 1; }
  read -r cx cy cz cyaw cpitch ctime < "$cam"
  screen=scene
  case "$gui" in inventory) screen=survival;; chat) screen=chat;; main) screen=title;; options) screen=title-options;; esac
  worldargs=(--connect "127.0.0.1:${PORT:-25871}" --name Player)
  [ "$world" = title ] && worldargs=()
  RUSTMC_VISUAL=1 RUSTMC_NO_SOUND=1 RUSTMC_CAM="$cx $cy $cz $cyaw $cpitch" RUSTMC_TIME=$ctime \
    "$ROOT/target/release/rustmc" --jar "$INPUTS/MCP-919/jars/versions/1.8.9/1.8.9.jar" \
    "${worldargs[@]}" \
    --screenshot "$OUT/$name.png" --screen "$screen"
done
