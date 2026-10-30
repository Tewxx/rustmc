#!/bin/bash
# Compare fresh vanilla and rustmc captures. Vanilla runs on a private Xvfb display on Linux.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ "$(uname -s)" = Linux ] && command -v xvfb-run >/dev/null || { echo "Full capture requires Linux and xvfb-run; no visible window will be opened." >&2; exit 1; }
mkdir -p shots
export VISUAL_SHOTS=$(mktemp -d "$PWD/shots/run.XXXXXX")
echo "captures: $VISUAL_SHOTS"
active=0
cleanup() { if [ "$active" = 1 ]; then tools/visual/server.sh stop; fi; }
trap cleanup EXIT
worlds=("$@")
[ ${#worlds[@]} -gt 0 ] || worlds=(title grid terrain)
for world in "${worlds[@]}"; do
  case "$world" in title|grid|terrain) ;; *) echo "unknown world: $world" >&2; exit 2;; esac
  if [ "$world" != title ]; then tools/visual/server.sh start "$world"; active=1; fi
  WORLD=$world tools/visual/vanilla.sh
  WORLD=$world tools/visual/run_rustmc.sh
  if [ "$active" = 1 ]; then tools/visual/server.sh stop; active=0; fi
done
diff_args=()
for world in "${worlds[@]}"; do diff_args+=(--world "$world"); done
tools/visual/diff.py "${diff_args[@]}"
