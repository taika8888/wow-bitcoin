use crate::display::Tone;
use crate::kraken::KrakenError;
use crate::market::{Candle, Time, Timeframe};
use crate::overlay::{OverlayMode, OverlayState};

#[derive(Clone, Debug, PartialEq)]
pub enum FetchFailure {
    /// No answer at all: offline, timed out, refused.
    Unreachable,
    Status(u16),
    /// Kraken answered 200 with a body that is not a usable response.
    Rejected(KrakenError),
}

impl FetchFailure {
    pub fn message(&self) -> String {
        match self {
            FetchFailure::Unreachable => "Kraken is not answering".into(),
            FetchFailure::Status(code) => format!("Kraken answered HTTP {code}"),
            FetchFailure::Rejected(KrakenError::Api(errors)) => {
                format!("Kraken refused: {}", errors.join(", "))
            }
            FetchFailure::Rejected(_) => "Kraken sent candles this app cannot read".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChartNotice {
    /// In place of the chart, when there are no candles to draw.
    Placeholder(String),
    /// A small marker over candles that the last refresh failed to update.
    Stale(String),
}

/// One timeframe's candles. A refresh in flight or a failed one keeps the last good series, so the chart never blanks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CandleLoad {
    series: Option<(Vec<Candle>, Time)>,
    in_flight: bool,
    /// The last finished request's failure, cleared by the next success.
    failure: Option<(FetchFailure, Time)>,
}

impl CandleLoad {
    pub const RETRY_AFTER_FAILURE: f64 = 10.0;

    pub fn candles(&self) -> &[Candle] {
        self.series.as_ref().map_or(&[], |(candles, _)| candles)
    }

    pub fn fetched_at(&self) -> Option<Time> {
        self.series.as_ref().map(|(_, at)| *at)
    }

    pub fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// The snapshot's `chart` field.
    pub fn name(&self) -> &'static str {
        if self.in_flight {
            "loading"
        } else if self.failure.is_some() {
            "failed"
        } else if self.series.is_none() {
            "idle"
        } else {
            "loaded"
        }
    }

    pub fn notice(&self, timeframe: Timeframe) -> Option<ChartNotice> {
        let label = timeframe.spec().label;
        match (&self.failure, self.candles().is_empty()) {
            (Some((kind, _)), false) => {
                Some(ChartNotice::Stale(format!("Stale: {}", kind.message())))
            }
            (Some((kind, _)), true) => Some(ChartNotice::Placeholder(format!(
                "{}, retrying",
                kind.message()
            ))),
            (None, false) => None,
            (None, true) if self.series.is_none() => {
                Some(ChartNotice::Placeholder(format!("Loading {label} candles")))
            }
            (None, true) => Some(ChartNotice::Placeholder(format!("No {label} candles"))),
        }
    }

    /// When this timeframe's next request is due, or None while one is in flight.
    pub fn due_at(&self, timeframe: Timeframe) -> Option<Time> {
        if self.in_flight {
            return None;
        }
        Some(match (&self.failure, &self.series) {
            (Some((_, at)), _) => at.plus(Self::RETRY_AFTER_FAILURE),
            (None, Some((_, at))) => at.plus(timeframe.spec().refresh_seconds.into()),
            (None, None) => Time::DISTANT_PAST,
        })
    }
}

/// Fetches only the timeframe on screen, only when due, never while its last request is in flight.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartFeed {
    loads: [CandleLoad; 3],
}

impl ChartFeed {
    pub fn get(&self, timeframe: Timeframe) -> &CandleLoad {
        &self.loads[timeframe as usize]
    }

    /// Marks the timeframe in flight and returns it when a request should start now. A second call before
    /// `finish` returns None, which is what keeps requests from overlapping.
    pub fn begin_fetch(&mut self, overlay: &OverlayState, now: Time) -> Option<Timeframe> {
        let OverlayMode::Expanded(timeframe) = overlay.mode else {
            return None;
        };
        let load = &mut self.loads[timeframe as usize];
        if !overlay.is_visible() || load.due_at(timeframe).is_none_or(|due| due > now) {
            return None;
        }
        load.in_flight = true;
        Some(timeframe)
    }

