#!/usr/bin/env python3
"""Break each new Mac parity property once; restore source even when a check fails."""
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parent.parent
crate = root / "windows/crates/ticker-core"
logs = root / "build/mutants-mac-core"
logs.mkdir(parents=True, exist_ok=True)
mutants = [
    ("price-step", "chart.rs", "&[magnitude, 2.0 * magnitude, 5.0 * magnitude]", "&[magnitude, 2.5 * magnitude, 5.0 * magnitude]", "axes_have_round_ticks_inside_the_domain_at_every_zoom"),
    ("tick-density", "chart.rs", "pub const MINIMUM_TICKS: f64 = 3.0;", "pub const MINIMUM_TICKS: f64 = 1.0;", "axes_have_round_ticks_inside_the_domain_at_every_zoom"),
    ("tick-domain", "chart.rs", "((low + offset) / step).ceil()", "((low + offset) / step).floor()", "axes_have_round_ticks_inside_the_domain_at_every_zoom"),
    ("local-time", "chart.rs", "f64::from(utc_offset),", "f64::from(utc_offset) * 0.0,", "axes_have_round_ticks_inside_the_domain_at_every_zoom"),
    ("tick-boundary", "chart.rs", "(0..count).map", "(1..count).map", "ticks_preserve_the_mac_step_and_boundary_rules"),
    ("tick-allocation", "chart.rs", ".clamp(0.0, 1024.0)", ".clamp(0.0, 2048.0)", "pathological_remote_domains_cannot_allocate_unbounded_ticks"),
    ("integer-underflow", "settings.rs", "number > i64::MIN as f64", "number >= i64::MIN as f64", "integer_settings_match_existing_swift_json_blobs"),
    ("integral-json", "settings.rs", "let number = value.as_f64()?;", "let number = value.as_i64()? as f64;", "integer_settings_match_existing_swift_json_blobs"),
    ("integer-overflow", "settings.rs", "number < -(i64::MIN as f64)", "number <= -(i64::MIN as f64)", "integer_settings_match_existing_swift_json_blobs"),
]
failed = []
for name, file, before, after, check in mutants:
    path = crate / "src" / file
    original = path.read_text()
    if original.count(before) != 1:
        raise SystemExit(f"stale mutant {name}: {before}")
    try:
        path.write_text(original.replace(before, after))
        result = subprocess.run(["cargo", "test", "-p", "ticker-core", "--test", "core", check], cwd=root / "windows", capture_output=True, text=True)
        output = result.stdout + result.stderr
        (logs / f"{name}.log").write_text(output)
        caught = result.returncode != 0 and f"{check} ... FAILED" in output
        print(f"{'PASS' if caught else 'FAIL'} mutant {name}: {check}", flush=True)
        if not caught:
            failed.append(name)
    finally:
        path.write_text(original)
if failed:
    raise SystemExit(f"uncaught or invalid mutants: {failed}")
