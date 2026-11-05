use ticker_core::{
    FeedSchedule, FeedStatus, Quote, Rgba, ScreenPoint, Segment, Settings, TickerDisplay, Time,
    Timeframe, Tone, format_change, format_price,
};

const FETCHED_AT: Time = Time(1_793_847_300.0);
const QUOTE: Quote = Quote {
    price: 80482.4,
    open_24h: 80442.0,
    fetched_at: FETCHED_AT,
};

mod formatting {
    use super::*;

    #[test]
    fn price() {
        for (value, decimals, expected) in [
            (80482.4, 0, "$80,482"),
            (80482.456, 2, "$80,482.46"),
            (80482.45, 1, "$80,482.5"),
            (999.5, 0, "$1,000"),
            (1_234_567.891, 1, "$1,234,567.9"),
            (42.0, 2, "$42.00"),
        ] {
            assert_eq!(
                format_price(value, decimals),
                expected,
                "{value} at {decimals}"
            );
        }
    }

    #[test]
    fn change() {
        for (fraction, text, tone) in [
            (0.0005, "+0.05%", Tone::Up),
            (-0.0123, "-1.23%", Tone::Down),
            (0.12345, "+12.35%", Tone::Up),
            (0.0, "0.00%", Tone::Neutral),
            (0.00004, "0.00%", Tone::Neutral),
            (-0.00004, "0.00%", Tone::Neutral),
        ] {
            assert_eq!(
                format_change(fraction),
                (text.to_string(), tone),
                "{fraction}"
            );
        }
    }

    #[test]
    fn live_ticker_text() {
        let display = TickerDisplay::new(&FeedStatus::Live(QUOTE), &Settings::default());
        assert_eq!(display.text(), "BTC $80,482  +0.05%");
        assert_eq!(
            display.segments.last(),
            Some(&Segment::new("+0.05%", Tone::Up))
        );
        assert!(!display.dimmed);
    }

    #[test]
    fn label_and_change_are_optional() {
        let mut settings = Settings {
            show_label: false,
            decimals: 2,
            ..Settings::default()
        };
        assert_eq!(
            TickerDisplay::new(&FeedStatus::Live(QUOTE), &settings).text(),
            "$80,482.40  +0.05%"
        );
        settings.show_label = true;
        settings.show_change = false;
        assert_eq!(
            TickerDisplay::new(&FeedStatus::Live(QUOTE), &settings).text(),
            "BTC $80,482.40"
        );
    }

    #[test]
    fn flat_change_is_neutral() {
        let flat = Quote {
            price: 80442.01,
            ..QUOTE
        };
        let display = TickerDisplay::new(&FeedStatus::Live(flat), &Settings::default());
        assert_eq!(display.text(), "BTC $80,442  0.00%");
        assert!(display.segments.iter().all(|s| s.tone == Tone::Neutral));
    }

    #[test]
    fn not_live_is_dimmed_and_labelled() {
        let settings = Settings::default();
        let stale = TickerDisplay::new(&FeedStatus::Stale(QUOTE), &settings);
        assert_eq!(stale.text(), "BTC $80,482  +0.05%  stale");
        assert!(stale.dimmed);
        let offline = TickerDisplay::new(&FeedStatus::Offline, &settings);
        assert_eq!(offline.text(), "BTC offline");
        assert!(offline.dimmed);
        let loading = TickerDisplay::new(&FeedStatus::Loading, &settings);
        assert_eq!(loading.text(), "BTC ...");
        assert!(loading.dimmed);
    }
}

mod settings {
    use super::*;