    /// Ignored unless that timeframe is in flight, so a duplicate or late reply cannot overwrite a newer series.
    pub fn finish(
        &mut self,
        timeframe: Timeframe,
        result: Result<Vec<Candle>, FetchFailure>,
        now: Time,
    ) {
        let load = &mut self.loads[timeframe as usize];
        if !load.in_flight {
            return;
        }
        load.in_flight = false;
        match result {
            Ok(candles) => {
                load.series = Some((candles, now));
                load.failure = None;
            }
            Err(kind) => load.failure = Some((kind, now)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// The middle of the candle's interval, so bars sit between interval boundaries.
    pub time: Time,
    pub low: f64,
    pub high: f64,
    /// Open and close ordered, widened to a minimum height so a flat candle still shows.
    pub body_low: f64,
    pub body_high: f64,
    pub tone: Tone,
}

/// What the chart draws, in data units; the app's layout maps it to pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct CandleChart {
    pub bars: Vec<Bar>,
    pub price_domain: (f64, f64),
    pub time_domain: (Time, Time),
    pub last_price: f64,
}

impl CandleChart {
    /// Fraction of the price range added above the highest high and below the lowest low.
    pub const PRICE_PADDING: f64 = 0.06;
    /// Minimum body height as a fraction of the price domain.
    pub const MINIMUM_BODY: f64 = 0.004;

    pub fn new(candles: &[Candle], timeframe: Timeframe) -> Option<CandleChart> {
        let (first, last) = (candles.first()?, candles.last()?);
        let low = candles.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
        let high = candles
            .iter()
            .map(|c| c.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let range = if high > low { high - low } else { high * 0.001 };
        let price_domain = (
            low - range * Self::PRICE_PADDING,
            high + range * Self::PRICE_PADDING,
        );
        let interval = timeframe.interval_seconds();
        let minimum_body = (price_domain.1 - price_domain.0) * Self::MINIMUM_BODY;
        let bars = candles
            .iter()
            .map(|c| {
                let middle = (c.open + c.close) / 2.0;
                let half = (c.close - c.open).abs().max(minimum_body) / 2.0;
                Bar {
                    time: c.time.plus(interval / 2.0),
                    low: c.low,
                    high: c.high,
                    body_low: middle - half,
                    body_high: middle + half,
                    tone: if c.close > c.open {
                        Tone::Up
                    } else if c.close < c.open {
                        Tone::Down
                    } else {
                        Tone::Neutral
                    },
                }
            })
            .collect();
        Some(CandleChart {
            bars,
            price_domain,
            time_domain: (first.time, last.time.plus(interval)),
            last_price: last.close,
        })
    }
}

/// Multiples of a nice step (1, 2, 2.5 or 5 times a power of ten) strictly inside `domain`, about `desired` of them.
pub fn axis_ticks(domain: (f64, f64), desired: usize) -> Vec<f64> {
    let span = domain.1 - domain.0;
    if span.is_nan() || span <= 0.0 || desired == 0 {
        return Vec::new();
    }
    let raw = span / desired as f64;
    let magnitude = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|&step| step >= raw)
        .unwrap_or(10.0 * magnitude);
    let mut ticks = Vec::new();
    let mut k = (domain.0 / step).floor() + 1.0;
    while k * step < domain.1 {
        ticks.push(k * step);
        k += 1.0;
    }
    ticks
}

/// Time-axis ticks: whole local-clock steps (minutes to days) strictly inside `domain`, at most `desired` of them.
pub fn time_ticks(
    domain: (Time, Time),
    desired: usize,
    utc_offset_seconds: i32,
) -> (Vec<Time>, f64) {
    const STEPS: [f64; 12] = [
        60.0, 300.0, 900.0, 1800.0, 3600.0, 7200.0, 10800.0, 21600.0, 43200.0, 86400.0, 172800.0,
        604800.0,
    ];
    let span = domain.1.since(domain.0);
    let step = STEPS
        .into_iter()
        .find(|step| span / step <= desired as f64)
        .unwrap_or(STEPS[STEPS.len() - 1]);
    let offset = f64::from(utc_offset_seconds);
    let mut ticks = Vec::new();
    let mut local = ((domain.0.0 + offset) / step).floor() * step + step;
    while local - offset < domain.1.0 {
        ticks.push(Time(local - offset));
        local += step;
    }
    (ticks, step)
}

/// "14:30" on an intraday axis, "11/5" (en-US month/day) once ticks are a day or more apart, in local time.
pub fn time_label(time: Time, step: f64, utc_offset_seconds: i32) -> String {
    let local = (time.0 + f64::from(utc_offset_seconds)).floor() as i64;
    let (days, seconds) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    if step < 86_400.0 {
        return format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60);
    }
    let (month, day) = month_day(days);
    format!("{month}/{day}")
}

/// Civil month and day from days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn month_day(days: i64) -> (i64, i64) {
    let z = days + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (month, day)
}
