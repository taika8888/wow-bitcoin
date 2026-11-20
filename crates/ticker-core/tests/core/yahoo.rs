use ticker_core::yahoo::{self, parse_candles, parse_quote};
use ticker_core::{
    BodyError, Candle, FetchFailure, FetchProblem, HttpOutcome, Quote, Session, Source, Timeframe, Timestamp,
};

use crate::fixtures;

const NOW: Timestamp = Timestamp::from_unix_seconds(1_795_111_775.0);

fn chart_body(timestamps: &str, open: &str, high: &str, low: &str, close: &str) -> String {
    format!(
        r#"{{"chart":{{"result":[{{"meta":{{}},"timestamp":[{timestamps}],"indicators":{{"quote":[{{"open":[{open}],"high":[{high}],"low":[{low}],"close":[{close}]}}]}}}}],"error":null}}}}"#
    )
}

/// Thursday 2026-11-19's regular session, 09:30 to 16:00 New York time.
const THURSDAY: &str = r#""tradingPeriods":{"regular":[[{"start":1795098600,"end":1795122000}]]}"#;

fn quote_meta(meta: &str) -> String {
    format!(r#"{{"chart":{{"result":[{{"meta":{{{meta}}}}}],"error":null}}}}"#)
}

fn quote_body(price: &str, previous_close: &str) -> String {
    quote_meta(&format!(
        r#""regularMarketPrice":{price},"chartPreviousClose":{previous_close},"regularMarketTime":1795122001,{THURSDAY}"#
    ))
}

/// Thursday's regular session closed at $97.17 in the 16:00:01 auction.
fn thursday_body(times: &str, closes: &str) -> String {
    format!(
        r#"{{"chart":{{"result":[{{"meta":{{"regularMarketPrice":97.17,"chartPreviousClose":83.0,"regularMarketTime":1795122001,{THURSDAY}}},"timestamp":[{times}],"indicators":{{"quote":[{{"close":[{closes}]}}]}}}}],"error":null}}}}"#
    )
}

fn at(seconds: f64) -> Timestamp {
    Timestamp::from_unix_seconds(seconds)
}

/// Yahoo sends single-precision prices widened to JSON numbers, so they compare at the cent.
fn cents(price: f64) -> f64 {
    (price * 100.0).round() / 100.0
}

#[test]
fn parses_the_recorded_regular_session_quote_against_the_previous_close() {
    let quote = parse_quote(fixtures::YAHOO_QUOTE.as_bytes(), NOW).unwrap();
    assert_eq!(quote, Quote { price: 86.77, reference: 97.17, session: Session::Regular, fetched_at: NOW });
}

#[test]
fn parses_an_after_hours_trade_against_the_last_regular_close() {
    let quote = parse_quote(fixtures::YAHOO_QUOTE_POST.as_bytes(), NOW).unwrap();
    assert_eq!(quote, Quote { price: 92.6, reference: 97.17, session: Session::Post, fetched_at: NOW });
}

#[test]
fn parses_a_pre_market_trade_against_the_last_regular_close() {
    let quote = parse_quote(fixtures::YAHOO_QUOTE_PRE.as_bytes(), NOW).unwrap();
    assert_eq!(quote, Quote { price: 89.85, reference: 97.17, session: Session::Pre, fetched_at: NOW });
}

#[test]
fn the_session_is_the_last_trade_s_unless_that_trade_is_the_regular_close() {
    let regular = (97.17, 83.0, Session::Regular);
    for (time, expected) in [
        (1_795_111_772.0, regular),
        (1_795_122_000.0, regular),
        (1_795_122_001.0, regular),
        (1_795_122_060.0, (92.6, 97.17, Session::Post)),
    ] {
        let quote = parse_quote(thursday_body(&time.to_string(), "92.6").as_bytes(), NOW).unwrap();
        assert_eq!((quote.price, quote.reference, quote.session), expected, "a trade at {time}");
    }
    let quote = parse_quote(thursday_body("1795136340,1795136398", "92.6,null").as_bytes(), NOW).unwrap();
    assert_eq!((quote.price, quote.session), (92.6, Session::Post), "the last row with a close");
    for (times, closes) in [("", ""), ("1795136340", "null")] {
        let quote = parse_quote(thursday_body(times, closes).as_bytes(), NOW).unwrap();
        assert_eq!((quote.price, quote.reference, quote.session), regular, "no trade in rows [{times}]");
    }
    let quote = parse_quote(quote_body("97.17", "83").as_bytes(), NOW).unwrap();
    assert_eq!((quote.price, quote.reference, quote.session), regular, "no rows at all");
}

#[test]
fn a_chart_error_is_an_api_error() {
    let body = br#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found, symbol may be delisted"}}}"#;
    assert_eq!(parse_quote(body, NOW), Err(BodyError::Api(vec!["No data found, symbol may be delisted".to_string()])));
    assert_eq!(
        parse_candles(body, Timeframe::M1),
        Err(BodyError::Api(vec!["No data found, symbol may be delisted".to_string()]))
    );
}

#[test]
fn rejects_quotes_that_are_not_finite_and_positive() {
    for (price, previous_close) in [("0", "83"), ("-1", "83"), ("95", "0"), ("95", "-83")] {
        assert!(
            matches!(parse_quote(quote_body(price, previous_close).as_bytes(), NOW), Err(BodyError::InvalidValue(_))),
            "{price} {previous_close}"
        );
    }
    let after_hours = thursday_body("1795136340", "0");
    assert!(matches!(parse_quote(after_hours.as_bytes(), NOW), Err(BodyError::InvalidValue(_))));
}

#[test]
fn rejects_bodies_that_are_not_a_chart() {
    for body in [
        "not json".to_string(),
        r#"{"finance":{}}"#.to_string(),
        r#"{"chart":{"result":null,"error":null}}"#.to_string(),
        r#"{"chart":{"result":[],"error":null}}"#.to_string(),
        r#"{"chart":{"result":[{}],"error":null}}"#.to_string(),
        quote_body("\"95\"", "83"),
        quote_body("null", "83"),
        quote_meta(&format!(r#""regularMarketPrice":95,"chartPreviousClose":83,{THURSDAY}"#)),
        quote_meta(r#""regularMarketPrice":95,"chartPreviousClose":83,"regularMarketTime":1795122001"#),
        quote_meta(
            r#""regularMarketPrice":95,"chartPreviousClose":83,"regularMarketTime":1795122001,"tradingPeriods":{"regular":[]}"#,
        ),
        quote_meta(
            r#""regularMarketPrice":95,"chartPreviousClose":83,"regularMarketTime":1795122001,"tradingPeriods":{"regular":[[{"start":1795098600}]]}"#,
        ),
        thursday_body("1795136340", "\"92.6\""),
    ] {
        assert!(matches!(parse_quote(body.as_bytes(), NOW), Err(BodyError::Malformed(_))), "{body}");
    }
}

/// Of the recorded rows, the ones with no trades are null and skipped, and the final zero-range row stamped with the
/// time of the request is dropped, so the newest kept candle is the one still forming. The recorded 1m forming candle
/// is null, so it takes the tick's $95.69.
#[test]
fn keeps_the_recorded_rows_with_trades_and_drops_the_live_tick() {
    for (timeframe, body, kept, newest, close) in [
        (Timeframe::M1, fixtures::YAHOO_1M, 83, 1_795_111_740.0, 95.69),
        (Timeframe::M15, fixtures::YAHOO_15M, 7, 1_795_111_200.0, 95.37),
        (Timeframe::H4, fixtures::YAHOO_4H, 1, 1_795_098_600.0, 95.37),
    ] {
        let candles = parse_candles(body.as_bytes(), timeframe).unwrap();
        assert_eq!(candles.len(), kept, "{timeframe:?}");
        assert_eq!(candles.last().map(|candle| candle.time), Some(at(newest)), "{timeframe:?}");
        assert_eq!(candles.last().map(|candle| cents(candle.close)), Some(close), "{timeframe:?}");
        for pair in candles.windows(2) {
            assert!(pair[1].time > pair[0].time, "{timeframe:?}: oldest first");
        }
    }
}

#[test]
fn a_4h_candle_keeps_its_session_aligned_time() {
    let candles = parse_candles(fixtures::YAHOO_4H.as_bytes(), Timeframe::H4).unwrap();
    let in_cents = |candle: &Candle| Candle {
        open: cents(candle.open),
        high: cents(candle.high),
        low: cents(candle.low),
        close: cents(candle.close),
        ..*candle
    };
    let opening = Candle { time: at(1_795_098_600.0), open: 83.0, high: 114.99, low: 83.0, close: 95.37 };
    assert_eq!(candles.iter().map(in_cents).collect::<Vec<_>>(), [opening]);
}

/// The quote's live answer is also a 4h chart of Friday so far: two pre-market candles at 04:00 and 08:00, then the
/// regular session's from 09:30, and the 10:55:21 tick, which is dropped.
#[test]
fn keeps_pre_market_4h_candles_on_their_session_boundaries() {
    let times = [1_795_165_200.0, 1_795_179_600.0, 1_795_185_000.0];
    assert_eq!(candle_times(fixtures::YAHOO_QUOTE, Timeframe::H4), times);
}

fn candle_times(body: &str, timeframe: Timeframe) -> Vec<f64> {
    parse_candles(body.as_bytes(), timeframe).unwrap().iter().map(|candle| candle.time.unix_seconds()).collect()
}

#[test]
fn drops_a_final_row_off_the_candle_grid_or_not_after_the_one_before() {
    let rows = |times: &str| chart_body(times, "1,2", "1,2", "1,2", "1,2");
    assert_eq!(candle_times(&rows("60,119"), Timeframe::M1), [60.0]);
    assert_eq!(candle_times(&rows("60,120"), Timeframe::M1), [60.0, 120.0]);
    assert_eq!(candle_times(&rows("60,60"), Timeframe::M1), [60.0]);
    assert_eq!(candle_times(&chart_body("60", "1", "1", "1", "1"), Timeframe::M1), [60.0]);
}

/// 09:30 New York time on 2026-11-20, a whole number of half hours since the epoch.
const OPEN: f64 = 1_795_185_000.0;

/// A trade more than an interval after the last candle, as when the forming interval has no row of its own yet, is
/// still the tick and not a candle of its own.
#[test]
fn drops_an_off_grid_tick_at_every_timeframe_however_late_it_comes() {
    for timeframe in Timeframe::ALL {
        for late in [17.0, f64::from(timeframe.spec().interval_minutes * 60) + 17.0] {
            let body = chart_body(&format!("{OPEN},{}", OPEN + late), "1,2", "1,2", "1,2", "1,2");
            assert_eq!(candle_times(&body, timeframe), [OPEN], "{timeframe:?} tick {late} s after the candle");
        }
    }
}

/// With no after-hours trades there is no tick, and the 4h answer ends on the 16:00 candle, 2.5 hours after the 13:30
/// one. Both are real.
#[test]
fn keeps_a_final_4h_candle_that_a_session_boundary_cut_short() {
    let (afternoon, close) = (OPEN + 4.0 * 3600.0, OPEN + 6.5 * 3600.0);
    let body = chart_body(&format!("{OPEN},{afternoon},{close}"), "1,2,3", "1,2,3", "1,2,3", "1,2,3");
    assert_eq!(candle_times(&body, Timeframe::H4), [OPEN, afternoon, close]);
}

#[test]
fn a_null_forming_candle_takes_the_live_tick_trade() {
    let body = chart_body("60,120,150", "1,null,2", "1,null,2", "1,null,2", "1,null,2");
    let candles = parse_candles(body.as_bytes(), Timeframe::M1).unwrap();
    let forming = Candle { time: at(120.0), open: 2.0, high: 2.0, low: 2.0, close: 2.0 };
    assert_eq!(candles.last(), Some(&forming));
    assert_eq!(candles.len(), 2);
}

#[test]
fn a_row_with_any_null_price_is_skipped() {
    let body = chart_body("60,120,180,240", "1,null,1,1", "2,2,null,2", "1,1,1,1", "2,2,2,2");
    let candles = parse_candles(body.as_bytes(), Timeframe::M1).unwrap();
    assert_eq!(candles.iter().map(|candle| candle.time).collect::<Vec<_>>(), [at(60.0), at(240.0)]);
}

#[test]
fn a_chart_with_no_timestamps_has_no_candles() {
    let body = br#"{"chart":{"result":[{"meta":{},"indicators":{"quote":[{}]}}],"error":null}}"#;
    assert_eq!(parse_candles(body, Timeframe::M15), Ok(Vec::new()));
}

#[test]
fn rejects_candles_whose_range_does_not_hold() {
    for (open, high, low, close) in
        [("10", "9", "12", "11"), ("10", "10.5", "9", "11"), ("10", "12", "-9", "11"), ("10", "12", "9", "0")]
    {
        let body = chart_body("60", open, high, low, close);
        assert!(
            matches!(parse_candles(body.as_bytes(), Timeframe::M1), Err(BodyError::InvalidValue(_))),
            "{open} {high} {low} {close}"
        );
    }
}

#[test]
fn rejects_candle_bodies_that_are_not_a_chart() {
    for body in [
        chart_body("\"60\"", "1", "1", "1", "1"),
        chart_body("60", "\"1\"", "1", "1", "1"),
        r#"{"chart":{"result":[{"timestamp":[60]}],"error":null}}"#.to_string(),
        r#"{"chart":{"result":[{"timestamp":[60],"indicators":{"quote":[{"open":[1]}]}}],"error":null}}"#.to_string(),
    ] {
        assert!(matches!(parse_candles(body.as_bytes(), Timeframe::M1), Err(BodyError::Malformed(_))), "{body}");
    }
}

#[test]
fn keeps_the_newest_720_candles() {
    let count = 800;
    let list = |value: &str| vec![value; count].join(",");
    let times: Vec<String> = (1..=count).map(|minute| (minute * 60).to_string()).collect();
    let body = chart_body(&times.join(","), &list("1"), &list("1"), &list("1"), &list("1"));
    let candles = parse_candles(body.as_bytes(), Timeframe::M1).unwrap();
    assert_eq!(candles.len(), 720);
    assert_eq!(candles.first().map(|candle| candle.time), Some(at(81.0 * 60.0)));
    assert_eq!(candles.last().map(|candle| candle.time), Some(at(800.0 * 60.0)));
}

#[test]
fn urls() {
    let chart = "https://query1.finance.yahoo.com/v8/finance/chart/ANT";
    assert_eq!(yahoo::QUOTE_URL, format!("{chart}?interval=4h&range=1d&includePrePost=true"));
    assert_eq!(yahoo::chart_url(Timeframe::M1), format!("{chart}?interval=1m&range=5d&includePrePost=true"));
    assert_eq!(yahoo::chart_url(Timeframe::M15), format!("{chart}?interval=15m&range=60d&includePrePost=true"));
    assert_eq!(yahoo::chart_url(Timeframe::H4), format!("{chart}?interval=4h&range=2y&includePrePost=true"));
}

#[test]
fn an_http_outcome_becomes_a_quote_or_names_yahoo_in_why_not() {
    let answered = |status, body: &str| HttpOutcome::Answered { status, body: body.as_bytes().to_vec() };
    let yahoo = Source::Yahoo;
    assert_eq!(yahoo.quote_from(&answered(200, fixtures::YAHOO_QUOTE), NOW).map(|quote| quote.price), Ok(86.77));
    let not_found = yahoo.quote_from(&answered(404, "{}"), NOW);
    assert_eq!(not_found, Err(FetchFailure { source: Source::Yahoo, problem: FetchProblem::Status(404) }));
    assert_eq!(not_found.unwrap_err().message(), "Yahoo Finance answered HTTP 404");
    let unreachable = yahoo.candles_from(&HttpOutcome::Unreachable, Timeframe::M1).unwrap_err();
    assert_eq!(unreachable.message(), "Yahoo Finance is not answering");
    let unreadable = yahoo.candles_from(&answered(200, "<html>"), Timeframe::M1).unwrap_err();
    assert_eq!(unreadable.message(), "Yahoo Finance sent data this app cannot read");
    let refused = yahoo.quote_from(&answered(200, r#"{"chart":{"result":null,"error":{"code":"Not Found"}}}"#), NOW);
    assert_eq!(refused.unwrap_err().message(), "Yahoo Finance refused: Not Found");
    assert_eq!(
        yahoo.candles_from(&answered(200, fixtures::YAHOO_15M), Timeframe::M15).map(|candles| candles.len()),
        Ok(7)
    );
}
