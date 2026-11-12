<img src="assets/icon.svg" width="64" height="64" alt="">

# BTC Ticker

BTC Ticker shows the live Bitcoin price in a small window that floats above World of Warcraft. The ticker never takes focus from the game, so WoW keeps the keyboard. Click the ticker to open a candlestick chart with 1m, 15m and 4h timeframes, and click it again to close the chart. Scroll or pinch over the chart to zoom from a quarter of the usual span out to the last 720 candles. The Windows app runs on Windows 11, and the macOS app runs on macOS 14 or later. Prices come from Kraken's public API.

## Download

Download the app from the [latest release](../../releases/latest). Each release has three files:

| File | What it holds |
| --- | --- |
| `btc-ticker-windows-x64.zip` | `btc-ticker.exe`, the Windows app |
| `btc-ticker-macos.zip` | `BTC Ticker.app`, the macOS app |
| `SHA256SUMS.txt` | The SHA-256 checksum of each zip |

To check a download, compare its checksum with the line for that zip in `SHA256SUMS.txt`:

```powershell
Get-FileHash .\btc-ticker-windows-x64.zip -Algorithm SHA256
```

```sh
shasum -a 256 btc-ticker-macos.zip
```

## Install on Windows

1. Unzip `btc-ticker-windows-x64.zip` and put `btc-ticker.exe` in any folder you like.
2. Run `btc-ticker.exe`.
3. The build is not code-signed, so Windows SmartScreen may say "Windows protected your PC". Click **More info**, then **Run anyway**.

The ticker starts at the top center of the primary monitor. Right-click the ticker, or the orange "B" in the notification area, for **Settings…**, **Lock Position** and **Quit BTC Ticker**. Windows 11 puts a new tray icon in the overflow, behind the `^` arrow, until you pin it.

## Install on macOS

1. Unzip `btc-ticker-macos.zip` and move `BTC Ticker.app` to your Applications folder.
2. Open `BTC Ticker.app`.
3. The build is not signed with an Apple Developer ID or notarized, so macOS blocks the first launch. On macOS 14, right-click the app, choose **Open**, then click **Open** again. On macOS 15 and later, open the app once, then go to **System Settings** > **Privacy & Security** and click **Open Anyway**.

Instead of step 3, you can remove the quarantine flag in Terminal:

```sh
xattr -dr com.apple.quarantine "/Applications/BTC Ticker.app"
```

The app has no Dock icon. Use the bitcoin icon in the menu bar, or right-click the ticker, for **Settings…**, **Lock Position** and **Quit BTC Ticker**.

## Run WoW in a windowed mode

Set WoW's display mode to **Windowed** or **Windowed (Fullscreen)**. In exclusive **Fullscreen** mode, the game owns the screen and no other window can draw over it, so the ticker is hidden.

## Use the ticker

- Click the ticker to open or close the chart. Click 1m, 15m or 4h on the open chart to switch the timeframe.
- Scroll, turn the mouse wheel or pinch over the open chart to zoom. The game keeps the keyboard and the focus, and it doesn't get the wheel while the pointer is over the chart. On Windows, this holds even with the "Scroll inactive windows when I hover over them" setting turned off.
- While the position is unlocked, drag the ticker to move it. On Windows, you can also drag the open chart, and drag its corner grip, opposite the ticker, to resize it. The new size is saved when you release the mouse.
- **Lock Position** stops dragging and resizing. Clicks still open the chart.

The **Settings** window has these options:

- Font size, decimals (0 to 2), and whether to show the `BTC` label and the change since the daily open.
- Price, up and down colors, and the background opacity.
- Chart width and height. A wider chart draws more bars before neighbouring candles merge into one.
- Refresh interval (2 to 300 seconds, default 5).
- Show only while World of Warcraft is frontmost.
- Lock position, and **Reset position**, which moves the ticker back to the top center of the screen.
- On Windows, **Start with Windows**, which starts the ticker when you sign in. It adds a `BTCTicker` entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and keeps no copy in the settings file. If you move the exe, untick and tick the box again.

The app also remembers the last timeframe you chose and the last zoom.

## Settings and reset

Settings are saved as soon as you change them. A saved position that no longer fits a screen is pulled back into the visible area.

On Windows, settings are a JSON file at `%APPDATA%\BTCTicker\settings.json`. The app applies edits to the file as soon as you save them, including edits made by hand. To reset every setting, quit the app, delete the file, and start the app again.

On macOS, settings are stored in the `dev.local.btcticker` defaults domain, under the key `btcTickerSettings`. To reset every setting, quit the app, run this command, and start the app again:

```sh
defaults delete dev.local.btcticker
```