    #[test]
    fn unreadable_or_empty_blobs_give_defaults() {
        for json in [
            None,
            Some(""),
            Some("not json"),
            Some("[1,2]"),
            Some("null"),
            Some("{}"),
            Some(r#"{"fontSize":"#),
        ] {
            assert_eq!(Settings::decode(json), Settings::default(), "{json:?}");
        }
    }

    #[test]
    fn missing_keys_take_defaults() {
        let expected = Settings {
            font_size: 30.0,
            locked: true,
            ..Settings::default()
        };
        assert_eq!(
            Settings::decode(Some(r#"{"fontSize":30,"locked":true,"someFutureKey":1}"#)),
            expected
        );
    }

    #[test]
    fn mistyped_keys_take_defaults_without_losing_the_rest() {
        let settings = Settings::decode(Some(
            r#"{"fontSize":"big","decimals":1.5,"textColor":{"red":1},"showLabel":false}"#,
        ));
        let defaults = Settings::default();
        assert_eq!(settings.font_size, defaults.font_size);
        assert_eq!(settings.decimals, defaults.decimals);
        assert_eq!(settings.text_color, defaults.text_color);
        assert!(!settings.show_label);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let settings = Settings::decode(Some(
            r#"{"fontSize":500,"opacity":-1,"decimals":7,"refreshSeconds":1,
                "upColor":{"red":2,"green":-1,"blue":0.5,"alpha":9}}"#,
        ));
        assert_eq!(settings.font_size, 72.0);
        assert_eq!(settings.opacity, 0.0);
        assert_eq!(settings.decimals, 2);
        assert_eq!(settings.refresh_seconds, 2);
        assert_eq!(
            settings.up_color,
            Rgba {
                red: 1.0,
                green: 0.0,
                blue: 0.5,
                alpha: 1.0
            }
        );
        assert_eq!(
            Settings::decode(Some(r#"{"fontSize":1,"refreshSeconds":100000}"#)).font_size,
            9.0
        );
        assert_eq!(
            Settings::decode(Some(r#"{"refreshSeconds":100000}"#)).refresh_seconds,
            300
        );
    }

    #[test]
    fn chart_size_is_clamped_to_its_ranges() {
        let small = Settings::decode(Some(r#"{"chartWidth":10,"chartHeight":-5}"#));
        assert_eq!((small.chart_width, small.chart_height), (240.0, 120.0));
        let large = Settings::decode(Some(r#"{"chartWidth":5000,"chartHeight":5000}"#));
        assert_eq!((large.chart_width, large.chart_height), (900.0, 600.0));
    }

    #[test]
    fn last_chosen_timeframe_loads_and_an_unknown_one_falls_back() {
        assert_eq!(
            Settings::decode(Some(r#"{"timeframe":"h4"}"#)).timeframe,
            Timeframe::H4
        );
        let unknown = Settings::decode(Some(r#"{"timeframe":"1w","fontSize":20}"#));
        assert_eq!(unknown.timeframe, Timeframe::M15);
        assert_eq!(unknown.font_size, 20.0);
    }

    #[test]
    fn defaults_match_the_product_spec() {
        let settings = Settings::default();
        assert_eq!(settings.refresh_seconds, 5);
        assert!(!settings.only_when_wow_active);
        assert!(!settings.locked);
        assert_eq!(settings.position, None);
        assert_eq!(settings.timeframe, Timeframe::M15);
    }

    #[test]
    fn round_trips() {
        let settings = Settings {
            position: Some(ScreenPoint { x: 120.0, y: 840.0 }),
            down_color: Rgba {
                red: 0.9,
                green: 0.1,
                blue: 0.2,
                alpha: 0.8,
            },
            only_when_wow_active: true,
            timeframe: Timeframe::M1,
            chart_width: 500.0,
            chart_height: 300.0,
            ..Settings::default()
        };
        assert_eq!(Settings::decode(Some(&settings.to_json())), settings);
    }

    #[test]
    fn the_blob_is_byte_identical_to_what_macos_writes() {
        assert_eq!(
            Settings::default().to_json(),
            r#"{"chartHeight":180,"chartWidth":360,"decimals":0,"downColor":{"alpha":1,"blue":0.42,"green":0.42,"red":1},"fontSize":15,"locked":false,"onlyWhenWoWActive":false,"opacity":0.55,"refreshSeconds":5,"showChange":true,"showLabel":true,"textColor":{"alpha":1,"blue":1,"green":1,"red":1},"timeframe":"m15","upColor":{"alpha":1,"blue":0.47,"green":0.86,"red":0.33}}"#
        );
    }
}

mod feed_status {
    use super::*;

    const START: Time = Time(1_000_000.0);

    #[test]
    fn without_a_quote_it_is_loading_then_offline() {
        let at = |dt: f64| FeedStatus::derive(None, START, START.plus(dt), 5.0);
        assert_eq!(at(0.0), FeedStatus::Loading);
        assert_eq!(at(15.0), FeedStatus::Loading);
        assert_eq!(at(15.001), FeedStatus::Offline);
    }

    #[test]
    fn a_quote_is_live_until_three_refreshes_are_missed() {
        let quote = Quote {
            price: 1.0,
            open_24h: 1.0,
            fetched_at: START.plus(100.0),
        };
        let at = |dt: f64, refresh: f64| {
            FeedStatus::derive(Some(&quote), START, START.plus(dt), refresh)
        };
        assert_eq!(at(100.0, 5.0), FeedStatus::Live(quote));
        assert_eq!(at(115.0, 5.0), FeedStatus::Live(quote));
        assert_eq!(at(115.001, 5.0), FeedStatus::Stale(quote));
        assert_eq!(at(130.0, 10.0), FeedStatus::Live(quote));
    }

    #[test]
    fn the_first_retry_keeps_the_cadence_then_backoff_doubles_and_caps_at_a_minute() {
        let delays: Vec<f64> = (0..=5).map(|f| FeedSchedule::delay(5, f)).collect();
        assert_eq!(delays, [5.0, 5.0, 10.0, 20.0, 40.0, 60.0]);
        assert_eq!(FeedSchedule::delay(5, 50), 60.0);
        assert_eq!(FeedSchedule::delay(120, 3), 120.0);
    }

    /// The feed loop's rule (a request starts at `next_start`, never before the previous one ended) run through
    /// good, failed, good, sampling the status every 100 ms in between.
    #[test]
    fn one_failed_request_never_shows_the_quote_as_stale() {
        for refresh_seconds in [2, 5, 30, 300] {
            for failure_takes in [0.2, -1.0] {
                let timeout = FeedSchedule::request_timeout(refresh_seconds);
                let failure = if failure_takes < 0.0 {
                    timeout
                } else {
                    failure_takes
                };
                let durations = [0.05, failure, timeout - 0.05];
                let mut schedule = FeedSchedule::default();
                let mut clock = START;
                let mut fetched = Vec::new();
                for (index, duration) in durations.into_iter().enumerate() {
                    let next = schedule.next_start(refresh_seconds);
                    if next > clock {
                        clock = next;
                    }
                    schedule.started(clock);
                    clock = clock.plus(duration);
                    schedule.finished(index != 1);
                    if index != 1 {
                        fetched.push(clock);
                    }
                }
                let quote = Quote {
                    price: 1.0,
                    open_24h: 1.0,
                    fetched_at: fetched[0],
                };
                let mut t = fetched[0];
                while t <= fetched[1] {
                    assert_eq!(
                        FeedStatus::derive(Some(&quote), START, t, refresh_seconds.into()),
                        FeedStatus::Live(quote),
                        "refresh {refresh_seconds} s, failure takes {failure} s: stale {} s after the last good quote, the next landed after {} s",
                        t.since(fetched[0]),
                        fetched[1].since(fetched[0])
                    );
                    t = t.plus(0.1);
                }
            }
        }
    }

    #[test]
    fn waking_cuts_the_backoff_short() {
        let mut schedule = FeedSchedule::default();
        schedule.started(START);
        for _ in 0..4 {
            schedule.finished(false);
        }
        assert_eq!(schedule.next_start(5), START.plus(40.0));
        schedule.woke();
        assert_eq!(schedule.next_start(5), START.plus(5.0));
    }

    #[test]
    fn a_new_refresh_interval_applies_to_the_wait_already_running() {
        let mut schedule = FeedSchedule::default();
        assert_eq!(
            schedule.next_start(300),
            Time::DISTANT_PAST,
            "the first request starts at once"
        );
        schedule.started(START);
        schedule.finished(true);
        assert_eq!(schedule.next_start(300), START.plus(300.0));
        assert_eq!(schedule.next_start(2), START.plus(2.0));
    }
}
