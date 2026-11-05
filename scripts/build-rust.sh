#!/usr/bin/env bash
# Builds the shared Rust core as the static library SwiftPM links: target/swift/libticker_ffi.a.
# UNIVERSAL=1 builds it for Apple silicon and Intel and joins the two with lipo, to match a universal swift build.
set -euo pipefail
cd "$(dirname "$0")/.."
out=target/swift/libticker_ffi.a
mkdir -p "$(dirname "$out")"
if [[ "${UNIVERSAL:-0}" == 1 ]]; then
	targets=(aarch64-apple-darwin x86_64-apple-darwin)
	cargo build --release --locked -p ticker-ffi --target "${targets[0]}" --target "${targets[1]}"
	lipo -create -output "$out" "target/${targets[0]}/release/libticker_ffi.a" "target/${targets[1]}/release/libticker_ffi.a"
else
	cargo build --release --locked -p ticker-ffi
	cp target/release/libticker_ffi.a "$out"
fi
# SwiftPM does not track archives passed in linker flags. Recompile the tiny C shim
# so a Rust-only change always relinks every Swift product that uses it.
touch Sources/CTickerCore/bridge.c
