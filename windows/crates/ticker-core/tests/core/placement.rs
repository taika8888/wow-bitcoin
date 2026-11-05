use ticker_core::{
    ExpandedLayout, Frontmost, Point, Rect, ScreenArea, ScreenPoint, Screens, Size, Timeframe, placement,
};

/// A 1728x1117 primary monitor with a 48 px taskbar along the bottom, and to its right a taller monitor
/// without a taskbar whose top sits 200 px above the primary's.
const LAPTOP: ScreenArea =
    ScreenArea { frame: Rect::new(0.0, 0.0, 1728.0, 1117.0), work: Rect::new(0.0, 0.0, 1728.0, 1069.0) };
const EXTERNAL: ScreenArea =
    ScreenArea { frame: Rect::new(1728.0, -200.0, 2560.0, 1440.0), work: Rect::new(1728.0, -200.0, 2560.0, 1440.0) };
const SIZE: Size = Size { width: 180.0, height: 27.0 };

fn screens() -> Screens {
    Screens { primary: LAPTOP, others: vec![EXTERNAL] }
}

fn placed(x: f64, y: f64, size: Size) -> Rect {
    placement::ticker_frame(Some(ScreenPoint { x, y }), size, &screens())
}

#[test]
fn nothing_saved_sits_top_center_of_the_primary_work_area_clear_of_the_minimap() {
    let frame = placement::ticker_frame(None, SIZE, &screens());
    assert_eq!(frame, Rect::new(864.0 - 90.0, 12.0, 180.0, 27.0));
}

#[test]
fn the_default_spot_follows_the_primary_monitor_wherever_it_is_listed() {
    let external_first = Screens { primary: LAPTOP, others: vec![EXTERNAL, EXTERNAL] };
    assert_eq!(placement::ticker_frame(None, SIZE, &external_first).min_x(), 774.0);
    let taskbar_on_top =
        Screens { primary: ScreenArea { work: Rect::new(0.0, 48.0, 1728.0, 1069.0), ..LAPTOP }, others: vec![] };
    assert_eq!(placement::ticker_frame(None, SIZE, &taskbar_on_top).min_y(), 60.0, "12 px below the taskbar");
}

#[test]
fn a_spot_well_inside_is_kept_exactly() {
    assert_eq!(placed(400.0, 500.0, SIZE), Rect::new(400.0, 500.0, 180.0, 27.0));
    assert_eq!(placed(3000.0, -150.0, SIZE), Rect::new(3000.0, -150.0, 180.0, 27.0));
}

#[test]
fn a_ticker_flush_right_stays_whole_when_its_text_widens() {
    let flush = placed(1728.0 - 180.0, 600.0, SIZE);
    let wider = placed(1728.0 - 180.0, 600.0, Size { width: 260.0, height: 43.0 });
    assert_eq!(flush.max_x(), 1728.0);
    assert_eq!((wider.max_x(), wider.width), (1728.0, 260.0), "grows leftward instead of past the edge");
}

#[test]
fn a_spot_hanging_off_the_work_area_is_pulled_in() {
    for (x, y) in [(-170.0, 600.0), (900.0, 1100.0), (900.0, -20.0), (1600.0, 1100.0)] {
        let frame = placed(x, y, SIZE);
        assert!(LAPTOP.work.contains_rect(&frame), "({x}, {y}) landed at {frame:?}, outside {:?}", LAPTOP.work);
        assert_eq!(frame.size(), SIZE);
    }
}

#[test]
fn a_ticker_straddling_two_monitors_moves_onto_the_one_holding_most_of_it() {
    assert!(EXTERNAL.work.contains_rect(&placed(1728.0 - 40.0, 300.0, SIZE)));
    assert!(LAPTOP.work.contains_rect(&placed(1728.0 - 140.0, 300.0, SIZE)));
}

#[test]
fn a_spot_on_a_monitor_that_is_gone_falls_back_to_the_default() {
    let default = placement::ticker_frame(None, SIZE, &screens());
    assert_eq!(placed(9000.0, 9000.0, SIZE), default);
    assert_eq!(placed(-180.0, 600.0, SIZE), default, "touching a monitor's edge is not being on it");
}

#[test]
fn a_ticker_larger_than_the_monitor_keeps_its_left_and_top_edges_visible() {
    let small = ScreenArea { frame: Rect::new(0.0, 0.0, 300.0, 200.0), work: Rect::new(0.0, 0.0, 300.0, 180.0) };
    let frame = placement::ticker_frame(
        Some(ScreenPoint { x: 100.0, y: 100.0 }),
        Size { width: 400.0, height: 300.0 },
        &Screens { primary: small, others: vec![] },
    );
    assert_eq!((frame.min_x(), frame.min_y()), (0.0, 0.0));
}

#[test]
fn a_drop_lands_whole_on_the_monitor_under_the_ticker_or_else_under_the_panel() {
    let off_bottom = Rect::new(500.0, 1060.0, 180.0, 27.0);
    assert_eq!(placement::dropped(&off_bottom, &off_bottom, &screens()), ScreenPoint { x: 500.0, y: 1069.0 - 27.0 });
    let ticker_off_screen = Rect::new(-400.0, 300.0, 180.0, 27.0);
    let panel = Rect::new(-400.0, 300.0, 600.0, 300.0);
    assert_eq!(placement::dropped(&ticker_off_screen, &panel, &screens()), ScreenPoint { x: 0.0, y: 300.0 });
}