The two apps do not share a settings format. Windows saves positions in physical pixels and macOS saves them in points, so you can't copy one app's settings to the other.

## Quit and uninstall

To quit, choose **Quit BTC Ticker** from the ticker's right-click menu. On Windows, the tray icon's menu has the same item. On macOS, the menu bar icon's menu has it.

Neither app has an installer.

- To uninstall on Windows, quit the app, then delete `btc-ticker.exe` and the `%APPDATA%\BTCTicker` folder.
- To uninstall on macOS, quit the app, move `BTC Ticker.app` to the Trash, then run `defaults delete dev.local.btcticker`.

## Privacy

The apps send only two kinds of request, both HTTPS GETs to Kraken's public REST API at `api.kraken.com`:

- `https://api.kraken.com/0/public/Ticker?pair=XBTUSD` for the price, once per refresh interval, and less often after repeated failures.
- `https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval=<minutes>` for the candles, only while the chart is open.

There are no accounts, no API keys, no analytics and no telemetry. Like any web request, a request to Kraken shows Kraken your IP address. Settings stay on your computer.

## Disclaimer

BTC Ticker is not affiliated with or endorsed by Blizzard Entertainment or Kraken. World of Warcraft is a trademark of Blizzard Entertainment.

The app draws only its own window above the game. It never reads the game's memory or files, injects code into the game, sends input to the game, or automates it. To show the ticker only while WoW is frontmost, it asks the operating system which app is in front.

Prices are for information only. They can be delayed or wrong, and nothing in this app is financial advice.

## Build from source

### macOS

Swift 6.1 from the Xcode Command Line Tools is enough. The app runs on macOS 14 or later.

```sh
./scripts/bundle.sh              # builds "build/BTC Ticker.app"
open "build/BTC Ticker.app"
```

### Windows

The Windows app is built from the Cargo workspace at the repo root. The pure `crates/ticker-core` crate decides everything, and `windows/btc-ticker` is the Win32 shell around it. It needs only the Rust toolchain (1.98 or later, MSVC target), with no C compiler or resource compiler.

```powershell
cargo build --release
.\target\release\btc-ticker.exe
```

## Verify a change

[CONTRIBUTING.md](CONTRIBUTING.md) explains how to work on the code. Each platform has one script that is the exit condition of its verify loop.

### macOS: `scripts/verify.sh`

`./scripts/verify.sh` prints one PASS, FAIL or INCONCLUSIVE line per check and exits 0 only when every line is PASS.

- A release build with warnings treated as errors.
- Unit tests over the pure `TickerCore` library, run against Kraken responses recorded in `Tests/TickerCoreTests`. One of them is an exhaustive model check. It walks every state the overlay reducer can reach under every event and asserts the invariants in each one. For example, a click never moves the ticker, and a locked ticker never drags.
- The app bundle, checked with `plutil -lint` and `codesign --verify`.
- End-to-end checks against the built app with a live Kraken feed. They check that the window sits on a higher layer than the WoW window, that WoW stays frontmost, that every timeframe loads its candles, that scrolling zooms out to every candle and back, and that the panel catches clicks even at opacity 0. They also save screenshots to `build/expanded-*.png`.

The end-to-end clicks and scrolls are synthesized `NSEvent`s sent through the panel, not real mouse input, and the check's name says so. Real input needs Accessibility permission for the terminal that runs the script. The `e2e-above-wow` check needs a WoW window under the ticker.

### Windows: `windows\scripts\verify.ps1`

Run this from the repo root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File windows\scripts\verify.ps1
```

It builds with warnings denied, runs clippy and the core's unit tests and model checks, then runs `btc-ticker-e2e` against the release exe. That end-to-end check covers the primary monitor with a stand-in game window, a copy of itself named `Wow.exe`, for about a minute and drives the app with real mouse input through `SendInput`.

Don't touch the mouse while it runs. It refuses to click any window but its own and the app's. If it sees the cursor move or presses it did not send, it stops sending input and reports that check INCONCLUSIVE and every later one INCONCLUSIVE as not run. It then still quits or kills the app it launched and checks that nothing is left behind.

It also drags and resizes the open chart, checks that resizing respects the lock and saves the size, turns the wheel over the chart, and for a moment switches wheel routing to the focused window for this session only. It saves screenshots of the collapsed ticker, each timeframe's chart, the widest zoom and the Settings window to `target/verify/*.png`. The app it launches uses a throwaway settings file, never yours, and its Start with Windows entry goes under `HKCU\Software\BTCTicker-test`, never your Run key.

## License

BTC Ticker is released under the [MIT License](LICENSE).

Created in [T3 Code](https://t3.codes).
