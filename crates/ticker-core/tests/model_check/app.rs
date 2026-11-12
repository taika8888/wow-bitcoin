//! Every interleaving of the inputs the shell can send, up to a depth bound, through the real `App::handle`.
//!
//! Time is abstracted to a few advances: ticks of 1, 4 and 16 s and a wake 1 s later. Results arrive at the clock's
//! current time. Every quote answer carries the same price, so quotes differ only by when they were fetched.

use ticker_core::{
    App, DragPhase, Effect, FeedSchedule, FeedStatus, Frontmost, HttpOutcome, Input, OverlayEvent, OverlayMode,
    PressTarget, Quote, RequestId, ScreenPoint, Settings, Timeframe, Timestamp, Zoom, ZoomStep,
};

use crate::engine::{Model, Report, explore};
use crate::words::{overlay_event, settings_words};

const T0: Timestamp = Timestamp::from_unix_seconds(1_793_847_300.0);
const DROP: ScreenPoint = ScreenPoint { x: 900.0, y: 600.0 };
const PRICE: f64 = 80500.0;
const OPEN: f64 = 80000.0;
const TICKS: [f64; 3] = [1.0, 4.0, 16.0];
const RETRY_AFTER_CANDLE_FAILURE: f64 = 10.0;

/// The app plus the checker's own records, kept from the inputs it sent and the effects it saw, never from the app's
/// internals: which requests are out, what it last delivered, the last good quote, and the quote schedule's anchor.
#[derive(Clone, Debug)]
pub struct World {
    app: App,
    clock: Timestamp,
    quotes_out: Vec<RequestId>,
    candles_out: [Vec<RequestId>; 3],
    delivered_quote: Option<RequestId>,
    delivered_candles: Option<(Timeframe, RequestId)>,
    last_good: Option<Quote>,
    quote_started: Option<Timestamp>,
    quote_failures: u32,
    /// Per timeframe: whether the last finished request succeeded, and when.
    candles_finished: [Option<(bool, Timestamp)>; 3],
}

pub struct Step {
    input: Input,
    /// A result for a request the checker already delivered: a duplicate or late answer.
    late: bool,
}

/// What the checker worked out about one transition, beside the two worlds.
struct Transition<'a> {
    before: &'a World,
    step: &'a Step,
    after: &'a World,
    effects: &'a [Effect],
    quote_due: bool,
    candles_due: Option<Timeframe>,
}

type Invariant = (&'static str, fn(&Transition) -> bool);

const INVARIANTS: [Invariant; 14] = [
    ("never two quote requests outstanding", |t| t.after.quotes_out.len() <= 1),
    ("never two candle requests outstanding for one timeframe", |t| {
        t.after.candles_out.iter().all(|out| out.len() <= 1)
    }),
    ("candles are fetched only for the visible expanded timeframe", |t| {
        fetched_candles(t.effects).all(|timeframe| {
            t.after.app.overlay().is_visible() && t.after.app.overlay().mode() == OverlayMode::Expanded(timeframe)
        })
    }),
    ("a quote request starts exactly when one is due and none is out", |t| {
        fetched_quote(t.effects).count() == usize::from(t.quote_due)
    }),
    ("the visible expanded chart's candles are requested exactly when due", |t| {
        fetched_candles(t.effects).collect::<Vec<_>>() == Vec::from_iter(t.candles_due)
    }),
    ("a late or unknown result changes nothing", |t| {
        !t.step.late || (t.after.app == t.before.app && t.effects.is_empty())
    }),
    ("a click never moves the ticker", |t| {
        let Input::Overlay(OverlayEvent::Released { .. }) = t.step.input else { return true };
        t.before.app.overlay().drag() == DragPhase::Dragging || position(t.after) == position(t.before)
    }),
    ("only a drag or a settings edit moves the ticker", |t| match &t.step.input {
        Input::Overlay(OverlayEvent::SettingsEdited(_)) | Input::SettingsChanged(_) => true,
        Input::Overlay(OverlayEvent::Released { .. }) if t.before.app.overlay().drag() == DragPhase::Dragging => true,
        _ => position(t.after) == position(t.before),
    }),
    ("a locked ticker never drags", |t| {
        !t.after.app.settings().locked || t.after.app.overlay().drag() != DragPhase::Dragging
    }),
    ("a hidden overlay is never pressed or dragging", |t| {
        t.after.app.overlay().is_visible() || t.after.app.overlay().drag() == DragPhase::Idle
    }),
    ("SaveSettings is emitted iff an in-app input changed the settings, and carries the new settings", |t| {
        let saved: Vec<&Settings> = t
            .effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::SaveSettings(settings) => Some(settings),
                _ => None,
            })
            .collect();
        let changed = t.after.app.settings() != t.before.app.settings();
        if changed && matches!(t.step.input, Input::Overlay(_)) {
            saved == [t.after.app.settings()]
        } else {
            saved.is_empty()
        }
    }),
    ("the feed status is FeedStatus::derive of the last good quote, the start and the clock", |t| {
        let refresh = f64::from(t.after.app.settings().refresh_seconds);
        t.after.app.status() == FeedStatus::derive(t.after.last_good.as_ref(), T0, t.after.clock, refresh)
    }),
    ("Render is emitted iff the overlay, the feed status, the shown text or a chart load changed", |t| {
        let (before, after) = (&t.before.app, &t.after.app);
        let changed = before.overlay() != after.overlay()
            || before.status() != after.status()
            || before.display() != after.display()
            || before.tooltip() != after.tooltip()
            || Timeframe::ALL.iter().any(|timeframe| before.chart_load(*timeframe) != after.chart_load(*timeframe));
        t.effects.contains(&Effect::Render) == changed
    }),
    ("nothing is overdue, and with no quote out the next wake is no later than the next quote", |t| {
        let wake = t.after.app.next_wake();
        let in_future = wake.is_none_or(|wake| wake > t.after.clock);
        let quote_due = next_quote_due(t.after);
        let soon_enough = !t.after.quotes_out.is_empty() || wake.zip(quote_due).is_some_and(|(wake, due)| wake <= due);
        in_future && soon_enough
    }),
];

