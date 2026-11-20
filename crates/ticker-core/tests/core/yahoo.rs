use ticker_core::yahoo::{self, parse_candles, parse_quote};
use ticker_core::{BodyError, Candle, FetchFailure, FetchProblem, HttpOutcome, Quote, Source, Timeframe, Timestamp};

use crate::fixtures;

const NOW: Timestamp = Timestamp::from_unix_seconds(1_795_111_775.0);

fn chart_body(timestamps: &str, open: &str, high: &str, low: &str, close: &str) -> String {
    format!(
        r#"{{"chart":{{"result":[{{"meta":{{}},"timestamp":[{timestamps}],"indicators":{{"quote":[{{"open":[{open}],"high":[{high}],"low":[{low}],"close":[{close}]}}]}}}}],"error":null}}}}"#
    )
}

fn quote_body(price: &str, previous_close: &str) -> String {
    format!(
        r#"{{"chart":{{"result":[{{"meta":{{"regularMarketPrice":{price},"chartPreviousClose":{previous_close}}}}}],"error":null}}}}"#
    )
}

fn extended_quote_body(closes: &str) -> String {
    format!(
        r#"{{"chart":{{"result":[{{"meta":{{"regularMarketPrice":86.94,"chartPreviousClose":97.17}},"indicators":{{"quote":[{{"close":[{closes}]}}]}}}}],"error":null}}}}"#
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
fn parses_the_recorded_quote_against_the_previous_close() {
    let quote = parse_quote(fixtures::YAHOO_QUOTE.as_bytes(), NOW).unwrap();
    assert_eq!(quote, Quote { price: 95.76, reference: 83.0, fetched_at: NOW });
}

/// After 4 PM the regular-session price stands still, while the newest close follows after-hours trades.
#[test]
fn the_quote_is_the_newest_close_extended_hours_included() {
    let price = |closes: &str| parse_quote(extended_quote_body(closes).as_bytes(), NOW).map(|quote| quote.price);
    assert_eq!(price("86.94,87.5,88.25"), Ok(88.25));
    assert_eq!(price("86.94,88.25,null"), Ok(88.25));
    assert_eq!(price("86.94,87.46499633789062"), Ok(87.465), "Yahoo's single-precision 87.465");
    assert_eq!(price("null,null"), Ok(86.94));
    assert_eq!(price(""), Ok(86.94));
    assert!(matches!(price("86.94,-1"), Err(BodyError::InvalidValue(_))));
    assert!(matches!(price("86.94,\"88\""), Err(BodyError::Malformed(_))));
    let reference = parse_quote(extended_quote_body("88.25").as_bytes(), NOW).map(|quote| quote.reference);
    assert_eq!(reference, Ok(97.17));
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

#[test]
fn drops_a_final_row_only_when_it_comes_less_than_an_interval_after_the_one_before() {
    let rows = |times: &str| chart_body(times, "1,2", "1,2", "1,2", "1,2");
    let times = |body: String| {
        parse_candles(body.as_bytes(), Timeframe::M1)
            .unwrap()
            .iter()
            .map(|candle| candle.time.unix_seconds())
            .collect::<Vec<_>>()
    };
    assert_eq!(times(rows("60,119")), [60.0]);
    assert_eq!(times(rows("60,120")), [60.0, 120.0]);
    assert_eq!(times(chart_body("60", "1", "1", "1", "1")), [60.0]);
}

/// Extended 4h candles start at 04:00, 08:00, 09:30, 13:30 and 16:00 New York time, so a final 16:00 candle comes
/// less than 4 hours after the one before. It has a range, so it is a candle, not the zero-range live tick.
#[test]
fn a_short_final_candle_with_a_range_is_kept() {
    let body = chart_body("0,9000", "1,2", "2,3", "1,1", "2,2");
    let times = parse_candles(body.as_bytes(), Timeframe::H4).unwrap();
    assert_eq!(times.iter().map(|candle| candle.time).collect::<Vec<_>>(), [at(0.0), at(9000.0)]);
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
    assert_eq!(yahoo::QUOTE_URL, format!("{chart}?interval=2m&range=1d&includePrePost=true"));
    assert_eq!(yahoo::chart_url(Timeframe::M1), format!("{chart}?interval=1m&range=5d&includePrePost=true"));
    assert_eq!(yahoo::chart_url(Timeframe::M15), format!("{chart}?interval=15m&range=1mo&includePrePost=true"));
    assert_eq!(yahoo::chart_url(Timeframe::H4), format!("{chart}?interval=4h&range=1y&includePrePost=true"));
}

#[test]
fn an_http_outcome_becomes_a_quote_or_names_yahoo_in_why_not() {
    let answered = |status, body: &str| HttpOutcome::Answered { status, body: body.as_bytes().to_vec() };
    let yahoo = Source::Yahoo;
    assert_eq!(yahoo.quote_from(&answered(200, fixtures::YAHOO_QUOTE), NOW).map(|quote| quote.price), Ok(95.76));
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
