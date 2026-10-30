#!/bin/sh
# Usage: scan_all.sh CORPUS_DIR OUT_DIR   (static analysis only; no mod code is executed)
set -e
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$2"
for j in "$1"/*.jar; do
  b=$(basename "$j" .jar)
  python3 -I "$here/modscan.py" "$j" --out "$2/$b.json"
done
