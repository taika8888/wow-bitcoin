mod support;

use support::{OHLC_1, OHLC_15, OHLC_240, TICKER};
use ticker_core::kraken::{self, KrakenError};
use ticker_core::{Candle, Quote, Time, Timeframe};

#[derive(Debug, PartialEq)]
enum Failure {
    Api,
    Malformed,
    InvalidValue,
}

fn failure<T: std::fmt::Debug>(result: Result<T, KrakenError>) -> Option<Failure> {
    match result {
        Ok(_) => None,
        Err(KrakenError::Api(_)) => Some(Failure::Api),
        Err(KrakenError::Malformed(_)) => Some(Failure::Malformed),
        Err(KrakenError::InvalidValue(_)) => Some(Failure::InvalidValue),
    }
}

fn ticker_body(last: &str, open: &str) -> Vec<u8> {
    format!(r#"{{"error":[],"result":{{"XXBTZUSD":{{"c":["{last}","0.1"],"o":"{open}"}}}}}}"#)
        .into_bytes()
}

fn ohlc_body(rows: &str) -> Vec<u8> {
    format!(r#"{{"error":[],"result":{{"XXBTZUSD":[{rows}],"last":1793847240}}}}"#).into_bytes()
}

mod ticker {
    use super::*;

    #[test]
    fn parses_the_recorded_ticker() {
        let at = Time(1_793_847_300.0);
        let quote = kraken::parse_quote(TICKER, at).unwrap();
        assert_eq!(
            quote,
            Quote {
                price: 80489.7,
                open_24h: 80440.8,
                fetched_at: at
            }
        );
        assert!((quote.change_24h() - (80489.7 - 80440.8) / 80440.8).abs() < 1e-12);
    }

    #[test]
    fn kraken_error_array_is_an_api_error() {
        let body = br#"{"error":["EQuery:Unknown asset pair"]}"#;
        assert_eq!(
            kraken::parse_quote(body, Time(0.0)),
            Err(KrakenError::Api(vec!["EQuery:Unknown asset pair".into()]))
        );
    }

    #[test]
    fn rejects_prices_that_are_not_finite_and_positive() {
        for last in ["nan", "NaN", "inf", "-80000", "0", "-0.0"] {
            assert_eq!(
                failure(kraken::parse_quote(&ticker_body(last, "80000"), Time(0.0))),
                Some(Failure::InvalidValue),
                "last price {last}"
            );
        }
    }

    #[test]
    fn rejects_a_negative_open() {
        assert_eq!(
            failure(kraken::parse_quote(&ticker_body("80000", "-1"), Time(0.0))),
            Some(Failure::InvalidValue)
        );
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
            assert_eq!(
                failure(kraken::parse_quote(body.as_bytes(), Time(0.0))),
                Some(Failure::Malformed),
                "{body}"
            );
        }
    }
}

mod ohlc {
    use super::*;

    #[test]
    fn keeps_the_newest_candles_oldest_first() {
        for (timeframe, body) in [
            (Timeframe::M1, OHLC_1),
            (Timeframe::M15, OHLC_15),
            (Timeframe::H4, OHLC_240),
        ] {
            let candles = kraken::parse_candles(body, timeframe).unwrap();
            assert_eq!(candles.len(), timeframe.spec().candles_kept);
            for pair in candles.windows(2) {
                assert_eq!(
                    pair[1].time.since(pair[0].time),
                    timeframe.interval_seconds(),
                    "{timeframe:?} candles are one interval apart"
                );
            }
            for candle in &candles {
                assert!(candle.low <= candle.open.min(candle.close));
                assert!(candle.high >= candle.open.max(candle.close));
            }
        }
    }

    #[test]
    fn newest_recorded_candle_is_last() {
        let candles = kraken::parse_candles(OHLC_1, Timeframe::M1).unwrap();
        assert_eq!(candles.last().unwrap().time, Time(1_793_847_300.0));
    }

    #[test]
    fn parses_a_row() {
        let body = ohlc_body(r#"[1793804100,"10","12","9","11","10.5","1.0",3]"#);
        assert_eq!(
            kraken::parse_candles(&body, Timeframe::M1).unwrap(),
            vec![Candle {
                time: Time(1_793_804_100.0),
                open: 10.0,
                high: 12.0,
                low: 9.0,
                close: 11.0
            }]
        );
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
                failure(kraken::parse_candles(&ohlc_body(row), Timeframe::M1)),
                Some(Failure::InvalidValue),
                "{row}"
            );
        }
    }

    #[test]
    fn rejects_short_rows() {
        assert_eq!(
            failure(kraken::parse_candles(
                &ohlc_body(r#"[1,"10","12"]"#),
                Timeframe::M1
            )),
            Some(Failure::Malformed)
        );
    }

    #[test]
    fn kraken_error_array_is_an_api_error() {
        let body = br#"{"error":["EGeneral:Too many requests"],"result":{}}"#;
        assert_eq!(
            failure(kraken::parse_candles(body, Timeframe::H4)),
            Some(Failure::Api)
        );
    }

    #[test]
    fn timeframe_registry() {
        let specs = Timeframe::ALL.map(Timeframe::spec);
        assert_eq!(specs.map(|s| s.kraken_interval_minutes), [1, 15, 240]);
        assert_eq!(specs.map(|s| s.candles_kept), [120, 96, 90]);
        assert_eq!(specs.map(|s| s.refresh_seconds), [15, 60, 300]);
        assert_eq!(
            kraken::ohlc_url(Timeframe::H4),
            "https://api.kraken.com/0/public/OHLC?pair=XBTUSD&interval=240"
        );
    }
}
