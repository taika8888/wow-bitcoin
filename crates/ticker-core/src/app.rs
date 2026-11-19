use std::fmt;
use std::time::Duration;

use crate::chart::{CandleChart, CandleLoad, ChartFeed};
use crate::display::TickerDisplay;
use crate::feed::{FeedSchedule, FeedStatus};
use crate::market::{Market, Quote, Timeframe};
use crate::overlay::{Frontmost, OverlayEvent, OverlayMode, OverlayState};
use crate::settings::Settings;
use crate::source::{CANDLES_TIMEOUT, FetchFailure, HttpOutcome};
use crate::time::Timestamp;

/// Names one request the core asked the shell to run. A result carrying any id other than the one the core is waiting
/// for is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(u64);

impl RequestId {
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "#{}", self.0)
    }
}

/// Everything that can happen to the app. Times must not go backwards; an earlier time counts as no time passing.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Overlay(OverlayEvent),
    /// From a heartbeat timer, and whenever `App::next_wake` passes.
    Tick(Timestamp),
    QuoteFinished {
        request: RequestId,
        outcome: HttpOutcome,
        now: Timestamp,
    },
    CandlesFinished {
        request: RequestId,
        timeframe: Timeframe,
        outcome: HttpOutcome,
        now: Timestamp,
    },
    /// The machine resumed from sleep.
    Woke(Timestamp),
    /// The settings file changed on disk from outside the app.
    SettingsChanged(Settings),
}

/// What the shell must do after an input, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Write these settings to the settings file.
    SaveSettings(Settings),
    /// GET `url` with `timeout` off the UI thread, then send `Input::QuoteFinished` with this request id.
    FetchQuote { request: RequestId, url: &'static str, timeout: Duration },
    /// GET `url` with `timeout` off the UI thread, then send `Input::CandlesFinished` with this request id.
    FetchCandles { request: RequestId, timeframe: Timeframe, url: String, timeout: Duration },
    /// The overlay, the feed status, the ticker text, the tooltip or a chart load changed: draw again from the read
    /// accessors.
    Render,
}

/// The whole app as a state machine, replacing the macOS app's feed loops. The shell owns windows, timers and the
/// network; this decides when to fetch, what a result means, what to save and when to redraw.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    overlay: OverlayState,
    now: Timestamp,
    feed: Feed,
    /// Counts across feeds, so a result for a replaced feed's request never matches the new feed's.
    requests_issued: u64,
}

/// Everything fetched for one market. Switching markets replaces it, so no price or candle of one market is ever shown
/// under another's label.
#[derive(Clone, Debug, PartialEq)]
struct Feed {
    market: Market,
    started_at: Timestamp,
    last_good: Option<Quote>,
    /// Why the last finished quote request failed, cleared by the next success.
    quote_failure: Option<FetchFailure>,
    schedule: FeedSchedule,
    quote_in_flight: Option<RequestId>,
    charts: ChartFeed,
}

impl Feed {
    fn new(market: Market, started_at: Timestamp) -> Feed {
        Feed {
            market,
            started_at,
            last_good: None,
            quote_failure: None,
            schedule: FeedSchedule::new(),
            quote_in_flight: None,
            charts: ChartFeed::default(),
        }
    }
}

impl App {
    /// Nothing is fetched until the first input; send `Input::Tick(now)` right away.
    pub fn new(settings: Settings, frontmost: Frontmost, now: Timestamp) -> App {
        let overlay = OverlayState::new(settings, frontmost);
        App { feed: Feed::new(overlay.settings().market, now), overlay, now, requests_issued: 0 }
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let overlay_before = self.overlay;
        let shown_before = self.shown();
        let edited_in_app = matches!(input, Input::Overlay(_));
        let mut charts_changed = false;
        match input {
            Input::Overlay(event) => self.overlay = self.overlay.reduce(event),
            Input::SettingsChanged(settings) => {
                self.overlay = self.overlay.reduce(OverlayEvent::SettingsEdited(settings))
            }
            Input::Tick(now) => self.advance(now),
            Input::Woke(now) => {
                self.advance(now);
                self.feed.schedule.woke();
            }
            Input::QuoteFinished { request, outcome, now } => {
                if self.feed.quote_in_flight != Some(request) {
                    return Vec::new();
                }
                self.advance(now);
                let feed = &mut self.feed;
                feed.quote_in_flight = None;
                let quote = feed.market.source().quote_from(&outcome, self.now);
                feed.schedule.finished(quote.is_ok());
                match quote {
                    Ok(quote) => {
                        feed.last_good = Some(quote);
                        feed.quote_failure = None;
                    }
                    Err(failure) => feed.quote_failure = Some(failure),
                }
            }
            Input::CandlesFinished { request, timeframe, outcome, now } => {
                if !self.feed.charts.awaits(timeframe, request) {
                    return Vec::new();
                }
                self.advance(now);
                let candles = self.feed.market.source().candles_from(&outcome, timeframe);
                self.feed.charts.finish(timeframe, candles, self.now);
                charts_changed = true;
            }
        }
        let market = self.settings().market;
        if self.feed.market != market {
            self.feed = Feed::new(market, self.now);
        }

        let mut effects = Vec::new();
        // The file already holds settings that arrived from it, and writing them back would race the editor.
        if edited_in_app && self.overlay.settings() != overlay_before.settings() {
            effects.push(Effect::SaveSettings(*self.overlay.settings()));
        }
        let refresh_seconds = self.overlay.settings().refresh_seconds;
        let source = self.feed.market.source();
        if self.feed.quote_in_flight.is_none() && self.feed.schedule.is_due(refresh_seconds, self.now) {
            let request = self.issue();
            self.feed.quote_in_flight = Some(request);
            self.feed.schedule.started(self.now);
            let timeout = FeedSchedule::request_timeout(refresh_seconds);
            effects.push(Effect::FetchQuote { request, url: source.quote_url(), timeout });
        }
        if let Some(timeframe) = self.feed.charts.due(&self.overlay, self.now) {
            let request = self.issue();
            self.feed.charts.begin(timeframe, request);
            charts_changed = true;
            let (url, timeout) = (source.candles_url(timeframe), CANDLES_TIMEOUT);
            effects.push(Effect::FetchCandles { request, timeframe, url, timeout });
        }
        if charts_changed || self.overlay != overlay_before || self.shown() != shown_before {
            effects.push(Effect::Render);
        }
        effects
    }

