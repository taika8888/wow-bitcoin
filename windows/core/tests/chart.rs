mod support;

use support::{OHLC_1, OHLC_15, OHLC_240};
use ticker_core::{
    Candle, CandleChart, CandleLoad, ChartFeed, ChartNotice, FetchFailure, Frontmost, KrakenError,
    OverlayMode, OverlayState, Settings, Time, Timeframe, Tone, axis_ticks, kraken, time_label,
    time_ticks,
};

const T0: Time = Time(1_793_847_300.0);

fn candle(open: f64, close: f64, minute: f64) -> Candle {
    Candle {
        time: T0.plus(minute * 60.0),
        open,
        high: open.max(close),
        low: open.min(close),
        close,
    }
}

mod candle_chart {
    use super::*;

    /// Values read straight from the fixture JSON, not from the parser.
    #[test]
    fn recorded_candles_fit_the_padded_domain() {
        for (body, timeframe, low, high, up, down) in [
            (OHLC_1, Timeframe::M1, 80190.1, 80827.6, 62, 57),
            (OHLC_15, Timeframe::M15, 75625.0, 81179.6, 49, 47),
            (OHLC_240, Timeframe::H4, 75625.0, 84840.0, 42, 48),
        ] {
            let candles = kraken::parse_candles(body, timeframe).unwrap();
            let chart = CandleChart::new(&candles, timeframe).unwrap();
            let range = high - low;
            let below = (low - chart.price_domain.0) / range;
            let above = (chart.price_domain.1 - high) / range;
            assert!(
                (0.05..=0.1).contains(&below),
                "{timeframe:?}: margin under the lowest wick is {below} of the range"
            );
            assert!(
                (0.05..=0.1).contains(&above),
                "{timeframe:?}: margin over the highest wick is {above} of the range"
            );
            assert_eq!(chart.bars.len(), timeframe.spec().candles_kept);
            let count = |tone| chart.bars.iter().filter(|b| b.tone == tone).count();
            assert_eq!(
                (count(Tone::Up), count(Tone::Down)),
                (up, down),
                "{timeframe:?}"
            );
            assert_eq!(chart.last_price, 80489.7);
            for bar in &chart.bars {
                assert!(bar.time >= chart.time_domain.0 && bar.time <= chart.time_domain.1);
                let inside = |v: f64| v >= chart.price_domain.0 && v <= chart.price_domain.1;
                assert!(inside(bar.body_low) && inside(bar.body_high));
            }
        }
    }

    #[test]
    fn tone_follows_close_against_open() {
        let chart = CandleChart::new(
            &[
                candle(10.0, 11.0, 0.0),
                candle(11.0, 10.0, 1.0),
                candle(10.0, 10.0, 2.0),
            ],
            Timeframe::M1,
        )
        .unwrap();
        let tones: Vec<Tone> = chart.bars.iter().map(|b| b.tone).collect();
        assert_eq!(tones, [Tone::Up, Tone::Down, Tone::Neutral]);
        assert_eq!(
            (chart.bars[0].body_low, chart.bars[0].body_high),
            (10.0, 11.0)
        );
        assert_eq!(
            (chart.bars[1].body_low, chart.bars[1].body_high),
            (10.0, 11.0)
        );
        assert_eq!(chart.last_price, 10.0);
    }

    #[test]
    fn a_flat_candle_still_has_a_visible_body() {
        let flat = Candle {
            high: 120.0,
            low: 80.0,
            ..candle(100.0, 100.0, 0.0)
        };
        let bar = CandleChart::new(&[flat], Timeframe::M1).unwrap().bars[0];
        assert!(bar.body_high > bar.body_low);
        assert!(bar.body_low < 100.0 && bar.body_high > 100.0);
    }

    #[test]
    fn a_series_with_no_range_still_has_a_domain() {
        let chart = CandleChart::new(
            &[candle(80000.0, 80000.0, 0.0), candle(80000.0, 80000.0, 1.0)],
            Timeframe::M1,
        )
        .unwrap();
        assert!(chart.price_domain.0 < 80000.0 && chart.price_domain.1 > 80000.0);
    }

    #[test]
    fn bars_sit_mid_interval_and_the_time_domain_covers_the_last_interval() {
        let chart = CandleChart::new(
            &[candle(1.0, 2.0, 0.0), candle(2.0, 3.0, 15.0)],
            Timeframe::M15,
        )
        .unwrap();
        let times: Vec<Time> = chart.bars.iter().map(|b| b.time).collect();
        assert_eq!(times, [T0.plus(450.0), T0.plus(15.0 * 60.0 + 450.0)]);
        assert_eq!(chart.time_domain, (T0, T0.plus(30.0 * 60.0)));
    }

    #[test]
    fn no_candles_no_chart() {
        assert_eq!(CandleChart::new(&[], Timeframe::H4), None);
    }
}

