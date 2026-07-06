#!/bin/bash
# Skrypt budowania i testowania z maksymalną optymalizacją
set -e
cd "$(dirname "$0")"

. "$HOME/.cargo/env"

echo "=== Kompilacja release ==="
cargo build --release

echo ""
echo "=== Testy (release) ==="
cargo test --release 2>&1 | tail -10

echo ""
echo "=== Benchmark 20 gier ==="
./target/release/statki bench-fast
