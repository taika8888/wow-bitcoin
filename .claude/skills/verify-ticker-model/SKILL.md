---
name: verify-ticker-model
description: Model-check the Windows BTC ticker's core state machines (App::handle and OverlayState::reduce in windows/crates/ticker-core) over every interleaving of inputs. Use before committing any change to windows/crates/ticker-core/src/app.rs, chart.rs, feed.rs, overlay.rs, market.rs or kraken.rs, when a model check in cargo test fails, or when a change is meant to alter fetch, schedule, click, drag, zoom, save or render behavior.
---

# Verify the ticker's state machines

The Windows ticker's core is two pure state machines. `App::handle` takes every input the shell sends (pointer events, ticks, wakes, fetch results, settings file changes) and returns the effects the shell runs (fetch, save, render). `OverlayState::reduce` decides clicks, drags, zoom, lock and visibility. Bugs there show up as duplicate requests to Kraken, a ticker that jumps or will not expand, a chart that zooms while hidden or skips a rung, settings written back over the user's edit, or a frozen display. All of these depend on input order, and unit tests reach few orders. The model checks try every order up to a bound, through the real code.

## What is verified

All of this lives in `windows/crates/ticker-core/tests/model_check/`, in the `model_check` test target. Each check is one invariant, checked on every transition. Each line below gives the property and the harm it prevents.

`app.rs`, the App check (14 invariants, in the `INVARIANTS` table):

- **never two quote requests outstanding.** Prevents doubling the load on Kraken and racing two answers into the display.
- **never two candle requests outstanding for one timeframe.** Prevents duplicate OHLC downloads and an older series overwriting a newer one.
- **candles are fetched only for the visible expanded timeframe.** Prevents network traffic for a chart nobody can see, for example while WoW is in the background.
- **a quote request starts exactly when one is due and none is out.** Prevents a frozen price (no fetch) and a hammering loop (early fetch), including after a refresh edit or a wake.
- **the visible expanded chart's candles are requested exactly when due.** Prevents a chart that never loads, never refreshes, or never retries 10 s after a failure.
- **a late or unknown result changes nothing.** Prevents a timed-out or duplicate answer from overwriting newer data or clearing the in-flight guard.
- **a click never moves the ticker.** Prevents the ticker jumping to the pointer when the user only meant to open the chart.
- **only a drag or a settings edit moves the ticker.** Prevents the ticker drifting on ticks, results or focus changes.
- **a locked ticker never drags.** Prevents Lock from being ignored mid-raid.
- **a hidden overlay is never pressed or dragging.** Prevents a stale press from turning into a drag when the overlay reappears.
- **SaveSettings is emitted iff an in-app input changed the settings, and carries the new settings.** Prevents lost settings (no save), useless disk writes (unchanged save), and writing back a file the user is editing (echo of `SettingsChanged`).
- **the feed status is FeedStatus::derive of the last good quote, the start and the clock.** Prevents the ticker showing live for an old price, or offline while a good quote is fresh.
- **Render is emitted iff the overlay, the feed status or a chart load changed.** Prevents a screen that lags the state (missed render) and wasted redraws.
- **nothing is overdue, and with no quote out the next wake is no later than the next quote.** Prevents a shell timer armed by `App::next_wake` from sleeping past a due fetch or status change.

`overlay.rs`, the reducer check (18 invariants, ported from the macOS `OverlayModelCheckTests.swift`):

