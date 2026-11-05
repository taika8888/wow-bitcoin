//! Kraken responses recorded for the macOS tests, read in place so both ports test against the same bytes.

pub const TICKER: &str = include_str!("../../../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
pub const OHLC_1: &str = include_str!("../../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
pub const OHLC_15: &str = include_str!("../../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
pub const OHLC_240: &str = include_str!("../../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");