const WORK: Rect = Rect::new(0.0, 0.0, 1728.0, 1085.0);
const SEGMENT: Size = Size { width: 34.0, height: 19.0 };
const CHART: Size = Size { width: 360.0, height: 180.0 };
const CORNERS: [Point; 4] = [
    Point { x: 20.0, y: 20.0 },
    Point { x: 1500.0, y: 20.0 },
    Point { x: 20.0, y: 1040.0 },
    Point { x: 1530.0, y: 1046.0 },
];

fn layout(origin: Point) -> (ExpandedLayout, Rect) {
    let ticker = Rect::at(origin, Size { width: 182.0, height: 27.0 });
    (ExpandedLayout::new(ticker, SEGMENT, CHART, WORK), ticker)
}

#[test]
fn the_ticker_stays_put_and_the_chart_opens_toward_the_middle() {
    for origin in CORNERS {
        let (layout, ticker) = layout(origin);
        let ticker_on_screen = layout.ticker.offset(layout.frame.x, layout.frame.y);
        let chart_on_screen = layout.chart.offset(layout.frame.x, layout.frame.y);
        assert_eq!(ticker_on_screen, ticker);
        assert!(WORK.contains_rect(&layout.frame));
        if ticker.mid_y() < WORK.mid_y() {
            assert!(chart_on_screen.min_y() >= ticker.max_y(), "a ticker in the upper half opens downward");
        } else {
            assert!(chart_on_screen.max_y() <= ticker.min_y(), "a ticker in the lower half opens upward");
        }
        if ticker.mid_x() < WORK.mid_x() {
            assert!(
                layout.frame.min_x() == ticker.min_x() && layout.frame.max_x() > ticker.max_x(),
                "left half opens right"
            );
        } else {
            assert!(
                layout.frame.max_x() == ticker.max_x() && layout.frame.min_x() < ticker.min_x(),
                "right half opens left"
            );
        }
        assert!(layout.chart.height == CHART.height && layout.chart.width >= CHART.width);
    }
}

#[test]
fn timeframe_targets_are_distinct_and_hit_testable() {
    for origin in CORNERS {
        let (layout, _) = layout(origin);
        let panel = Rect::new(0.0, 0.0, layout.frame.width, layout.frame.height);
        let rects = Timeframe::ALL.map(|timeframe| layout.timeframe_rect(timeframe));
        assert!(rects.windows(2).all(|pair| pair[0].max_x() <= pair[1].min_x()), "1m, 15m, 4h read left to right");
        for (timeframe, rect) in Timeframe::ALL.into_iter().zip(rects) {
            assert!(panel.contains_rect(&rect));
            assert!(!rect.intersects(&layout.ticker) && !rect.intersects(&layout.chart));
            assert_eq!(layout.timeframe_at(Point { x: rect.mid_x(), y: rect.mid_y() }), Some(timeframe));
        }
        assert_eq!(layout.timeframe_at(Point { x: layout.ticker.mid_x(), y: layout.ticker.mid_y() }), None);
        assert!(!layout.chart.intersects(&layout.ticker));
    }
}

#[test]
fn a_small_work_area_keeps_the_panel_on_screen() {
    let small = Rect::new(0.0, 0.0, 400.0, 220.0);
    let ticker = Rect::new(210.0, 3.0, 182.0, 27.0);
    let fits = ExpandedLayout::new(ticker, SEGMENT, CHART, small);
    assert!(small.contains_rect(&fits.frame));
    let tiny = Rect::new(0.0, 0.0, 300.0, 150.0);
    let clamped = ExpandedLayout::new(ticker, SEGMENT, CHART, tiny);
    assert_eq!((clamped.frame.min_x(), clamped.frame.min_y()), (0.0, 0.0), "too small: the left and top edges stay in");
}

#[test]
fn the_foreground_process_is_classified_by_pid_then_executable_name() {
    const OURS: u32 = 4242;
    let wow = r"C:\Program Files (x86)\World of Warcraft\_retail_\Wow.exe";
    assert_eq!(Frontmost::classify(wow, 77, OURS), Frontmost::Wow);
    assert_eq!(
        Frontmost::classify(r"C:\Program Files (x86)\World of Warcraft\_forever_\Wow.exe", 77, OURS),
        Frontmost::Wow
    );
    for name in Frontmost::WOW_EXECUTABLES {
        assert_eq!(Frontmost::classify(&name.to_uppercase(), 77, OURS), Frontmost::Wow, "{name}, any case");
        assert_eq!(Frontmost::classify(&name.to_lowercase(), 77, OURS), Frontmost::Wow, "{name}, any case");
    }
    assert_eq!(Frontmost::classify(r"C:\Windows\explorer.exe", 77, OURS), Frontmost::Other);
    assert_eq!(Frontmost::classify("Wow.exe.bak", 77, OURS), Frontmost::Other);
    assert_eq!(Frontmost::classify(r"C:\Games\NotWow.exe", 77, OURS), Frontmost::Other);
    assert_eq!(Frontmost::classify("", 0, OURS), Frontmost::Other, "no foreground window");
    assert_eq!(Frontmost::classify(r"C:\Apps\btc-ticker.exe", OURS, OURS), Frontmost::Ticker);
}