- **a locked overlay is never dragging.** Lock holds.
- **a hidden overlay is never pressed or dragging.** No ghost drags.
- **with onlyWhenWoWActive on, visible iff WoW (or the ticker's own settings) is frontmost.** The overlay neither covers other apps nor vanishes over WoW.
- **only a drag changes position, apart from a settings edit.** No drift.
- **a click on the body expands to the last chosen timeframe, or collapses.** The click toggles the chart and opens it on the remembered timeframe.
- **a locked press released over the body is a click, however far the pointer moved.** A locked ticker still opens its chart when the hand shakes.
- **releasing over a timeframe while collapsed, or outside the overlay, never changes the mode.** No surprise toggles.
- **a release always ends the press.** No stuck press.
- **expanded carries one of Timeframe::ALL, and it is the persisted choice.** The chart and the saved choice cannot disagree.
- **a click on a timeframe while expanded shows that timeframe.** The timeframe controls work.
- **collapsing and re-expanding a visible overlay returns to the last chosen timeframe.** The choice survives a toggle.
- **hiding and showing never change the mode.** Alt-tabbing out of WoW does not close the chart.
- **a locked overlay moves only through a settings edit.** Lock holds against drops.
- **a finished drag stores the drop point and never changes the mode.** Drags persist and are not clicks.
- **only a zoom event on a visible chart changes the zoom, apart from a settings edit.** The wheel over a collapsed or hidden overlay, or any other input, never changes the saved zoom.
- **a zoom event on a visible chart moves one rung its way, or stays at the end of the ladder.** One notch is one rung, and the ends hold.
- **a zoom event changes nothing but the zoom: never mode, timeframe, position, lock or drag.** Scrolling over the chart cannot close it, switch it, move it or drop a press.
- **on a visible chart away from the ends of the ladder, zooming in then out returns to the same state.** The ladder has no one-way rungs.

The macOS invariant that the zoom is always a rung of the ladder has no Windows port: `Zoom` is an enum, so no other value can exist.

Each test also asserts reachability facts, such as dragging, a stale quote, offline, two failures in a row, a failed candle fetch, a locked press, every zoom rung on the chart, a hidden zoomed chart, a zoom kept across collapsing, and a zoomed chart with candles out or loaded. These keep the invariants from passing vacuously. If one of those asserts fails, the model lost coverage. That is not a bug in the app.

## Tool and why

The tool is an explicit-state breadth-first search over the real Rust code, in `cargo test`. `tests/model_check/engine.rs` keeps a visited set of 128-bit fingerprints of each world's `Debug` form. It applies every input through the crate's public API, so the model is the code. It records each state's parent, so every violation replays as the shortest input sequence that triggers it.

- TLA+ would check a hand-written model of the code, not the code itself. Java and TLC are not installed on this machine either.
- Kani does not run on Windows.
- A property test samples random paths. BFS covers every path up to the bound and gives the shortest counterexample.

The only dependency is the Rust toolchain the repo already uses (`rust-version` 1.98 in `windows/Cargo.toml`). The test profile is built at `opt-level = 3` with debug assertions on, so the walks run in seconds.

## Bounds and assumptions

The reducer check covers its whole reachable state space and asserts `unexpanded == 0`. It starts from 12 worlds (locked × onlyWhenWoWActive × the 3 frontmost apps). Its alphabet is: press, pointer moves of 1 px and 50 px, toggle Lock, zoom in and out, release over the body, outside, or each of the 3 timeframe controls at 2 drop points, each of the 3 apps activated, settings edits for each locked × onlyWhenWoWActive pair, clearing the position, switching the timeframe to 1m, and setting the zoom to all.

The App check is bounded by depth. It starts from `App::new(Settings::default(), Frontmost::Wow, T0)`.

- **Time.** Ticks at +1 s, +4 s and +16 s, and a wake at +1 s. Results arrive at the current clock. Times never go backwards.
- **Results.** Each outstanding quote or candle request can succeed (one fixed Kraken body) or fail (`HttpOutcome::Unreachable`). The last delivered quote and candle result can also arrive again, as a late duplicate.
- **Settings.** In-app edits to refresh 2 s, refresh 30 s, or flipping onlyWhenWoWActive. A file change to refresh 2 s with the 4h timeframe.
- **Full alphabet** (`every_interleaving_of_every_input_keeps_the_app_invariants`). Every input kind, including press, pointer moves of 2 px and 50 px, release over the body, outside, the 1m or 4h control, Lock, zoom in and out, and each of the 3 apps activated. Depth 7.
- **Feed alphabet** (`every_deeper_interleaving_of_feed_inputs_keeps_the_app_invariants`). Drops pointer moves, outside and 4h releases, Lock, zoom, and activating the ticker, which the reducer check already covers exhaustively. Depth 8.

Measured on this machine at the sync commit, inside `verify.ps1`'s `cargo test` run:

| Check | States | Transitions | Scope | Time |
| --- | --- | --- | --- | --- |
| overlay | 3,456 | 89,856 | exhaustive, longest path 13 | 0.2 s |
| app (full) | 163,494 | 1,047,044 | depth 7, 119,271 states at the bound | 5.2 s |
| app feed | 291,029 | 1,322,762 | depth 8, 208,978 states at the bound | 7.0 s |

The whole `model_check` target takes about 7 s once built, because the tests run in parallel. `cargo test` prints these lines as `model check ...`. Run it with `-- --show-output` to see them.

Out of scope, so a pass says nothing about:

- HTTP statuses other than 200, malformed bodies, and other prices (`tests/core/kraken.rs` covers parsing);
- refresh values other than 2, 5 and 30 s, and paths longer than the depth bound;
- clocks that go backwards (`advance` clamps them, unchecked here);
- layout, placement, display formatting, `Settings::decode`, `Snapshot`, and `CandleChart` geometry, including which candles a zoom shows and how they merge into the bar budget (`tests/core/` covers these);
- everything either native shell does, including how wheel deltas add up to zoom steps (the end-to-end checks cover that).

The macOS app now uses this same core through `ticker-ffi`. Axis tick and settings-decoder parity are covered in `tests/core/`, with deliberate mutations in `scripts/mutants-mac-core.py` (run from the repository root). Swift tests cover the C boundary and Mac layout; the former Swift reducer and feed tests have been replaced by these shared-core checks.

The App checker computes "a quote is due" with the code's own `FeedSchedule::delay`, and "candles are due" with the code's `Timeframe::spec().refresh_seconds`. So it checks that the schedule is honored, not the backoff numbers themselves. `tests/core/app.rs` pins those numbers.

## Run it

From `windows/` in the repo:

```
cargo test -p ticker-core --test model_check
```

Add `-- --show-output` to print the state counts. `powershell -NoProfile -ExecutionPolicy Bypass -File windows\scripts\verify.ps1`, run from the repo root, also runs it as part of the full gate.

To prove the checks can still fail, run this from the repo root:

```
powershell -NoProfile -ExecutionPolicy Bypass -File windows\scripts\mutants.ps1 [-Only <mutant-name>]
```

It needs a clean `windows/crates/ticker-core/src`, and it refuses to start otherwise. A killed run skips its restore and can leave a mutant in the source, so before you commit after an interrupted run, check `git status` and restore with `git checkout -- windows/crates/ticker-core/src`. Each mutant takes about 13 s, and the full set of 23 takes about 5 minutes. Logs go to `windows/target/verify/mutants/<name>.log`.

## Read a counterexample

A failure panics with one block per broken invariant:

```
invariant broken: a late or unknown result changes nothing
  reached by:
    start: a fresh app at t+0s, WoW in the foreground, locked false, onlyWhenWoWActive false, refresh 5 s, timeframe 15m, position default
    1. press the mouse on the overlay
    2. tick at t+16s
    3. quote request #1 succeeds
    4. late duplicate: quote request #1 succeeds
```

The steps are the inputs the shell sent, in order, in `tests/model_check/words.rs` wording. The path is the shortest that breaks that invariant. To reproduce it, write the steps as a unit test. App paths go in `tests/core/app.rs` and feed `App::handle`. Reducer paths go beside `a_press_is_a_drag_only_when_unlocked_and_past_the_threshold` in `tests/model_check/overlay.rs` and fold the events through `OverlayState::reduce`. Assert the property. Then fix the code, and keep the test.

## Real bug or modeling error

Each checker keeps its own records (`World` in `app.rs` and `overlay.rs`). These are which requests are out, the last delivered result, the last good quote, the schedule anchor, failures in a row, and the last candle outcome per timeframe. It builds them only from the inputs it sent and the effects it saw, never from the app's private fields. A violation is a disagreement between those records and the app.

- **Real bug.** Replay the steps against the product contract: the macOS app's behavior, the `Effect` and `Input` docs in `src/app.rs`, or the overlay's `OverlayEvent` docs. If the app's output is wrong, fix the code. Never weaken the invariant to make the check pass.
- **Modeling error.** The checker's record diverges from what the contract says, because the change intended new behavior. Examples: a new retry delay the checker's `candles_due` does not know about, or a new `Input` the `apply` match ignores. Update the record keeping or the invariant in the same commit as the behavior change, and say why in the commit message. Then rerun `mutants.ps1`. Every mutant must still be caught.
- **Vacuity failure.** A `reached.*` assert or `unexpanded == 0` fails. The alphabet no longer reaches a state the invariants need, so extend the alphabet.
- **New input or effect.** Add it to `steps`, `describe` and `apply`, add an invariant for the promise it makes, and add a mutant that breaks that promise.

## Mutants

`windows/scripts/mutants.ps1` holds the table. Each row edits the real source, then expects its named invariant to break with a counterexample. If its Find text no longer matches the code, the row reports "stale", so update the Find. Results at the sync commit:

| Mutant | File | Breaks | Caught by | Result |
| --- | --- | --- | --- | --- |
| quote-in-flight-guard-dropped | src/app.rs | quote in-flight guard | never two quote requests outstanding | PASS |
| stale-quote-id-accepted | src/app.rs | quote id check | a late or unknown result changes nothing | PASS |
| stale-candle-id-accepted-while-loading | src/app.rs | candle id check | a late or unknown result changes nothing | PASS |
| request-ids-reused | src/app.rs | unique request ids | a late or unknown result changes nothing | PASS |
| candles-fetched-while-hidden | src/chart.rs | visibility gate | candles are fetched only for the visible expanded timeframe | PASS |
| candles-fetched-while-collapsed | src/chart.rs | expanded gate | candles are fetched only for the visible expanded timeframe | PASS |
| candle-in-flight-guard-dropped | src/chart.rs | candle in-flight guard | never two candle requests outstanding for one timeframe | PASS |
| candle-failure-kept-after-success | src/chart.rs | success clears the last failure | the visible expanded chart's candles are requested exactly when due | PASS |
| refresh-edit-not-rescheduled | src/app.rs | fetch after an in-app edit | a quote request starts exactly when one is due and none is out | PASS |
| wake-keeps-backoff | src/app.rs | backoff reset on wake | a quote request starts exactly when one is due and none is out | PASS |
| tick-does-not-advance-time | src/app.rs | clock advance on tick | the feed status is FeedStatus::derive of the last good quote, the start and the clock | PASS |
| settings-file-written-back | src/app.rs | no echo of file changes | SaveSettings is emitted iff an in-app input changed the settings, and carries the new settings | PASS |
| saves-unchanged-settings | src/app.rs | save only on change | SaveSettings is emitted iff an in-app input changed the settings, and carries the new settings | PASS |
| no-render-for-chart-loads | src/app.rs | render on chart load change | Render is emitted iff the overlay, the feed status or a chart load changed | PASS |
| click-moves-ticker | src/overlay.rs | click does not move | a click never moves the ticker | PASS |
| locked-check-dropped-from-pointer-move | src/overlay.rs | lock test on pointer move | a locked press released over the body is a click, however far the pointer moved | PASS |
| locked-ticker-drags | src/overlay.rs | both lock tests | a locked ticker never drags | PASS |
| hidden-overlay-keeps-drag | src/overlay.rs | press reset on hide | a hidden overlay is never pressed or dragging | PASS |
| zoom-while-hidden | src/overlay.rs | visibility gate on zoom | only a zoom event on a visible chart changes the zoom, apart from a settings edit | PASS |
| zoom-while-collapsed | src/overlay.rs | expanded gate on zoom | only a zoom event on a visible chart changes the zoom, apart from a settings edit | PASS |
| zoom-in-steps-out | src/market.rs | zoom direction | a zoom event on a visible chart moves one rung its way, or stays at the end of the ladder | PASS |
| zoom-drops-the-press | src/overlay.rs | zoom touches only the zoom | a zoom event changes nothing but the zoom: never mode, timeframe, position, lock or drag | PASS |
| zoom-out-skips-a-rung | src/market.rs | one rung per step | on a visible chart away from the ends of the ladder, zooming in then out returns to the same state | PASS |

`locked-check-dropped-from-pointer-move` is not an equivalent mutant. The locked normalization at the end of `reduce` turns its drag back into `Idle`, so the release that follows is no longer a click, and the locked ticker stops opening its chart.

## Coverage record

| Check | Covers |
| --- | --- |
| App (`tests/model_check/app.rs`) | `src/app.rs`: `App::new`, `App::handle`, `App::next_wake`, `App::status`, `App::chart_load`, `advance`, `issue`. `src/chart.rs`: `ChartFeed::{wanted, due, due_at, begin, awaits, finish}`, `CandleLoad::{due_at, is_loading, failure}`. `src/feed.rs`: `FeedSchedule::{started, finished, woke, next_start, is_due, delay, request_timeout}`, `FeedStatus::derive`. `src/kraken.rs`: `quote_from`, `candles_from` (on the two fixed bodies and `Unreachable`). `src/overlay.rs`: `OverlayState::reduce`, `is_visible`, `mode`. `src/market.rs`: `Zoom::stepped`. `src/settings.rs`: `Settings::clamped`. |
| Reducer (`tests/model_check/overlay.rs`) | `src/overlay.rs`: `OverlayState::{new, reduce, mode, is_visible, drag, settings}`, the `Zoom` event included. `src/market.rs`: `Zoom::stepped`. `src/settings.rs`: `Settings::clamped`. |
| Engine (`tests/model_check/engine.rs`, `words.rs`) | The search, fingerprints and counterexample wording, shared by both checks. |
| Mutants (`windows/scripts/mutants.ps1`) | The rows above. |

**Sync commit: `a0f1dc6`** ("Prove the model checks catch broken zoom"). At this commit, both checks were confirmed to describe the covered code, chart zoom included. All 23 mutants were caught, and `verify.ps1` printed only PASS. To see whether the verification needs another look, run this from the repo root:

```
git log --oneline a0f1dc6..HEAD -- windows/crates/ticker-core/src windows/crates/ticker-core/tests/model_check windows/scripts/mutants.ps1
```

If it lists commits, read their diffs against the coverage record. Update the model, invariants and mutants where behavior changed intentionally. Rerun `cargo test -p ticker-core --test model_check` and `mutants.ps1`. Only then move the sync commit forward to the commit you checked, and update the table of measured numbers.
