use std::ops::RangeInclusive;

use crate::app::RequestId;
use crate::display::Tone;
use crate::kraken::FetchFailure;
use crate::market::{Candle, Quote, Timeframe, Zoom};
use crate::overlay::{OverlayMode, OverlayState};
use crate::time::Timestamp;

#[derive(Clone, Debug, PartialEq)]
pub struct CandleSeries {
    pub candles: Vec<Candle>,
    pub fetched_at: Timestamp,
}

impl CandleSeries {
    /// What the chart draws. A quote taken after this fetch and inside the newest candle's interval is a trade Kraken
    /// had not reported yet, so it moves that candle's close and widens its wicks. Any later quote waits for the next
    /// fetch to bring the candle it belongs to.
    pub fn chart(
        &self,
        quote: Option<&Quote>,
        timeframe: Timeframe,
        zoom: Zoom,
        max_bars: usize,
    ) -> Option<CandleChart> {
        let newest = self.candles.last()?;
        let forming = newest.time..newest.time.plus(timeframe.interval_seconds());
        let trade = quote
            .filter(|quote| quote.fetched_at > self.fetched_at && forming.contains(&quote.fetched_at))
            .map(|quote| quote.price);
        CandleChart::build(&self.candles, trade, timeframe, zoom, max_bars)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FailedFetch {
    pub kind: FetchFailure,
    pub at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChartNotice {
    /// In place of the chart, when there are no candles to draw.
    Placeholder(String),
    /// A small marker over candles that the last refresh failed to update.
    Stale(String),
}

/// One timeframe's candles. A refresh in flight or a failed one keeps the last good series, so the chart never blanks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CandleLoad {
    series: Option<CandleSeries>,
    in_flight: Option<RequestId>,
    /// The last finished request's failure, cleared by the next success.
    failure: Option<FailedFetch>,
}

impl CandleLoad {
    pub const RETRY_AFTER_FAILURE_SECONDS: f64 = 10.0;

    pub fn series(&self) -> Option<&CandleSeries> {
        self.series.as_ref()
    }

    pub fn failure(&self) -> Option<&FailedFetch> {
        self.failure.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn name(&self) -> &'static str {
        match (self.in_flight, &self.failure, &self.series) {
            (Some(_), _, _) => "loading",
            (None, Some(_), _) => "failed",
            (None, None, Some(_)) => "loaded",
            (None, None, None) => "idle",
        }
    }

    pub fn notice(&self, timeframe: Timeframe) -> Option<ChartNotice> {
        let has_candles = self.series.as_ref().is_some_and(|series| !series.candles.is_empty());
        let label = timeframe.spec().label;
        match (&self.failure, has_candles) {
            (Some(failure), true) => Some(ChartNotice::Stale(format!("Stale: {}", failure.kind.message()))),
            (Some(failure), false) => Some(ChartNotice::Placeholder(format!("{}, retrying", failure.kind.message()))),
            (None, true) => None,
            (None, false) if self.series.is_none() => {
                Some(ChartNotice::Placeholder(format!("Loading {label} candles")))
            }
            (None, false) => Some(ChartNotice::Placeholder(format!("No {label} candles"))),
        }
    }

    /// When the next request may start, once none is in flight: None means at once.
    fn due_at(&self, timeframe: Timeframe) -> Option<Timestamp> {
        match (&self.failure, &self.series) {
            (Some(failure), _) => Some(failure.at.plus(Self::RETRY_AFTER_FAILURE_SECONDS)),
            (None, Some(series)) => Some(series.fetched_at.plus(f64::from(timeframe.spec().refresh_seconds))),
            (None, None) => None,
        }
    }
}

/// Decides which timeframe to fetch: only the one on screen, only when due, never while its last request is in flight.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ChartFeed {
    loads: [CandleLoad; 3],
}

impl ChartFeed {
    pub(crate) fn get(&self, timeframe: Timeframe) -> &CandleLoad {
        &self.loads[timeframe.index()]
    }

    /// The visible expanded timeframe with no request in flight, and when its next request may start (None: at once).
    fn wanted(&self, overlay: &OverlayState) -> Option<(Timeframe, Option<Timestamp>)> {
        let OverlayMode::Expanded(timeframe) = overlay.mode() else { return None };
        let load = self.get(timeframe);
        (overlay.is_visible() && !load.is_loading()).then(|| (timeframe, load.due_at(timeframe)))
    }

    pub(crate) fn due(&self, overlay: &OverlayState, now: Timestamp) -> Option<Timeframe> {
        self.wanted(overlay).and_then(|(timeframe, at)| at.is_none_or(|at| now >= at).then_some(timeframe))
    }

    /// When a tick could next start a fetch, if one is waiting on time alone.
    pub(crate) fn due_at(&self, overlay: &OverlayState, now: Timestamp) -> Option<Timestamp> {
        self.wanted(overlay).map(|(_, at)| at.unwrap_or(now))
    }

