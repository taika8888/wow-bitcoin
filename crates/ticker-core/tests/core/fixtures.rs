//! Kraken and Yahoo Finance responses recorded for the tests, read in place so both ports test against the same bytes.

pub const TICKER: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
pub const OHLC_1: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
pub const OHLC_15: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
pub const OHLC_240: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");

/// The quote request's answer at 10:55 New York time on 2026-11-20, in the regular session.
pub const YAHOO_QUOTE: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/quote.json");
/// Friday 2026-11-20's last five pre-market minutes, recorded that morning, with the meta set to what Yahoo sends
/// before the open: Thursday's regular close of $97.17 at 16:00:01, and the IPO price as the previous close.
pub const YAHOO_QUOTE_PRE: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/quote_pre.json");
/// Thursday's last minutes of the regular session and of after hours, recorded on Friday, ending in the 19:59:58 tick
/// and with the meta set to what Yahoo sent at 02:07 Friday.
pub const YAHOO_QUOTE_POST: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/quote_post.json");

/// Yahoo Finance chart API responses for ANT, regular session only, recorded on its first trading day, 2026-11-19.
pub const YAHOO_1M: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/chart_1m.json");
pub const YAHOO_15M: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/chart_15m.json");
pub const YAHOO_4H: &str = include_str!("../../../../Tests/TickerCoreTests/Fixtures/yahoo/chart_4h.json");
