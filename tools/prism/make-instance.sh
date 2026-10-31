#!/usr/bin/env bash
# Writes rustmc-prism-instance.zip (instance.cfg + mmc-pack.json) with the wrapper path of this machine.
# Usage: tools/prism/make-instance.sh [path/to/rustmc]   (default: target/release/rustmc next to the repo)
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
bin="${1:-$root/target/release/rustmc}"
[ -x "$bin" ] || { echo "no rustmc binary at $bin (cargo build --release -p mc-client)" >&2; exit 1; }
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"
tmp="$(mktemp -d)"
cat > "$tmp/mmc-pack.json" <<'EOF'
{
    "components": [
        {"cachedName": "LWJGL 2", "cachedVersion": "2.9.4-nightly-20150209", "dependencyOnly": true, "uid": "org.lwjgl", "version": "2.9.4-nightly-20150209"},
        {"cachedName": "Minecraft", "cachedRequires": [{"suggests": "2.9.4-nightly-20150209", "uid": "org.lwjgl"}], "cachedVersion": "1.8.9", "important": true, "uid": "net.minecraft", "version": "1.8.9"}
    ],
    "formatVersion": 1
}
EOF
# Quote the path so spaces survive Prism's wrapper-command splitting.
printf '[General]\nInstanceType=OneSix\nname=rustmc\nOverrideCommands=true\nWrapperCommand="%s"\n' "$bin" > "$tmp/instance.cfg"
out="$PWD/rustmc-prism-instance.zip"
rm -f "$out"
(cd "$tmp" && zip -q "$out" instance.cfg mmc-pack.json)
rm -rf "$tmp"
echo "wrote $out"
