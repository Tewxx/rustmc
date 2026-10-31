#!/bin/bash
# Launch rustmc with the Forge mods in tests/forge/build/game/mods-forge, signed in as the active Prism account.
# The session goes to the client on stdin (its Prism wrapper mode), never argv or disk.
#
# Usage: tools/forgetest/play-mods.sh [options]
#   -s HOST[:PORT]   server (default test.ccbluex.net)
#   -a NAME          Prism account by profile name (default: the active one)
#   -b SECONDS       run the uncapped benchmark for SECONDS, then exit
#   -n               no mods (baseline: do not start the Forge runtime)
#   -t               print per-frame Rust<->Java boundary timings (RUSTMC_FORGE_TIMING)
#   -T               print every stub call (RUSTMC_FORGE_TRACE; slow, huge log)
#   -r               rebuild the release client and the stub jar first
set -euo pipefail
. "$(dirname "$0")/env.sh"
SERVER=test.ccbluex.net ACCOUNT="" BENCH="" NOMODS="" TIMING="" TRACE="" REBUILD=""
while getopts "s:a:b:ntTr" o; do
  case $o in
    s) SERVER=$OPTARG ;; a) ACCOUNT=$OPTARG ;; b) BENCH=$OPTARG ;;
    n) NOMODS=1 ;; t) TIMING=1 ;; T) TRACE=1 ;; r) REBUILD=1 ;;
    *) sed -n '2,13p' "$0"; exit 2 ;;
  esac
done
GAME="$FT_BUILD/game"
STUBS="$FT_BUILD/stubs-meowtils.jar"
BIN="$FT_REPO/target/release/rustmc"
if [ -n "$REBUILD" ] || [ ! -x "$BIN" ]; then (cd "$FT_REPO" && cargo build --release --offline --bin rustmc); fi
if [ -n "$REBUILD" ] || [ ! -f "$STUBS" ]; then
  STUB_MIXIN_SCANS="$RUSTMC_MAIN/.mc-tools/scan/Meowtils-2.0.1.json" "$FT_REPO/tools/shadow/stubs.sh" "$STUBS"
fi
mkdir -p "$GAME/mods-forge" "$GAME/tmp"
# Guava 21 (Mixin 0.8 in the corpus mods needs it) replaces the 17.0 vanilla ships.
ft_libs | sed 's#/17.0/guava-17.0#/21.0/guava-21.0#' > "$GAME/forge_cp.txt"
if ! ls "$GAME"/mods-forge/*.jar >/dev/null 2>&1; then
  cp "$RUSTMC_MAIN"/.mc-tools/mod-corpus/{Meowtils-2.0.1,lunarforge}.jar "$GAME/mods-forge/"
fi
export FT_GAME="$GAME" FT_SERVER="$SERVER" FT_ACCOUNT="$ACCOUNT" FT_BENCH="$BENCH" FT_NOMODS="$NOMODS" \
  FT_TIMING="$TIMING" FT_TRACE="$TRACE" FT_BIN="$BIN" FT_STUBS="$STUBS" FT_JAR="$MCP/jars/versions/1.8.9/1.8.9.jar" \
  FT_PRISM="$PRISM" FT_JDK8="$JDK8"
exec python3 -I - <<'PY'
import json, os, subprocess, sys
e = os.environ
accts = json.load(open(e["FT_PRISM"] + "/accounts.json"))["accounts"]
pick = [a for a in accts if (a["profile"]["name"] == e["FT_ACCOUNT"] if e["FT_ACCOUNT"] else a.get("active"))]
if not pick: sys.exit("no matching Prism account (is one signed in and active?)")
a = pick[0]
host, _, port = e["FT_SERVER"].partition(":")
script = "".join(f"param {p}\n" for p in [
    "--username", a["profile"]["name"], "--uuid", a["profile"]["id"], "--accessToken", a["ygg"]["token"],
    "--userType", "msa", "--version", "1.8.9", "--gameDir", e["FT_GAME"],
    "--assetsDir", e["FT_PRISM"] + "/assets", "--assetIndex", "1.8"]) + f"serverAddress {host}\nserverPort {port or 25565}\nlaunch\n"
env = dict(os.environ, JAVA_HOME=e["FT_JDK8"], INST_MC_DIR=e["FT_GAME"],
    RUSTMC_FORGE_STUBS=e["FT_STUBS"], RUSTMC_FORGE_CP=open(e["FT_GAME"] + "/forge_cp.txt").read().strip(),
    RUSTMC_JVM_OPTS="-Djava.io.tmpdir=" + e["FT_GAME"] + "/tmp")
if e["FT_NOMODS"]: env.pop("RUSTMC_FORGE_STUBS")
if e["FT_BENCH"]: env.update(RUSTMC_BENCH=e["FT_BENCH"], RUSTMC_PRESENT_MODE="immediate", RUSTMC_FRAME_LATENCY="1")
if e["FT_TIMING"]: env["RUSTMC_FORGE_TIMING"] = "1"
if e["FT_TRACE"]: env["RUSTMC_FORGE_TRACE"] = "1"
print(f"rustmc as {a['profile']['name']} -> {e['FT_SERVER']}" + (" (no mods)" if e["FT_NOMODS"] else ""), file=sys.stderr)
p = subprocess.Popen([e["FT_BIN"], "-cp", e["FT_JAR"], "org.prismlauncher.EntryPoint"], stdin=subprocess.PIPE, env=env)
p.stdin.write(script.encode()); p.stdin.close()
sys.exit(p.wait())
PY
