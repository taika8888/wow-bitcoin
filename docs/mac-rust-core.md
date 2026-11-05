# macOS on the shared Rust core

The Mac shell owns AppKit windows, bottom-left screen placement, UserDefaults/KVO, URLSession and timers. `ticker-ffi` owns one `ticker_core::App`; its JSON boundary does not add serde requirements to the core's state-machine types. The C header documents ownership, thread confinement and error returns. All calls are serialized on the main actor. Settings still use the `btcTickerSettings` JSON string in the `dev.local.btcticker` defaults domain; test runs use the separate existing suite.

A one-second heartbeat and one-shot `nextWake` timer feed ticks. Network effects supply their URL, timeout and request ID; replies retain both HTTP status and body. External KVO edits use `SettingsChanged`, and only `SaveSettings` writes defaults. No feed, reducer, parsing, price formatting or candle-construction implementation remains in Swift. `PaintedView.swift` is unchanged.

The bridge's metadata view (`max_bars = 0`) omits bars. Swift caches the full decoded chart by candle revision, timeframe, zoom, bar budget and timezone; quote updates do not merge or JSON-encode the same candles again. The chart view also keeps its existing CPU bitmap when its geometry, data, notice, colors and settings are unchanged; backing-scale changes still invalidate through PaintedView. Tick dates come from Rust; native DateFormatter still localizes their labels, just as before. The timezone offset is taken at the exact chart start, including when bar merging crosses a DST boundary.

## Build

`scripts/build.sh`, `scripts/test.sh`, `scripts/bundle.sh` and `scripts/verify.sh` build Cargo before SwiftPM. The documented direct `swift build` / `swift test` workflow requires `scripts/build-rust.sh` first. That script runs `cargo build --release -p ticker-ffi --manifest-path windows/Cargo.toml`, then touches the tiny C shim: SwiftPM does not otherwise notice an updated archive in linker flags. This deliberately forces relinking rather than risking a stale executable after a Rust-only edit. This is the scripts fallback from the task, rather than a plugin that depends on SwiftPM output paths or Cargo downloads inside its sandbox.

Release uses LTO, one codegen unit, abort-on-panic and stripped Rust symbols. Only `ticker-ffi` selects `opt-level = "s"`; the Windows executable and core retain Rust's default release `opt-level = 3`. Windows cross-compilation is checked; no claim of Windows runtime benchmarking is made.

## Parity audit

Swift references below are at baseline **87def14**, since those implementations are removed on this branch. Rust references describe this implementation.

| Difference and concrete input | Sources | Decision |
| --- | --- | --- |
| Integral decimal/exponent settings: `{"decimals":1.0,"zoom":5.0,"refreshSeconds":2e1}` decoded as 1 / all / 20 in Swift, but as defaults in Rust. Signed overflow `{"refreshSeconds":9223372036854775808}` defaulted to 5 in Swift but clamped to 300 in Rust. | Swift `Sources/TickerCore/Settings.swift:62` (integer reads at 73, 77, 81); Rust `windows/crates/ticker-core/src/settings.rs:173` (`integer`). | Shared decoder now accepts integral numeric forms and rejects signed overflow; Swift bridge and Rust regression tests plus three caught mutations. Normal previously saved JSON also round-trips field-for-field, including colors, visibility, negative x and bottom-left y. |
| Mac price/time ticks were absent from core. A 0...10 price domain gives `[0,2,4,6,8,10]`; 0...600 seconds gives `[0,120,240,360,480,600]`. Windows's old time algorithm gives `[0,300,600]`. | Swift `Sources/TickerCore/Chart.swift:175`; Rust `windows/crates/ticker-core/src/chart.rs:163` and `:173` (`price_ticks` / `time_ticks`); Windows `windows/crates/btc-ticker/src/render.rs:501` and `:515`. | Ported Mac rules and tests to core, with five caught mutations. Left Windows axes unchanged: its renderer has no label-collision filtering and expects at most five time labels; replacing them with up to eight would require a separate Windows visual change. |
| Zero bar budget: a nonempty candle series with `maxBars: 0` divides by zero in Swift; Rust coerces to one bar. | Swift `Sources/TickerCore/Chart.swift:140`; Rust `windows/crates/ticker-core/src/chart.rs:200` (`max_bars.max(1)`). | Keep Rust's safe behavior. The Mac layout always supplies at least one; the bridge separately defines zero as a metadata-only read. |
| Malformed Kraken booleans: `{"result":{"XXBTZUSD":{"c":[true],"o":true}}}` becomes a $1 quote through Swift NSNumber; Rust rejects it. A candle timestamp `true` is similarly accepted as one second in Swift and rejected by Rust. | Swift `Sources/TickerCore/Kraken.swift:39` and `:70`; Rust `windows/crates/ticker-core/src/kraken.rs:94` and `:136`. | Retain Rust's stricter parser. These are outside Kraken's documented numeric wire format; legitimate fixtures and live responses retain the same behavior. |
| External settings edits were normalized and written back by Swift's `send(.settingsEdited)` whenever they changed settings. For example, editing `{"locked":true}` caused a full JSON rewrite. | Swift `Sources/BTCTicker/AppModel.swift:21` and `:27`; Rust `windows/crates/ticker-core/src/app.rs:138` (`edited_in_app`). | Use the specified `SettingsChanged` input, so external edits apply without an echo write. In-app edits still persist. Tested at both sides of the bridge. |

