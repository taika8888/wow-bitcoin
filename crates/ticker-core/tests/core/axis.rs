use ticker_core::kraken::parse_candles;
use ticker_core::{CandleChart, Tick, Timeframe, Timestamp, Zoom, spaced_labels};

use crate::chart::{DEFAULT_BUDGET, candle};
use crate::fixtures;

const RECORDED: [(&str, Timeframe); 3] =
    [(fixtures::OHLC_1, Timeframe::M1), (fixtures::OHLC_15, Timeframe::M15), (fixtures::OHLC_240, Timeframe::H4)];

const GAP: f64 = 6.0;
const HOUR: i32 = 3600;
const DAY: f64 = 86_400.0;
/// 2026-11-03 14:00 UTC.
const NOV_3_1400_UTC: f64 = 1_793_714_400.0;
/// 2024-02-26 00:00 UTC, three days before a leap day.
const FEB_26_2024_UTC: f64 = 1_708_905_600.0;
/// 2026-03-08 07:00 UTC, when New York's clocks jump from 02:00 EST to 03:00 EDT.
const DST_START_2026_UTC: f64 = 1_772_953_200.0;

/// A chart whose only content is its time domain, from `start` to `end` in Unix seconds.
fn spanning(start: f64, end: f64) -> CandleChart {
    CandleChart {
        bars: Vec::new(),
        price_domain: 0.0..=1.0,
        time_domain: Timestamp::from_unix_seconds(start)..=Timestamp::from_unix_seconds(end),
        last_price: 0.0,
        candle_count: 0,
    }
}

fn priced(low: f64, high: f64) -> CandleChart {
    CandleChart { price_domain: low..=high, ..spanning(0.0, 1.0) }
}

fn labels<T>(ticks: &[Tick<T>]) -> Vec<&str> {
    ticks.iter().map(|tick| tick.label.as_str()).collect()
}

fn seconds(ticks: &[Tick<Timestamp>]) -> Vec<f64> {
    ticks.iter().map(|tick| tick.value.unix_seconds()).collect()
}

#[test]
fn the_axes_get_a_few_round_ticks_inside_the_domain() {
    for (body, timeframe) in RECORDED {
        let candles = parse_candles(body.as_bytes()).unwrap();
        for zoom in Zoom::ALL {
            let case = format!("{timeframe:?} {zoom:?}");
            let chart = CandleChart::new(&candles, timeframe, zoom, DEFAULT_BUDGET).unwrap();
            let prices: Vec<f64> = chart.price_ticks().iter().map(|tick| tick.value).collect();
            assert!((3..=8).contains(&prices.len()), "{case}: {} price ticks", prices.len());
            assert!(prices.iter().all(|price| chart.price_domain.contains(price)), "{case}: {prices:?}");
            let step = prices[1] - prices[0];
            let digit = step / 10f64.powf(step.log10().floor());
            assert!([1.0, 2.0, 5.0].iter().any(|nice| (digit - nice).abs() < 1e-9), "{case}: price step {step}");

            let utc_offset = -5 * HOUR;
            let times = seconds(&chart.time_ticks(utc_offset, false));
            assert!((3..=8).contains(&times.len()), "{case}: {} time ticks", times.len());
            let domain = chart.time_domain.start().unix_seconds()..=chart.time_domain.end().unix_seconds();
            assert!(times.iter().all(|time| domain.contains(time)), "{case}: {times:?}");
            let time_step = times[1] - times[0];
            assert_eq!(time_step % 60.0, 0.0, "{case}: time step {time_step}");
            assert!(
                times.iter().all(|time| (time + f64::from(utc_offset)) % time_step == 0.0),
                "{case}: ticks fall on round local times: {times:?}"
            );
        }
    }
}

#[test]
fn a_chart_with_no_span_has_no_ticks() {
    let chart = CandleChart::new(&[candle(0.0, 0.0, 0)], Timeframe::M1, Zoom::Standard, DEFAULT_BUDGET).unwrap();
    assert_eq!(chart.price_ticks(), []);
    assert_eq!(spanning(NOV_3_1400_UTC, NOV_3_1400_UTC).time_ticks(0, false), []);
}

#[test]
fn the_price_step_is_the_coarsest_one_two_or_five_that_leaves_three_ticks() {
    let twelve = priced(0.0, 12.0).price_ticks();
    assert_eq!(twelve.iter().map(|tick| tick.value).collect::<Vec<_>>(), [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0]);
    assert_eq!(labels(&priced(80_100.0, 86_900.0).price_ticks()), ["$82,000", "$84,000", "$86,000"]);
}

#[test]
fn a_four_month_chart_gets_monthly_ticks_not_weekly_ones() {
    let candles = parse_candles(fixtures::OHLC_240.as_bytes()).unwrap();
    let chart = CandleChart::new(&candles, Timeframe::H4, Zoom::All, DEFAULT_BUDGET).unwrap();
    let times = seconds(&chart.time_ticks(0, false));
    assert!((3..=5).contains(&times.len()), "{} ticks over {} candles", times.len(), candles.len());
    assert!(times.windows(2).all(|pair| pair[1] - pair[0] == 30.0 * DAY), "{times:?}");
}

