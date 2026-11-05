#!/usr/bin/env bash
# SwiftPM links the host's Rust archive. Keep Cargo's incremental build in front of SwiftPM.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/build-rust.sh
swift build "$@"
