use ticker_core::{
    ExpandedLayout, Point, Rect, ScreenArea, ScreenPoint, Size, Timeframe, placement,
};

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn laptop() -> ScreenArea {
    ScreenArea {
        frame: rect(0.0, 0.0, 1728.0, 1117.0),
        work: rect(0.0, 0.0, 1728.0, 1069.0),
        scale: 1.0,
    }
}

fn external() -> ScreenArea {
    ScreenArea {
        frame: rect(1728.0, -200.0, 2560.0, 1440.0),
        work: rect(1728.0, -200.0, 2560.0, 1392.0),
        scale: 1.0,
    }
}

const SIZE: Size = Size {
    width: 180.0,
    height: 27.0,
};

fn placed_sized(x: f64, y: f64, size: Size) -> Rect {
    placement::frame(Some(ScreenPoint { x, y }), size, &[laptop(), external()])
}

fn placed(x: f64, y: f64) -> Rect {
    placed_sized(x, y, SIZE)
}

mod ticker_placement {
    use super::*;

    #[test]
    fn nothing_saved_sits_top_center_of_the_first_screen_clear_of_the_minimap() {
        assert_eq!(
            placement::frame(None, SIZE, &[laptop(), external()]),
            rect(864.0 - 90.0, 12.0, 180.0, 27.0)
        );
    }

    #[test]
    fn a_spot_well_inside_is_kept_exactly() {
        assert_eq!(placed(400.0, 500.0), rect(400.0, 500.0, 180.0, 27.0));
        assert_eq!(placed(3000.0, 0.0), rect(3000.0, 0.0, 180.0, 27.0));
    }

    #[test]
    fn a_ticker_flush_right_stays_whole_when_its_text_widens() {
        let flush = placed(1728.0 - 180.0, 600.0);
        let wider = placed_sized(
            1728.0 - 180.0,
            600.0,
            Size {
                width: 260.0,
                height: 43.0,
            },
        );
        assert_eq!(flush.max_x(), 1728.0);
        assert_eq!(
            (wider.max_x(), wider.width),
            (1728.0, 260.0),
            "grows leftward instead of past the edge"
        );
    }

    #[test]
    fn a_spot_hanging_off_the_visible_area_is_pulled_in() {
        for (x, y) in [
            (-170.0, 600.0),
            (900.0, -10.0),
            (900.0, 1060.0),
            (1600.0, 1100.0),
        ] {
            let frame = placed(x, y);
            assert!(
                laptop().work.contains_rect(&frame),
                "{frame:?} left the work area"
            );
            assert_eq!(frame.size(), SIZE);
        }
    }

    #[test]
    fn a_ticker_straddling_two_screens_moves_onto_the_one_holding_most_of_it() {
        assert!(external().work.contains_rect(&placed(1728.0 - 40.0, 300.0)));
        assert!(laptop().work.contains_rect(&placed(1728.0 - 140.0, 300.0)));
    }

    /// Swift's `max(by:)` keeps the first of equal overlaps.
    #[test]
    fn a_ticker_split_evenly_between_two_screens_goes_to_the_first() {
        assert_eq!(placed(1728.0 - 90.0, 300.0).max_x(), 1728.0);
        let reversed = placement::frame(
            Some(ScreenPoint {
                x: 1728.0 - 90.0,
                y: 300.0,
            }),
            SIZE,
            &[external(), laptop()],
        );
        assert_eq!(reversed.x, 1728.0);
    }

    /// CGRect.intersection of rects that share only an edge or a corner is an empty rect, not null, so Swift still
    /// counts that screen.
    #[test]
    fn a_ticker_touching_a_screen_only_at_an_edge_is_pulled_onto_it() {
        let alone = [laptop()];
        let at = |x: f64, y: f64| placement::frame(Some(ScreenPoint { x, y }), SIZE, &alone);
        assert_eq!(at(1728.0, 300.0), rect(1728.0 - 180.0, 300.0, 180.0, 27.0));
        assert_eq!(
            at(1728.0, 1117.0),
            rect(1728.0 - 180.0, 1069.0 - 27.0, 180.0, 27.0)
        );
        assert_eq!(
            at(1729.0, 300.0),
            placement::frame(None, SIZE, &alone),
            "a gap is no overlap"
        );
    }

    #[test]
    fn a_spot_on_a_display_that_is_gone_falls_back_to_the_default() {
        assert_eq!(
            placed(9000.0, 9000.0),
            placement::frame(None, SIZE, &[laptop(), external()])
        );
    }