fn overlay(mode: OverlayMode, visible: bool) -> OverlayState {
    let settings = Settings {
        only_when_wow_active: !visible,
        ..Settings::default()
    };
    OverlayState {
        mode,
        ..OverlayState::new(settings, Frontmost::Other)
    }
}

fn expanded(timeframe: Timeframe) -> OverlayState {
    overlay(OverlayMode::Expanded(timeframe), true)
}

mod chart_feed {
    use super::*;

    fn candles() -> Vec<Candle> {
        vec![candle(10.0, 11.0, 0.0)]
    }

    #[test]
    fn fetches_nothing_while_collapsed_or_hidden() {
        let mut feed = ChartFeed::default();
        assert_eq!(
            feed.begin_fetch(&overlay(OverlayMode::Collapsed, true), T0),
            None
        );
        assert_eq!(
            feed.begin_fetch(&overlay(OverlayMode::Expanded(Timeframe::M1), false), T0),
            None
        );
        assert_eq!(*feed.get(Timeframe::M1), CandleLoad::default());
    }

    #[test]
    fn never_overlaps_requests_for_one_timeframe() {
        let mut feed = ChartFeed::default();
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::M1), T0),
            Some(Timeframe::M1)
        );
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::M1), T0.plus(600.0)),
            None
        );
        assert!(
            feed.get(Timeframe::M1).in_flight() && feed.get(Timeframe::M1).fetched_at().is_none()
        );
    }

    #[test]
    fn refetches_only_once_the_refresh_interval_passes() {
        for timeframe in Timeframe::ALL {
            let mut feed = ChartFeed::default();
            let refresh = f64::from(timeframe.spec().refresh_seconds);
            feed.begin_fetch(&expanded(timeframe), T0);
            feed.finish(timeframe, Ok(candles()), T0);
            let load = feed.get(timeframe);
            assert_eq!(
                (load.candles(), load.fetched_at()),
                (candles().as_slice(), Some(T0))
            );
            assert_eq!(load.notice(timeframe), None);
            assert_eq!(
                feed.begin_fetch(&expanded(timeframe), T0.plus(refresh - 1.0)),
                None
            );
            assert_eq!(
                feed.begin_fetch(&expanded(timeframe), T0.plus(refresh)),
                Some(timeframe)
            );
            assert_eq!(
                feed.get(timeframe).candles(),
                candles().as_slice(),
                "a refresh keeps showing the last candles"
            );
        }
    }

    #[test]
    fn switching_back_refetches_only_when_stale() {
        let mut feed = ChartFeed::default();
        feed.begin_fetch(&expanded(Timeframe::M1), T0);
        feed.finish(Timeframe::M1, Ok(candles()), T0);
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::H4), T0.plus(1.0)),
            Some(Timeframe::H4)
        );
        feed.finish(Timeframe::H4, Ok(candles()), T0.plus(2.0));
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::M1), T0.plus(5.0)),
            None
        );
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::M1), T0.plus(20.0)),
            Some(Timeframe::M1)
        );
    }

    #[test]
    fn a_failure_keeps_the_last_candles_and_retries() {
        let mut feed = ChartFeed::default();
        feed.begin_fetch(&expanded(Timeframe::H4), T0);
        feed.finish(Timeframe::H4, Ok(candles()), T0);
        let later = T0.plus(300.0);
        feed.begin_fetch(&expanded(Timeframe::H4), later);
        feed.finish(Timeframe::H4, Err(FetchFailure::Unreachable), later);
        assert_eq!(feed.get(Timeframe::H4).name(), "failed");
        assert_eq!(feed.get(Timeframe::H4).candles(), candles().as_slice());
        let retry = CandleLoad::RETRY_AFTER_FAILURE;
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::H4), later.plus(retry - 1.0)),
            None
        );
        assert_eq!(
            feed.begin_fetch(&expanded(Timeframe::H4), later.plus(retry)),
            Some(Timeframe::H4)
        );
    }

    #[test]
    fn a_completion_with_no_request_in_flight_is_ignored() {
        let mut feed = ChartFeed::default();
        feed.finish(Timeframe::M15, Ok(candles()), T0);
        assert_eq!(*feed.get(Timeframe::M15), CandleLoad::default());
    }

    #[test]
    fn old_candles_are_marked_stale_from_a_failed_refresh_until_one_succeeds() {
        let mut feed = ChartFeed::default();
        let stale = Some(ChartNotice::Stale("Stale: Kraken is not answering".into()));
        feed.begin_fetch(&expanded(Timeframe::M1), T0);
        feed.finish(Timeframe::M1, Ok(candles()), T0);
        assert_eq!(feed.get(Timeframe::M1).notice(Timeframe::M1), None);
        let refresh = T0.plus(60.0);
        feed.begin_fetch(&expanded(Timeframe::M1), refresh);
        feed.finish(Timeframe::M1, Err(FetchFailure::Unreachable), refresh);
        assert_eq!(feed.get(Timeframe::M1).notice(Timeframe::M1), stale);
        feed.begin_fetch(
            &expanded(Timeframe::M1),
            refresh.plus(CandleLoad::RETRY_AFTER_FAILURE),
        );
        assert_eq!(
            feed.get(Timeframe::M1).notice(Timeframe::M1),
            stale,
            "still stale while the retry is out"
        );
        feed.finish(Timeframe::M1, Ok(candles()), refresh.plus(11.0));
        assert_eq!(feed.get(Timeframe::M1).notice(Timeframe::M1), None);
    }

    #[test]
    fn the_message_names_what_went_wrong() {
        for (failure, message) in [
            (FetchFailure::Unreachable, "Kraken is not answering"),
            (FetchFailure::Status(503), "Kraken answered HTTP 503"),
            (
                FetchFailure::Rejected(KrakenError::Malformed("OHLC row 3".into())),
                "Kraken sent candles this app cannot read",
            ),
            (
                FetchFailure::Rejected(KrakenError::InvalidValue(
                    "row 3 high/low do not bound open/close".into(),
                )),
                "Kraken sent candles this app cannot read",
            ),
            (
                FetchFailure::Rejected(KrakenError::Api(vec!["EGeneral:Too many requests".into()])),
                "Kraken refused: EGeneral:Too many requests",
            ),
        ] {
            let mut feed = ChartFeed::default();
            feed.begin_fetch(&expanded(Timeframe::H4), T0);
            feed.finish(Timeframe::H4, Err(failure), T0);
            assert_eq!(
                feed.get(Timeframe::H4).notice(Timeframe::H4),
                Some(ChartNotice::Placeholder(format!("{message}, retrying")))
            );
        }
    }

    #[test]
    fn with_nothing_to_draw_the_placeholder_says_why() {
        let mut feed = ChartFeed::default();
        assert_eq!(
            feed.get(Timeframe::M15).notice(Timeframe::M15),
            Some(ChartNotice::Placeholder("Loading 15m candles".into()))
        );
        feed.begin_fetch(&expanded(Timeframe::M15), T0);
        feed.finish(Timeframe::M15, Ok(Vec::new()), T0);
        assert_eq!(
            feed.get(Timeframe::M15).notice(Timeframe::M15),
            Some(ChartNotice::Placeholder("No 15m candles".into()))
        );
    }
}

