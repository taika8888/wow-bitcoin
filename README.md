<img src="assets/icon.svg" width="64" height="64" alt="">

# BTC Ticker

A macOS app that floats a live Bitcoin price above World of Warcraft. Click the ticker to open a candlestick chart with 1m, 15m and 4h timeframes, and click it again to close the chart. Scroll or pinch over the chart to zoom from a quarter of the usual span out to the last 720 candles Kraken sends. The ticker never takes focus from the game. Prices come from Kraken's public API.

## Build and run

Swift 6.1 from the Xcode Command Line Tools is enough.

```sh
./scripts/bundle.sh              # builds "build/BTC Ticker.app"
open "build/BTC Ticker.app"
```

The app has no Dock icon. Use the bitcoin icon in the menu bar, or right-click the ticker, to reach Settings, Lock/Unlock and Quit.

## Settings

- Font size, decimals (0 to 2), and whether to show the `BTC` label and the change since the daily open.
- Price, up and down colors, and the background opacity.
- Chart width and height. A wider chart draws more bars before neighbouring candles merge into one.
- The chart's zoom, kept from the last scroll or pinch.
- Refresh interval (2 to 300 seconds, default 5).
- Show only while World of Warcraft is frontmost.
- Lock position. Drag the ticker to move it while it is unlocked. Clicks open the chart whether it is locked or not.

Settings are stored in the `dev.local.btcticker` defaults domain. A saved position that no longer fits a screen gets pulled back into the visible area.

## Verification

`./scripts/verify.sh` is the loop's exit condition. It prints one PASS, FAIL or INCONCLUSIVE line per check and exits 0 only when every line is PASS.

- A release build with warnings treated as errors.
- Unit tests over the pure `TickerCore` library, run against Kraken responses recorded in `Tests/TickerCoreTests`. One of them is an exhaustive model check. It walks every state the overlay reducer can reach under every event and asserts the invariants in each one. For example, a click never moves the ticker, and a locked ticker never drags.
- The app bundle, checked with `plutil -lint` and `codesign --verify`.
- End-to-end checks against the built app with a live Kraken feed. They check that the window sits on a higher layer than the WoW window, that WoW stays frontmost, that every timeframe loads its candles, that scrolling zooms out to every candle and back, and that the panel catches clicks even at opacity 0. They also save screenshots to `build/expanded-*.png`.

The end-to-end clicks and scrolls are synthesized `NSEvent`s sent through the panel, not real mouse input, and the check's name says so. Real input needs Accessibility permission for the terminal that runs the script. The `e2e-above-wow` check needs a WoW window under the ticker.

## Windows

A native Windows 11 port lives in `windows/`: the pure `ticker-core` crate decides everything, and `btc-ticker` is the Win32 shell around it. It needs only the Rust toolchain (1.98 or later, MSVC target), with no C compiler or resource compiler.

```powershell
cd windows; cargo build --release
.\target\release\btc-ticker.exe
```

The ticker starts at the top center of the primary monitor. Right-click it, or the orange "B" in the notification area, for Settings, Lock/Unlock Position and Quit. Windows 11 puts a new tray icon in the overflow, behind the `^` arrow, until you pin it. Run WoW in **Windowed (Fullscreen)** mode, since exclusive fullscreen can hide overlays.

As on macOS, turn the mouse wheel, scroll the touchpad or pinch over the open chart to zoom, from a quarter of the usual span out to all 720 candles Kraken sends. The game keeps the keyboard and the focus, and it doesn't get the wheel while the pointer is over the chart, even with Windows' "Scroll inactive windows when I hover over them" setting turned off.

Settings are stored as JSON in `%APPDATA%\BTCTicker\settings.json` and apply as soon as the file changes, including edits made by hand. Positions are physical pixels on the virtual screen, so the file is not interchangeable with the macOS app's.

`powershell -NoProfile -ExecutionPolicy Bypass -File windows\scripts\verify.ps1`, run from the repo root, is the Windows loop's exit condition. It builds with warnings denied, runs clippy and the core's unit tests and model checks, then runs `btc-ticker-e2e` against the release exe. That end-to-end check covers the primary monitor with a stand-in game window, a copy of itself named `Wow.exe`, for about a minute and drives the app with real mouse input through `SendInput`. Don't touch the mouse while it runs: it refuses to click any window but its own and the app's. If it sees the cursor move or presses it did not send, it stops sending input, reports that check INCONCLUSIVE and every later one INCONCLUSIVE as not run, then still quits or kills the app it launched and checks that nothing is left behind. It also turns the wheel over the chart, and for a moment switches wheel routing to the focused window for this session only. It saves screenshots of the collapsed ticker, each timeframe's chart, the widest zoom and the Settings window to `windows/target/verify/*.png`. The app it launches uses a throwaway settings file, never yours.
