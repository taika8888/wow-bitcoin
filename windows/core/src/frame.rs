//! What is on screen, derived from `App` state in one pass and used three ways: painted by `ticker-paint`,
//! hit-tested by `App` for pointer input, and published in the snapshot. One source, so a click target cannot drift
//! from what was drawn.
use crate::layout::{Point, Rect, ScreenArea};
use crate::market::Timeframe;
use crate::overlay::{MenuItem, PressTarget};
use crate::settings::Rgba;

/// Everything the shell knows about the desktop, re-read on WM_DISPLAYCHANGE, WM_DPICHANGED and WM_SETTINGCHANGE.
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    /// Primary first.
    pub screens: Vec<ScreenArea>,
    /// For the chart's time labels.
    pub utc_offset_seconds: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Physical pixels: the setting in DIPs times the monitor scale.
    pub px: f64,
}

/// Text metrics, implemented by `ticker_paint::Fonts` over the one embedded font, so layout and painting agree to
/// the pixel and the Mac tests measure what Windows will draw.
pub trait Measure {
    fn advance(&self, text: &str, style: TextStyle) -> f64;
    fn line_height(&self, style: TextStyle) -> f64;
}

impl<T: Measure + ?Sized> Measure for &T {
    fn advance(&self, text: &str, style: TextStyle) -> f64 {
        (**self).advance(text, style)
    }

    fn line_height(&self, style: TextStyle) -> f64 {
        (**self).line_height(style)
    }
}

/// Display list in window-local physical px, back to front.
#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    RoundRect {
        rect: Rect,
        radius: f64,
        color: Rgba,
    },
    Rect {
        rect: Rect,
        color: Rgba,
    },
    Line {
        from: Point,
        to: Point,
        width: f64,
        dash: Option<(f64, f64)>,
        color: Rgba,
    },
    /// `origin` is the top-left of the line box; the painter puts the baseline at its font's ascent below it.
    Text {
        origin: Point,
        text: String,
        style: TextStyle,
        color: Rgba,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub visible: bool,
    /// Screen rect of the layered window, in whole pixels.
    pub window: Rect,
    pub scale: f64,
    /// The rest are window-local.
    pub ticker: Rect,
    pub timeframes: Vec<(Timeframe, Rect)>,
    pub menu: Vec<(MenuItem, Rect)>,
    pub draw: Vec<Draw>,
    /// Bars laid out, for the snapshot: "loaded" with 0 bars drawn is a failure the harness can see.
    pub bars_drawn: usize,
}

impl Frame {
    /// `at` in screen px. Menu beats timeframe beats body; anything outside the window is Outside.
    pub fn hit(&self, at: Point) -> PressTarget {
        if !self.visible || !self.window.contains(at) {
            return PressTarget::Outside;
        }
        let local = Point {
            x: at.x - self.window.x,
            y: at.y - self.window.y,
        };
        if let Some((item, _)) = self.menu.iter().find(|(_, r)| r.contains(local)) {
            return PressTarget::Menu(*item);
        }
        if let Some((timeframe, _)) = self.timeframes.iter().find(|(_, r)| r.contains(local)) {
            return PressTarget::Timeframe(*timeframe);
        }
        PressTarget::Body
    }

    /// Named click targets in screen px: "ticker", each timeframe label while expanded, "menu:<item>" while open.
    pub fn targets(&self) -> Vec<(String, Rect)> {
        let origin = self.window.origin();
        let mut targets = vec![("ticker".to_string(), self.ticker.offset(origin))];
        targets.extend(
            self.timeframes
                .iter()
                .map(|(t, r)| (t.spec().label.to_string(), r.offset(origin))),
        );
        targets.extend(
            self.menu
                .iter()
                .map(|(item, r)| (format!("menu:{}", item.name()), r.offset(origin))),
        );
        targets
    }
}
