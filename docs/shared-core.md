# One Rust core for the Mac and Windows apps

BTC Ticker floats a live Bitcoin price over World of Warcraft without taking focus, and a click opens a candlestick chart. The macOS app is Swift and the Windows app is Rust. This plan makes the Rust crate `ticker-core` the only core for both, so a behavior is written once, tested once and model-checked once. This page records the decision, the shape of the boundary, the migration and what the owner accepts in exchange.

Two design runners wrote competing proofs. Design A is a hand-written JSON C ABI, on branch `t3/design-shared-rust-core` under `design/shared-core-A/`. Design B is UniFFI-generated Swift, on branch `t3/uniffi-shared-core-design` under `design/shared-core-B/`. Neither branch is on `origin`. Paths below such as `crates/ticker-core` are the ones after migration PR 1. Today they sit under `windows/crates/`.

## Two cores drift apart

The logic exists twice. On the Mac it is Swift `Sources/TickerCore` plus most of `AppModel.swift`. On Windows it is `windows/crates/ticker-core` plus chart rules that live in the Win32 shell's `render.rs`. Every fix lands in one copy and has to be ported by hand to the other, and the ports lag.

The two inventories, read at `87def14`, found these gaps among others:

- The same chart gets different gridlines. Swift picks steps of 1, 2 or 5 times a power of ten. Windows picks 1, 2, 2.5, 5 or 10.
- The Mac saves the ticker's `position` in bottom-left AppKit points. Windows saves top-left pixels under the same JSON key, so neither app can read the other's settings.
- Mac PR #5 fixed overlapping time labels in Swift. Windows still drew them on top of each other until PR #11 ported the fix.

