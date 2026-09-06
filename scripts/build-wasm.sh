#!/usr/bin/env bash
# Build the WebAssembly engine and place it in web/ for the GitHub Pages
# app. Usage: scripts/build-wasm.sh [--minify]
#
# Requirements: rustup with the wasm32-unknown-unknown target.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=wasm32-unknown-unknown
OUT=web/engine.wasm

if ! rustup target list --installed | grep -q "$TARGET"; then
    echo ">>> Installing $TARGET target"
    rustup target add "$TARGET"
fi

echo ">>> Building sonar-wasm (release)"
cargo build --release --target "$TARGET" -p sonar-wasm

SRC="target/$TARGET/release/sonar_wasm.wasm"
cp "$SRC" "$OUT"
echo ">>> Copied $SRC -> $OUT ($(du -h "$OUT" | cut -f1))"

if [[ "${1:-}" == "--minify" ]] && command -v wasm-opt >/dev/null 2>&1; then
    echo ">>> Running wasm-opt"
    wasm-opt -O3 --strip-debug "$OUT" -o "$OUT.opt" && mv "$OUT.opt" "$OUT"
    echo ">>> Optimised size: $(du -h "$OUT" | cut -f1)"
fi

echo ">>> Done. Serve web/ statically, e.g.:"
echo "    cd web && python3 -m http.server 8123"