Core state-machine scheduling and reducer behavior are unchanged; the only App API additions expose request IDs for the C boundary. Finite real-world tick domains preserve the Mac algorithm. The new tick helper rejects non-finite/zero spans and bounds pathological remote time spans to 1,024 ticks, instead of allowing an unbounded allocation.

## Validation and measurements

All non-GUI gates passed on 2026-11-05:

- `cargo test -p ticker-core -p ticker-ffi`: **104 tests** (94 core, 5 model checks, 5 FFI); no failures.
- Model coverage: overlay 3,456 states / 89,856 transitions, exhaustive; app 163,494 states / 1,047,044 transitions at depth 7; feed 291,029 states / 1,322,762 transitions at depth 8. Existing 18 overlay and 14 app invariants remain intact.
- `scripts/mutants-mac-core.py`: **9/9 deliberately broken properties caught**, source restored afterward.
- `cargo clippy -p ticker-core -p ticker-ffi`: clean with warnings denied.
- `cargo check --target x86_64-pc-windows-msvc -p btc-ticker`: passed in both development and release profiles.
- `swift build -c release -Xswiftc -warnings-as-errors`: passed; `swift test`: **25 tests passed**, including three fixture/timeframe bridge cases.
- `scripts/bundle.sh`, `plutil -lint`, and `codesign --verify --strict`: passed.

The baseline bundle was built from main **87def14** in the explicitly requested `/tmp/btc-main` worktree, copied to `/tmp/btc-main-app/BTC Ticker.app`, then that temporary worktree was removed. No other worktree or branch was modified. Executable bytes from `ls -l`: **843,088 main**, **1,818,784 branch**; growth **975,696 bytes** (0.930 MiB).

### GUI checks pending an unlocked desktop

`ioreg -n Root -d1 -a` reported `CGSSessionScreenIsLocked = true`. As instructed, the earlier lock-screen `verify.sh` run is **invalid, neither a pass nor a fail**. The final GUI gate was skipped, and no desktop was switched. Every GUI check below needs an unlocked rerun of unchanged `scripts/e2e.swift` through `scripts/verify.sh`:

- `e2e-live-quote`, `e2e-refresh-restarts`, `e2e-main-menu`, `e2e-window-on-screen`, `e2e-above-wow`.
- `e2e-settings-live`, `e2e-only-when-wow`, `e2e-clamped`, `e2e-expand`.
- `e2e-capture-4h`, `e2e-timeframe-15m`, `e2e-capture-15m`, `e2e-timeframe-1m`, `e2e-capture-1m`, `e2e-capture-1m-all`.
- `e2e-zoom-wheel`, `e2e-zoom-trackpad`, `e2e-collapse`, `e2e-reexpand`.
- `e2e-opaque-to-clicks`, `e2e-ticker-anchored`, `e2e-click`, `e2e-focus-kept`, `e2e-undisturbed`, `e2e-quit`.

`e2e-above-wow` may remain inconclusive without an on-screen WoW window under the overlay, the one allowed inconclusive result. Unlocked compositing/footprint remains unverified. Before each GUI or footprint run, only the installed `~/Applications/BTC Ticker.app` process was present; no competing test ticker was present.

### Footprint: screen locked

The unchanged notification-driven `scripts/footprint.swift` was run with main first and this branch second, `--runs 3`, while the screen was locked. These are not claims about unlocked compositing. Initial measurements found idle CPU 0.31% main versus 0.38% branch; idle footprint 31.8 versus 32.3 MB, peak 39.0 versus 39.0 MB. Skipping chart-only repaints alone left CPU at 0.36% versus 0.30% and noisy idle footprints of 33.4 versus 31.0 MB. Sampling then identified avoidable heartbeat NSDictionary serialization and attempted error decoding on every successful reply. The final implementation serializes the two clock fields directly, decodes successful replies first, and avoids redoing unchanged ticker text, layout and window frames as well as chart bitmaps. Snapshots still include each fetchedAt update.

| Phase / metric | Main median (min–max) | Rust median (min–max) |
| --- | ---: | ---: |
| collapsed (MB) | 11.5 (11.5–11.8) | 11.6 (11.6–11.6) |
| 15m (MB) | 20.5 (18.8–21.0) | 19.9 (19.9–22.0) |
| 1m (MB) | 23.0 (21.7–24.7) | 24.5 (21.7–26.8) |
| 4h (MB) | 27.0 (25.2–31.4) | 27.0 (24.5–27.6) |
| 4h-all (MB) | 29.3 (29.1–35.3) | 26.3 (25.9–31.9) |
| idle-open (MB) | 31.0 (30.4–39.0) | 30.9 (29.4–34.7) |
| collapsed-again (MB) | 26.7 (26.3–34.7) | 26.6 (23.8–30.4) |
| idleCPU (%) | 0.29 (0.28–0.41) | 0.29 (0.29–0.31) |
| peak (MB) | 37.6 (37.6–45.7) | 34.6 (34.1–38.6) |

All six final launches completed with zero measurement errors. Final medians meet the requested idle memory, peak memory and CPU goals under the explicitly labeled locked-screen conditions. Normal unlocked-screen measurements and the 25 GUI checks above remain for the parent to rerun. The final bundled executable is byte-for-byte identical to the executable measured in those runs (`cmp` passed). Raw local logs: `build/footprint-locked-optimized.log`, `build/rust-final.log`, `build/mutants-run.log`, `build/swift-test-final.log`, and `build/bundle-final.log`.
