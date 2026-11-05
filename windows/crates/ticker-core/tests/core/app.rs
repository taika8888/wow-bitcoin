//! The app driven the way the shell drives it: inputs in, effects out, state read back through the accessors.

use std::collections::BTreeMap;
use std::time::Duration;

use ticker_core::kraken::{self, CANDLES_TIMEOUT, TICKER_URL};
use ticker_core::{
    App, CandleLoad, ChartNotice, Effect, FeedStatus, FetchFailure, Frontmost, HttpOutcome, Input, KrakenError,
    OverlayEvent, OverlayMode, PressTarget, Quote, Rect, RequestId, ScreenPoint, Settings, Snapshot, SnapshotWindow,
    Timeframe, Timestamp,
};

use crate::chart::T0;
use crate::fixtures;

const DROP: ScreenPoint = ScreenPoint { x: 900.0, y: 600.0 };

fn answered(body: &str) -> HttpOutcome {
    HttpOutcome::Answered { status: 200, body: body.as_bytes().to_vec() }
}

fn price_body(price: f64) -> HttpOutcome {
    answered(&format!(r#"{{"error":[],"result":{{"XXBTZUSD":{{"c":["{price}","0.1"],"o":"80000"}}}}}}"#))
}

fn candles_body() -> HttpOutcome {
    answered(r#"{"error":[],"result":{"XXBTZUSD":[[1793847240,"10","12","9","11","10.5","1.0",3]],"last":1}}"#)
}

fn empty_candles_body() -> HttpOutcome {
    answered(r#"{"error":[],"result":{"XXBTZUSD":[],"last":1}}"#)
}

fn quote_request(effects: &[Effect]) -> Option<RequestId> {
    effects.iter().find_map(|effect| match effect {
        Effect::FetchQuote { request, .. } => Some(*request),
        _ => None,
    })
}

fn candle_request(effects: &[Effect]) -> Option<(Timeframe, RequestId)> {
    effects.iter().find_map(|effect| match effect {
        Effect::FetchCandles { timeframe, request, .. } => Some((*timeframe, *request)),
        _ => None,
    })
}

fn saved(effects: &[Effect]) -> Vec<Settings> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SaveSettings(settings) => Some(*settings),
            _ => None,
        })
        .collect()
}

fn renders(effects: &[Effect]) -> bool {
    effects.contains(&Effect::Render)
}

fn at(seconds: f64) -> Timestamp {
    T0.plus(seconds)
}

/// The shell's side of the conversation, recording requests the app has asked for and not yet heard back about.
struct Shell {
    app: App,
    quote: Option<RequestId>,
    candles: Option<(Timeframe, RequestId)>,
}

impl Shell {
    fn new(settings: Settings) -> Self {
        Self { app: App::new(settings, Frontmost::Wow, T0), quote: None, candles: None }
    }

    fn send(&mut self, input: Input) -> Vec<Effect> {
        let effects = self.app.handle(input);
        self.quote = quote_request(&effects).or(self.quote);
        self.candles = candle_request(&effects).or(self.candles);
        effects
    }

    fn tick(&mut self, seconds: f64) -> Vec<Effect> {
        self.send(Input::Tick(at(seconds)))
    }

    fn overlay(&mut self, event: OverlayEvent) -> Vec<Effect> {
        self.send(Input::Overlay(event))
    }

    fn click(&mut self, over: PressTarget) -> Vec<Effect> {
        self.overlay(OverlayEvent::Pressed);
        self.overlay(OverlayEvent::Released { over, ticker_origin: DROP })
    }

    fn finish_quote(&mut self, outcome: HttpOutcome, seconds: f64) -> Vec<Effect> {
        let request = self.quote.take().expect("a quote request is out");
        self.send(Input::QuoteFinished { request, outcome, now: at(seconds) })
    }

    fn finish_candles(&mut self, outcome: HttpOutcome, seconds: f64) -> Vec<Effect> {
        let (timeframe, request) = self.candles.take().expect("a candle request is out");
        self.send(Input::CandlesFinished { request, timeframe, outcome, now: at(seconds) })
    }

    fn load(&self, timeframe: Timeframe) -> &CandleLoad {
        self.app.chart_load(timeframe)
    }
}

/// Expanded on `timeframe` at T0, with the first quote and the first candles answered.
fn expanded_on(timeframe: Timeframe) -> Shell {
    let mut shell = Shell::new(Settings { timeframe, ..Settings::default() });
    shell.tick(0.0);
    shell.finish_quote(price_body(80000.0), 0.0);
    shell.click(PressTarget::Body);
    shell
}

#[test]
fn the_first_tick_asks_for_a_quote_with_a_timeout_no_longer_than_the_interval() {
    let mut app = App::new(Settings { refresh_seconds: 2, ..Settings::default() }, Frontmost::Wow, T0);
    assert_eq!(app.next_wake(), Some(T0), "a fresh app wants a tick at once");
    let effects = app.handle(Input::Tick(T0));
    let request = quote_request(&effects).unwrap();
    assert_eq!(effects[0], Effect::FetchQuote { request, url: TICKER_URL, timeout: Duration::from_secs(2) });
    assert_eq!(candle_request(&effects), None, "nothing is fetched for a collapsed chart");
}

#[test]
fn a_quote_request_waits_for_its_answer_however_long_it_takes() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    for seconds in [5.0, 30.0, 600.0] {
        assert_eq!(quote_request(&shell.tick(seconds)), None, "{seconds}s in, the first request is still out");
    }
}

