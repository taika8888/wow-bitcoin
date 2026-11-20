//! Every interleaving of the inputs the shell can send, up to a depth bound, through the real `App::handle`.
//!
//! Time is abstracted to a few advances: ticks of 1, 4 and 16 s and a wake 1 s later. Results arrive at the clock's
//! current time. Every quote answer for one market carries the same price, so quotes of a market differ only by when
//! they were fetched, and the two markets' prices and candles differ, so the checker can tell whose are on screen.

use ticker_core::{
    App, DragPhase, Effect, FeedSchedule, FeedStatus, Frontmost, HttpOutcome, Input, Market, OverlayEvent, OverlayMode,
    PressTarget, Quote, RequestId, ScreenPoint, Session, Settings, Timeframe, Timestamp, Zoom, ZoomStep,
};

use crate::engine::{Model, Report, explore};
use crate::words::{overlay_event, settings_words};

const T0: Timestamp = Timestamp::from_unix_seconds(1_793_847_300.0);
const DROP: ScreenPoint = ScreenPoint { x: 900.0, y: 600.0 };
const TICKS: [f64; 3] = [1.0, 4.0, 16.0];
const RETRY_AFTER_CANDLE_FAILURE: f64 = 10.0;

/// The price, the reference price and the candle close every answer for the market carries.
const fn answers(market: Market) -> (f64, f64, f64) {
    match market {
        Market::Btc => (80500.0, 80000.0, 11.0),
        Market::Ant => (95.75, 83.0, 96.5),
    }
}

/// A request the shell is still running for a feed the app replaced on a market switch. The shell answers it anyway.
#[derive(Clone, Copy, Debug)]
struct Abandoned {
    request: RequestId,
    market: Market,
    /// None for a quote request.
    timeframe: Option<Timeframe>,
}

/// The app plus the checker's own records, kept from the inputs it sent and the effects it saw, never from the app's
/// internals: which requests are out, what it last delivered, the last good quote, and the quote schedule's anchor.
/// Every record but `abandoned` and the deliveries is about the current market's feed, which a switch starts over.
#[derive(Clone, Debug)]
pub struct World {
    app: App,
    clock: Timestamp,
    feed_started: Timestamp,
    quotes_out: Vec<RequestId>,
    candles_out: [Vec<RequestId>; 3],
    abandoned: Vec<Abandoned>,
    delivered_quote: Option<(RequestId, Market)>,
    delivered_candles: Option<(Timeframe, RequestId, Market)>,
    last_good: Option<Quote>,
    quote_started: Option<Timestamp>,
    quote_failures: u32,
    /// Per timeframe: whether the last finished request succeeded, and when.
    candles_finished: [Option<(bool, Timestamp)>; 3],
}

