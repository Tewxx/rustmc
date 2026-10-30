#!/bin/bash
# Build a standalone native component; does not install or enable the mod.
set -euo pipefail
cd "$(dirname "$0")/../../.."
ROOT=$PWD
if [ ! -f crates/mc-mods/wit/rustmc.wit ]; then echo 'The mc-mods host/SDK is required to componentize this guest.' >&2; exit 1; fi
cmp -s mods/wasm/lunarforge/wit/rustmc.wit crates/mc-mods/wit/rustmc.wit || { echo 'Guest WIT differs from the host; reconcile the ABI before building.' >&2; exit 1; }
cargo build --locked --target-dir "$ROOT/mods/wasm/target" --manifest-path mods/wasm/Cargo.toml -p lunarforge-port --target wasm32-unknown-unknown --release
OUT=${1:-$ROOT/mods/wasm/target/lunarforge_fps.rmod}
COMPONENT=$ROOT/mods/wasm/target/lunarforge_fps.component.wasm
cargo run -p mc-mods --example componentize -- "$ROOT/mods/wasm/target/wasm32-unknown-unknown/release/lunarforge_port.wasm" "$COMPONENT"
python3 - "$ROOT/mods/wasm/lunarforge/mod.toml" "$COMPONENT" "$OUT" <<'PY'
import pathlib, sys, zipfile
manifest, component, output = map(pathlib.Path, sys.argv[1:])
output.parent.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(output, 'w') as z:
    for name, path in [('mod.toml', manifest), ('mod.wasm', component)]:
        info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
        info.external_attr = 0o100644 << 16
        z.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
print(output)
PY
