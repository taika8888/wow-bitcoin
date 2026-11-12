use ticker_core::{
    DisplaySegment, FeedSchedule, FeedStatus, FetchFailure, KrakenError, Quote, Rgba, ScreenPoint, Settings,
    TickerDisplay, Timeframe, Timestamp, Tone, Zoom,
};

const FETCHED_AT: Timestamp = Timestamp::from_unix_seconds(1_793_847_300.0);
const QUOTE: Quote = Quote { price: 80482.4, open_24h: 80442.0, fetched_at: FETCHED_AT };

#[test]
fn price_formatting() {
    for (value, decimals, expected) in [
        (80482.4, 0, "$80,482"),
        (80482.456, 2, "$80,482.46"),
        (80482.45, 1, "$80,482.5"),
        (999.5, 0, "$1,000"),
        (1_234_567.891, 1, "$1,234,567.9"),
        (42.0, 2, "$42.00"),
        (1.005, 2, "$1.01"),
        (0.4, 0, "$0"),
    ] {
        assert_eq!(TickerDisplay::price(value, decimals), expected, "{value} with {decimals} decimals");
    }
}

#[test]
fn change_formatting() {
    for (fraction, text, tone) in [
        (0.0005, "+0.05%", Tone::Up),
        (-0.0123, "-1.23%", Tone::Down),
        (0.12345, "+12.35%", Tone::Up),
        (0.0, "0.00%", Tone::Neutral),
        (0.00004, "0.00%", Tone::Neutral),
        (-0.00004, "0.00%", Tone::Neutral),
        (0.2, "+20.00%", Tone::Up),
        (-12.5, "-1250.00%", Tone::Down),
    ] {
        assert_eq!(TickerDisplay::change(fraction), (text.to_string(), tone), "{fraction}");
    }
}

#[test]
fn live_ticker_text() {
    let display = TickerDisplay::new(&FeedStatus::Live(QUOTE), &Settings::default(), FETCHED_AT);
    assert_eq!(display.text(), "BTC $80,482  +0.05%");
    assert_eq!(display.segments.last(), Some(&DisplaySegment::new("+0.05%", Tone::Up)));
    assert!(!display.dimmed);
}

#[test]
fn label_and_change_are_optional() {
    let mut settings = Settings { show_label: false, decimals: 2, ..Settings::default() };
    assert_eq!(TickerDisplay::new(&FeedStatus::Live(QUOTE), &settings, FETCHED_AT).text(), "$80,482.40  +0.05%");
    settings.show_label = true;
    settings.show_change = false;
    assert_eq!(TickerDisplay::new(&FeedStatus::Live(QUOTE), &settings, FETCHED_AT).text(), "BTC $80,482.40");
}

#[test]
fn flat_change_is_neutral() {
    let flat = Quote { price: 80442.01, open_24h: 80442.0, fetched_at: FETCHED_AT };
    let display = TickerDisplay::new(&FeedStatus::Live(flat), &Settings::default(), FETCHED_AT);
    assert_eq!(display.text(), "BTC $80,442  0.00%");
    assert!(display.segments.iter().all(|segment| segment.tone == Tone::Neutral));
}

#[test]
fn a_stale_age_counts_whole_seconds_then_minutes_hours_and_days() {
    for (seconds, age) in [
        (-2.0, "0s"),
        (0.0, "0s"),
        (15.9, "15s"),
        (59.99, "59s"),
        (60.0, "1m"),
        (3599.0, "59m"),
        (3600.0, "1h"),
        (86_399.0, "23h"),
        (86_400.0, "1d"),
        (10.0 * 86_400.0, "10d"),
    ] {
        assert_eq!(TickerDisplay::age(FETCHED_AT, FETCHED_AT.plus(seconds)), age, "{seconds} s old");
    }
}