    #[test]
    fn a_ticker_wider_than_the_screen_keeps_its_left_and_top_edges_visible() {
        let small = ScreenArea {
            frame: rect(0.0, 0.0, 300.0, 200.0),
            work: rect(0.0, 0.0, 300.0, 180.0),
            scale: 1.0,
        };
        let frame = placement::frame(
            Some(ScreenPoint { x: 100.0, y: 100.0 }),
            Size {
                width: 400.0,
                height: 300.0,
            },
            &[small],
        );
        assert_eq!((frame.x, frame.y), (0.0, 0.0));
    }

    #[test]
    fn the_default_margin_scales_with_the_primary_monitor() {
        for (scale, top) in [(1.0, 12.0), (1.25, 15.0), (1.5, 18.0), (2.0, 24.0)] {
            let screen = ScreenArea { scale, ..laptop() };
            assert_eq!(
                placement::frame(None, SIZE, &[screen]).y,
                top,
                "scale {scale}"
            );
        }
    }
}

mod expanded_layout {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1728.0,
        height: 1085.0,
    };
    const SEGMENT: Size = Size {
        width: 34.0,
        height: 19.0,
    };
    const CHART: Size = Size {
        width: 360.0,
        height: 180.0,
    };
    const CORNERS: [(f64, f64); 4] = [
        (20.0, 20.0),
        (1500.0, 20.0),
        (20.0, 1038.0),
        (1530.0, 1040.0),
    ];

    fn layout(x: f64, y: f64, scale: f64) -> (ExpandedLayout, Rect) {
        let ticker = rect(x, y, 182.0, 27.0);
        (
            ExpandedLayout::new(ticker, SEGMENT, CHART, &SCREEN, scale),
            ticker,
        )
    }

    #[test]
    fn the_ticker_stays_put_and_the_chart_opens_toward_the_middle() {
        for (x, y) in CORNERS {
            let (layout, ticker) = layout(x, y, 1.0);
            let origin = layout.window.origin();
            let chart = layout.chart.offset(origin);
            assert_eq!(layout.ticker.offset(origin), ticker);
            assert!(SCREEN.contains_rect(&layout.window));
            if ticker.mid().y < SCREEN.mid().y {
                assert!(
                    chart.y >= ticker.max_y(),
                    "a ticker in the top half opens downward"
                );
            } else {
                assert!(
                    chart.max_y() <= ticker.y,
                    "a ticker in the bottom half opens upward"
                );
            }
            if ticker.mid().x < SCREEN.mid().x {
                assert!(
                    layout.window.x == ticker.x && layout.window.max_x() > ticker.max_x(),
                    "a ticker on the left opens rightward"
                );
            } else {
                assert!(
                    layout.window.max_x() == ticker.max_x() && layout.window.x < ticker.x,
                    "a ticker on the right opens leftward"
                );
            }
            assert!(layout.chart.height == CHART.height && layout.chart.width >= CHART.width);
        }
    }

    #[test]
    fn timeframe_targets_are_distinct_and_hit_testable() {
        for (x, y) in CORNERS {
            let (layout, _) = layout(x, y, 1.0);
            let panel = Rect::new(Point::default(), layout.window.size());
            let rects: Vec<Rect> = layout.timeframes.iter().map(|(_, r)| *r).collect();
            assert!(
                rects.windows(2).all(|w| w[0].max_x() <= w[1].x),
                "1m, 15m, 4h read left to right"
            );
            for (timeframe, rect) in layout.timeframes {
                assert!(panel.contains_rect(&rect));
                assert!(!rect.intersects(&layout.ticker) && !rect.intersects(&layout.chart));
                assert_eq!(layout.timeframe_at(rect.mid()), Some(timeframe));
            }
            assert_eq!(layout.timeframe_at(layout.ticker.mid()), None);
            assert!(!layout.chart.intersects(&layout.ticker));
            assert_eq!(
                layout.timeframes.map(|(t, _)| t),
                Timeframe::ALL,
                "one segment per timeframe"
            );
        }
    }

    #[test]
    fn a_screen_too_small_for_the_chart_keeps_the_panel_on_screen() {
        let small = rect(0.0, 0.0, 400.0, 220.0);
        let ticker = rect(210.0, 3.0, 182.0, 27.0);
        let layout = ExpandedLayout::new(ticker, SEGMENT, CHART, &small, 1.0);
        assert!(layout.window.x >= small.x && layout.window.y >= small.y);
    }

    #[test]
    fn insets_and_gaps_scale_with_the_monitor() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let (layout, _) = layout(20.0, 20.0, scale);
            let inset = ExpandedLayout::INSET * scale;
            assert_eq!(layout.chart.x, inset, "scale {scale}");
            assert_eq!(
                layout.timeframes[0].1.x - layout.ticker.max_x(),
                ExpandedLayout::GAP * scale,
                "scale {scale}"
            );
            assert_eq!(
                layout.window.height,
                27.0 + CHART.height + inset,
                "scale {scale}"
            );
        }
    }
}
