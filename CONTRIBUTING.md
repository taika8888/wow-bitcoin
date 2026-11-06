# Contributing to BTC Ticker

Bug reports, fixes and small features are welcome. For a large change, open an issue first so the approach can be agreed before you write the code. To report a security problem, follow [SECURITY.md](SECURITY.md) instead of opening an issue.

## Find your way around

| Path | What it holds |
| --- | --- |
| `Package.swift`, `Sources/`, `Tests/` | The macOS app. `BTCTicker` is the shell, and `TickerCore` is the pure core library with its tests. |
| `scripts/` | `bundle.sh`, which builds the app bundle, and `verify.sh` with its end-to-end check `e2e.swift` |
| `Cargo.toml`, `crates/ticker-core/` | The Cargo workspace and the pure Rust core, with its unit tests and model checks |
| `windows/btc-ticker/` | The Win32 shell around `ticker-core` |
| `windows/e2e/` | The Windows end-to-end check, `btc-ticker-e2e` |
| `windows/scripts/` | `verify.ps1` and `mutants.ps1` |
| `docs/shared-core.md` | The plan to run both apps on one Rust core |

## Build and test the macOS app

You need Swift 6.1 or later from the Xcode Command Line Tools. The app itself runs on macOS 14 or later.

```sh
swift build
swift test
./scripts/bundle.sh              # builds "build/BTC Ticker.app"
```

`./scripts/verify.sh` is the exit condition of the macOS verify loop. It runs a release build with warnings as errors, the unit tests, the bundle checks and an end-to-end check against the built app with a live Kraken feed. It prints one PASS, FAIL or INCONCLUSIVE line per check and exits 0 only when every line is PASS. The `e2e-above-wow` check needs a WoW window under the ticker. The [README](README.md#macos-scriptsverifysh) lists every check.

## Build and test the Windows app

You need the Rust toolchain, 1.98 or later, with the MSVC target. No C compiler is needed. The app targets Windows 11.

```powershell
cargo build --release
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

`cargo test --workspace` includes the model checks in `crates/ticker-core/tests/model_check/`. They walk every interleaving of inputs to `App::handle` and `OverlayState::reduce` up to a bound and check the invariants after each step.

`windows\scripts\verify.ps1` is the exit condition of the Windows verify loop. Run it from the repo root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File windows\scripts\verify.ps1
```

It builds with warnings denied, runs clippy, the unit tests and the model checks, then runs the end-to-end check with real mouse input for about a minute. Don't touch the mouse while it runs. The [README](README.md#windows-windowsscriptsverifyps1) describes what it checks.

`windows\scripts\mutants.ps1` proves the model checks can fail. Each mutant breaks one property in the `ticker-core` source and expects the named invariant to catch it. Run it after you change a model check or an invariant.

## Work on the shared core from any OS

`crates/ticker-core` has no platform code, so its tests run on macOS and Linux too:

```sh
cargo test -p ticker-core
```

`cargo test --workspace` builds the Win32 crates as well, so it works only on Windows.

The `verify-ticker-model` skill in `.claude/skills/verify-ticker-model/SKILL.md` lists every model-check invariant and the harm it prevents. Run the model checks before you commit a change to `app.rs`, `chart.rs`, `feed.rs`, `overlay.rs`, `market.rs` or `kraken.rs` in `crates/ticker-core/src`. Claude Code finds the skill in that folder. People can read it as a guide.

[docs/shared-core.md](docs/shared-core.md) is the plan to make `crates/ticker-core` the only core for both apps. When it is done, the Swift `Sources/TickerCore` library goes away. If your change touches logic that both apps share, read the plan first.

## CI runs on pull requests

GitHub Actions runs two jobs on pull requests. Each job runs only when the pull request touches the paths in its workflow file.

- `.github/workflows/windows.yml` runs `verify.ps1`, including the end-to-end check, on `windows-latest`. Changes under `crates/` or `windows/`, the root `Cargo.*` files, and the shared Kraken fixtures trigger it.
- `.github/workflows/macos.yml` runs the release build, `swift test` and the bundle checks on `macos-latest`. Changes under `Sources/`, `Tests/`, `scripts/`, `Support/`, `assets/` or `crates/`, and `Package.swift`, trigger it. It does not run the macOS end-to-end check, because that check needs a WoW window. Run `scripts/verify.sh` on a Mac for it.

A pull request that changes only docs runs neither job.

## Follow the house style

- Write a comment only for a non-obvious why. The code shows what it does.
- Write each commit message as one plain sentence in the imperative, with no type prefix and no trailing period. For example, "Move the Cargo workspace to the repo root".
- Format Rust with `cargo fmt`. The settings are in `rustfmt.toml`.
- Keep each pull request to one change. In its description, say how you verified it, for example by pasting the PASS lines from `verify.sh` or `verify.ps1`. An INCONCLUSIVE line is not a pass.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