The full list is in [Behavior differences today](#behavior-differences-today).

## The decision is design A, with two parts of B

The plan adopts A's interface and A's layout:

- Seven `extern "C"` functions, JSON messages and no callbacks. The shell reads a view, measures text, then asks for a frame.
- A Cargo workspace at the repo root, with `crates/ticker-core` and `crates/ticker-ffi` shared, Win32 under `windows/`, and Swift where it is today.

A wins on three counts:

1. Rust never calls into Swift, so the core can never be entered again while it is mid-render. In B, Rust calls a Swift `TextMeasure` during `scene`, and that callback is B's hardest seam.
2. A's `ticker-ffi` depends only on `serde` and `serde_json`, which `ticker-core` already uses. B's `ticker-ffi` build graph has 50 crates against the core's 12 (measured). B also needs a `uniffi-bindgen` tool that took 144 to 156 s to build (measured), and it checks in 3,391 lines of generated Swift.
3. A JSON message can be logged on a user's Mac and replayed in a Rust test.

B's real advantage is that Swift and Rust agree on types at compile time. A gets most of that back in two ways. Golden message files under `crates/ticker-ffi/tests/messages/` are checked by both the Rust and the Swift tests. `btc_abi_version` is checked at launch and bumped on any change to a message's shape.

Two ideas come from B:

- Drift and golden checks run on cheap Linux or Windows runners wherever possible. The macOS job only builds, tests and bundles. GitHub bills a private repo's macOS minute at 10 times a Linux minute.
- A second entry into a core fails loudly. A's thread check already does this.

There is no macOS bakeoff between A and B. A clean compile of both would not change the pick, because the reasons above do not depend on whether B's Swift compiles.

## The shape of the shared core

### Crates

| Path | What it holds |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | The workspace, moved from `windows/` |
| `crates/ticker-core/` | The pure core. No FFI and no I/O. |
| `crates/ticker-ffi/` | The C ABI for the Mac: `src/`, `include/ticker_core.h`, golden messages in `tests/` |
| `windows/btc-ticker/`, `windows/e2e/` | The Win32 shell and its end-to-end check |
| `windows/scripts/` | `verify.ps1` and `mutants.ps1` |
| `Package.swift`, `Sources/BTCTicker/` | The Mac shell, where it is today |
| `Sources/CTickerCore/module.modulemap` | Points SwiftPM at the header and links `ticker_ffi` |

SwiftPM looks only under `Sources/` and `Tests/`, and Cargo looks only at its members, so the two builds share the root without seeing each other. The Win32 shell keeps calling `ticker-core` as plain Rust and never touches the ABI or JSON.

### The seven functions

| Function | What it does |
| --- | --- |
| `btc_abi_version()` | Returns the ABI version. Swift compares it with its own at launch. |
| `btc_core_new(config)` | Builds a core from `{settings, frontmost, now}`. Returns NULL on a malformed config. |
| `btc_core_handle(core, message, body)` | Applies one input and returns `{effects, nextWake}`. A fetch's HTTP body travels beside the JSON as raw bytes. |
| `btc_core_view(core, request)` | Reads state back: ticker runs, settings, settings limits, the expanded chart's state. |
| `btc_core_frame(core, request)` | Returns the render view-model for the text sizes the shell measured. Added in PR 5. |
| `btc_buffer_free(buffer)` | Frees a reply. The caller frees each one exactly once. |
| `btc_core_free(core)` | Drops the core. Accepts NULL. |

Every reply is `{"ok": ...}` or `{"error": "..."}`. An error never changes state. The effects are `saveSettings`, `fetchQuote`, `fetchCandles` and `render`. The shell runs them in order, then arms one timer for `nextWake`.

### The frame view-model

The shell measures text, because only it has the fonts. The core makes every other decision, and the shell paints what it is given.

```rust
pub struct FrameRequest { screens, scale, pixel_scale, ticker_size, segment_size, axis, utc_offset, hour12, window_origin }

pub struct Frame {
	pub visible: bool,
	pub window: Rect,
	pub background: Fill,
	pub ticker: TextBlock,
	pub expanded: Option<ExpandedFrame>,   // timeframe tabs and the chart
	pub targets: BTreeMap<String, Rect>,   // hit testing and the e2e snapshot
	pub max_bars: usize,
}

pub enum ChartContent {
	Placeholder(Label),
	Plot { plot, grid, price_labels, time_labels, candles, last_price, stale },
}
```

All geometry is top-left with y growing down, in the shell's layout units. The core estimates a time label's width as its character count times `axis.digit_width`. That overestimates, so overlap skipping drops a label rather than letting two touch, and it saves a third round trip per frame. `Frame::released` hit-tests a pointer release against the last frame and pulls a drop into the work area.

### Threads and memory

Both shells call the core on their UI thread only. `BtcCore` records the thread that created it. A `handle`, `view` or `frame` call from any other thread gets an error and changes nothing. `btc_core_free` may run on any thread, because a Swift 6 `deinit` is nonisolated. On the Mac, `Core` and `Shell` are `@MainActor`, and URLSession results hop back to the main actor before they reach the core.

The caller owns every pointer it passes, and Rust copies what it keeps. Every reply is a buffer that Rust allocated and the caller hands back to `btc_buffer_free`. Each call runs inside `catch_unwind`. A panic returns an error reply and poisons the core, so every later call fails and Swift crashes with the message. The release profile must keep `panic = "unwind"`.

## Seven PRs, each with its own check

The Windows-verifiable PRs come first, so the shared core is complete and proven before any Swift changes. PR 6 and PR 7 are split so a linking problem and a behavior problem never share one macOS round trip.

| # | PR | Check | Status |
| --- | --- | --- | --- |
| 1 | Move the workspace to the root (branch `move-rust-workspace`). `git mv` plus path fixes in `verify.ps1`, `mutants.ps1`, the README, `.gitignore`, the `verify-ticker-model` skill, the fixture paths the Rust tests load with `include_str!`, and `.github/workflows/windows.yml`. The workflow's path filters must add `crates/**` and the root `Cargo.*` files, or a core change would skip the Windows gate, and its artifact paths move from `windows/target` to `target`. No code changes. | `cargo test --workspace`, `cargo clippy --workspace --all-targets`, and the `verify.ps1` job on `windows-latest` from PR #8. | In progress |
| 2 | macOS CI baseline (branch `ci-macos-baseline`). A macOS job that builds, tests and bundles today's Swift app unchanged. The `windows-latest` job from PR #8 already runs the build, clippy, the core tests and the model checks on every core change. Path filters keep the macOS job off Windows-only PRs. | Both jobs green. This records the macOS minutes before anything changes. | In progress |
| 3 | Chart axes in the core. Port Swift `priceTicks`, `timeTicks` and their `ChartTests`, plus an `hour12` time format, into `ticker-core`. `render.rs` drops its own `price_ticks` and `time_ticks`. Label skipping (`axis.rs` `spaced_labels`) and the fade shares (`ink.rs` `ChartInk`) already landed in PR #11. | `cargo test` with the ported Swift cases. The parity PNGs from PR #11's render test show Windows' new gridlines. | Not started |
| 4 | The frame view-model. Add `ticker_core::frame`, zoom travel as `{steps, restart}` and `Frontmost::classify_bundle`. Move time-label placement into the frame, with one rule for both apps. Fix `next_wake` first: `FeedStatus::derive` turns stale only once the age is past the threshold, but `next_wake` schedules exactly the threshold and then drops the deadline, so a shell with no heartbeat never sees the stale transition. Windows hides this with its 1 s heartbeat, and the Mac shell in PR 7 runs on the one timer. The Win32 `overlay.rs` and `render.rs` paint from `Frame`. Golden tests pin `Frame` values for recorded candles. | `cargo test`, including the model checks over the new zoom travel state and a test that a tick at exactly `next_wake` turns the status stale. The `verify.ps1` e2e, whose snapshot schema does not change. | Not started |
| 5 | `ticker-ffi`. The proof crate plus `btc_core_frame`, the new `released` and `zoom` inputs, `limits` in the view, and golden message files a scripted session must reproduce byte for byte. A C smoke test runs through the header. | `cargo test -p ticker-ffi`, and the C smoke with MSVC here, gcc on Linux and Apple clang on macOS CI. | Not started |
| 6 | Swift links the core, with no behavior change. Add `Sources/CTickerCore`, `scripts/build-core.sh`, the linker settings, `Core.swift`, the `Wire.swift` mirrors, Swift tests that decode every golden message, and a Swift test that drives a scripted session through `Core.swift` and the real ABI, checking the requests Swift encodes, the replies it decodes and that every buffer is freed. The app still runs on Swift `TickerCore`. | The macOS CI job. This is the first real compile of the Swift half, and the first time Swift's own encoding meets the core. | Not started |
| 7 | The Mac app switches to the core. `Shell.swift` replaces `AppModel`. `Overlay.swift` and `ChartView.swift` paint from `Frame`. The saved position is converted once. Move the recorded Kraken fixtures from `Tests/TickerCoreTests/Fixtures` to a shared `Tests/fixtures` and point the Rust, C and Swift tests at it, then delete `Sources/TickerCore` and `Tests/TickerCoreTests`. | The macOS CI job, then `scripts/verify.sh` (build, bundle, e2e) on the owner's Mac. This is the only PR that needs the owner's Mac. | Not started |

## Mac behavior changes the owner accepts

PR 3 and PR 4 ship the shared rules on Windows first, where the parity PNGs and the e2e show them. PR 7 brings them to the Mac all at once.

| Change | Default the plan takes | Alternative |
| --- | --- | --- |
| Saved position | PR 7 converts the bottom-left `position` once to top-left, as `y' = primaryHeight - y - tickerHeight` after the first measurement, and sets a migrated flag in UserDefaults. If the conversion is wrong, `placement::ticker_frame` clamps the ticker into a work area, so the worst case is a moved ticker, never a lost one. | Drop the old position and start once at the default top-center spot. |
| Localized time labels | The core writes "14:30" or "2:30 PM" from the shell's `hour12`, and English dates such as "Nov 3". A locale that writes "3. Nov." loses that. | The shell formats the tick times the frame gives it with `DateFormatter`. That brings back a measuring round trip for overlap skipping. |
| Chart look | One rule each for gridlines, label placement, the placeholder size and the stale capsule spot. PR 3 takes the Mac's axis rules, because they are the newer, tested ones. Windows changes more than the Mac. Mac timeframe labels start multiplying the price color's alpha. | Take Windows' measured price axis and centered labels instead. Then the Mac changes more. |
| Zoom travel | Travel moves into the core with Windows' rule, which resets on a reversal. A trackpad user who reverses mid-gesture loses the partial travel the Mac keeps today. | Put the Mac's rule in the core, so Windows stops resetting on a reversal. |
| Crashes show Rust stacks | A core bug shows up as a Rust stack. A panic poisons the core and Swift calls `fatalError("Rust core: ...")`. lldb can step into Rust but shows no Rust values without extra setup. | Swift logs the error reply and builds a fresh core from the saved settings. This keeps the app running but can hide a bug. |

## What the plan costs

The numbers come from design A's proof on this Windows PC with Rust 1.98.1, except where a line names B. "Measured" and "guess" are the designs' own labels.

| Cost | Value | Label |
| --- | --- | --- |
| Cold release build of `ticker-ffi`, Windows | 12.1 s. The first run took 37.8 s and is treated as a cold-disk outlier, which is a guess. | Measured |
| Rebuild after touching `ticker-core` | 7.4 s | Measured |
| Cold staticlib for `aarch64-apple-darwin` and `x86_64-apple-darwin`, built on Windows | 12.4 s and 17.6 s | Measured |
| `ticker_ffi.dll`, release, a stand-in for what the core adds | 527,360 bytes | Measured |
| `libticker_ffi.a`, aarch64, release, and with LTO | 19.4 MB and 6.3 MB. An archive carries all of std, and the linker keeps only what is used. | Measured |
| Growth of the Mac executable | Under 1 MB | Guess |
| `handle(tick)` with no effects, from C | 0.3 µs | Measured |
| `view` with 360 bars, a 39 KB reply | 96 to 108 µs | Measured |
| No-op `cargo build` before `swift build` | Well under a second | Guess |
| A Rust toolchain on every Mac that builds the app | About 1 GB for rustup | Guess |
| macOS CI job, warm | 4 to 6 minutes | Guess |
| One macOS round trip, billed | 40 to 60 minutes at the 10x multiplier | Guess |
| `Wire.swift` `Codable` mirrors to keep in step by hand | About 200 lines | A's estimate, unlabeled |
| B's `uniffi-bindgen` build, for comparison | 144 to 156 s | Measured |

Before PR 7, `swift build` on a fresh checkout fails with `library not found for -lticker_ffi` until `scripts/build-core.sh` has run. `bundle.sh` and `verify.sh` run it first.

## Behavior differences today

This list merges both inventories and drops duplicates. They read the sources at `87def14`, and nothing ran on a Mac. "Inferred" marks an entry that follows from the code with no test behind it. "Until PR n" names the PR that brings both apps to one rule. The entries in the cores close when PR 7 deletes Swift `TickerCore`.

### In the cores

1. Saved positions are not interchangeable. The Mac saves bottom-left AppKit points and Rust saves top-left virtual-screen pixels under the same `position` key. The layout geometry agrees once the Mac shell converts. Open.
2. Rust clamps edited settings in `OverlayState::new` and on `SettingsEdited`. Swift clamps only when decoding. Not visible, because the Mac's sliders stay in range. Open.
3. Swift reads `"decimals": 2.0` as 2. Rust accepts only JSON integers and falls back to the default. Visible only with a hand-edited file. Open.
4. Swift writes `"fontSize":15` and Rust writes `"fontSize":15.0`. Each reads the other's. Open.
5. Swift accepts a Kraken price sent as a JSON boolean and Rust rejects it. Kraken never sends one. Inferred. Open.
6. Swift `CandleChart.init?` traps on a bar budget of 0. Rust takes `max_bars.max(1)`. Not visible, because the budget is at least 1. Open.
7. Rust takes only the fetch result whose `RequestId` it issued. Swift takes any result while a fetch is in flight. Quote and candle requests can overlap, and switching timeframes can leave a candle request out for each one, but each app keeps at most one quote request and one candle request per timeframe, so the difference is not visible. Open.
8. The Mac re-derives the feed status on a 1-second clock. Rust's `next_wake` aims at the moment a quote turns stale but schedules exactly the threshold, one instant before `FeedStatus::derive` agrees, and Windows' 1 s heartbeat catches it. Both can lag by up to a second. Open until PR 4.
9. Swift rounds prices with ICU's `.toNearestOrAwayFromZero`. Rust rounds the shortest decimal form by hand. A value such as 1.005 could round differently. Inferred. Open.

### In the chart

10. Price ticks differ. Swift takes the coarsest step of 1, 2 or 5 times a power of ten that leaves at least 3 ticks. Windows takes the finest step of 1, 2, 2.5, 5 or 10 that is at least a fifth of the span. Open until PR 3.
11. Time ticks differ. Swift has 17 steps from 1 minute to 30 days and keeps at least 3 ticks. Windows has 13 steps from 1 minute to 7 days and aims for at most 5. That is a target, not a bound: with 7 days as its largest step, a 120-day 4h chart zoomed to all gets about 17 ticks, and inclusive endpoints can add one. Open until PR 3.
12. Both apps place ticks with one UTC offset and format each label in local time. Swift takes the offset at the chart's start (`ChartView.swift`) and formats through `DateFormatter`. Windows takes it at the chart's end (`render.rs`) and formats through `local_time`. On a chart that spans a DST change, they place ticks an hour apart. A shared core that formats every label with one `utc_offset` would also print the wrong hour for ticks on the far side of the change. PR 3 has to pick: send the shell the offset at each tick, or accept the wrong hour across a DST change. Open until PR 3.
13. The Mac formats time labels with the locale's `jmm` and `MMMd` templates. Windows always writes "14:30" and "Nov 3". Open, and PR 7 settles it as in the table above.
14. Overlapping time labels. Both apps now skip a label that would start within 6 points of the previous one. Closed by PR #11, which added `spaced_labels` to `ticker-core`.
15. Time label placement still differs. The Mac left-aligns a label at its tick plus 2. Windows centers it on its tick. Both clamp into the plot. Placement lives in the shells (`ChartView.swift` and `render.rs`), not in the tick functions PR 3 ports, so it moves into the frame. Open until PR 4.
16. The fade from the price color's opacity. Grid 12%, axis labels 70%, last price 75% and placeholder 60% now multiply the color's alpha on both apps. Closed by PR #11, which put the shares in `ChartInk`.
17. Mac timeframe labels replace the price color's alpha with 1 or 0.6, so they ignore a translucent price color. Windows multiplies. Open until PR 7.
18. The plot rect differs. The Mac reserves a fixed 60 points for price labels. Windows measures the widest label and adds a 6-pixel gap, so its bar budget and drawn plot width disagree slightly. Open until PR 4.
19. The placeholder is 10 pt on the Mac and 12 DIP on Windows. The stale badge sits at the chart's corner on the Mac and the plot's corner on Windows. Open until PR 7 at the latest.
20. Windows gives wicks and bodies a 1-pixel minimum height and rounds the wick's x. The Mac does neither. Open until PR 7 at the latest.

### In the shells

21. Zoom travel differs. The Mac steps once per wheel event, once per 50 points of trackpad travel and once per 0.25 of pinch. It ignores momentum, restarts at a gesture's start and keeps travel across a reversal. Windows steps once per 120 wheel units and resets on a reversal and when the cursor leaves the chart. Open until PR 7.
22. Both drag thresholds are 3, but in points on the Mac and physical pixels on Windows. At 200% scaling, Windows starts a drag after half the Mac's travel. Open.
23. The e2e snapshot schemas differ. Rust has `hwnd` and `maxBars`, and Swift has `windowNumber` and `shortcuts`. The Mac keeps a small Swift `Snapshot` so `scripts/e2e.swift` does not change. Open by design.
