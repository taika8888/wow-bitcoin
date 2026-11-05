//! Fixtures are the recorded Kraken responses next to the Swift tests, read in place so both platforms share them.
#![allow(dead_code)]

pub const TICKER: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
pub const OHLC_1: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
pub const OHLC_15: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
pub const OHLC_240: &[u8] =
    include_bytes!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");
