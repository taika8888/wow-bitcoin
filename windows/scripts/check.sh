#!/usr/bin/env bash
# Everything that can fail on the Mac fails here, before a billed Windows minute.
# One PASS/FAIL line per step; exit 0 only when every line is PASS.
set -uo pipefail
cd "$(dirname "$0")/.."
failed=0
win=x86_64-pc-windows-msvc

step() {
	local name=$1
	shift
	local out
	if out=$("$@" 2>&1); then
		printf 'PASS  %s\n' "$name"
	else
		printf 'FAIL  %s\n' "$name"
		tail -n 30 <<<"$out" | sed 's/^/    | /'
		failed=1
	fi
}

if ! rustup target list --installed | grep -qx "$win"; then
	echo "FAIL  target $win is not installed (rustup target add $win)"
	exit 1
fi

step fmt cargo fmt --all --check
step clippy-host cargo clippy --workspace --all-targets --locked -- -D warnings
step clippy-windows cargo clippy --workspace --all-targets --locked --target "$win" -- -D warnings
step test cargo test --workspace --locked
exit $failed