fn fetched_quote(effects: &[Effect]) -> impl Iterator<Item = RequestId> + '_ {
    effects.iter().filter_map(|effect| match effect {
        Effect::FetchQuote { request, .. } => Some(*request),
        _ => None,
    })
}

fn fetched_candles(effects: &[Effect]) -> impl Iterator<Item = Timeframe> + '_ {
    effects.iter().filter_map(|effect| match effect {
        Effect::FetchCandles { timeframe, .. } => Some(*timeframe),
        _ => None,
    })
}

fn position(world: &World) -> Option<ScreenPoint> {
    world.app.settings().position
}

fn index(timeframe: Timeframe) -> usize {
    Timeframe::ALL.iter().position(|candidate| *candidate == timeframe).unwrap_or_default()
}

/// When the checker's own record of the schedule says the next quote may start: the first one at once.
fn next_quote_due(world: &World) -> Option<Timestamp> {
    let refresh = world.app.settings().refresh_seconds;
    Some(world.quote_started.map_or(T0, |start| start.plus(FeedSchedule::delay(refresh, world.quote_failures))))
}

fn candles_due(world: &World) -> Option<Timeframe> {
    let overlay = world.app.overlay();
    let OverlayMode::Expanded(timeframe) = overlay.mode() else { return None };
    if !overlay.is_visible() || !world.candles_out[index(timeframe)].is_empty() {
        return None;
    }
    let due = match world.candles_finished[index(timeframe)] {
        None => true,
        Some((true, at)) => world.clock >= at.plus(f64::from(timeframe.spec().refresh_seconds)),
        Some((false, at)) => world.clock >= at.plus(RETRY_AFTER_CANDLE_FAILURE),
    };
    due.then_some(timeframe)
}

