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
