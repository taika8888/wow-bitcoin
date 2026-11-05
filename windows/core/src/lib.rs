//! Everything BTC Ticker decides, with no platform code. The Win32 shell feeds `Input`s to one `App`, applies the
//! `Effects` it returns, and paints the `Frame` it lays out.
mod chart;
mod display;
mod feed;
mod market;
mod overlay;
mod settings;

pub mod kraken;

pub use chart::{
    Bar, CandleChart, CandleLoad, ChartFeed, ChartNotice, FetchFailure, axis_ticks, time_label,
    time_ticks,
};
pub use display::{Segment, TickerDisplay, Tone, format_change, format_price};
pub use feed::{FeedSchedule, FeedStatus};
pub use kraken::KrakenError;
pub use market::{Candle, Quote, Time, Timeframe, TimeframeSpec};
pub use overlay::{
    DragPhase, Frontmost, MenuItem, OverlayEvent, OverlayMode, OverlayState, PressTarget, reduce,
};
pub use settings::{FORM, Field, FieldKind, Rgba, ScreenPoint, Settings};
