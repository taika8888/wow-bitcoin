use ticker_core::kraken::parse_candles;
use ticker_core::{Candle, CandleChart, Timeframe, Timestamp, Tone, Zoom, ZoomStep};

use crate::fixtures;

pub const T0: Timestamp = Timestamp::from_unix_seconds(1_793_847_300.0);

pub fn candle(open: f64, close: f64, minute: u32) -> Candle {
    Candle { time: T0.plus(f64::from(minute) * 60.0), open, high: open.max(close), low: open.min(close), close }
}

fn wicked(open: f64, close: f64, high: f64, low: f64, minute: u32) -> Candle {
    Candle { high, low, ..candle(open, close, minute) }
}

/// Values read straight from the fixture JSON, not from the parser.
const RECORDED: [(&str, Timeframe, f64, f64, usize, usize); 3] = [
    (fixtures::OHLC_1, Timeframe::M1, 80190.1, 80827.6, 62, 57),
    (fixtures::OHLC_15, Timeframe::M15, 75625.0, 81179.6, 49, 47),
    (fixtures::OHLC_240, Timeframe::H4, 75625.0, 84840.0, 42, 48),
];

/// A 360 DIP chart, the default width, less 60 DIP of price axis, at 2 DIP a bar.
pub const DEFAULT_BUDGET: usize = 150;

fn chart(candles: &[Candle], timeframe: Timeframe) -> CandleChart {
    CandleChart::new(candles, timeframe, Zoom::Standard, DEFAULT_BUDGET).unwrap()
}

