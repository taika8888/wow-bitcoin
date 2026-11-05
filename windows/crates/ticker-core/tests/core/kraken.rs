use ticker_core::kraken::{self, parse_candles, parse_quote};
use ticker_core::{Candle, FetchFailure, HttpOutcome, KrakenError, Quote, Timeframe, Timestamp};

use crate::fixtures;

const NOW: Timestamp = Timestamp::from_unix_seconds(1_793_847_300.0);

fn ticker_body(last: &str, open: &str) -> String {
    format!(r#"{{"error":[],"result":{{"XXBTZUSD":{{"c":["{last}","0.1"],"o":"{open}"}}}}}}"#)
}

fn ohlc_body(rows: &str) -> String {
    format!(r#"{{"error":[],"result":{{"XXBTZUSD":[{rows}],"last":1793847240}}}}"#)
}

#[derive(Debug, PartialEq)]
enum Failure {
    Api,
    Malformed,
    InvalidValue,
}

fn failure<T>(result: Result<T, KrakenError>) -> Option<Failure> {
    match result {
        Ok(_) => None,
        Err(KrakenError::Api(_)) => Some(Failure::Api),
        Err(KrakenError::Malformed(_)) => Some(Failure::Malformed),
        Err(KrakenError::InvalidValue(_)) => Some(Failure::InvalidValue),
    }
}

#[test]
fn parses_the_recorded_ticker() {
    let quote = parse_quote(fixtures::TICKER.as_bytes(), NOW).unwrap();
    assert_eq!(quote, Quote { price: 80489.7, open_24h: 80440.8, fetched_at: NOW });
    assert!((quote.change_24h() - (80489.7 - 80440.8) / 80440.8).abs() < 1e-12);
}

#[test]
fn kraken_error_array_is_an_api_error() {
    let body = br#"{"error":["EQuery:Unknown asset pair"]}"#;
    assert_eq!(parse_quote(body, NOW), Err(KrakenError::Api(vec!["EQuery:Unknown asset pair".to_string()])));
}

#[test]
fn rejects_prices_that_are_not_finite_and_positive() {
    for last in ["nan", "NaN", "inf", "-80000", "0", "-0.0"] {
        assert_eq!(
            failure(parse_quote(ticker_body(last, "80000").as_bytes(), NOW)),
            Some(Failure::InvalidValue),
            "{last}"
        );
    }
}

#[test]
fn rejects_a_negative_open() {
    assert_eq!(failure(parse_quote(ticker_body("80000", "-1").as_bytes(), NOW)), Some(Failure::InvalidValue));
}

#[test]
fn rejects_bodies_that_are_not_a_ticker() {
    for body in [
        "not json",
        r#"{"error":[]}"#,
        r#"{"error":[],"result":{}}"#,
        r#"{"error":[],"result":{"XXBTZUSD":{"c":[],"o":"1"}}}"#,
        r#"{"error":[],"result":{"XXBTZUSD":{"c":["abc"],"o":"1"}}}"#,
        r#"{"error":[],"result":{"XXBTZUSD":{"c":["1"],"o":"1"},"XETHZUSD":{"c":["1"],"o":"1"}}}"#,
    ] {
        assert_eq!(failure(parse_quote(body.as_bytes(), NOW)), Some(Failure::Malformed), "{body}");
    }
}

#[test]
fn keeps_the_newest_candles_oldest_first() {
    for (timeframe, body) in
        [(Timeframe::M1, fixtures::OHLC_1), (Timeframe::M15, fixtures::OHLC_15), (Timeframe::H4, fixtures::OHLC_240)]
    {
        let candles = parse_candles(body.as_bytes(), timeframe).unwrap();
        assert_eq!(candles.len(), timeframe.spec().candles_kept);
        let step = f64::from(timeframe.spec().kraken_interval_minutes * 60);
        for pair in candles.windows(2) {
            assert_eq!(pair[1].time.seconds_since(pair[0].time), step);
        }
        for candle in &candles {
            assert!(candle.low <= candle.open.min(candle.close));
            assert!(candle.high >= candle.open.max(candle.close));
        }
    }
}

#[test]
fn newest_recorded_candle_is_last() {
    let candles = parse_candles(fixtures::OHLC_1.as_bytes(), Timeframe::M1).unwrap();
    assert_eq!(candles.last().map(|candle| candle.time), Some(NOW));
}

#[test]
fn parses_a_row() {
    let candles =
        parse_candles(ohlc_body(r#"[1793804100,"10","12","9","11","10.5","1.0",3]"#).as_bytes(), Timeframe::M1);
    let time = Timestamp::from_unix_seconds(1_793_804_100.0);
    assert_eq!(candles, Ok(vec![Candle { time, open: 10.0, high: 12.0, low: 9.0, close: 11.0 }]));
}

#[test]
fn rejects_candles_whose_range_does_not_hold() {
    for row in [
        r#"[1,"10","9","12","11","10","1",1]"#,
        r#"[1,"10","10.5","9","11","10","1",1]"#,
        r#"[1,"10","12","10.5","11","10","1",1]"#,
        r#"[1,"10","12","9","nan","10","1",1]"#,
        r#"[1,"10","12","-9","11","10","1",1]"#,
    ] {
        assert_eq!(
            failure(parse_candles(ohlc_body(row).as_bytes(), Timeframe::M1)),
            Some(Failure::InvalidValue),
            "{row}"
        );
    }
}

#[test]
fn one_bad_row_rejects_the_response() {
    let rows = r#"[1,"10","12","9","11","10","1",1],[2,"10","12","9","11"],[3,"10","9","12","11","10","1",1]"#;
    assert_eq!(failure(parse_candles(ohlc_body(rows).as_bytes(), Timeframe::M1)), Some(Failure::InvalidValue));
}

#[test]
fn rejects_short_rows() {
    assert_eq!(
        failure(parse_candles(ohlc_body(r#"[1,"10","12"]"#).as_bytes(), Timeframe::M1)),
        Some(Failure::Malformed)
    );
}

#[test]
fn kraken_error_array_in_ohlc_is_an_api_error() {
    let body = br#"{"error":["EGeneral:Too many requests"],"result":{}}"#;
    assert_eq!(failure(parse_candles(body, Timeframe::H4)), Some(Failure::Api));
}

#[test]
fn timeframe_registry() {
    let specs = Timeframe::ALL.map(Timeframe::spec);
    assert_eq!(specs.map(|spec| spec.label), ["1m", "15m", "4h"]);
    assert_eq!(specs.map(|spec| spec.kraken_interval_minutes), [1, 15, 240]);
    assert_eq!(specs.map(|spec| spec.candles_kept), [120, 96, 90]);
    assert_eq!(specs.map(|spec| spec.refresh_seconds), [15, 60, 300]);
    assert_eq!(kraken::ohlc_url(Timeframe::H4), "https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval=240");
    assert_eq!(kraken::TICKER_URL, "https://api.kraken.com/0/public/Ticker?pair=XBTUSD");
}

#[test]
fn an_http_outcome_becomes_a_quote_or_says_why_not() {
    let answered = |status, body: &str| HttpOutcome::Answered { status, body: body.as_bytes().to_vec() };
    assert_eq!(kraken::quote_from(&answered(200, fixtures::TICKER), NOW).map(|quote| quote.price), Ok(80489.7));
    assert_eq!(kraken::quote_from(&answered(503, fixtures::TICKER), NOW), Err(FetchFailure::Status(503)));
    assert_eq!(kraken::quote_from(&HttpOutcome::Unreachable, NOW), Err(FetchFailure::Unreachable));
    assert!(matches!(
        kraken::candles_from(&answered(200, "<html>"), Timeframe::M1),
        Err(FetchFailure::Rejected(KrakenError::Malformed(_)))
    ));
    assert_eq!(
        kraken::candles_from(&answered(200, fixtures::OHLC_15), Timeframe::M15).map(|candles| candles.len()),
        Ok(96)
    );
}