    pub fn overlay(&self) -> &OverlayState {
        &self.overlay
    }

    pub fn settings(&self) -> &Settings {
        self.overlay.settings()
    }

    /// The latest time any input carried.
    pub fn now(&self) -> Timestamp {
        self.now
    }

    pub fn status(&self) -> FeedStatus {
        let refresh = f64::from(self.settings().refresh_seconds);
        FeedStatus::derive(self.feed.last_good.as_ref(), self.feed.started_at, self.now, refresh)
    }

    pub fn display(&self) -> TickerDisplay {
        TickerDisplay::new(&self.status(), self.settings(), self.now)
    }

    pub fn quote_failure(&self) -> Option<&FetchFailure> {
        self.feed.quote_failure.as_ref()
    }

    pub fn tooltip(&self) -> String {
        TickerDisplay::tooltip(&self.status(), self.quote_failure(), self.settings(), self.now)
    }

    pub fn chart_load(&self, timeframe: Timeframe) -> &CandleLoad {
        self.feed.charts.get(timeframe)
    }

    /// The expanded timeframe and its load, or None while collapsed.
    pub fn expanded(&self) -> Option<(Timeframe, &CandleLoad)> {
        match self.overlay.mode() {
            OverlayMode::Expanded(timeframe) => Some((timeframe, self.chart_load(timeframe))),
            OverlayMode::Collapsed => None,
        }
    }

    /// What the expanded chart draws at the saved zoom in at most `max_bars` bars, with the last good quote in its
    /// newest candle, or None while collapsed or with no candles yet.
    pub fn chart(&self, max_bars: usize) -> Option<CandleChart> {
        let (timeframe, load) = self.expanded()?;
        load.series()?.chart(self.feed.last_good.as_ref(), timeframe, self.settings().zoom, max_bars)
    }

    /// The earliest time at which a tick would start a request or turn the status stale or offline. The shell arms a
    /// one-shot timer for it after every input, beside its heartbeat. A time at or before `now()` means tick at once.
    pub fn next_wake(&self) -> Option<Timestamp> {
        let refresh_seconds = self.settings().refresh_seconds;
        let quote = match self.feed.quote_in_flight {
            Some(_) => None,
            None => Some(self.feed.schedule.next_start(refresh_seconds).unwrap_or(self.now)),
        };
        let anchor = self.feed.last_good.map_or(self.feed.started_at, |quote| quote.fetched_at);
        let turns = anchor.plus(f64::from(refresh_seconds) * FeedStatus::MISSED_REFRESHES_BEFORE_STALE);
        let status = (turns > self.now).then_some(turns);
        let candles = self.feed.charts.due_at(&self.overlay, self.now);
        [quote, status, candles]
            .into_iter()
            .flatten()
            .reduce(|soonest, time| if time < soonest { time } else { soonest })
    }

    /// A stale price's age is in the text, so the shell's 1 s heartbeat tick is what moves it on, not `next_wake`.
    fn shown(&self) -> (FeedStatus, TickerDisplay, String) {
        (self.status(), self.display(), self.tooltip())
    }

    fn advance(&mut self, now: Timestamp) {
        self.now = self.now.max(now);
    }

    fn issue(&mut self) -> RequestId {
        self.requests_issued += 1;
        RequestId(self.requests_issued)
    }
}