#[test]
fn the_cadence_runs_from_each_start_not_from_each_answer() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    shell.finish_quote(price_body(80000.0), 3.0);
    assert_eq!(shell.app.next_wake(), Some(at(5.0)));
    assert_eq!(quote_request(&shell.tick(4.9)), None);
    assert!(quote_request(&shell.tick(5.0)).is_some());
}

#[test]
fn a_failed_quote_retries_at_the_cadence_once_then_backs_off() {
    let mut shell = Shell::new(Settings::default());
    let mut starts = Vec::new();
    let mut clock = 0.0;
    while starts.len() < 5 {
        if quote_request(&shell.tick(clock)).is_some() {
            starts.push(clock);
            shell.finish_quote(HttpOutcome::Unreachable, clock);
        }
        clock += 1.0;
    }
    assert_eq!(starts, [0.0, 5.0, 5.0 + 10.0, 15.0 + 20.0, 35.0 + 40.0], "waits of 5, 10, 20 and 40 s from each start");
}

#[test]
fn waking_from_sleep_drops_the_backoff() {
    let mut shell = Shell::new(Settings::default());
    for clock in [0.0, 5.0, 15.0] {
        shell.tick(clock);
        shell.finish_quote(HttpOutcome::Unreachable, clock);
    }
    assert_eq!(shell.app.next_wake(), Some(at(35.0)), "three failures wait 20 s");
    assert_eq!(quote_request(&shell.send(Input::Woke(at(17.0)))), None);
    assert_eq!(shell.app.next_wake(), Some(at(20.0)), "after waking the next try keeps the normal cadence");
    assert!(quote_request(&shell.send(Input::Woke(at(20.0)))).is_some(), "a wake past the due time fetches at once");
}

#[test]
fn a_shorter_interval_applies_to_the_wait_already_running() {
    let mut shell = Shell::new(Settings { refresh_seconds: 300, ..Settings::default() });
    shell.tick(0.0);
    shell.finish_quote(price_body(80000.0), 0.0);
    shell.tick(10.0);
    let shorter = Settings { refresh_seconds: 2, ..*shell.app.settings() };
    let effects = shell.overlay(OverlayEvent::SettingsEdited(shorter));
    assert!(quote_request(&effects).is_some(), "10 s since the last start is past the new 2 s interval");
    assert_eq!(saved(&effects), [shorter]);
}

