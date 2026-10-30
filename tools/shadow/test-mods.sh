#!/bin/bash
# Runs crates/mc-forge tests (incl. the real AccountManager jar) against the stub jar. Usage: test-mods.sh [MOD_JAR]
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
export RUSTMC_STUBS_JAR="${RUSTMC_STUBS_JAR:-$FT_BUILD/stubs.jar}"
[ -f "$RUSTMC_STUBS_JAR" ] || "$HERE/stubs.sh" "$RUSTMC_STUBS_JAR"
export RUSTMC_MOD_JAR="${1:-$RUSTMC_MAIN/.mc-tools/mod-corpus/AccountManager-1.8.jar}"
# LaunchWrapper and Minecraft libs from ft_libs, plus Mixin (not part of vanilla/Forge).
export RUSTMC_FORGE_CP="$(ft_libs):$PL/org/spongepowered/mixin/0.8.7/mixin-0.8.7.jar"
export JAVA_HOME="$JDK8"
# A mixin mod compiled against the stub jar: proves Mixin applies to stubbed vanilla classes.
MIX="$FT_BUILD/stubmixin"; rm -rf "$MIX"; mkdir -p "$MIX/classes"
"$FT_JAVAC" -nowarn -proc:none -cp "$RUSTMC_STUBS_JAR:$RUSTMC_FORGE_CP" -d "$MIX/classes" "$FT_REPO"/crates/mc-forge/tests/mods/stubmixin/ft/stubmixin/*.java
cp "$FT_REPO/crates/mc-forge/tests/mods/stubmixin/mixins.ftstub.json" "$MIX/classes/"
"$JDK8/bin/jar" cfm "$MIX/stubmixin.jar" "$FT_REPO/crates/mc-forge/tests/mods/stubmixin/manifest.txt" -C "$MIX/classes" .
export RUSTMC_MIXIN_MOD_JAR="$MIX/stubmixin.jar"
cd "$FT_REPO" && cargo test -p mc-forge -- --nocapture
