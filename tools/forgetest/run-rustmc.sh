#!/bin/bash
# rustmc side of the Forge trace suite: run the built tests/forge mods on rustmc's Forge API, write their ft.Trace
# files, and diff each group against the trace recorded in real Forge (tests/forge/oracle). Groups without a
# recorded oracle are reported as unverified, not as passing.
#
# Usage: tools/forgetest/run-rustmc.sh [--no-build]
set -euo pipefail
. "$(dirname "$0")/env.sh"
BUILD=1
case "${1:-}" in
  "") ;;
  --no-build) BUILD=0 ;;
  *) echo "unknown arg $1" >&2; exit 2 ;;
esac
if [ "$BUILD" = 1 ]; then "$FT_HERE/build.sh"; fi
export RUSTMC_STUBS_JAR="${RUSTMC_STUBS_JAR:-$FT_BUILD/stubs.jar}"
# Also rebuild it when the Java glue (Bus, ModLoader) changed, not only when the jar is missing.
if [ ! -f "$RUSTMC_STUBS_JAR" ] || [ -n "$(find "$FT_REPO/crates/mc-forge/java" -newer "$RUSTMC_STUBS_JAR" -name '*.java')" ]; then
  "$FT_REPO/tools/shadow/stubs.sh" "$RUSTMC_STUBS_JAR"
fi
export RUSTMC_FORGE_CP="$(ft_libs)"
export JAVA_HOME="$JDK8"
export FT_MODS_DIR="$FT_BUILD/mods"
export FT_TRACE_OUT="$FT_BUILD/rustmc-trace"
rm -rf "$FT_TRACE_OUT"; mkdir -p "$FT_TRACE_OUT"
cd "$FT_REPO" && cargo test --offline -p mc-forge --test forge_traces -- --nocapture

python3 - "$FT_REPO/tests/forge/oracle" "$FT_TRACE_OUT" <<'PY'
import json, pathlib, sys
oracle, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
cases, groups, failed = set(), 0, 0
def pick(group):
    """The recorded client trace for a group; a snapshot from a server session is a last resort."""
    files = sorted(oracle.glob(f'{group}-*.jsonl'))
    return next((f for f in files if '-client' in f.name), files[0])

groups_seen = sorted({f.name.split('-')[0] for f in oracle.glob('*.jsonl')})
for golden in [pick(g) for g in groups_seen]:
    group = golden.name.split('-')[0]
    want = golden.read_text().splitlines()
    path = out / f'{group}.jsonl'
    have = path.read_text().splitlines() if path.exists() else []
    cases.update(json.loads(l)['test'] for l in want)
    if have == want:
        groups += 1
        print(f'{group}: PASS ({len(want)} lines)')
        continue
    failed += 1
    print(f'{group}: FAIL ({len(want)} oracle lines, {len(have)} actual)')
    for i, (a, b) in enumerate(zip(want, have)):
        if a != b:
            print(f'  first difference at line {i + 1}\n    forge : {a}\n    rustmc: {b}')
            break
    else:
        print(f'  {"missing" if len(have) < len(want) else "extra"} lines after {min(len(want), len(have))}')
groups += failed
print(f'{groups - failed}/{groups} groups matching the Forge oracle; {len(cases)} defined cases')
sys.exit(1 if failed else 0)
PY