#[test]
fn a_shorter_interval_from_the_settings_file_also_applies_at_once_and_is_not_written_back() {
    let mut shell = Shell::new(Settings { refresh_seconds: 300, ..Settings::default() });
    shell.tick(0.0);
    shell.finish_quote(price_body(80000.0), 0.0);
    shell.tick(10.0);
    let from_disk = Settings { refresh_seconds: 2, ..*shell.app.settings() };
    let effects = shell.send(Input::SettingsChanged(from_disk));
    assert!(quote_request(&effects).is_some());
    assert_eq!(saved(&effects), []);
    assert_eq!(shell.app.settings(), &from_disk);
}

#[test]
fn a_late_or_unknown_quote_result_is_ignored() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    let first = shell.quote.unwrap();
    shell.finish_quote(price_body(80000.0), 0.0);
    shell.tick(5.0);
    let before = shell.app.clone();
    let late = shell.app.handle(Input::QuoteFinished { request: first, outcome: price_body(1.0), now: at(6.0) });
    assert_eq!(late, []);
    assert_eq!(shell.app, before);
    shell.finish_quote(price_body(90000.0), 6.0);
    assert_eq!(shell.app.status().quote().map(|quote| quote.price), Some(90000.0));
}

#[test]
fn the_quote_turns_stale_on_the_tick_that_crosses_three_missed_refreshes() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    let effects = shell.finish_quote(price_body(80482.4), 0.0);
    assert!(renders(&effects));
    let quote = Quote { price: 80482.4, open_24h: 80000.0, fetched_at: T0 };
    assert_eq!(shell.app.status(), FeedStatus::Live(quote));
    shell.tick(5.0);
    assert!(!renders(&shell.tick(15.0)), "nothing on screen changes at exactly the threshold");
    assert!(renders(&shell.tick(15.5)));
    assert_eq!(shell.app.status(), FeedStatus::Stale(quote));
    assert_eq!(shell.app.display().text(), "BTC $80,482  +0.60%  stale");
}

#[test]
fn without_any_quote_the_ticker_goes_from_loading_to_offline() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    assert_eq!(shell.app.display().text(), "BTC ...");
    shell.finish_quote(HttpOutcome::Answered { status: 503, body: Vec::new() }, 1.0);
    shell.tick(16.0);
    assert_eq!(shell.app.status(), FeedStatus::Offline);
    assert_eq!(shell.app.display().text(), "BTC offline");
}

#[test]
fn an_idle_tick_changes_nothing_and_asks_for_nothing() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    shell.finish_quote(price_body(80000.0), 0.0);
    let before = shell.app.clone();
    assert_eq!(shell.tick(1.0), []);
    assert_eq!(shell.app.overlay(), before.overlay());
    assert_eq!(shell.app.status(), before.status());
}

#[test]
fn the_next_wake_is_the_soonest_of_the_next_quote_the_status_turning_and_the_chart_refresh() {
    let mut shell = Shell::new(Settings { refresh_seconds: 300, timeframe: Timeframe::M1, ..Settings::default() });
    shell.tick(0.0);
    assert_eq!(shell.app.next_wake(), Some(at(900.0)), "with the quote out, only going offline is left");
    shell.finish_quote(price_body(80000.0), 0.0);
    assert_eq!(shell.app.next_wake(), Some(at(300.0)), "the next quote");
    shell.click(PressTarget::Body);
    shell.finish_candles(candles_body(), 0.0);
    assert_eq!(shell.app.next_wake(), Some(at(15.0)), "the 1m chart's refresh");
}

#[test]
fn settings_are_saved_only_when_an_in_app_input_changes_them() {
    let mut shell = Shell::new(Settings::default());
    shell.tick(0.0);
    assert_eq!(
        saved(&shell.overlay(OverlayEvent::SettingsEdited(*shell.app.settings()))),
        [],
        "an edit to the same values"
    );
    assert_eq!(saved(&shell.click(PressTarget::Body)), [], "a click expands but changes no setting");
    let locked = shell.overlay(OverlayEvent::ToggleLock);
    assert_eq!(saved(&locked), [Settings { locked: true, ..Settings::default() }]);
    shell.overlay(OverlayEvent::ToggleLock);
    shell.overlay(OverlayEvent::Pressed);
    shell.overlay(OverlayEvent::PointerMoved { distance: 40.0 });
    let dropped = shell.overlay(OverlayEvent::Released { over: PressTarget::Body, ticker_origin: DROP });
    assert_eq!(saved(&dropped).iter().map(|settings| settings.position).collect::<Vec<_>>(), [Some(DROP)]);
    let chosen = shell.click(PressTarget::Timeframe(Timeframe::H4));
    assert_eq!(saved(&chosen).iter().map(|settings| settings.timeframe).collect::<Vec<_>>(), [Timeframe::H4]);
}

