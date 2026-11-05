<img src="assets/icon.svg" width="64" height="64" alt="">

# BTC Ticker

Floats a live Bitcoin price above World of Warcraft, on macOS and on Windows. Click the ticker to open a candlestick chart with 1m, 15m and 4h timeframes, and click it again to close the chart. The ticker never takes focus from the game. Prices come from Kraken's public API.

The macOS app is the Swift package at the repository root. The Windows app is the Rust workspace in `windows/`, described in [Windows](#windows).

## Build and run on macOS

Swift 6.1 from the Xcode Command Line Tools is enough.

```sh
./scripts/bundle.sh              # builds "build/BTC Ticker.app"
open "build/BTC Ticker.app"
```

The app has no Dock icon. Use the bitcoin icon in the menu bar, or right-click the ticker, to reach Settings, Lock/Unlock and Quit.

## Settings on macOS

- Font size, decimals (0 to 2), and whether to show the `BTC` label and the change since the daily open.
- Price, up and down colors, and the background opacity.
- Chart width and height.
- Refresh interval (2 to 300 seconds, default 5).
- Show only while World of Warcraft is frontmost.
- Lock position. Drag the ticker to move it while it is unlocked. Clicks open the chart whether it is locked or not.

Settings are stored in the `dev.local.btcticker` defaults domain. A saved position that no longer fits a screen gets pulled back into the visible area.

## Verification on macOS

`./scripts/verify.sh` is the loop's exit condition. It prints one PASS, FAIL or INCONCLUSIVE line per check and exits 0 only when every line is PASS.

- A release build with warnings treated as errors.
- Unit tests over the pure `TickerCore` library, run against Kraken responses recorded in `Tests/TickerCoreTests`. One of them is an exhaustive model check. It walks every state the overlay reducer can reach under every event and asserts the invariants in each one. For example, a click never moves the ticker, and a locked ticker never drags.
- The app bundle, checked with `plutil -lint` and `codesign --verify`.
- End-to-end checks against the built app with a live Kraken feed. They check that the window sits on a higher layer than the WoW window, that WoW stays frontmost, that every timeframe loads its candles, and that the panel catches clicks even at opacity 0. They also save screenshots to `build/expanded-*.png`.

The end-to-end clicks are synthesized `NSEvent`s sent through the panel, not real mouse input, and the check's name says so. Real input needs Accessibility permission for the terminal that runs the script. The `e2e-above-wow` check needs a WoW window under the ticker.

## Windows

### Run WoW in a windowed mode

Set **Display Mode** in WoW's System settings to **Windowed** or **Windowed (Fullscreen)**. Windows draws no other window over a game in exclusive fullscreen, so there the ticker stays hidden behind WoW.

### Get the app

Each green run of the `windows` workflow uploads the exe it tested as the `btc-ticker-<commit>` artifact. Download it from the run's page and start `btc-ticker.exe`. There is no installer and no runtime to install.

To build it yourself, install Rust with the MSVC build tools, then run:

```sh
cd windows
cargo build --release    # builds windows\target\release\btc-ticker.exe
```

The ticker has no taskbar button. Use the bitcoin icon in the notification area to reach Settings, Lock/Unlock and Quit. Right-clicking the ticker shows the same menu, drawn inside the ticker so that WoW keeps the focus.

### Settings on Windows

The settings are the same as on macOS, with one addition: each color also has an opacity slider. The app stores them as one JSON value, `btcTickerSettings` under `HKCU\Software\BTCTicker`, with the same keys as macOS. On Windows, `position` is the top-left corner of the ticker in physical pixels.

"Show only while World of Warcraft is frontmost" matches the foreground window's process against `Wow.exe`, `WowT.exe`, `WowB.exe`, `WowClassic.exe`, `WowClassicT.exe` and `WowClassicB.exe`.

### Verification on Windows

`windows/scripts/check.sh` runs on any machine with Rust and the `x86_64-pc-windows-msvc` target, macOS included. It prints one PASS or FAIL line per step and exits 0 only when every line is PASS.

- `cargo fmt` and `cargo clippy` with warnings as errors, for the host and for the Windows target. The Windows lint type-checks the Win32 shell without linking it.
- Unit tests over `ticker-core` against the recorded Kraken responses in `Tests/TickerCoreTests`. A test fails when a Swift test in `Tests/TickerCoreTests` has no Rust counterpart.
- The model check, ported from `OverlayModelCheckTests.swift`. It walks every state the overlay reducer can reach (1026 states and 31,806 transitions) and asserts 18 invariants in each: the 14 from macOS and 4 for the drawn menu. Each invariant also has a deliberately broken reducer that the check must catch.
- Pixel tests over `ticker-paint`. For example, every pixel of the window keeps an alpha of at least 1/255. That keeps a ticker at 0% background opacity invisible but still clickable, where Windows would otherwise pass clicks through to the game.

`.github/workflows/windows.yml` runs the build, the same tests and an end-to-end check on GitHub's `windows-latest` runner. The end-to-end check starts the built app next to a stand-in game window named `Wow.exe`, which is itself topmost. It then checks the following through Win32:

- The ticker is on screen, above the stand-in, and back on top after the stand-in raises itself.
- The ticker has `WS_EX_NOACTIVATE` and `WS_EX_TOOLWINDOW`.
- The stand-in stays the foreground window through launch, settings edits and every click.
- The chart opens and loads 120, 96 and 90 candles from live Kraken for 1m, 15m and 4h.
- The ticker catches clicks across its whole area at 0% background opacity.

It saves a screenshot of each timeframe as an artifact. The clicks are synthesized with `SendInput`, not made with a physical mouse, and the names of those checks say so. Each check that could pass without testing anything first proves that it can fail on that runner. For example, a harness window takes the foreground on purpose, and the focus check must see it.

The runner has one 1024x768 screen at 96 DPI. Scaling at other DPIs is covered only by the unit tests, and multiple monitors and a real WoW client are not tested.
