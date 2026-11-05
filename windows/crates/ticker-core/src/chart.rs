use std::ops::RangeInclusive;

use crate::display::Tone;
use crate::market::{Candle, Timeframe};
use crate::time::Timestamp;

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
}

impl CandleChart {
    /// Fraction of the price range added above the highest high and below the lowest low.
    pub const PRICE_PADDING: f64 = 0.06;
    /// Minimum body height as a fraction of the price domain.
    pub const MINIMUM_BODY: f64 = 0.004;

    pub fn new(candles: &[Candle], timeframe: Timeframe) -> Option<CandleChart> {
        let (first, last) = (candles.first()?, candles.last()?);
        let low = candles.iter().map(|candle| candle.low).fold(f64::INFINITY, f64::min);
        let high = candles.iter().map(|candle| candle.high).fold(f64::NEG_INFINITY, f64::max);
        let range = if high > low { high - low } else { high * 0.001 };
        let price_domain = (low - range * Self::PRICE_PADDING)..=(high + range * Self::PRICE_PADDING);
        let interval = timeframe.interval_seconds();
        let minimum_body = (price_domain.end() - price_domain.start()) * Self::MINIMUM_BODY;
        let bars = candles
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