pub struct Step {
    input: Input,
    /// A result for a request the checker already delivered, or one a market switch abandoned: a duplicate or late
    /// answer.
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

const INVARIANTS: [Invariant; 16] = [
    ("never two quote requests outstanding for the current market", |t| t.after.quotes_out.len() <= 1),
    ("never two candle requests outstanding for one timeframe of the current market", |t| {
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
    ("every request goes to the current market's source", |t| {
        let source = t.after.app.settings().market.source();
        t.effects.iter().all(|effect| match effect {
            Effect::FetchQuote { url, .. } => *url == source.quote_url(),
            Effect::FetchCandles { url, timeframe, .. } => *url == source.candles_url(*timeframe),
            Effect::SaveSettings(_) | Effect::Render => true,
        })
    }),
    ("the shown quote and candles always belong to the current market", |t| {
        let app = &t.after.app;
        let (price, _, close) = answers(app.settings().market);
        let quote_matches = app.status().quote().is_none_or(|quote| quote.price == price);
        let candles_match = Timeframe::ALL.iter().all(|timeframe| {
            app.chart_load(*timeframe)
                .series()
                .is_none_or(|series| series.candles.iter().all(|candle| candle.close == close))
        });
        quote_matches && candles_match
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
    ("the feed status is FeedStatus::derive of the last good quote, the feed's start and the clock", |t| {
        let refresh = f64::from(t.after.app.settings().refresh_seconds);
        t.after.app.status()
            == FeedStatus::derive(t.after.last_good.as_ref(), t.after.feed_started, t.after.clock, refresh)
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

fn other(market: Market) -> Market {
    match market {
        Market::Btc => Market::Ant,
        Market::Ant => Market::Btc,
    }
}

/// When the checker's own record of the schedule says the next quote may start: the first one of a feed at once.
fn next_quote_due(world: &World) -> Option<Timestamp> {
    let refresh = world.app.settings().refresh_seconds;
    Some(
        world
            .quote_started
            .map_or(world.feed_started, |start| start.plus(FeedSchedule::delay(refresh, world.quote_failures))),
    )
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

fn answered(body: String) -> HttpOutcome {
    HttpOutcome::Answered { status: 200, body: body.into_bytes() }
}

/// What the market's source answers to a quote request.
fn quote_body(market: Market) -> HttpOutcome {
    let (price, reference, _) = answers(market);
    answered(match market {
        Market::Btc => format!(r#"{{"error":[],"result":{{"XXBTZUSD":{{"c":["{price}","0.1"],"o":"{reference}"}}}}}}"#),
        Market::Ant => format!(
            r#"{{"chart":{{"result":[{{"meta":{{"regularMarketPrice":{price},"chartPreviousClose":{reference},"regularMarketTime":1795122001,"tradingPeriods":{{"regular":[[{{"start":1795098600,"end":1795122000}}]]}}}}}}],"error":null}}}}"#
        ),
    })
}

/// What the market's source answers to a candle request: one candle.
fn candles_body(market: Market) -> HttpOutcome {
    let (_, _, close) = answers(market);
    answered(match market {
        Market::Btc => {
            format!(
                r#"{{"error":[],"result":{{"XXBTZUSD":[[1793847240,"10","12","9","{close}","10.5","1.0",3]],"last":1}}}}"#
            )
        }
        Market::Ant => format!(
            r#"{{"chart":{{"result":[{{"timestamp":[1793847240],"indicators":{{"quote":[{{"open":[95],"high":[97],"low":[94],"close":[{close}]}}]}}}}],"error":null}}}}"#
        ),
    })
}

/// Which inputs the model sends. `Full` is every kind the brief names; `Feed` drops the pointer details that the
/// reducer's own exhaustive check already covers, and the market switch, which starts the feed over and so adds no
/// deeper fetch history, so it can go deeper into fetch and schedule interleavings.
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
                OverlayEvent::SettingsEdited(Settings { market: other(settings.market), ..*settings }),
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
            feed_started: T0,
            quotes_out: Vec::new(),
            candles_out: Default::default(),
            abandoned: Vec::new(),
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
        let late = |input| Step { input, late: true };
        let settings = world.app.settings();
        let market = settings.market;
        let mut steps: Vec<Step> =
            self.overlay_events(settings).into_iter().map(|event| fresh(Input::Overlay(event))).collect();
        steps.extend(TICKS.map(|seconds| fresh(Input::Tick(world.clock.plus(seconds)))));
        steps.push(fresh(Input::Woke(world.clock.plus(1.0))));
        steps.push(fresh(Input::SettingsChanged(Settings {
            refresh_seconds: 2,
            timeframe: Timeframe::H4,
            ..*settings
        })));
        if self.alphabet == Alphabet::Full {
            steps.push(fresh(Input::SettingsChanged(Settings { market: other(market), ..*settings })));
        }
        let now = world.clock;
        for &request in &world.quotes_out {
            for outcome in [quote_body(market), HttpOutcome::Unreachable] {
                steps.push(fresh(Input::QuoteFinished { request, outcome, now }));
            }
        }
        if let Some((request, market)) = world.delivered_quote {
            steps.push(late(Input::QuoteFinished { request, outcome: quote_body(market), now }));
        }
        for timeframe in Timeframe::ALL {
            for &request in &world.candles_out[index(timeframe)] {
                for outcome in [candles_body(market), HttpOutcome::Unreachable] {
                    steps.push(fresh(Input::CandlesFinished { request, timeframe, outcome, now }));
                }
            }
        }
        if let Some((timeframe, request, market)) = world.delivered_candles {
            steps.push(late(Input::CandlesFinished { request, timeframe, outcome: candles_body(market), now }));
        }
        for &Abandoned { request, market, timeframe } in &world.abandoned {
            steps.push(late(match timeframe {
                None => Input::QuoteFinished { request, outcome: quote_body(market), now },
                Some(timeframe) => Input::CandlesFinished { request, timeframe, outcome: candles_body(market), now },
            }));
        }
        steps
    }

    fn describe(&self, step: &Step) -> String {
        let late = if step.late { "late or abandoned: " } else { "" };
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
        let market = world.app.settings().market;
        match &step.input {
            Input::Tick(now) => after.clock = *now,
            Input::Woke(now) => {
                after.clock = *now;
                after.quote_failures = 0;
            }
            Input::QuoteFinished { request, .. } | Input::CandlesFinished { request, .. } if step.late => {
                after.abandoned.retain(|abandoned| abandoned.request != *request);
            }
            Input::QuoteFinished { request, outcome, .. } => {
                after.quotes_out.retain(|out| out != request);
                after.delivered_quote = Some((*request, market));
                if *outcome == HttpOutcome::Unreachable {
                    after.quote_failures += 1;
                } else {
                    let (price, reference, _) = answers(market);
                    after.quote_failures = 0;
                    after.last_good =
                        Some(Quote { price, reference, session: Session::Regular, fetched_at: after.clock });
                }
            }
            Input::CandlesFinished { request, timeframe, outcome, .. } => {
                after.candles_out[index(*timeframe)].retain(|out| out != request);
                after.delivered_candles = Some((*timeframe, *request, market));
                after.candles_finished[index(*timeframe)] = Some((*outcome != HttpOutcome::Unreachable, after.clock));
            }
            _ => {}
        }
        if after.app.settings().market != market {
            after.abandoned.extend(after.quotes_out.drain(..).map(|request| Abandoned {
                request,
                market,
                timeframe: None,
            }));
            for timeframe in Timeframe::ALL {
                let out = after.candles_out[index(timeframe)].drain(..);
                after.abandoned.extend(out.map(|request| Abandoned { request, market, timeframe: Some(timeframe) }));
            }
            after.feed_started = after.clock;
            after.last_good = None;
            after.quote_started = None;
            after.quote_failures = 0;
            after.candles_finished = [None; 3];
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
    ant_live: bool,
    ant_candles: bool,
    switched_with_requests_out: bool,
    switched_back: bool,
}

fn check(name: &'static str, alphabet: Alphabet, depth: usize) -> (Report, Reached) {
    let mut reached = Reached::default();
    let report = explore(name, &AppModel { alphabet }, Some(depth), |world: &World| {
        let app = &world.app;
        let ant = app.settings().market == Market::Ant;
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
        reached.ant_live |= ant && matches!(app.status(), FeedStatus::Live(_));
        reached.ant_candles |= ant && app.expanded().is_some_and(|(_, load)| load.series().is_some());
        reached.switched_with_requests_out |= world.abandoned.len() >= 2;
        reached.switched_back |= !ant && world.abandoned.iter().any(|abandoned| abandoned.market == Market::Ant);
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
    assert!(reached.ant_live, "a live ANT quote must be reachable, or the market invariant never sees one");
    assert!(reached.switched_with_requests_out, "a switch with a quote and candles out must be reachable");
    assert!(reached.ant_candles, "ANT candles on the chart must be reachable");
    assert!(reached.switched_back, "switching to ANT and back with an ANT request out must be reachable");
}

#[test]
fn every_deeper_interleaving_of_feed_inputs_keeps_the_app_invariants() {
    let (_, reached) = check("app feed", Alphabet::Feed, FEED_DEPTH);
    assert!(reached.backed_off, "two failures in a row must be reachable, or backoff is never checked");
    assert!(reached.candles_failed, "a failed candle fetch must be reachable");
    assert!(reached.two_timeframes_loading, "two timeframes loading at once must be reachable");
}
