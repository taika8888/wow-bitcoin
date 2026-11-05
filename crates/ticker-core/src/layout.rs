use crate::geometry::{Point, Rect, Size};
use crate::market::Timeframe;
use crate::settings::ScreenPoint;

/// One monitor: its whole rect, and the work area the taskbar and docked app bars leave free.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenArea {
    pub frame: Rect,
    pub work: Rect,
}

/// Every monitor, with the primary one held apart so "the primary monitor" always exists.
#[derive(Clone, Debug, PartialEq)]
pub struct Screens {
    pub primary: ScreenArea,
    pub others: Vec<ScreenArea>,
}

impl Screens {
    /// The primary monitor first.
    pub fn iter(&self) -> impl Iterator<Item = &ScreenArea> {
        std::iter::once(&self.primary).chain(&self.others)
    }
}

/// The expanded overlay. The ticker line keeps the collapsed ticker's exact screen rect, and the chart opens toward
/// the middle of the screen: below a ticker in the upper half, to the right of one in the left half.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpandedLayout {
    pub opens_down: bool,
    pub opens_right: bool,
    /// The panel's rect on screen, clamped into the work area only when it is too small to keep the ticker put.
    pub frame: Rect,
    /// The rest are in panel coordinates, with the panel's top-left corner at (0, 0).
    pub ticker: Rect,
    timeframes: [Rect; 3],
    pub chart: Rect,
}

impl ExpandedLayout {
    pub const INSET: f64 = 8.0;
    pub const GAP: f64 = 8.0;
    /// Room the chart leaves for its trailing price labels, in device-independent pixels.
    pub const PRICE_AXIS_WIDTH: f64 = 60.0;
    /// The narrowest a candlestick gets before neighbours merge, in device-independent pixels. Counting in DIPs, as
    /// the macOS app counts in points, keeps the bar count the same at every display scale.
    pub const DIPS_PER_BAR: f64 = 2.0;

    /// `work` is the work area of the monitor the ticker sits on.
    pub fn new(ticker_frame: Rect, segment: Size, chart: Size, work: Rect) -> Self {
        let inset = Self::INSET;
        let segments_width = segment.width * Timeframe::ALL.len() as f64;
        let width = (ticker_frame.width + Self::GAP + segments_width + inset).max(chart.width + inset * 2.0);
        let height = ticker_frame.height + chart.height + inset;
        let opens_down = ticker_frame.mid_y() < work.mid_y();
        let opens_right = ticker_frame.mid_x() < work.mid_x();

        let ticker = Rect::new(
            if opens_right { 0.0 } else { width - ticker_frame.width },
            if opens_down { 0.0 } else { height - ticker_frame.height },
            ticker_frame.width,
            ticker_frame.height,
        );
        let segments_x =
            if opens_right { ticker.max_x() + Self::GAP } else { ticker.min_x() - Self::GAP - segments_width };
        let timeframes = Timeframe::ALL.map(|timeframe| {
            let x = segments_x + timeframe.index() as f64 * segment.width;
            Rect::new(x, ticker.mid_y() - segment.height / 2.0, segment.width, segment.height)
        });
        let chart =
            Rect::new(inset, if opens_down { ticker.max_y() } else { inset }, width - inset * 2.0, chart.height);

        let x = ticker_frame.min_x() - ticker.min_x();
        let y = ticker_frame.min_y() - ticker.min_y();
        let frame = Rect::new(
            x.min(work.max_x() - width).max(work.min_x()),
            y.min(work.max_y() - height).max(work.min_y()),
            width,
            height,
        );
        Self { opens_down, opens_right, frame, ticker, timeframes, chart }
    }

    /// How many bars the chart draws at most on a monitor at `scale` physical pixels per DIP, at least 1.
    pub fn max_bars(&self, scale: f64) -> usize {
        ((self.chart.width / scale - Self::PRICE_AXIS_WIDTH) / Self::DIPS_PER_BAR).max(1.0) as usize
    }

    pub fn timeframe_rect(&self, timeframe: Timeframe) -> Rect {
        self.timeframes[timeframe.index()]
    }

    /// The timeframe control under a point in panel coordinates.
    pub fn timeframe_at(&self, point: Point) -> Option<Timeframe> {
        Timeframe::ALL.into_iter().find(|timeframe| self.timeframe_rect(*timeframe).contains(point))
    }
}

/// Where the collapsed ticker sits on screen.
pub mod placement {
    use super::{Rect, ScreenArea, ScreenPoint, Screens, Size};

    /// Distance from the top of the primary monitor's work area to the default spot.
    pub const MARGIN_TOP: f64 = 12.0;

    /// The monitor the rect overlaps most, the primary one on a tie, or None when it overlaps none.
    pub fn screen_for(rect: &Rect, screens: &Screens) -> Option<ScreenArea> {
        let mut best: Option<(ScreenArea, f64)> = None;
        for screen in screens.iter() {
            if let Some(overlap) = rect.intersection(&screen.frame)
                && best.is_none_or(|(_, area)| overlap.area() > area)
            {
                best = Some((*screen, overlap.area()));
            }
        }
        best.map(|(screen, _)| screen)
    }

    /// Moves the rect the least distance that puts it inside the area; one larger than the area keeps its left and
    /// top edges in.
    pub fn clamp(rect: &Rect, area: &Rect) -> Rect {
        Rect::new(
            rect.min_x().min(area.max_x() - rect.width).max(area.min_x()),
            rect.min_y().min(area.max_y() - rect.height).max(area.min_y()),
            rect.width,
            rect.height,
        )
    }

    /// The saved origin, kept whole inside the work area of the monitor the ticker overlaps most, or the top center of
    /// the primary monitor's work area (WoW keeps its minimap and buffs top right) when nothing is saved or the saved
    /// spot is on no monitor.
    pub fn ticker_frame(saved: Option<ScreenPoint>, size: Size, screens: &Screens) -> Rect {
        if let Some(saved) = saved {
            let rect = Rect::new(saved.x, saved.y, size.width, size.height);
            if let Some(screen) = screen_for(&rect, screens) {
                return clamp(&rect, &screen.work);
            }
        }
        let work = screens.primary.work;
        Rect::new((work.mid_x() - size.width / 2.0).round(), work.min_y() + MARGIN_TOP, size.width, size.height)
    }

    /// Where a drag drops the ticker line: its rect on screen, pulled whole into the work area of the monitor it
    /// overlaps most, or of the monitor the panel overlaps most when the ticker line itself is on none.
    pub fn dropped(ticker: &Rect, panel: &Rect, screens: &Screens) -> ScreenPoint {
        let landed = screen_for(ticker, screens)
            .or_else(|| screen_for(panel, screens))
            .map_or(*ticker, |screen| clamp(ticker, &screen.work));
        ScreenPoint { x: landed.min_x(), y: landed.min_y() }
    }
}