#[test]
fn the_tooltip_gives_the_price_its_state_and_why_it_is_not_updating() {
    let settings = Settings::default();
    let unreachable = Some(&FetchFailure::Unreachable);
    let later = FETCHED_AT.plus(200.0);
    for (status, failure, tooltip) in [
        (FeedStatus::Live(QUOTE), None, "BTC Ticker · $80,482 · live"),
        (FeedStatus::Live(QUOTE), unreachable, "BTC Ticker · $80,482 · live"),
        (FeedStatus::Stale(QUOTE), unreachable, "BTC Ticker · $80,482 · stale 3m · Kraken is not answering"),
        (FeedStatus::Stale(QUOTE), None, "BTC Ticker · $80,482 · stale 3m"),
        (FeedStatus::Offline, Some(&FetchFailure::Status(503)), "BTC Ticker · offline · Kraken answered HTTP 503"),
        (FeedStatus::Loading, None, "BTC Ticker · loading"),
    ] {
        assert_eq!(TickerDisplay::tooltip(&status, failure, &settings, later), tooltip);
    }
    let two_decimals = Settings { decimals: 2, ..settings };
    let live = TickerDisplay::tooltip(&FeedStatus::Live(QUOTE), None, &two_decimals, FETCHED_AT);
    assert_eq!(live, "BTC Ticker · $80,482.40 · live");
}

#[test]
fn a_long_reason_is_cut_to_fit_the_tray_tooltip() {
    let refused = FetchFailure::Rejected(KrakenError::Api(vec!["EService:Unavailable ü".repeat(10)]));
    let tooltip = TickerDisplay::tooltip(&FeedStatus::Offline, Some(&refused), &Settings::default(), FETCHED_AT);
    assert_eq!(tooltip.encode_utf16().count(), TickerDisplay::TOOLTIP_MAX_UTF16);
    assert!(tooltip.starts_with("BTC Ticker · offline · Kraken refused: EService:Unavailable"));
}

#[test]
fn not_live_is_dimmed_and_labelled() {
    let settings = Settings::default();
    let stale = TickerDisplay::new(&FeedStatus::Stale(QUOTE), &settings, FETCHED_AT.plus(200.0));
    assert_eq!(stale.text(), "BTC $80,482  +0.05%  stale 3m");
    assert!(stale.dimmed);
    let offline = TickerDisplay::new(&FeedStatus::Offline, &settings, FETCHED_AT);
    assert_eq!(offline.text(), "BTC offline");
    assert!(offline.dimmed);
    let loading = TickerDisplay::new(&FeedStatus::Loading, &settings, FETCHED_AT);
    assert_eq!(loading.text(), "BTC ...");
    assert!(loading.dimmed);
}

