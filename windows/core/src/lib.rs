//! Everything BTC Ticker decides, with no platform code. The Win32 shell feeds `Input`s to one `App`, applies the
//! `Effects` it returns, and paints the `Frame` it lays out.
mod app;
mod chart;
mod display;
mod feed;
mod frame;
mod layout;
mod market;
mod overlay;
mod settings;
mod snapshot;

pub mod kraken;

pub use app::{App, Effects, HttpReply, Input, PointerInput, Request, ShellFacts};
pub use chart::{
    Bar, CandleChart, CandleLoad, ChartFeed, ChartNotice, FetchFailure, axis_ticks, time_label,
    time_ticks,
};
pub use display::{Segment, TickerDisplay, Tone, format_change, format_price};
pub use feed::{FeedSchedule, FeedStatus};
pub use frame::{Draw, Env, Frame, Measure, TextStyle};
pub use kraken::KrakenError;
pub use layout::{ExpandedLayout, Point, Rect, ScreenArea, Size, placement};
pub use market::{Candle, Quote, Time, Timeframe, TimeframeSpec};
pub use overlay::{
    DragPhase, Frontmost, MenuItem, OverlayEvent, OverlayMode, OverlayState, PressTarget, reduce,
};
pub use settings::{FORM, Field, FieldKind, Rgba, ScreenPoint, Settings};
pub use snapshot::{
    SEEDED_SETTINGS_KEY, SETTINGS_KEY, SETTINGS_VALUE, SYNTHETIC_INPUT_TAG, Snapshot, SnapshotFrame,
};