#[test]
fn out_of_range_settings_from_the_shell_are_clamped_before_use() {
    let mut app =
        App::new(Settings { refresh_seconds: 0, font_size: 1000.0, ..Settings::default() }, Frontmost::Wow, T0);
    assert_eq!((app.settings().refresh_seconds, app.settings().font_size), (2, 72.0));
    let effects =
        app.handle(Input::Overlay(OverlayEvent::SettingsEdited(Settings { opacity: 7.0, ..Settings::default() })));
    assert_eq!(saved(&effects).iter().map(|settings| settings.opacity).collect::<Vec<_>>(), [1.0]);
}

#[test]
fn fetches_no_candles_while_collapsed_or_hidden() {
    let mut shell = Shell::new(Settings { only_when_wow_active: true, ..Settings::default() });
    assert_eq!(candle_request(&shell.tick(0.0)), None, "collapsed");
    let effects = shell.click(PressTarget::Body);
    assert_eq!(candle_request(&effects).map(|(timeframe, _)| timeframe), Some(Timeframe::M15));
    shell.finish_candles(candles_body(), 1.0);
    shell.overlay(OverlayEvent::AppActivated(Frontmost::Other));
    assert!(!shell.app.overlay().is_visible());
    assert_eq!(candle_request(&shell.tick(600.0)), None, "hidden");
    let shown = shell.overlay(OverlayEvent::AppActivated(Frontmost::Wow));
    assert_eq!(candle_request(&shown).map(|(timeframe, _)| timeframe), Some(Timeframe::M15), "shown again and overdue");
}

#[test]
fn never_overlaps_requests_for_one_timeframe() {
    let mut shell = expanded_on(Timeframe::M1);
    assert_eq!(shell.candles.map(|(timeframe, _)| timeframe), Some(Timeframe::M1));
    assert_eq!(candle_request(&shell.tick(600.0)), None);
    assert!(shell.load(Timeframe::M1).is_loading() && shell.load(Timeframe::M1).series().is_none());
    assert_eq!(shell.load(Timeframe::M1).name(), "loading");
}

#[test]
fn refetches_only_once_the_refresh_interval_passes() {
    for timeframe in Timeframe::ALL {
        let mut shell = expanded_on(timeframe);
        let refresh = f64::from(timeframe.spec().refresh_seconds);
        shell.finish_candles(candles_body(), 0.0);
        let series = shell.load(timeframe).series().unwrap().clone();
        assert_eq!((series.candles.len(), series.fetched_at), (1, T0));
        assert_eq!(shell.load(timeframe).notice(timeframe), None);
        let mut clock = 1.0;
        while clock < refresh {
            assert_eq!(candle_request(&shell.tick(clock)), None, "{timeframe:?} at {clock}s");
            if let Some(quote) = shell.quote {
                shell.app.handle(Input::QuoteFinished { request: quote, outcome: price_body(80000.0), now: at(clock) });
                shell.quote = None;
            }
            clock += 1.0;
        }
        assert_eq!(candle_request(&shell.tick(refresh)).map(|(timeframe, _)| timeframe), Some(timeframe));
        assert_eq!(shell.load(timeframe).series(), Some(&series), "a refresh keeps showing the last candles");
    }
}

