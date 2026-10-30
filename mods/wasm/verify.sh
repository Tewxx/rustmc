#!/bin/bash
# Build the pending components and exercise them in a no-WASI mock host; opens no window.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
for guest in accountmanager lunarforge-input; do
  for wit in rustmc accounts interactive; do
    cmp -s "mods/wasm/$guest/wit/$wit.wit" "crates/mc-mods/wit/$wit.wit" || { echo "$guest $wit WIT differs from the host" >&2; exit 1; }
  done
done
cargo test --locked --target-dir "$ROOT/mods/wasm/target" --manifest-path mods/wasm/Cargo.toml
cargo build --locked --target-dir "$ROOT/mods/wasm/target" --manifest-path mods/wasm/Cargo.toml --target wasm32-unknown-unknown --release
for pair in 'accountmanager_port accountmanager' 'lunarforge_cps_port lunarforge_cps'; do
  read -r input output <<< "$pair"
  cargo run -p mc-mods --example componentize -- "$ROOT/mods/wasm/target/wasm32-unknown-unknown/release/$input.wasm" "$ROOT/mods/wasm/target/$output.component.wasm"
done
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" cargo run --locked --manifest-path mods/wasm/harness/Cargo.toml --bin account -- "$ROOT/mods/wasm/target/accountmanager.component.wasm"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}" cargo run --locked --manifest-path mods/wasm/harness/Cargo.toml --bin cps -- "$ROOT/mods/wasm/target/lunarforge_cps.component.wasm"
