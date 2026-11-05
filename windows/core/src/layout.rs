//! Geometry in Windows virtual-screen physical pixels, top-left origin, y down. Per-monitor DPI is the `scale` of
//! each `ScreenArea`.
use crate::market::Timeframe;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(origin: Point, size: Size) -> Rect {
        Rect {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    pub fn origin(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    pub fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn mid(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }

    /// Half-open, like a Win32 RECT: the right and bottom edges are outside.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.max_x() && p.y >= self.y && p.y < self.max_y()
    }

    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.max_x() <= self.max_x()
            && other.max_y() <= self.max_y()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.intersection_area(other) > 0.0
    }

    pub fn intersection_area(&self, other: &Rect) -> f64 {
        let width = self.max_x().min(other.max_x()) - self.x.max(other.x);
        let height = self.max_y().min(other.max_y()) - self.y.max(other.y);
        if width > 0.0 && height > 0.0 {
            width * height
        } else {
            0.0
        }
    }

    pub fn offset(&self, by: Point) -> Rect {
        Rect {
            x: self.x + by.x,
            y: self.y + by.y,
            ..*self
        }
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            width: self.max_x().max(other.max_x()) - x,
            height: self.max_y().max(other.max_y()) - y,
        }
    }
}

/// One monitor: MONITORINFO.rcMonitor, rcWork, and GetDpiForMonitor / 96.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenArea {
    pub frame: Rect,
    pub work: Rect,
    pub scale: f64,
}

pub mod placement {
    use super::{Point, Rect, ScreenArea, Size};
    use crate::settings::ScreenPoint;

    /// Distance from the top of the work area for the default spot, in DIPs.
    pub const MARGIN: Size = Size {
        width: 16.0,
        height: 12.0,
    };

    /// The screen the rect overlaps most, or None when it overlaps none.
    pub fn screen_for<'a>(rect: &Rect, screens: &'a [ScreenArea]) -> Option<&'a ScreenArea> {
        screens
            .iter()
            .map(|screen| (screen, rect.intersection_area(&screen.frame)))
            .filter(|(_, area)| *area > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(screen, _)| screen)
    }

    /// The least move that puts `rect` inside `area`; one larger than the area keeps its left and top edges in.
    pub fn clamp(rect: Rect, area: &Rect) -> Rect {
        Rect {
            x: rect.x.min(area.max_x() - rect.width).max(area.x),
            y: rect.y.min(area.max_y() - rect.height).max(area.y),
            ..rect
        }
    }

    /// The saved origin kept whole inside the work area of the screen it overlaps most, or the top center of the
    /// primary screen (`screens[0]`; WoW keeps its minimap and buffs top right) when nothing is saved or the saved
    /// spot is on no screen.
    pub fn frame(saved: Option<ScreenPoint>, size: Size, screens: &[ScreenArea]) -> Rect {
        if let Some(saved) = saved {
            let rect = Rect::new(
                Point {
                    x: saved.x,
                    y: saved.y,
                },
                size,
            );
            if let Some(screen) = screen_for(&rect, screens) {
                return clamp(rect, &screen.work);
            }
        }
        let Some(primary) = screens.first() else {
            return Rect::new(Point::default(), size);
        };
        let work = primary.work;
        Rect::new(
            Point {
                x: (work.mid().x - size.width / 2.0).round(),
                y: work.y + (MARGIN.height * primary.scale).round(),
            },
            size,
        )
    }
}

/// The expanded overlay. The ticker line keeps its collapsed screen rect; the chart opens toward the screen middle.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedLayout {
    pub opens_down: bool,
    pub opens_right: bool,
    /// Window rect on screen, clamped into the work area only when the screen is too small to keep the ticker put.
    pub window: Rect,
    /// The rest are window-local.
    pub ticker: Rect,
    pub timeframes: [(Timeframe, Rect); 3],
    pub chart: Rect,
}

impl ExpandedLayout {
    /// In DIPs; the layout works in physical px, so these are multiplied by the monitor's scale.
    pub const INSET: f64 = 8.0;
    pub const GAP: f64 = 8.0;

    /// Every input in physical px.
    pub fn new(
        ticker_on_screen: Rect,
        segment: Size,
        chart: Size,
        work: &Rect,
        scale: f64,
    ) -> Self {
        let inset = (Self::INSET * scale).round();
        let gap = (Self::GAP * scale).round();
        let segments_width = segment.width * Timeframe::ALL.len() as f64;
        let width =
            (ticker_on_screen.width + gap + segments_width + inset).max(chart.width + inset * 2.0);
        let height = ticker_on_screen.height + chart.height + inset;
        let opens_down = ticker_on_screen.mid().y < work.mid().y;
        let opens_right = ticker_on_screen.mid().x < work.mid().x;

        let ticker = Rect {
            x: if opens_right {
                0.0
            } else {
                width - ticker_on_screen.width
            },
            y: if opens_down {
                0.0
            } else {
                height - ticker_on_screen.height
            },
            ..ticker_on_screen
        };
        let segments_x = if opens_right {
            ticker.max_x() + gap
        } else {
            ticker.x - gap - segments_width
        };
        let segment_y = (ticker.mid().y - segment.height / 2.0).round();
        let timeframes = std::array::from_fn(|index| {
            (
                Timeframe::ALL[index],
                Rect {
                    x: segments_x + index as f64 * segment.width,
                    y: segment_y,
                    width: segment.width,
                    height: segment.height,
                },
            )
        });
        let chart = Rect {
            x: inset,
            y: if opens_down { ticker.max_y() } else { inset },
            width: width - inset * 2.0,
            height: chart.height,
        };
        let window = placement::clamp(
            Rect {
                x: ticker_on_screen.x - ticker.x,
                y: ticker_on_screen.y - ticker.y,
                width,
                height,
            },
            work,
        );
        ExpandedLayout {
            opens_down,
            opens_right,
            window,
            ticker,
            timeframes,
            chart,
        }
    }

    pub fn timeframe_at(&self, local: Point) -> Option<Timeframe> {
        self.timeframes
            .iter()
            .find(|(_, rect)| rect.contains(local))
            .map(|(timeframe, _)| *timeframe)
    }
}
