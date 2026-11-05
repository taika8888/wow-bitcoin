//! Written by the app to `BTCTICKER_STATE_FILE` after every presented frame and read by the e2e harness through
//! this same type. Field names follow the macOS Snapshot where they mean the same thing.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `dwExtraInfo` on every SendInput event the e2e harness injects. The overlay reads GetMessageExtraInfo on each
/// press and counts any other value as a foreign press.
pub const SYNTHETIC_INPUT_TAG: usize = 0xB7C7_1C4E;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Bumped on every snapshot, so the harness waits for `seq` to pass a value instead of sleeping.
    pub seq: u64,
    pub text: String,
    pub feed: String,
    /// When the quote on screen was fetched, in seconds since 1970.
    pub fetched_at: Option<f64>,
    pub mode: String,
    pub locked: bool,
    pub opacity: f64,
    pub visible: bool,
    pub menu_open: bool,
    /// HWND as an integer, checked against FindWindow, GetWindowRect and the z-order.
    pub hwnd: u64,
    pub frame: SnapshotFrame,
    pub scale: f64,
    /// The rest describe the chart and are absent or zero while collapsed.
    pub timeframe: Option<String>,
    pub chart: Option<String>,
    pub candles: usize,
    /// Open time of the newest candle drawn, so the harness can tell a live series from a stale one.
    pub newest_candle: Option<f64>,
    /// Click targets in screen px: "ticker", each timeframe label while expanded, "menu:<item>" while open.
    pub targets: BTreeMap<String, SnapshotFrame>,
    /// WM_ACTIVATE (WA_ACTIVE or WA_CLICKACTIVE) received by the overlay. Clicking it must never raise it.
    pub activations: u32,
    /// Left presses whose GetMessageExtraInfo is not `SYNTHETIC_INPUT_TAG`: someone touched the mouse mid-run.
    pub foreign_presses: u32,
    pub presses: u32,
    /// GWL_EXSTYLE as the app set it; the harness reads the real one and compares.
    pub ex_style: u32,
    /// Shell_NotifyIconW(NIM_ADD) succeeded.
    pub tray_icon: bool,
}

impl Snapshot {
    /// Sorted keys, like the macOS snapshot.
    pub fn to_json(&self) -> String {
        serde_json::to_value(self)
            .expect("Snapshot always serializes")
            .to_string()
    }
}