#[test]
fn the_standard_zoom_draws_every_shown_candle_inside_the_padded_domain() {
    for (body, timeframe, low, high, up, down) in RECORDED {
        let candles = parse_candles(body.as_bytes()).unwrap();
        let chart = chart(&candles, timeframe);
        let range = high - low;
        let below = (low - chart.price_domain.start()) / range;
        let above = (chart.price_domain.end() - high) / range;
        assert!((0.05..=0.1).contains(&below), "{timeframe:?}: margin under the lowest wick is {below} of the range");
        assert!((0.05..=0.1).contains(&above), "{timeframe:?}: margin over the highest wick is {above} of the range");
        assert_eq!(chart.bars.len(), timeframe.spec().candles_shown);
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
fn every_zoom_stays_in_budget_and_draws_the_newest_candles() {
    for (body, timeframe, ..) in RECORDED {
        let candles = parse_candles(body.as_bytes()).unwrap();
        let base = f64::from(timeframe.spec().kraken_interval_minutes * 60);
        for zoom in Zoom::ALL {
            for budget in [DEFAULT_BUDGET, 7, 1] {
                let case = format!("{timeframe:?} {zoom:?} budget {budget}");
                let chart = CandleChart::new(&candles, timeframe, zoom, budget).unwrap();
                let visible = zoom.visible_count(timeframe, candles.len());
                let per_bar = (visible as f64 / budget as f64).ceil() as usize;
                assert!(chart.bars.len() <= budget, "{case}");
                assert_eq!(
                    chart.bars.len(),
                    visible / per_bar,
                    "{case}: only a partial run at the oldest end is left out"
                );
                assert_eq!(chart.candle_count, chart.bars.len() * per_bar, "{case}");
                let drawn = &candles[candles.len() - chart.candle_count..];
                let newest = candles.last().unwrap();
                assert_eq!(chart.time_domain, drawn[0].time..=newest.time.plus(base), "{case}");
                assert_eq!(chart.last_price, newest.close, "{case}");

                for (bar, run) in chart.bars.iter().zip(drawn.chunks(per_bar)) {
                    let (open, close) = (run[0].open, run[run.len() - 1].close);
                    assert_eq!(bar.time, run[0].time.plus(base * per_bar as f64 / 2.0), "{case}");
                    assert_eq!(bar.low, run.iter().map(|candle| candle.low).fold(f64::INFINITY, f64::min), "{case}");
                    assert_eq!(bar.high, run.iter().map(|candle| candle.high).fold(0.0, f64::max), "{case}");
                    assert!(((bar.body_low + bar.body_high) / 2.0 - (open + close) / 2.0).abs() < 1e-6, "{case}");
                    assert!(bar.body_high - bar.body_low >= (close - open).abs() - 1e-6, "{case}");
                    let tone = if close > open {
                        Tone::Up
                    } else if close < open {
                        Tone::Down
                    } else {
                        Tone::Neutral
                    };
                    assert_eq!(bar.tone, tone, "{case}");
                }

                let low = drawn.iter().map(|candle| candle.low).fold(f64::INFINITY, f64::min);
                let high = drawn.iter().map(|candle| candle.high).fold(0.0, f64::max);
                let padding = (high - low) * CandleChart::PRICE_PADDING;
                assert!(
                    (chart.price_domain.start() - (low - padding)).abs() < 1e-6
                        && (chart.price_domain.end() - (high + padding)).abs() < 1e-6,
                    "{case}: the price axis fits the drawn candles and no others"
                );
            }
        }
    }
}

#[test]
fn a_bar_merges_its_run_from_first_open_to_last_close() {
    let candles = [
        wicked(10.0, 12.0, 13.0, 9.0, 0),
        wicked(12.0, 11.0, 14.0, 10.0, 1),
        wicked(11.0, 15.0, 15.0, 11.0, 2),
        wicked(15.0, 14.0, 16.0, 13.0, 3),
        wicked(14.0, 9.0, 15.0, 8.0, 4),
        wicked(9.0, 10.0, 11.0, 7.0, 5),
    ];
    let chart = CandleChart::new(&candles, Timeframe::M1, Zoom::Standard, 2).unwrap();
    let field = |read: fn(&ticker_core::Bar) -> f64| chart.bars.iter().map(read).collect::<Vec<_>>();
    assert_eq!(field(|bar| bar.low), [9.0, 7.0]);
    assert_eq!(field(|bar| bar.high), [15.0, 16.0]);
    assert_eq!(field(|bar| bar.body_low), [10.0, 10.0]);
    assert_eq!(field(|bar| bar.body_high), [15.0, 15.0]);
    assert_eq!(chart.bars.iter().map(|bar| bar.tone).collect::<Vec<_>>(), [Tone::Up, Tone::Down]);
    assert_eq!(chart.bars.iter().map(|bar| bar.time).collect::<Vec<_>>(), [T0.plus(90.0), T0.plus(270.0)]);
    assert_eq!(chart.time_domain, T0..=T0.plus(360.0));

    let mut uneven = candles.to_vec();
    uneven.push(candle(10.0, 13.0, 6));
    let uneven = CandleChart::new(&uneven, Timeframe::M1, Zoom::Standard, 2).unwrap();
    assert_eq!(
        (uneven.bars.len(), uneven.candle_count),
        (1, 4),
        "seven candles in runs of four leave the oldest three out"
    );
    let bar = uneven.bars[0];
    assert_eq!((bar.low, bar.high, bar.body_low, bar.body_high), (7.0, 16.0, 13.0, 15.0));
}

#[test]
fn a_spike_off_screen_does_not_stretch_the_price_axis() {
    let mut candles = vec![wicked(100.0, 101.0, 1000.0, 1.0, 0)];
    candles.extend((1..240).map(|minute| candle(100.0, 101.0, minute)));
    let standard = chart(&candles, Timeframe::M1);
    assert!(*standard.price_domain.start() > 99.0 && *standard.price_domain.end() < 102.0);
    let all = CandleChart::new(&candles, Timeframe::M1, Zoom::All, DEFAULT_BUDGET).unwrap();
    assert!(all.price_domain.contains(&1.0) && all.price_domain.contains(&1000.0));
}

#[test]
fn tone_follows_close_against_open() {
    let chart = chart(&[candle(10.0, 11.0, 0), candle(11.0, 10.0, 1), candle(10.0, 10.0, 2)], Timeframe::M1);
    assert_eq!(chart.bars.iter().map(|bar| bar.tone).collect::<Vec<_>>(), [Tone::Up, Tone::Down, Tone::Neutral]);
    assert_eq!((chart.bars[0].body_low, chart.bars[0].body_high), (10.0, 11.0));
    assert_eq!((chart.bars[1].body_low, chart.bars[1].body_high), (10.0, 11.0));
    assert_eq!(chart.last_price, 10.0);
}

#[test]
fn a_flat_candle_still_has_a_visible_body() {
    let bar = chart(&[wicked(100.0, 100.0, 120.0, 80.0, 0)], Timeframe::M1).bars[0];
    assert!(bar.body_high > bar.body_low);
    assert!(bar.body_low < 100.0 && bar.body_high > 100.0);
}

#[test]
fn a_series_with_no_range_still_has_a_domain() {
    let chart = chart(&[candle(80000.0, 80000.0, 0), candle(80000.0, 80000.0, 1)], Timeframe::M1);
    assert!(*chart.price_domain.start() < 80000.0 && *chart.price_domain.end() > 80000.0);
}

#[test]
fn bars_sit_mid_interval_and_the_time_domain_covers_the_last_interval() {
    let chart = chart(&[candle(1.0, 2.0, 0), candle(2.0, 3.0, 15)], Timeframe::M15);
    assert_eq!(
        chart.bars.iter().map(|bar| bar.time).collect::<Vec<_>>(),
        [T0.plus(450.0), T0.plus(15.0 * 60.0 + 450.0)]
    );
    assert_eq!(chart.time_domain, T0..=T0.plus(30.0 * 60.0));
}

#[test]
fn no_candles_no_chart() {
    for budget in [DEFAULT_BUDGET, 0] {
        assert_eq!(CandleChart::new(&[], Timeframe::H4, Zoom::Standard, budget), None);
    }
}

#[test]
fn each_rung_shows_a_multiple_of_the_standard_view_or_every_candle() {
    for (timeframe, counts) in [
        (Timeframe::M1, [30, 60, 120, 240, 480, 720]),
        (Timeframe::M15, [24, 48, 96, 192, 384, 720]),
        (Timeframe::H4, [22, 45, 90, 180, 360, 720]),
    ] {
        assert_eq!(Zoom::ALL.map(|zoom| zoom.visible_count(timeframe, 720)), counts, "{timeframe:?}");
    }
}

#[test]
fn no_rung_shows_more_candles_than_there_are_or_none_when_there_are_some() {
    assert_eq!(Zoom::ALL.map(|zoom| zoom.visible_count(Timeframe::M1, 100)), [30, 60, 100, 100, 100, 100]);
    assert_eq!(Zoom::ALL.map(|zoom| zoom.visible_count(Timeframe::H4, 1)), [1; 6]);
    assert_eq!(Zoom::ALL.map(|zoom| zoom.visible_count(Timeframe::M15, 0)), [0; 6]);
}

#[test]
fn stepping_walks_the_ladder_and_stops_at_its_ends() {
    let outward: Vec<Zoom> =
        std::iter::successors(Some(Zoom::Quarter), |zoom| (*zoom != Zoom::All).then(|| zoom.stepped(ZoomStep::Out)))
            .collect();
    assert_eq!(outward, [Zoom::Quarter, Zoom::Half, Zoom::Standard, Zoom::Double, Zoom::Quadruple, Zoom::All]);
    let inward: Vec<Zoom> =
        std::iter::successors(Some(Zoom::All), |zoom| (*zoom != Zoom::Quarter).then(|| zoom.stepped(ZoomStep::In)))
            .collect();
    assert_eq!(inward, outward.iter().rev().copied().collect::<Vec<_>>());
    assert_eq!(Zoom::Quarter.stepped(ZoomStep::In), Zoom::Quarter);
    assert_eq!(Zoom::All.stepped(ZoomStep::Out), Zoom::All);
    assert_eq!(Zoom::ALL.map(Zoom::label), ["×1/4", "×1/2", "×1", "×2", "×4", "all"]);
}

#[test]
fn axes_have_round_ticks_inside_the_domain_at_every_zoom() {
    for (body, timeframe, ..) in RECORDED {
        let candles = parse_candles(body.as_bytes()).unwrap();
        for zoom in Zoom::ALL {
            let chart = CandleChart::new(&candles, timeframe, zoom, DEFAULT_BUDGET).unwrap();
            let prices = chart.price_ticks();
            assert!((3..=8).contains(&prices.len()));
            assert!(prices.iter().all(|price| chart.price_domain.contains(price)));
            let step = prices[1] - prices[0];
            let digit = step / 10f64.powf(step.log10().floor());
            assert!([1.0, 2.0, 5.0].iter().any(|nice| (digit - nice).abs() < 1e-9));
            for offset in [-18000, 0, 19800, 20700, 50400] {
                let times = chart.time_ticks(offset);
                assert!((3..=8).contains(&times.len()));
                assert!(times.iter().all(|time| chart.time_domain.contains(time)));
                let step = times[1].seconds_since(times[0]);
                assert_eq!(step % 60.0, 0.0);
                assert!(times.iter().all(|time| (time.unix_seconds() + f64::from(offset)) % step == 0.0));
            }
        }
    }
}

#[test]
fn ticks_preserve_the_mac_step_and_boundary_rules() {
    let mut chart = chart(&[candle(0.0, 0.0, 0)], Timeframe::M1);
    assert!(chart.price_ticks().is_empty());
    chart.price_domain = 0.0..=10.0;
    assert_eq!(chart.price_ticks(), [0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
    chart.price_domain = 80001.0..=80999.0;
    assert_eq!(chart.price_ticks(), [80200.0, 80400.0, 80600.0, 80800.0]);
    chart.time_domain = Timestamp::from_unix_seconds(0.0)..=Timestamp::from_unix_seconds(600.0);
    assert_eq!(
        chart.time_ticks(0).iter().map(|time| time.unix_seconds()).collect::<Vec<_>>(),
        [0.0, 120.0, 240.0, 360.0, 480.0, 600.0]
    );
    chart.time_domain = T0..=T0;
    assert!(chart.time_ticks(0).is_empty());
}

#[test]
fn pathological_remote_domains_cannot_allocate_unbounded_ticks() {
    let mut chart = chart(&[candle(100.0, 101.0, 0)], Timeframe::M1);
    chart.time_domain = Timestamp::from_unix_seconds(0.0)..=Timestamp::from_unix_seconds(1e12);
    assert_eq!(chart.time_ticks(0).len(), 1024);
    chart.time_domain = T0..=Timestamp::from_unix_seconds(f64::INFINITY);
    assert!(chart.time_ticks(0).is_empty());
    chart.price_domain = 0.0..=f64::INFINITY;
    assert!(chart.price_ticks().is_empty());
}
