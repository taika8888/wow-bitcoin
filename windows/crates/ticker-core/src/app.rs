use std::fmt;
use std::time::Duration;

use crate::chart::{CandleChart, CandleLoad, ChartFeed};
use crate::display::TickerDisplay;
use crate::feed::{FeedSchedule, FeedStatus};
use crate::kraken::{self, HttpOutcome};
use crate::market::{Quote, Timeframe};
use crate::overlay::{Frontmost, OverlayEvent, OverlayMode, OverlayState};
use crate::settings::Settings;
use crate::time::Timestamp;

/// Names one request the core asked the shell to run. A result carrying any id other than the one the core is waiting
/// for is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequestId(u64);

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
    /// The overlay, the feed status or a chart load changed: draw again from the read accessors.
    Render,
}

/// The whole app as a state machine, replacing the macOS app's feed loops. The shell owns windows, timers and the
/// network; this decides when to fetch, what a result means, what to save and when to redraw.
#[derive(Clone, Debug, PartialEq)]
pub struct App {
    overlay: OverlayState,
    started_at: Timestamp,
    now: Timestamp,
    last_good: Option<Quote>,
    schedule: FeedSchedule,
    quote_in_flight: Option<RequestId>,
    charts: ChartFeed,
    requests_issued: u64,
}

impl App {
    /// Nothing is fetched until the first input; send `Input::Tick(now)` right away.
    pub fn new(settings: Settings, frontmost: Frontmost, now: Timestamp) -> App {
        App {
            overlay: OverlayState::new(settings, frontmost),
            started_at: now,
            now,
            last_good: None,
            schedule: FeedSchedule::new(),
            quote_in_flight: None,
            charts: ChartFeed::default(),
            requests_issued: 0,
        }
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        let overlay_before = self.overlay;
        let status_before = self.status();
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
                self.schedule.woke();
            }
            Input::QuoteFinished { request, outcome, now } => {
                if self.quote_in_flight != Some(request) {
                    return Vec::new();
                }
                self.quote_in_flight = None;
                self.advance(now);
                let quote = kraken::quote_from(&outcome, self.now);
                self.schedule.finished(quote.is_ok());
                if let Ok(quote) = quote {
                    self.last_good = Some(quote);
                }
            }
            Input::CandlesFinished { request, timeframe, outcome, now } => {
                if !self.charts.awaits(timeframe, request) {
                    return Vec::new();
                }
                self.advance(now);
                self.charts.finish(timeframe, kraken::candles_from(&outcome, timeframe), self.now);
                charts_changed = true;
            }
        }

        let mut effects = Vec::new();
        // The file already holds settings that arrived from it, and writing them back would race the editor.
        if edited_in_app && self.overlay.settings() != overlay_before.settings() {
            effects.push(Effect::SaveSettings(*self.overlay.settings()));
        }
        let refresh_seconds = self.overlay.settings().refresh_seconds;
        if self.quote_in_flight.is_none() && self.schedule.is_due(refresh_seconds, self.now) {
            let request = self.issue();
            self.quote_in_flight = Some(request);
            self.schedule.started(self.now);
            let timeout = FeedSchedule::request_timeout(refresh_seconds);
            effects.push(Effect::FetchQuote { request, url: kraken::TICKER_URL, timeout });
        }
        if let Some(timeframe) = self.charts.due(&self.overlay, self.now) {
            let request = self.issue();
            self.charts.begin(timeframe, request);
            charts_changed = true;
            let (url, timeout) = (kraken::ohlc_url(timeframe), kraken::CANDLES_TIMEOUT);
            effects.push(Effect::FetchCandles { request, timeframe, url, timeout });
        }
        if charts_changed || self.overlay != overlay_before || self.status() != status_before {
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
        FeedStatus::derive(self.last_good.as_ref(), self.started_at, self.now, refresh)
    }

    pub fn display(&self) -> TickerDisplay {
        TickerDisplay::new(&self.status(), self.settings())
    }

    pub fn chart_load(&self, timeframe: Timeframe) -> &CandleLoad {
        self.charts.get(timeframe)
    }

    /// The expanded timeframe and its load, or None while collapsed.
    pub fn expanded(&self) -> Option<(Timeframe, &CandleLoad)> {
        match self.overlay.mode() {
            OverlayMode::Expanded(timeframe) => Some((timeframe, self.chart_load(timeframe))),
            OverlayMode::Collapsed => None,
        }
    }

    /// What the expanded chart draws, or None while collapsed or with no candles yet.
    pub fn chart(&self) -> Option<CandleChart> {
        let (timeframe, load) = self.expanded()?;
        CandleChart::new(&load.series()?.candles, timeframe)
    }

    /// The earliest time at which a tick would start a request or turn the status stale or offline. The shell arms a
    /// one-shot timer for it after every input, beside its heartbeat. A time at or before `now()` means tick at once.
    pub fn next_wake(&self) -> Option<Timestamp> {
        let refresh_seconds = self.settings().refresh_seconds;
        let quote = match self.quote_in_flight {
            Some(_) => None,
            None => Some(self.schedule.next_start(refresh_seconds).unwrap_or(self.now)),
        };
        let anchor = self.last_good.map_or(self.started_at, |quote| quote.fetched_at);
        let turns = anchor.plus(f64::from(refresh_seconds) * FeedStatus::MISSED_REFRESHES_BEFORE_STALE);
        let status = (turns > self.now).then_some(turns);
        let candles = self.charts.due_at(&self.overlay, self.now);
        [quote, status, candles]
            .into_iter()
            .flatten()
            .reduce(|soonest, time| if time < soonest { time } else { soonest })
    }

    fn advance(&mut self, now: Timestamp) {
        self.now = self.now.max(now);
    }

    fn issue(&mut self) -> RequestId {
        self.requests_issued += 1;
        RequestId(self.requests_issued)
    }
}
