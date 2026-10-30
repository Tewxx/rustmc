#!/bin/bash
# The bridge is not implemented. Report each defined trace case as outstanding.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
python3 - "$ROOT/tests/forge/mods" <<'PY'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
ids = set()
for path in root.rglob('*.java'):
    ids.update(re.findall(r'\.log\("([A-Z]+-\d+)"', path.read_text()))
for case in sorted(ids):
    print(f'{case}: NOT IMPLEMENTED')
print(f'0/{len(ids)} passing (Forge bridge not implemented)')
sys.exit(1)
PY
