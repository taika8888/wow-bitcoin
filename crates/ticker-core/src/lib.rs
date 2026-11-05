//! The BTC Ticker overlay's pure core. It reads no clock, opens no window and makes no request:
//! the shell passes time and results in, and acts on the effects that come back.

mod app;
mod axis;
mod chart;
mod display;
mod feed;
mod geometry;
mod ink;
pub mod kraken;
mod layout;
mod market;
mod overlay;
mod settings;
mod snapshot;
mod time;

pub use app::{App, Effect, Input, RequestId};
pub use axis::spaced_labels;
pub use chart::{Bar, CandleChart, CandleLoad, CandleSeries, ChartNotice, FailedFetch};
pub use display::{DisplaySegment, TickerDisplay, Tone};
pub use feed::{FeedSchedule, FeedStatus};
pub use geometry::{Point, Rect, Size};
pub use ink::ChartInk;
pub use kraken::{FetchFailure, HttpOutcome, KrakenError};
pub use layout::{ExpandedLayout, ScreenArea, Screens, placement};
pub use market::{Candle, Quote, Spec, Timeframe, Zoom, ZoomStep};
pub use overlay::{DragPhase, Frontmost, OverlayEvent, OverlayMode, OverlayState, PressTarget};
pub use settings::{Rgba, ScreenPoint, Settings};
pub use snapshot::{Snapshot, SnapshotWindow};
pub use time::Timestamp;
