#!/bin/bash
# Reports which mixins of a corpus mod apply against the stub jar. Usage: corpus-mixins.sh [JAR ...]
# (default: every jar in .mc-tools/mod-corpus that has a TweakClass). Needs `tools/modscan/scan_all.sh` output.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
# Corpus mods shade their own Mixin; a second copy on the class path conflicts with it.
export RUSTMC_FORGE_CP="$(ft_libs)"
export JAVA_HOME="$JDK8"
# Rebuild the stub jar with the classes these mods' mixins need real.
SCANS=""
JARS=("$@"); [ ${#JARS[@]} -gt 0 ] || JARS=("$RUSTMC_MAIN"/.mc-tools/mod-corpus/{Meowtils,RavenB4}*.jar)
for jar in "${JARS[@]}"; do
  [ -f "$RUSTMC_MAIN/.mc-tools/scan/$(basename "$jar" .jar).json" ] || python3 -I "$FT_REPO/tools/modscan/modscan.py" "$jar" --out "$RUSTMC_MAIN/.mc-tools/scan/$(basename "$jar" .jar).json"
  SCANS="$SCANS $RUSTMC_MAIN/.mc-tools/scan/$(basename "$jar" .jar).json"
done
export RUSTMC_STUBS_JAR="$FT_BUILD/stubs-mixin.jar"
STUB_MIXIN_SCANS="$SCANS" "$HERE/stubs.sh" "$RUSTMC_STUBS_JAR" 2>&1 | grep -E 'SKIPPED|stubbed ' || true
for jar in "${JARS[@]}"; do
  scan="$RUSTMC_MAIN/.mc-tools/scan/$(basename "$jar" .jar).json"
  [ -f "$scan" ] || python3 -I "$FT_REPO/tools/modscan/modscan.py" "$jar" --out "$scan"
  export RUSTMC_CORPUS_JAR="$jar"
  RUSTMC_MIXIN_TARGETS=$(python3 -I -c 'import json,sys; print(",".join(sorted({t for m in json.load(open(sys.argv[1]))["mixin_classes"] for t in m["targets"]})))' "$scan")
  echo "== $(basename "$jar")"
  (cd "$FT_REPO" && RUSTMC_MIXIN_TARGETS="$RUSTMC_MIXIN_TARGETS" cargo test -q -p mc-forge --test corpus_mixins -- --nocapture 2>&1 | grep -E 'MIXIN REPORT|FAILED|panicked|SIGSEGV|Exception' || true)
done