mod axes {
    use super::*;

    #[test]
    fn price_ticks_are_nice_steps_strictly_inside_the_domain() {
        assert_eq!(
            axis_ticks((79000.0, 81000.0), 4),
            [79500.0, 80000.0, 80500.0]
        );
        assert_eq!(
            axis_ticks((80190.0, 80830.0), 4),
            [80200.0, 80400.0, 80600.0, 80800.0]
        );
        assert_eq!(axis_ticks((0.0, 1.0), 4), [0.25, 0.5, 0.75]);
        assert_eq!(axis_ticks((5.0, 5.0), 4), Vec::<f64>::new());
    }

    #[test]
    fn every_recorded_chart_gets_a_readable_number_of_price_ticks() {
        for (body, timeframe) in [
            (OHLC_1, Timeframe::M1),
            (OHLC_15, Timeframe::M15),
            (OHLC_240, Timeframe::H4),
        ] {
            let chart =
                CandleChart::new(&kraken::parse_candles(body, timeframe).unwrap(), timeframe)
                    .unwrap();
            let ticks = axis_ticks(chart.price_domain, 4);
            assert!((2..=6).contains(&ticks.len()), "{timeframe:?}: {ticks:?}");
        }
    }

    #[test]
    fn time_ticks_fall_on_whole_local_steps() {
        let (ticks, step) = time_ticks((T0, T0.plus(2.0 * 3600.0)), 4, 0);
        assert_eq!(step, 1800.0);
        assert_eq!(
            ticks
                .iter()
                .map(|&t| time_label(t, step, 0))
                .collect::<Vec<_>>(),
            ["03:00", "03:30", "04:00", "04:30"]
        );
        let (ticks, step) = time_ticks((T0, T0.plus(2.0 * 3600.0)), 4, 3600);
        assert_eq!(
            time_label(ticks[0], step, 3600),
            "04:00",
            "an hour east of UTC"
        );
    }

    #[test]
    fn day_ticks_are_labelled_by_date() {
        let (ticks, step) = time_ticks((T0, T0.plus(15.0 * 86400.0)), 4, 0);
        assert_eq!(step, 7.0 * 86400.0);
        assert!(ticks.len() <= 4 && !ticks.is_empty());
        assert_eq!(time_label(T0, 86400.0, 0), "11/5");
        assert_eq!(time_label(Time(0.0), 86400.0, 0), "1/1");
        assert_eq!(
            time_label(Time(951_782_400.0), 86400.0, 0),
            "2/29",
            "leap day 2000"
        );
    }
}