fn quote_body() -> HttpOutcome {
    let body = format!(r#"{{"error":[],"result":{{"XXBTZUSD":{{"c":["{PRICE}","0.1"],"o":"{OPEN}"}}}}}}"#);
    HttpOutcome::Answered { status: 200, body: body.into_bytes() }
}

fn candles_body() -> HttpOutcome {
    let body = r#"{"error":[],"result":{"XXBTZUSD":[[1793847240,"10","12","9","11","10.5","1.0",3]],"last":1}}"#;
    HttpOutcome::Answered { status: 200, body: body.as_bytes().to_vec() }
}

/// Which inputs the model sends. `Full` is every kind the brief names; `Feed` drops the pointer details that the
/// reducer's own exhaustive check already covers, so it can go deeper into fetch and schedule interleavings.
#[derive(Clone, Copy, PartialEq)]
enum Alphabet {
    Full,
    Feed,
}

struct AppModel {
    alphabet: Alphabet,
}

impl AppModel {
    fn overlay_events(&self, settings: &Settings) -> Vec<OverlayEvent> {
        let release = |over| OverlayEvent::Released { over, ticker_origin: DROP };
        let mut events =
            vec![OverlayEvent::Pressed, release(PressTarget::Body), release(PressTarget::Timeframe(Timeframe::M1))];
        if self.alphabet == Alphabet::Full {
            events.extend([
                OverlayEvent::PointerMoved { distance: 2.0 },
                OverlayEvent::PointerMoved { distance: 50.0 },
                release(PressTarget::Outside),
                release(PressTarget::Timeframe(Timeframe::H4)),
                OverlayEvent::ToggleLock,
                OverlayEvent::Zoom(ZoomStep::In),
                OverlayEvent::Zoom(ZoomStep::Out),
                OverlayEvent::AppActivated(Frontmost::Ticker),
            ]);
        }
        events.extend([Frontmost::Wow, Frontmost::Other].map(OverlayEvent::AppActivated));
        events.extend([
            OverlayEvent::SettingsEdited(Settings { refresh_seconds: 2, ..*settings }),
            OverlayEvent::SettingsEdited(Settings { refresh_seconds: 30, ..*settings }),
            OverlayEvent::SettingsEdited(Settings {
                only_when_wow_active: !settings.only_when_wow_active,
                ..*settings
            }),
        ]);
        events
    }
}

impl Model for AppModel {
    type World = World;
    type Step = Step;

    fn initial(&self) -> Vec<World> {
        vec![World {
            app: App::new(Settings::default(), Frontmost::Wow, T0),
            clock: T0,
            quotes_out: Vec::new(),
            candles_out: Default::default(),
            delivered_quote: None,
            delivered_candles: None,
            last_good: None,
            quote_started: None,
            quote_failures: 0,
            candles_finished: [None; 3],
        }]
    }

    fn steps(&self, world: &World) -> Vec<Step> {
        let fresh = |input| Step { input, late: false };
        let settings = world.app.settings();
        let mut steps: Vec<Step> =
            self.overlay_events(settings).into_iter().map(|event| fresh(Input::Overlay(event))).collect();
        steps.extend(TICKS.map(|seconds| fresh(Input::Tick(world.clock.plus(seconds)))));
        steps.push(fresh(Input::Woke(world.clock.plus(1.0))));
        steps.push(fresh(Input::SettingsChanged(Settings {
            refresh_seconds: 2,
            timeframe: Timeframe::H4,
            ..*settings
        })));
        let now = world.clock;
        for &request in &world.quotes_out {
            for outcome in [quote_body(), HttpOutcome::Unreachable] {
                steps.push(fresh(Input::QuoteFinished { request, outcome, now }));
            }
        }
        if let Some(request) = world.delivered_quote {
            steps.push(Step { input: Input::QuoteFinished { request, outcome: quote_body(), now }, late: true });
        }
        for timeframe in Timeframe::ALL {
            for &request in &world.candles_out[index(timeframe)] {
                for outcome in [candles_body(), HttpOutcome::Unreachable] {
                    steps.push(fresh(Input::CandlesFinished { request, timeframe, outcome, now }));
                }
            }
        }
        if let Some((timeframe, request)) = world.delivered_candles {
            let input = Input::CandlesFinished { request, timeframe, outcome: candles_body(), now };
            steps.push(Step { input, late: true });
        }
        steps
    }

    fn describe(&self, step: &Step) -> String {
        let late = if step.late { "late duplicate: " } else { "" };
        let result = |outcome: &HttpOutcome| if *outcome == HttpOutcome::Unreachable { "fails" } else { "succeeds" };
        let text = match &step.input {
            Input::Overlay(event) => overlay_event(event),
            Input::Tick(now) => format!("tick at t+{}s", now.seconds_since(T0)),
            Input::Woke(now) => format!("wake from sleep at t+{}s", now.seconds_since(T0)),
            Input::SettingsChanged(settings) => format!("the settings file changes to {}", settings_words(settings)),
            Input::QuoteFinished { request, outcome, .. } => format!("quote request {request} {}", result(outcome)),
            Input::CandlesFinished { request, timeframe, outcome, .. } => {
                format!("{} candle request {request} {}", timeframe.spec().label, result(outcome))
            }
        };
        format!("{late}{text}")
    }

    fn describe_initial(&self, world: &World) -> String {
        format!("a fresh app at t+0s, WoW in the foreground, {}", settings_words(world.app.settings()))
    }

    fn apply(&self, world: &World, step: &Step) -> (World, Vec<&'static str>) {
        let mut after = world.clone();
        let effects = after.app.handle(step.input.clone());
        match &step.input {
            Input::Tick(now) => after.clock = *now,
            Input::Woke(now) => {
                after.clock = *now;
                after.quote_failures = 0;
            }
            Input::QuoteFinished { request, outcome, .. } if !step.late => {
                after.quotes_out.retain(|out| out != request);
                after.delivered_quote = Some(*request);
                if *outcome == HttpOutcome::Unreachable {
                    after.quote_failures += 1;
                } else {
                    after.quote_failures = 0;
                    after.last_good = Some(Quote { price: PRICE, open_24h: OPEN, fetched_at: after.clock });
                }
            }
            Input::CandlesFinished { request, timeframe, outcome, .. } if !step.late => {
                after.candles_out[index(*timeframe)].retain(|out| out != request);
                after.delivered_candles = Some((*timeframe, *request));
                after.candles_finished[index(*timeframe)] = Some((*outcome != HttpOutcome::Unreachable, after.clock));
            }
            _ => {}
        }
        let quote_due = after.quotes_out.is_empty() && next_quote_due(&after).is_none_or(|due| after.clock >= due);
        let candles_due = candles_due(&after);
        for effect in &effects {
            match effect {
                Effect::FetchQuote { request, .. } => {
                    after.quotes_out.push(*request);
                    after.quote_started = Some(after.clock);
                }
                Effect::FetchCandles { request, timeframe, .. } => after.candles_out[index(*timeframe)].push(*request),
                Effect::SaveSettings(_) | Effect::Render => {}
            }
        }
        let transition = Transition { before: world, step, after: &after, effects: &effects, quote_due, candles_due };
        let broken = INVARIANTS.iter().filter(|(_, holds)| !holds(&transition)).map(|(name, _)| *name).collect();
        (after, broken)
    }
}

/// Reachability facts that keep the invariants from passing vacuously.
#[derive(Default)]
struct Reached {
    quote_out_and_hidden: bool,
    two_timeframes_loading: bool,
    stale: bool,
    stale_for_a_minute: bool,
    offline: bool,
    backed_off: bool,
    dragging: bool,
    candles_failed: bool,
    zoomed_while_loading: bool,
}

fn check(name: &'static str, alphabet: Alphabet, depth: usize) -> (Report, Reached) {
    let mut reached = Reached::default();
    let report = explore(name, &AppModel { alphabet }, Some(depth), |world: &World| {
        let app = &world.app;
        reached.quote_out_and_hidden |= !world.quotes_out.is_empty() && !app.overlay().is_visible();
        reached.two_timeframes_loading |=
            Timeframe::ALL.iter().filter(|t| app.chart_load(**t).is_loading()).count() >= 2;
        reached.stale |= matches!(app.status(), FeedStatus::Stale(_));
        reached.stale_for_a_minute |=
            matches!(app.status(), FeedStatus::Stale(quote) if world.clock.seconds_since(quote.fetched_at) >= 60.0);
        reached.offline |= app.status() == FeedStatus::Offline;
        reached.backed_off |= world.quote_failures >= 2;
        reached.dragging |= app.overlay().drag() == DragPhase::Dragging;
        reached.candles_failed |= Timeframe::ALL.iter().any(|t| app.chart_load(*t).failure().is_some());
        reached.zoomed_while_loading |= app.settings().zoom != Zoom::Standard
            && app.expanded().is_some_and(|(_, load)| load.is_loading() || load.series().is_some());
    });
    println!("{} ({} invariants)", report.summary(), INVARIANTS.len());
    assert!(report.violations.is_empty(), "{}", report.failures());
    (report, reached)
}

pub const FULL_DEPTH: usize = 7;
pub const FEED_DEPTH: usize = 8;

#[test]
fn every_interleaving_of_every_input_keeps_the_app_invariants() {
    let (_, reached) = check("app", Alphabet::Full, FULL_DEPTH);
    assert!(reached.dragging, "dragging must be reachable, or the drag invariants are vacuous");
    assert!(reached.quote_out_and_hidden, "a quote out while hidden must be reachable");
    assert!(reached.two_timeframes_loading, "two timeframes loading at once must be reachable");
    assert!(reached.stale, "a stale quote must be reachable, or the status invariant never sees one");
    assert!(reached.offline, "offline must be reachable");
    assert!(
        reached.stale_for_a_minute,
        "a quote stale for a minute must be reachable, or the age never turns to minutes"
    );
    assert!(
        reached.zoomed_while_loading,
        "a zoomed chart with candles out or loaded must be reachable, or zooming is never checked against fetches"
    );
}

#[test]
fn every_deeper_interleaving_of_feed_inputs_keeps_the_app_invariants() {
    let (_, reached) = check("app feed", Alphabet::Feed, FEED_DEPTH);
    assert!(reached.backed_off, "two failures in a row must be reachable, or backoff is never checked");
    assert!(reached.candles_failed, "a failed candle fetch must be reachable");
    assert!(reached.two_timeframes_loading, "two timeframes loading at once must be reachable");
}