#[test]
fn unreadable_or_empty_blobs_give_defaults() {
    for json in [None, Some(""), Some("not json"), Some("[1,2]"), Some("null"), Some("{}"), Some(r#"{"fontSize":"#)] {
        assert_eq!(Settings::decode(json), Settings::default(), "{json:?}");
    }
}

#[test]
fn missing_keys_take_defaults() {
    let expected = Settings { font_size: 30.0, locked: true, ..Settings::default() };
    assert_eq!(Settings::decode(Some(r#"{"fontSize":30,"locked":true,"someFutureKey":1}"#)), expected);
}

#[test]
fn mistyped_keys_take_defaults_without_losing_the_rest() {
    let settings = Settings::decode(Some(
        r#"{"fontSize":"big","decimals":1.5,"textColor":{"red":1},"showLabel":false,"locked":1,"position":{"x":"3","y":4}}"#,
    ));
    let defaults = Settings::default();
    assert_eq!(settings.font_size, defaults.font_size);
    assert_eq!(settings.decimals, defaults.decimals);
    assert_eq!(settings.text_color, defaults.text_color);
    assert_eq!(settings.locked, defaults.locked);
    assert_eq!(settings.position, None);
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
    assert_eq!(settings.up_color, Rgba::new(1.0, 0.0, 0.5, 1.0));
    assert_eq!(Settings::decode(Some(r#"{"fontSize":1,"refreshSeconds":100000}"#)).font_size, 9.0);
    assert_eq!(Settings::decode(Some(r#"{"refreshSeconds":100000}"#)).refresh_seconds, 300);
    assert_eq!(Settings::decode(Some(r#"{"refreshSeconds":-4,"decimals":-1}"#)).refresh_seconds, 2);
    assert_eq!(Settings::decode(Some(r#"{"decimals":-1}"#)).decimals, 0);
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
    assert_eq!(Settings::decode(Some(r#"{"timeframe":"h4"}"#)).timeframe, Timeframe::H4);
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
    assert_eq!(settings.zoom, Zoom::Standard);
}

#[test]
fn last_zoom_loads_and_an_unknown_or_missing_one_is_the_standard_view() {
    let zoom = |json: &str| Settings::decode(Some(json)).zoom;
    assert_eq!(zoom(r#"{"zoom":5}"#), Zoom::All);
    assert_eq!(zoom(r#"{"zoom":0}"#), Zoom::Quarter);
    for unknown in [r#"{"zoom":9}"#, r#"{"zoom":-1}"#, r#"{"zoom":2.5}"#, r#"{"zoom":"all"}"#, r#"{"timeframe":"h4"}"#]
    {
        assert_eq!(zoom(unknown), Zoom::Standard, "{unknown}");
    }
    assert_eq!(Settings::decode(Some(r#"{"zoom":9,"fontSize":20}"#)).font_size, 20.0);
}

#[test]
fn round_trips() {
    let settings = Settings {
        position: Some(ScreenPoint { x: 120.0, y: 840.0 }),
        down_color: Rgba::new(0.9, 0.1, 0.2, 0.8),
        only_when_wow_active: true,
        timeframe: Timeframe::M1,
        zoom: Zoom::Quarter,
        chart_width: 500.0,
        chart_height: 300.0,
        ..Settings::default()
    };
    assert_eq!(Settings::decode(Some(&settings.json())), settings);
}

#[test]
fn json_has_sorted_keys_and_the_mac_key_names() {
    let json = Settings { position: Some(ScreenPoint { x: 1.0, y: 2.0 }), ..Settings::default() }.json();
    let keys: Vec<&str> =
        json.match_indices("\":").map(|(end, _)| &json[json[..end].rfind('"').unwrap() + 1..end]).collect();
    let top_level = [
        "chartHeight",
        "chartWidth",
        "decimals",
        "downColor",
        "fontSize",
        "locked",
        "onlyWhenWoWActive",
        "opacity",
        "position",
        "refreshSeconds",
        "showChange",
        "showLabel",
        "textColor",
        "timeframe",
        "upColor",
        "zoom",
    ];
    let top: Vec<&str> = keys.iter().copied().filter(|key| top_level.contains(key)).collect();
    assert_eq!(top, top_level);
    assert!(json.contains(r#""timeframe":"m15""#));
    assert!(json.contains(r#""zoom":2"#), "a zoom saves as its rung, as the macOS app saves it");
    assert!(!Settings::default().json().contains("position"), "no saved position writes no key");
}

const START: Timestamp = Timestamp::from_unix_seconds(1_000_000.0);

#[test]
fn without_a_quote_it_is_loading_then_offline() {
    assert_eq!(FeedStatus::derive(None, START, START, 5.0), FeedStatus::Loading);
    assert_eq!(FeedStatus::derive(None, START, START.plus(15.0), 5.0), FeedStatus::Loading);
    assert_eq!(FeedStatus::derive(None, START, START.plus(15.001), 5.0), FeedStatus::Offline);
}

#[test]
fn a_quote_is_live_until_three_refreshes_are_missed() {
    let quote = Quote { price: 1.0, open_24h: 1.0, fetched_at: START.plus(100.0) };
    assert_eq!(FeedStatus::derive(Some(&quote), START, START.plus(100.0), 5.0), FeedStatus::Live(quote));
    assert_eq!(FeedStatus::derive(Some(&quote), START, START.plus(115.0), 5.0), FeedStatus::Live(quote));
    assert_eq!(FeedStatus::derive(Some(&quote), START, START.plus(115.001), 5.0), FeedStatus::Stale(quote));
    assert_eq!(FeedStatus::derive(Some(&quote), START, START.plus(130.0), 10.0), FeedStatus::Live(quote));
}

#[test]
fn the_first_retry_keeps_the_cadence_then_backoff_doubles_and_caps_at_a_minute() {
    let delays: Vec<f64> = (0..=5).map(|failures| FeedSchedule::delay(5, failures)).collect();
    assert_eq!(delays, [5.0, 5.0, 10.0, 20.0, 40.0, 60.0]);
    assert_eq!(FeedSchedule::delay(5, 50), 60.0);
    assert_eq!(FeedSchedule::delay(120, 3), 120.0);
}

#[test]
fn the_request_timeout_is_ten_seconds_or_the_interval_if_shorter() {
    assert_eq!(FeedSchedule::request_timeout(2).as_secs_f64(), 2.0);
    assert_eq!(FeedSchedule::request_timeout(5).as_secs_f64(), 5.0);
    assert_eq!(FeedSchedule::request_timeout(300).as_secs_f64(), 10.0);
}

/// Runs the feed loop's rule (a request starts at `next_start`, never before the previous one ended) through
/// good, failed, good, and samples the status every 100 ms in between.
#[test]
fn one_failed_request_never_shows_the_quote_as_stale() {
    for refresh_seconds in [2, 5, 30, 300] {
        for failure_takes in [0.2, -1.0] {
            let timeout = FeedSchedule::request_timeout(refresh_seconds).as_secs_f64();
            let durations = [0.05, if failure_takes < 0.0 { timeout } else { failure_takes }, timeout - 0.05];
            let mut schedule = FeedSchedule::new();
            let mut clock = START;
            let mut fetched = Vec::new();
            for (index, duration) in durations.into_iter().enumerate() {
                if let Some(next) = schedule.next_start(refresh_seconds) {
                    clock = clock.max(next);
                }
                schedule.started(clock);
                clock = clock.plus(duration);
                schedule.finished(index != 1);
                if index != 1 {
                    fetched.push(clock);
                }
            }
            let quote = Quote { price: 1.0, open_24h: 1.0, fetched_at: fetched[0] };
            let span = fetched[1].seconds_since(fetched[0]);
            let first_stale =
                (0..=(span * 10.0) as u32).map(|step| fetched[0].plus(f64::from(step) / 10.0)).find(|&now| {
                    FeedStatus::derive(Some(&quote), START, now, f64::from(refresh_seconds)) != FeedStatus::Live(quote)
                });
            assert_eq!(
                first_stale, None,
                "refresh {refresh_seconds}s: the next good quote landed {span}s after the last"
            );
        }
    }
}

#[test]
fn waking_cuts_the_backoff_short() {
    let mut schedule = FeedSchedule::new();
    schedule.started(START);
    for _ in 0..4 {
        schedule.finished(false);
    }
    assert_eq!(schedule.next_start(5), Some(START.plus(40.0)));
    schedule.woke();
    assert_eq!(schedule.next_start(5), Some(START.plus(5.0)));
}

#[test]
fn a_new_refresh_interval_applies_to_the_wait_already_running() {
    let mut schedule = FeedSchedule::new();
    assert_eq!(schedule.next_start(300), None, "the first request starts at once");
    schedule.started(START);
    schedule.finished(true);
    assert_eq!(schedule.next_start(300), Some(START.plus(300.0)));
    assert_eq!(schedule.next_start(2), Some(START.plus(2.0)));
}