#[test]
fn times_within_a_day_read_on_a_24_or_12_hour_clock() {
    let chart = spanning(NOV_3_1400_UTC, NOV_3_1400_UTC + 5400.0);
    assert_eq!(labels(&chart.time_ticks(0, false)), ["14:00", "14:30", "15:00", "15:30"]);
    assert_eq!(labels(&chart.time_ticks(0, true)), ["2:00 PM", "2:30 PM", "3:00 PM", "3:30 PM"]);
    assert_eq!(labels(&chart.time_ticks(-5 * HOUR, false)), ["09:00", "09:30", "10:00", "10:30"]);
}

#[test]
fn midnight_and_noon_read_twelve_on_a_12_hour_clock() {
    let midnight = spanning(NOV_3_1400_UTC + 9.0 * 3600.0, NOV_3_1400_UTC + 12.0 * 3600.0);
    assert_eq!(labels(&midnight.time_ticks(0, true)), ["11:00 PM", "12:00 AM", "1:00 AM", "2:00 AM"]);
    let noon = spanning(NOV_3_1400_UTC - 3.0 * 3600.0, NOV_3_1400_UTC);
    assert_eq!(labels(&noon.time_ticks(0, true)), ["11:00 AM", "12:00 PM", "1:00 PM", "2:00 PM"]);
}

#[test]
fn ticks_fall_on_round_hours_of_the_given_offset() {
    let india = 5 * HOUR + 1800;
    let ticks = spanning(NOV_3_1400_UTC, NOV_3_1400_UTC + 3.0 * 3600.0).time_ticks(india, false);
    assert_eq!(labels(&ticks), ["20:00", "21:00", "22:00"]);
    assert_eq!(seconds(&ticks), [1800.0, 5400.0, 9000.0].map(|after| NOV_3_1400_UTC + after));
}

#[test]
fn steps_of_a_day_or_more_read_as_month_and_day() {
    let leap = spanning(FEB_26_2024_UTC, FEB_26_2024_UTC + 5.0 * DAY);
    assert_eq!(labels(&leap.time_ticks(0, false)), ["Feb 26", "Feb 27", "Feb 28", "Feb 29", "Mar 1", "Mar 2"]);
    let new_york = leap.time_ticks(-5 * HOUR, true);
    assert_eq!(labels(&new_york), ["Feb 26", "Feb 27", "Feb 28", "Feb 29", "Mar 1"]);
    assert_eq!(seconds(&new_york)[0], FEB_26_2024_UTC + 5.0 * 3600.0, "local midnight in New York is 05:00 UTC");
}

/// The shell passes the offset at the chart's end. Ticks before a DST change keep that offset, so New York's 00:00 EST
/// reads "01:00", an hour off. docs/shared-core.md entry 12 records this as the accepted default.
#[test]
fn every_label_uses_the_one_offset_even_across_a_dst_change() {
    let chart = spanning(DST_START_2026_UTC - 2.0 * 3600.0, DST_START_2026_UTC + 2.0 * 3600.0);
    assert_eq!(labels(&chart.time_ticks(-4 * HOUR, false)), ["01:00", "02:00", "03:00", "04:00", "05:00"]);
}

#[test]
fn labels_with_room_between_them_are_all_drawn() {
    assert_eq!(spaced_labels(&[0.0..30.0, 40.0..70.0, 80.0..110.0], GAP), [true, true, true]);
}

#[test]
fn a_label_that_would_overlap_the_one_before_it_is_skipped() {
    assert_eq!(spaced_labels(&[0.0..30.0, 20.0..50.0], GAP), [true, false]);
}

#[test]
fn the_next_label_is_spaced_from_the_last_one_drawn_not_the_one_skipped() {
    assert_eq!(spaced_labels(&[0.0..30.0, 20.0..50.0, 40.0..70.0], GAP), [true, false, true]);
}

#[test]
fn a_label_needs_the_whole_gap_after_the_last_one_drawn() {
    assert_eq!(spaced_labels(&[0.0..30.0, 35.0..60.0], GAP), [true, false]);
    assert_eq!(spaced_labels(&[0.0..30.0, 36.0..60.0], GAP), [true, true]);
}

#[test]
fn a_crowded_row_keeps_every_label_that_fits() {
    let crowded: Vec<_> = (0..17).map(|index| f64::from(index) * 12.0).map(|left| left..left + 30.0).collect();
    let drawn = spaced_labels(&crowded, GAP);
    assert_eq!(drawn.iter().filter(|drawn| **drawn).count(), 6);
    assert!(drawn.iter().step_by(3).all(|drawn| *drawn), "every third label starts 36 past the last drawn: {drawn:?}");
}

#[test]
fn no_labels_draw_nothing() {
    assert_eq!(spaced_labels(&[], GAP), Vec::<bool>::new());
}