#[test]
fn switching_back_refetches_only_when_stale() {
    let mut shell = expanded_on(Timeframe::M1);
    shell.finish_candles(candles_body(), 0.0);
    shell.tick(1.0);
    let h4 = shell.click(PressTarget::Timeframe(Timeframe::H4));
    assert_eq!(candle_request(&h4).map(|(timeframe, _)| timeframe), Some(Timeframe::H4));
    shell.finish_candles(candles_body(), 2.0);
    shell.tick(5.0);
    assert_eq!(candle_request(&shell.click(PressTarget::Timeframe(Timeframe::M1))), None, "1m is 5 s old");
    shell.click(PressTarget::Timeframe(Timeframe::H4));
    shell.tick(20.0);
    let back = shell.click(PressTarget::Timeframe(Timeframe::M1));
    assert_eq!(candle_request(&back).map(|(timeframe, _)| timeframe), Some(Timeframe::M1), "1m is 20 s old");
}

#[test]
fn a_failure_keeps_the_last_candles_and_retries() {
    let mut shell = expanded_on(Timeframe::H4);
    shell.finish_candles(candles_body(), 0.0);
    let candles = shell.load(Timeframe::H4).series().unwrap().candles.clone();
    shell.tick(300.0);
    shell.finish_candles(HttpOutcome::Unreachable, 300.0);
    assert_eq!(shell.load(Timeframe::H4).name(), "failed");
    assert_eq!(shell.load(Timeframe::H4).series().map(|series| &series.candles), Some(&candles));
    let retry = 300.0 + CandleLoad::RETRY_AFTER_FAILURE_SECONDS;
    assert_eq!(candle_request(&shell.tick(retry - 1.0)), None);
    assert_eq!(candle_request(&shell.tick(retry)).map(|(timeframe, _)| timeframe), Some(Timeframe::H4));
}

#[test]
fn a_candle_result_nobody_waits_for_is_ignored() {
    let mut shell = expanded_on(Timeframe::M15);
    let (_, first) = shell.candles.unwrap();
    shell.finish_candles(candles_body(), 1.0);
    let before = shell.app.clone();
    let replay =
        Input::CandlesFinished { request: first, timeframe: Timeframe::M15, outcome: candles_body(), now: at(2.0) };
    assert_eq!(shell.app.handle(replay), []);
    assert_eq!(shell.app, before);
    shell.tick(61.0);
    let (_, second) = shell.candles.unwrap();
    let wrong_timeframe =
        Input::CandlesFinished { request: second, timeframe: Timeframe::M1, outcome: candles_body(), now: at(62.0) };
    assert_eq!(shell.app.handle(wrong_timeframe), [], "the right id for another timeframe");
    assert!(shell.load(Timeframe::M15).is_loading());
}

#[test]
fn old_candles_are_marked_stale_from_a_failed_refresh_until_one_succeeds() {
    let mut shell = expanded_on(Timeframe::M1);
    shell.finish_candles(candles_body(), 0.0);
    assert_eq!(shell.load(Timeframe::M1).notice(Timeframe::M1), None);
    shell.tick(60.0);
    shell.finish_candles(HttpOutcome::Unreachable, 60.0);
    let stale = Some(ChartNotice::Stale("Stale: Kraken is not answering".to_string()));
    assert_eq!(shell.load(Timeframe::M1).notice(Timeframe::M1), stale);
    shell.tick(70.0);
    assert!(shell.load(Timeframe::M1).is_loading());
    assert_eq!(shell.load(Timeframe::M1).notice(Timeframe::M1), stale, "still stale while the retry is out");
    shell.finish_candles(candles_body(), 71.0);
    assert_eq!(shell.load(Timeframe::M1).notice(Timeframe::M1), None);
}

