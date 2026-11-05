#!/usr/bin/env python3
"""Break each new Mac parity property once; restore source even when a check fails."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
crate = root / "crates/ticker-core"
logs = root / "build/mutants-mac-core"
logs.mkdir(parents=True, exist_ok=True)
mutants = [
    ("tick-allocation", "axis.rs", "const MAXIMUM_TICKS: u32 = 1024;", "const MAXIMUM_TICKS: u32 = 2048;", "a_malformed_domain_gets_a_bounded_number_of_ticks"),
    ("tick-infinite-span", "axis.rs", "if span <= 0.0 || !span.is_finite() {", "if span <= 0.0 {", "a_malformed_domain_gets_a_bounded_number_of_ticks"),
    ("integer-underflow", "settings.rs", "number > i64::MIN as f64", "number >= i64::MIN as f64", "integer_settings_match_existing_swift_json_blobs"),
    ("integral-json", "settings.rs", "let number = value.as_f64()?;", "let number = value.as_i64()? as f64;", "integer_settings_match_existing_swift_json_blobs"),
    ("integer-overflow", "settings.rs", "number < -(i64::MIN as f64)", "number <= -(i64::MIN as f64)", "integer_settings_match_existing_swift_json_blobs"),
]
failed = []
for name, file, before, after, check in mutants:
    path = crate / "src" / file
    # Bytes, so a restore never rewrites line endings.
    original = path.read_bytes()
    if original.count(before.encode()) != 1:
        raise SystemExit(f"stale mutant {name}: {before}")
    try:
        path.write_bytes(original.replace(before.encode(), after.encode()))
        result = subprocess.run(["cargo", "test", "-p", "ticker-core", "--test", "core", check], cwd=root, capture_output=True, text=True)
        output = result.stdout + result.stderr
        (logs / f"{name}.log").write_text(output)
        caught = result.returncode != 0 and f"{check} ... FAILED" in output
        print(f"{'PASS' if caught else 'FAIL'} mutant {name}: {check}", flush=True)
        if not caught:
            failed.append(name)
    finally:
        path.write_bytes(original)
if failed:
    raise SystemExit(f"uncaught or invalid mutants: {failed}")
