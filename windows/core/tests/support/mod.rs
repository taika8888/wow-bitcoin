#![allow(dead_code)]

pub const TICKER: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
pub const OHLC_1: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
pub const OHLC_15: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
pub const OHLC_240: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");

use ticker_core::{Measure, TextStyle};

pub struct FakeMeasure;

impl Measure for FakeMeasure {
    fn advance(&self, text: &str, style: TextStyle) -> f64 {
        text.chars().count() as f64 * 0.6 * style.px
    }

    fn line_height(&self, style: TextStyle) -> f64 {
        1.25 * style.px
    }
}
