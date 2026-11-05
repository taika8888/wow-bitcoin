#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p ticker-ffi --manifest-path windows/Cargo.toml
# SwiftPM does not track archives passed in linker flags. Recompile the tiny C shim
# so a Rust-only change always relinks every Swift product that uses it.
touch Sources/CTickerCore/bridge.c
