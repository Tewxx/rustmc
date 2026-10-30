#!/bin/bash
# Build the input-enabled CPS component; it requires a future input-capable host.
set -euo pipefail
cd "$(dirname "$0")/../../.."
ROOT=$PWD
for wit in rustmc accounts interactive; do
  [ -f "crates/mc-mods/wit/$wit.wit" ] && cmp -s "mods/wasm/lunarforge-input/wit/$wit.wit" "crates/mc-mods/wit/$wit.wit" || {
    echo "Host $wit WIT is missing or differs; reconcile ABI before packaging." >&2; exit 1;
  }
done
cargo build --locked --target-dir "$ROOT/mods/wasm/target" --manifest-path mods/wasm/Cargo.toml -p lunarforge-cps-port --target wasm32-unknown-unknown --release
OUT=${1:-$ROOT/mods/wasm/target/lunarforge_cps.rmod}
COMPONENT=$ROOT/mods/wasm/target/lunarforge_cps.component.wasm
cargo run -p mc-mods --example componentize -- "$ROOT/mods/wasm/target/wasm32-unknown-unknown/release/lunarforge_cps_port.wasm" "$COMPONENT"
python3 - "$ROOT/mods/wasm/lunarforge-input/mod.toml" "$COMPONENT" "$OUT" <<'PY'
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
