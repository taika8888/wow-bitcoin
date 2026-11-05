//! Everything BTC Ticker decides, with no platform code. The Win32 shell feeds `Input`s to one `App`, applies the
//! `Effects` it returns, and paints the `Frame` it lays out.
mod market;

pub mod kraken;

pub use kraken::KrakenError;
pub use market::{Candle, Quote, Time, Timeframe, TimeframeSpec};
