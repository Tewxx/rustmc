#!/bin/bash
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
. "$HERE/../forgetest/env.sh"
D=$(mktemp -d "${TMPDIR:-/tmp}/rustmc-mixin-diff.XXXXXX")
trap 'rm -rf "$D"' EXIT
(cd "$D" && JAVA_TOOL_OPTIONS=-Dmixin.debug.export=true "$FT_REPO/tools/jvm-spike/run-mixin.sh" >/dev/null)
ASM="$PL/org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar"
mkdir -p "$FT_BUILD/shadow-tools"
"$FT_JAVAC" -cp "$ASM" -d "$FT_BUILD/shadow-tools" "$HERE/MethodDiff.java"
DIFF=$("$FT_JAVA" -cp "$FT_BUILD/shadow-tools:$ASM" MethodDiff "$FT_BUILD/classes/ftmixin/ft/mixintest/Target.class" "$D/.mixin.out/class/ft/mixintest/Target.class")
case "$DIFF" in *'value()I'*) ;; *) echo "missing modified value(): $DIFF" >&2; exit 1;; esac
case "$DIFF" in *'handler$'*) ;; *) echo "missing injected handler: $DIFF" >&2; exit 1;; esac
echo 'method diff detects Mixin-modified method and injected handler'