    pub(crate) fn begin(&mut self, timeframe: Timeframe, request: RequestId) {
        self.loads[timeframe.index()].in_flight = Some(request);
    }

    pub(crate) fn awaits(&self, timeframe: Timeframe, request: RequestId) -> bool {
        self.get(timeframe).in_flight == Some(request)
    }

    pub(crate) fn finish(&mut self, timeframe: Timeframe, result: Result<Vec<Candle>, FetchFailure>, now: Timestamp) {
        let load = &mut self.loads[timeframe.index()];
        load.in_flight = None;
        match result {
            Ok(candles) => {
                load.series = Some(CandleSeries { candles, fetched_at: now });
                load.failure = None;
            }
            Err(kind) => load.failure = Some(FailedFetch { kind, at: now }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// The middle of the candle's interval, so bars sit between interval boundaries.
    pub time: Timestamp,
    pub low: f64,
    pub high: f64,
    /// Open and close ordered, widened to a minimum height so a flat candle still shows.
    pub body_low: f64,
    pub body_high: f64,
    pub tone: Tone,
}

/// What the chart draws. The renderer owns scales and axes; this owns which values go in them.
#[derive(Clone, Debug, PartialEq)]
pub struct CandleChart {
    pub bars: Vec<Bar>,
    pub price_domain: RangeInclusive<f64>,
    pub time_domain: RangeInclusive<Timestamp>,
    pub last_price: f64,
    /// How many candles the bars stand for, which is more than `bars.len()` once runs of candles merge.
    pub candle_count: usize,
}

impl CandleChart {
    /// Fraction of the price range added above the highest high and below the lowest low.
    pub const PRICE_PADDING: f64 = 0.06;
    /// Minimum body height as a fraction of the price domain.
    pub const MINIMUM_BODY: f64 = 0.004;

    /// Shows the newest candles the zoom asks for, with the price domain fitted to those alone. Past `max_bars` (at
    /// least 1), each bar merges a run of adjacent candles counted from the newest, and a partial run at the oldest end
    /// is left out.
    pub fn new(all: &[Candle], timeframe: Timeframe, zoom: Zoom, max_bars: usize) -> Option<CandleChart> {
        Self::build(all, None, timeframe, zoom, max_bars)
    }

    fn build(
        all: &[Candle],
        trade: Option<f64>,
        timeframe: Timeframe,
        zoom: Zoom,
        max_bars: usize,
    ) -> Option<CandleChart> {
        let visible = zoom.visible_count(timeframe, all.len());
        let per_bar = visible.div_ceil(max_bars.max(1));
        if per_bar == 0 {
            return None;
        }
        let candles = &all[all.len() - (visible - visible % per_bar)..];
        let mut merged: Vec<Candle> = candles
            .chunks_exact(per_bar)
            .map(|run| Candle {
                time: run[0].time,
                open: run[0].open,
                high: run.iter().map(|candle| candle.high).fold(f64::NEG_INFINITY, f64::max),
                low: run.iter().map(|candle| candle.low).fold(f64::INFINITY, f64::min),
                close: run[run.len() - 1].close,
            })
            .collect();
        if let (Some(price), Some(newest)) = (trade, merged.last_mut()) {
            *newest = Candle { high: newest.high.max(price), low: newest.low.min(price), close: price, ..*newest };
        }
        let (first, last) = (merged.first()?, merged.last()?);
        let low = merged.iter().map(|candle| candle.low).fold(f64::INFINITY, f64::min);
        let high = merged.iter().map(|candle| candle.high).fold(f64::NEG_INFINITY, f64::max);
        let range = if high > low { high - low } else { high * 0.001 };
        let price_domain = (low - range * Self::PRICE_PADDING)..=(high + range * Self::PRICE_PADDING);
        let interval = timeframe.interval_seconds() * per_bar as f64;
        let minimum_body = (price_domain.end() - price_domain.start()) * Self::MINIMUM_BODY;
        let bars = merged
            .iter()
            .map(|candle| {
                let middle = (candle.open + candle.close) / 2.0;
                let half = (candle.close - candle.open).abs().max(minimum_body) / 2.0;
                Bar {
                    time: candle.time.plus(interval / 2.0),
                    low: candle.low,
                    high: candle.high,
                    body_low: middle - half,
                    body_high: middle + half,
                    tone: tone(candle.open, candle.close),
                }
            })
            .collect();
        Some(CandleChart {
            bars,
            price_domain,
            time_domain: first.time..=last.time.plus(interval),
            last_price: last.close,
            candle_count: candles.len(),
        })
    }
}

fn tone(open: f64, close: f64) -> Tone {
    if close > open {
        Tone::Up
    } else if close < open {
        Tone::Down
    } else {
        Tone::Neutral
    }
}
