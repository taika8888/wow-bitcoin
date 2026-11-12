use std::collections::BTreeMap;

use serde::Serialize;

use crate::app::App;
use crate::geometry::Rect;

/// What only the shell knows about the overlay window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapshotWindow {
    pub hwnd: i64,
    /// Virtual-screen physical pixels, top-left origin, the same space as `GetWindowRect`.
    pub frame: Rect,
    /// Hit targets in the same space as `frame`: "ticker" always, and each timeframe label and the "chart" while
    /// expanded.
    pub targets: BTreeMap<String, Rect>,
    /// The most bars the drawn chart has room for, from `ExpandedLayout::max_bars`; 0 while collapsed.
    pub max_bars: usize,
    /// How often the overlay process became the foreground process. Clicking the overlay must never raise it.
    pub activations: u32,
    /// Presses on the overlay that did not come from the end-to-end check's own synthesized clicks.
    pub foreign_presses: u32,
}

/// What the app writes to `BTCTICKER_STATE_FILE` after every change, for the end-to-end check.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub text: String,
    pub tooltip: String,
    pub feed: &'static str,
    /// When the quote on screen was fetched, in seconds since the Unix epoch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<f64>,
    pub mode: &'static str,
    pub locked: bool,
    pub opacity: f64,
    pub visible: bool,
    pub hwnd: i64,
    pub frame: Rect,
    /// The chart's timeframe label and load, absent while collapsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeframe: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chart: Option<&'static str>,
    /// Bars on the chart, zero while collapsed. Runs of candles merge once the zoom shows more than fit.
    pub candles: usize,
    /// Candles those bars stand for.
    pub visible_candles: usize,
    pub max_bars: usize,
    pub targets: BTreeMap<String, Rect>,
    pub activations: u32,
    pub foreign_presses: u32,
}

impl Snapshot {
    pub fn new(app: &App, window: SnapshotWindow) -> Snapshot {
        let status = app.status();
        let expanded = app.expanded();
        let chart = app.chart(window.max_bars);
        Snapshot {
            text: app.display().text(),
            tooltip: app.tooltip(),
            feed: status.name(),
            fetched_at: status.quote().map(|quote| quote.fetched_at.unix_seconds()),
            mode: app.overlay().mode().name(),
            locked: app.settings().locked,
            opacity: app.settings().opacity,
            visible: app.overlay().is_visible(),
            hwnd: window.hwnd,
            frame: window.frame,
            timeframe: expanded.map(|(timeframe, _)| timeframe.spec().label),
            zoom: expanded.map(|_| app.settings().zoom.label()),
            chart: expanded.map(|(_, load)| load.name()),
            candles: chart.as_ref().map_or(0, |chart| chart.bars.len()),
            visible_candles: chart.as_ref().map_or(0, |chart| chart.candle_count),
            max_bars: window.max_bars,
            targets: window.targets,
            activations: window.activations,
            foreign_presses: window.foreign_presses,
        }
    }

    /// Sorted keys, so an unchanged state writes the same bytes and the shell can skip the write.
    pub fn json(&self) -> String {
        serde_json::to_value(self).map(|value| value.to_string()).unwrap_or_default()
    }
}
