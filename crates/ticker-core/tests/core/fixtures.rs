//! Kraken and Yahoo Finance responses recorded for the tests, read in place so both ports test against the same bytes.

pub const TICKER: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
pub const OHLC_1: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
pub const OHLC_15: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
pub const OHLC_240: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");

/// Yahoo Finance chart API responses for ANT, recorded on its first trading day, 2026-11-19.
pub const YAHOO_QUOTE: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/quote.json");
pub const YAHOO_1M: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/chart_1m.json");
pub const YAHOO_15M: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/chart_15m.json");
pub const YAHOO_4H: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/chart_4h.json");
