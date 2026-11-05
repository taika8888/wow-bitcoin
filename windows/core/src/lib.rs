//! Everything BTC Ticker decides, with no platform code. The Win32 shell feeds `Input`s to one `App`, applies the
//! `Effects` it returns, and paints the `Frame` it lays out.
mod display;
mod feed;
mod market;
mod settings;

pub mod kraken;

pub use display::{Segment, TickerDisplay, Tone, format_change, format_price};
pub use feed::{FeedSchedule, FeedStatus};
pub use kraken::KrakenError;
pub use market::{Candle, Quote, Time, Timeframe, TimeframeSpec};
pub use settings::{FORM, Field, FieldKind, Rgba, ScreenPoint, Settings};
