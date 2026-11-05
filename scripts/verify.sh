#!/usr/bin/env bash
# Exit 0 only when every check prints PASS. INCONCLUSIVE counts as failure.
set -uo pipefail
cd "$(dirname "$0")/.."

logs=build/verify
mkdir -p "$logs"
app="build/BTC Ticker.app"
failed=0

report() {
	printf '%-12s %s%s\n' "$1" "$2" "${3:+  ($3)}"
	[ "$1" = PASS ] || failed=1
}

show_log() {
	tail -n 25 "$1" | sed 's/^/    | /'
}

if swift build -c release -Xswiftc -warnings-as-errors >"$logs/build.log" 2>&1; then
	report PASS build "release, warnings as errors"
else
	report FAIL build "$logs/build.log"
	show_log "$logs/build.log"
fi

if swift test >"$logs/test.log" 2>&1; then
	report PASS test "$(grep -Eo 'Test run with [0-9]+ tests? passed' "$logs/test.log" | tail -1)"
else
	report FAIL test "$logs/test.log"
	grep -E '✘|error:' "$logs/test.log" | head -n 25 | sed 's/^/    | /'
fi

bundle_ok=0
if scripts/bundle.sh >"$logs/bundle.log" 2>&1 \
	&& plutil -lint "$app/Contents/Info.plist" >>"$logs/bundle.log" 2>&1 \
	&& codesign --verify --strict --verbose=2 "$app" >>"$logs/bundle.log" 2>&1; then
	bundle_ok=1
	report PASS bundle "$app, plutil -lint and codesign --verify"
else
	report FAIL bundle "$logs/bundle.log"
	show_log "$logs/bundle.log"
fi

if [ "$bundle_ok" = 1 ]; then
	pidfile="$logs/e2e.pid"
	rm -f "$pidfile"
	swift scripts/e2e.swift "$app" "$pidfile" >"$logs/e2e.log" 2>&1
	e2e_status=$?
	if [ -s "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
		kill "$(cat "$pidfile")" 2>/dev/null
		report FAIL e2e-cleanup "app pid $(cat "$pidfile") was still running after e2e, killed it"
	fi
	results=$(grep -E '^(PASS|FAIL|INCONCLUSIVE) ' "$logs/e2e.log")
	if [ -z "$results" ]; then
		report FAIL e2e "no results, exit $e2e_status, $logs/e2e.log"
		show_log "$logs/e2e.log"
	else
		printf '%s\n' "$results"
		if grep -qvE '^PASS ' <<<"$results"; then
			failed=1
		elif [ "$e2e_status" != 0 ]; then
			report FAIL e2e "every line passed but the script exited $e2e_status, $logs/e2e.log"
		fi
	fi
else
	report INCONCLUSIVE e2e "no app bundle to launch"
fi

exit "$failed"