#[test]
fn the_placeholder_names_what_went_wrong() {
    for (outcome, message) in [
        (HttpOutcome::Unreachable, "Kraken is not answering"),
        (HttpOutcome::Answered { status: 503, body: Vec::new() }, "Kraken answered HTTP 503"),
        (answered(r#"{"error":[],"result":{"XXBTZUSD":[[1,"10","12"]]}}"#), "Kraken sent candles this app cannot read"),
        (
            answered(r#"{"error":[],"result":{"XXBTZUSD":[[1,"10","9","12","11"]]}}"#),
            "Kraken sent candles this app cannot read",
        ),
        (answered(r#"{"error":["EGeneral:Too many requests"]}"#), "Kraken refused: EGeneral:Too many requests"),
    ] {
        let mut shell = expanded_on(Timeframe::H4);
        shell.finish_candles(outcome, 0.0);
        let notice = shell.load(Timeframe::H4).notice(Timeframe::H4);
        assert_eq!(notice, Some(ChartNotice::Placeholder(format!("{message}, retrying"))));
    }
    assert_eq!(
        FetchFailure::Rejected(KrakenError::InvalidValue("row 3".to_string())).message(),
        "Kraken sent candles this app cannot read"
    );
}

#[test]
fn with_nothing_to_draw_the_placeholder_says_why() {
    let mut shell = expanded_on(Timeframe::M15);
    assert_eq!(
        shell.load(Timeframe::M15).notice(Timeframe::M15),
        Some(ChartNotice::Placeholder("Loading 15m candles".into()))
    );
    assert_eq!(shell.app.chart(), None);
    shell.finish_candles(empty_candles_body(), 0.0);
    assert_eq!(
        shell.load(Timeframe::M15).notice(Timeframe::M15),
        Some(ChartNotice::Placeholder("No 15m candles".into()))
    );
}

#[test]
fn the_expanded_chart_reads_back_the_recorded_candles() {
    let mut shell = expanded_on(Timeframe::M15);
    assert_eq!(
        shell.app.expanded().map(|(timeframe, load)| (timeframe, load.name())),
        Some((Timeframe::M15, "loading"))
    );
    shell.finish_candles(answered(fixtures::OHLC_15), 0.0);
    assert_eq!(shell.app.chart().map(|chart| chart.bars.len()), Some(96));
    shell.click(PressTarget::Body);
    assert_eq!(shell.app.overlay().mode(), OverlayMode::Collapsed);
    assert_eq!((shell.app.expanded(), shell.app.chart()), (None, None));
}

#[test]
fn the_snapshot_describes_what_is_on_screen_with_sorted_keys() {
    let mut shell = expanded_on(Timeframe::M15);
    shell.finish_candles(answered(fixtures::OHLC_15), 0.0);
    let frame = Rect::new(-1200.0, 40.0, 376.0, 215.0);
    let targets = BTreeMap::from([("ticker".to_string(), Rect::new(-1200.0, 40.0, 182.0, 27.0))]);
    let window = SnapshotWindow { hwnd: 0x40a2c, frame, targets, activations: 0, foreign_presses: 0 };
    let snapshot = Snapshot::new(&shell.app, window);
    assert_eq!(snapshot.text, "BTC $80,000  0.00%");
    assert_eq!(
        (snapshot.feed, snapshot.mode, snapshot.timeframe, snapshot.chart),
        ("live", "expanded", Some("15m"), Some("loaded"))
    );
    assert_eq!((snapshot.candles, snapshot.fetched_at), (96, Some(T0.unix_seconds())));
    let json = snapshot.json();
    assert!(json.starts_with(r#"{"activations":0,"candles":96,"chart":"loaded","feed":"live","fetchedAt":"#), "{json}");
    assert!(json.contains(r#""frame":{"height":215.0,"width":376.0,"x":-1200.0,"y":40.0},"hwnd":264748,"#), "{json}");
    shell.click(PressTarget::Body);
    let collapsed = Snapshot::new(&shell.app, SnapshotWindow::default());
    assert_eq!((collapsed.timeframe, collapsed.chart, collapsed.candles), (None, None, 0));
    assert!(!collapsed.json().contains("timeframe"));
}

#[test]
fn the_kraken_urls_in_fetch_effects_name_the_timeframe() {
    let mut shell = Shell::new(Settings { timeframe: Timeframe::H4, ..Settings::default() });
    shell.tick(0.0);
    let effects = shell.click(PressTarget::Body);
    let (_, request) = candle_request(&effects).unwrap();
    assert!(effects.contains(&Effect::FetchCandles {
        request,
        timeframe: Timeframe::H4,
        url: kraken::ohlc_url(Timeframe::H4),
        timeout: CANDLES_TIMEOUT
    }));
}
