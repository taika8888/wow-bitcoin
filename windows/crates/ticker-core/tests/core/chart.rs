use ticker_core::kraken::parse_candles;
use ticker_core::{Candle, CandleChart, Timeframe, Timestamp, Tone};

use crate::fixtures;

pub const T0: Timestamp = Timestamp::from_unix_seconds(1_793_847_300.0);

pub fn candle(open: f64, close: f64, minute: u32) -> Candle {
    Candle { time: T0.plus(f64::from(minute) * 60.0), open, high: open.max(close), low: open.min(close), close }
}

/// Values read straight from the fixture JSON, not from the parser.
const RECORDED: [(&str, Timeframe, f64, f64, usize, usize); 3] = [
    (fixtures::OHLC_1, Timeframe::M1, 80190.1, 80827.6, 62, 57),
    (fixtures::OHLC_15, Timeframe::M15, 75625.0, 81179.6, 49, 47),
    (fixtures::OHLC_240, Timeframe::H4, 75625.0, 84840.0, 42, 48),
];

#[test]
fn recorded_candles_fit_the_padded_domain() {
    for (body, timeframe, low, high, up, down) in RECORDED {
        let candles = parse_candles(body.as_bytes(), timeframe).unwrap();
        let chart = CandleChart::new(&candles, timeframe).unwrap();
        let range = high - low;
        let below = (low - chart.price_domain.start()) / range;
        let above = (chart.price_domain.end() - high) / range;
        assert!((0.05..=0.1).contains(&below), "{timeframe:?}: margin under the lowest wick is {below} of the range");
        assert!((0.05..=0.1).contains(&above), "{timeframe:?}: margin over the highest wick is {above} of the range");
        assert_eq!(chart.bars.len(), timeframe.spec().candles_kept);
        assert_eq!(chart.bars.iter().filter(|bar| bar.tone == Tone::Up).count(), up);
        assert_eq!(chart.bars.iter().filter(|bar| bar.tone == Tone::Down).count(), down);
        assert_eq!(chart.last_price, 80489.7);
        for bar in &chart.bars {
            assert!(chart.time_domain.contains(&bar.time));
            assert!(chart.price_domain.contains(&bar.body_low) && chart.price_domain.contains(&bar.body_high));
        }
    }
}

#[test]
fn tone_follows_close_against_open() {
    let chart = CandleChart::new(&[candle(10.0, 11.0, 0), candle(11.0, 10.0, 1), candle(10.0, 10.0, 2)], Timeframe::M1)
        .unwrap();
    assert_eq!(chart.bars.iter().map(|bar| bar.tone).collect::<Vec<_>>(), [Tone::Up, Tone::Down, Tone::Neutral]);
    assert_eq!((chart.bars[0].body_low, chart.bars[0].body_high), (10.0, 11.0));
    assert_eq!((chart.bars[1].body_low, chart.bars[1].body_high), (10.0, 11.0));
    assert_eq!(chart.last_price, 10.0);
}

#[test]
fn a_flat_candle_still_has_a_visible_body() {
    let flat = Candle { time: T0, open: 100.0, high: 120.0, low: 80.0, close: 100.0 };
    let bar = CandleChart::new(&[flat], Timeframe::M1).unwrap().bars[0];
    assert!(bar.body_high > bar.body_low);
    assert!(bar.body_low < 100.0 && bar.body_high > 100.0);
}

#[test]
fn a_series_with_no_range_still_has_a_domain() {
    let chart = CandleChart::new(&[candle(80000.0, 80000.0, 0), candle(80000.0, 80000.0, 1)], Timeframe::M1).unwrap();
    assert!(*chart.price_domain.start() < 80000.0 && *chart.price_domain.end() > 80000.0);
}

#[test]
fn bars_sit_mid_interval_and_the_time_domain_covers_the_last_interval() {
    let chart = CandleChart::new(&[candle(1.0, 2.0, 0), candle(2.0, 3.0, 15)], Timeframe::M15).unwrap();
    assert_eq!(
        chart.bars.iter().map(|bar| bar.time).collect::<Vec<_>>(),
        [T0.plus(450.0), T0.plus(15.0 * 60.0 + 450.0)]
    );
    assert_eq!(chart.time_domain, T0..=T0.plus(30.0 * 60.0));
}

#[test]
fn no_candles_no_chart() {
    assert_eq!(CandleChart::new(&[], Timeframe::H4), None);
}
