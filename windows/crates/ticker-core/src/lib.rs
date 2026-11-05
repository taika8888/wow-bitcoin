//! The BTC Ticker overlay's pure core. It reads no clock, opens no window and makes no request:
//! the shell passes time and results in, and acts on the effects that come back.

mod chart;
mod display;
mod feed;
pub mod kraken;
mod market;
mod settings;
mod time;

pub use chart::{Bar, CandleChart};
pub use display::{DisplaySegment, TickerDisplay, Tone};
pub use feed::{FeedSchedule, FeedStatus};
pub use kraken::{FetchFailure, HttpOutcome, KrakenError};
pub use market::{Candle, Quote, Spec, Timeframe};
pub use settings::{Rgba, ScreenPoint, Settings};
pub use time::Timestamp;
