use btc_ticker_e2e::checks::{Check, Report, Verdict};
use btc_ticker_e2e::judge::{self, DesktopFacts, FocusFacts, Sample, Span, StyleFacts};
use btc_ticker_e2e::pixels::{self, STANDIN_FILL};
use ticker_core::{
    App, Env, FeedStatus, Frame, Frontmost, HttpReply, Input, PointerInput, Quote, Rect,
    ScreenArea, Settings, ShellFacts, Snapshot, TickerDisplay, Time, Timeframe,
};
use ticker_paint::Fonts;

const TICKER: &[u8] = include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
const OHLC_1: &[u8] = include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
const OHLC_15: &[u8] =
    include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
const OHLC_240: &[u8] =
    include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");
const T0: Time = Time(1_793_847_300.0);

fn pass() -> Verdict {
    Verdict::Pass(String::new())
}

mod report {
    use super::*;

    #[test]
    fn an_unreported_check_fails_the_run() {
        let mut report = Report::default();
        for check in &Check::ALL[1..] {
            report.record(*check, pass());
        }
        let (out, code) = report.finish();
        assert_ne!(code, 0);
        assert!(out.lines().next().unwrap().starts_with("FAIL"));
        assert!(out.lines().next().unwrap().contains("(not run)"));
        assert_eq!(out.lines().count(), Check::ALL.len());
    }

    #[test]
    fn every_check_passing_exits_zero() {
        let mut report = Report::default();
        for check in Check::ALL {
            report.record(*check, pass());
        }
        assert_eq!(report.finish().1, 0);
    }

    #[test]
    fn inconclusive_fails_the_run() {
        let mut report = Report::default();
        report.record(
            Check::Desktop,
            Verdict::Inconclusive("no input desktop".into()),
        );
        report.inconclusive_rest("desktop: no input desktop");
        let (out, code) = report.finish();
        assert_ne!(code, 0);
        assert!(out.lines().all(|l| l.starts_with("INCONCLUSIVE")));
    }

    #[test]
    fn a_check_reported_twice_fails() {
        let mut report = Report::default();
        report.record(Check::Launch, pass());
        assert!(!report.record(Check::Launch, pass()));
        assert_eq!(report.get(Check::Launch).unwrap().status(), "FAIL");
    }

    #[test]
    fn synthesized_input_checks_say_so_in_their_names() {
        for check in [
            Check::Expand,
            Check::Timeframe1m,
            Check::Collapse,
            Check::OpaqueToClicks,
        ] {
            assert!(check.name().contains("SendInput"), "{}", check.name());
        }
    }
}

mod ticker_text {
    use super::*;

    fn shown(settings: Settings, price: f64, open: f64) -> String {
        let quote = Quote {
            price,
            open_24h: open,
            fetched_at: T0,
        };
        TickerDisplay::new(&FeedStatus::Live(quote), &settings).text()
    }

    #[test]
    fn parses_what_ticker_display_writes() {
        let text = shown(Settings::default(), 80_482.4, 79_000.0);
        let parsed = judge::parse_ticker(&text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(
            (parsed.label, parsed.price, parsed.decimals, parsed.change),
            (true, 80_482.0, 0, true)
        );
        let edited = Settings {
            show_label: false,
            decimals: 2,
            ..Settings::default()
        };
        let text = shown(edited, 1_080_482.126, 1_100_000.0);
        let parsed = judge::parse_ticker(&text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(
            (parsed.label, parsed.price, parsed.decimals),
            (false, 1_080_482.13, 2)
        );
    }

    #[test]
    fn rejects_placeholders_and_bad_grouping() {
        for text in [
            "BTC ...",
            "BTC offline",
            "$80,48",
            "$1,2345",
            "BTC 80,482",
            "$80,482.",
            "$80,482  +1.2%",
        ] {
            assert_eq!(judge::parse_ticker(text), None, "{text}");
        }
    }

    fn snapshot(text: &str, feed: &str) -> Snapshot {
        let mut snapshot = driven(Settings::default())
            .0
            .snapshot(ShellFacts::default());
        snapshot.text = text.into();
        snapshot.feed = feed.into();
        snapshot
    }

    #[test]
    fn live_quote_compares_with_the_harness_fetch() {
        let live = snapshot("BTC $80,482  +1.23%", "live");
        assert!(judge::live_quote(&live, &Ok(80_100.0)).passed());
        assert_eq!(judge::live_quote(&live, &Ok(70_000.0)).status(), "FAIL");
        assert_eq!(
            judge::live_quote(&live, &Err("timeout".into())).status(),
            "INCONCLUSIVE"
        );
        let stale = snapshot("BTC $80,482  +1.23%  stale", "stale");
        assert_eq!(judge::live_quote(&stale, &Ok(80_482.0)).status(), "FAIL");
    }
}

fn env() -> Env {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width: 1024.0,
        height: 768.0,
    };
    Env {
        screens: vec![ScreenArea {
            frame: full,
            work: Rect {
                height: 728.0,
                ..full
            },
            scale: 1.0,
        }],
        utc_offset_seconds: 0,
    }
}

fn fonts() -> &'static Fonts {
    static FONTS: std::sync::OnceLock<Fonts> = std::sync::OnceLock::new();
    FONTS.get_or_init(Fonts::embedded)
}

/// An app showing the fixture quote, and its last frame.
fn driven(settings: Settings) -> (App<&'static Fonts>, Frame) {
    let mut app = App::new(Some(&settings.to_json()), Frontmost::Wow, T0, fonts());
    let mut frame = app.handle(Input::Tick, T0, &env()).present.unwrap();
    let reply = HttpReply::Answered {
        status: 200,
        body: TICKER.to_vec(),
    };
    if let Some(next) = app.handle(Input::QuoteReply(reply), T0, &env()).present {
        frame = next;
    }
    (app, frame)
}

fn click(app: &mut App<&'static Fonts>, frame: &mut Frame, target: &str) {
    let at = frame
        .targets()
        .into_iter()
        .find(|(name, _)| name == target)
        .unwrap()
        .1
        .mid();
    for pointer in [PointerInput::LeftDown(at), PointerInput::LeftUp(at)] {
        if let Some(next) = app.handle(Input::Pointer(pointer), T0, &env()).present {
            *frame = next;
        }
    }
}

fn loaded(app: &mut App<&'static Fonts>, frame: &mut Frame, timeframe: Timeframe, body: &[u8]) {
    let reply = HttpReply::Answered {
        status: 200,
        body: body.to_vec(),
    };
    if let Some(next) = app
        .handle(Input::CandlesReply(timeframe, reply), T0, &env())
        .present
    {
        *frame = next;
    }
}

mod chart {
    use super::*;

    fn expanded_15m() -> Snapshot {
        let (mut app, mut frame) = driven(Settings::default());
        click(&mut app, &mut frame, "ticker");
        loaded(&mut app, &mut frame, Timeframe::M15, OHLC_15);
        app.snapshot(ShellFacts::default())
    }

    #[test]
    fn a_fresh_full_series_near_the_reference_passes() {
        let snapshot = expanded_15m();
        let newest = snapshot.newest_candle.unwrap();
        let close = snapshot.last_close.unwrap();
        let verdict = judge::chart(&snapshot, Timeframe::M15, newest + 60.0, &Ok(close * 1.01));
        assert!(verdict.passed(), "{verdict:?}");
    }

    #[test]
    fn an_old_series_or_a_far_close_fails() {
        let snapshot = expanded_15m();
        let newest = snapshot.newest_candle.unwrap();
        let close = snapshot.last_close.unwrap();
        let stale = judge::chart(&snapshot, Timeframe::M15, newest + 1801.0, &Ok(close));
        assert_eq!(stale.status(), "FAIL", "{stale:?}");
        let far = judge::chart(&snapshot, Timeframe::M15, newest, &Ok(close * 1.1));
        assert_eq!(far.status(), "FAIL", "{far:?}");
        let other = judge::chart(&snapshot, Timeframe::H4, newest, &Ok(close));
        assert_eq!(other.status(), "FAIL", "{other:?}");
    }

    #[test]
    fn the_chart_band_excludes_the_ticker_row() {
        let window = Rect {
            x: 100.0,
            y: 10.0,
            width: 400.0,
            height: 220.0,
        };
        let top = Rect {
            height: 30.0,
            width: 180.0,
            ..window
        };
        assert_eq!(
            judge::chart_band(&window, &top),
            Rect {
                y: 40.0,
                height: 190.0,
                ..window
            }
        );
        let bottom = Rect { y: 200.0, ..top };
        assert_eq!(
            judge::chart_band(&window, &bottom),
            Rect {
                height: 190.0,
                ..window
            }
        );
    }
}

mod capture {
    use super::*;

    /// What the screen shows: ticker-paint's premultiplied pixels over the opaque stand-in fill.
    fn on_standin(frame: &Frame) -> (Vec<u8>, usize) {
        let image = ticker_paint::paint(frame, fonts());
        let [r, g, b] = STANDIN_FILL.map(u32::from);
        let screen = image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let keep = 255 - u32::from(p[3]);
                let over = |src: u8, dst: u32| (u32::from(src) + dst * keep / 255) as u8;
                [over(p[0], b), over(p[1], g), over(p[2], r), 255]
            })
            .collect();
        (screen, image.width as usize)
    }

    fn band(frame: &Frame) -> (usize, usize, usize, usize) {
        let window = Rect {
            x: 0.0,
            y: 0.0,
            ..frame.window
        };
        let band = judge::chart_band(&window, &frame.ticker);
        (
            band.x as usize,
            band.y as usize,
            band.width as usize,
            band.height as usize,
        )
    }

    fn inspect(frame: &Frame) -> pixels::Inspection {
        let (screen, stride) = on_standin(frame);
        pixels::inspect(&pixels::crop(&screen, stride, band(frame)))
    }

    #[test]
    fn every_timeframe_shows_candles_over_the_standin_at_any_opacity() {
        for opacity in [0.0, 0.55, 1.0] {
            let (mut app, mut frame) = driven(Settings {
                opacity,
                timeframe: Timeframe::H4,
                ..Settings::default()
            });
            click(&mut app, &mut frame, "ticker");
            loaded(&mut app, &mut frame, Timeframe::H4, OHLC_240);
            let found = inspect(&frame);
            assert!(found.shows_candles(), "4h at opacity {opacity}: {found}");
            for (label, timeframe, body) in [
                ("15m", Timeframe::M15, OHLC_15),
                ("1m", Timeframe::M1, OHLC_1),
            ] {
                click(&mut app, &mut frame, label);
                loaded(&mut app, &mut frame, timeframe, body);
                let found = inspect(&frame);
                assert!(
                    found.shows_candles(),
                    "{label} at opacity {opacity}: {found}"
                );
            }
        }
    }

    #[test]
    fn a_chart_still_loading_shows_no_candles() {
        let (mut app, mut frame) = driven(Settings::default());
        click(&mut app, &mut frame, "ticker");
        assert_eq!(frame.bars_drawn, 0);
        let found = inspect(&frame);
        assert!(!found.shows_candles(), "{found}");
    }

    #[test]
    fn crop_takes_the_rect_and_clips_to_the_image() {
        let image: Vec<u8> = (0..4 * 3 * 4).map(|i| i as u8).collect();
        assert_eq!(
            pixels::crop(&image, 4, (1, 1, 2, 1)),
            (20..28).collect::<Vec<u8>>()
        );
        assert_eq!(pixels::crop(&image, 4, (3, 2, 5, 5)).len(), 4);
    }
}

mod window {
    use super::*;
    use judge::{
        WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT,
    };

    const OVERLAY: u32 = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;

    fn facts(real: u32) -> StyleFacts {
        StyleFacts {
            real,
            published: OVERLAY,
            per_monitor_v2: true,
            others: Vec::new(),
        }
    }

    #[test]
    fn the_overlay_styles_pass_and_each_wrong_bit_fails() {
        assert!(judge::styles(&facts(OVERLAY)).passed());
        for wrong in [
            OVERLAY | WS_EX_TRANSPARENT,
            OVERLAY | WS_EX_APPWINDOW,
            OVERLAY & !WS_EX_NOACTIVATE,
            OVERLAY & !WS_EX_LAYERED,
        ] {
            assert_eq!(judge::styles(&facts(wrong)).status(), "FAIL", "{wrong:#x}");
        }
        let unaware = StyleFacts {
            per_monitor_v2: false,
            ..facts(OVERLAY)
        };
        assert_eq!(judge::styles(&unaware).status(), "FAIL");
        let with_button = StyleFacts {
            others: vec![("BTCTickerSettings".into(), 0)],
            ..facts(OVERLAY)
        };
        assert_eq!(judge::styles(&with_button).status(), "FAIL");
    }

    #[test]
    fn above_needs_both_windows_in_the_list() {
        assert!(judge::above(&[1, 7, 9], 7, 9).passed());
        assert_eq!(judge::above(&[1, 9, 7], 7, 9).status(), "FAIL");
        assert_eq!(judge::above(&[1, 9], 7, 9).status(), "INCONCLUSIVE");
    }

    #[test]
    fn topmost_recovery_must_first_be_lost() {
        assert_eq!(
            judge::topmost_recovers(false, Some(10), 3000).status(),
            "INCONCLUSIVE"
        );
        assert!(judge::topmost_recovers(true, Some(1900), 3000).passed());
        assert_eq!(
            judge::topmost_recovers(true, Some(3500), 3000).status(),
            "FAIL"
        );
        assert_eq!(judge::topmost_recovers(true, None, 3000).status(), "FAIL");
    }

    #[test]
    fn the_default_spot_is_top_center_of_the_work_area() {
        let work = Rect {
            x: 0.0,
            y: 0.0,
            width: 1024.0,
            height: 720.0,
        };
        let ticker = Rect {
            x: 430.0,
            y: 12.0,
            width: 164.0,
            height: 29.0,
        };
        assert!(judge::at_default_spot(&ticker, &work, 1.0));
        assert!(!judge::at_default_spot(
            &Rect {
                x: 860.0,
                y: 0.0,
                ..ticker
            },
            &work,
            1.0
        ));
        assert!(judge::at_default_spot(
            &Rect { y: 18.0, ..ticker },
            &work,
            1.5
        ));
    }

    #[test]
    fn clamped_means_flush_with_the_corner_and_inside() {
        let work = Rect {
            x: 0.0,
            y: 0.0,
            width: 1024.0,
            height: 728.0,
        };
        let flush = Rect {
            x: 844.0,
            y: 0.0,
            width: 180.0,
            height: 28.0,
        };
        assert!(judge::clamped(&flush, &work));
        assert!(!judge::clamped(&Rect { x: 864.0, ..flush }, &work));
        assert!(!judge::clamped(&Rect { y: -5.0, ..flush }, &work));
    }

    #[test]
    fn the_desktop_must_be_interactive_and_readable() {
        let good = DesktopFacts {
            session_id: Some(2),
            input_desktop: Some("Default".into()),
            screen: (1024, 768),
            cursor_misses: Vec::new(),
            cursor_tries: 5,
            click_reached_standin: true,
            capture: Some(STANDIN_FILL),
            standin_fill: STANDIN_FILL,
        };
        assert!(judge::desktop(&good).passed());
        for bad in [
            DesktopFacts {
                session_id: Some(0),
                ..clone(&good)
            },
            DesktopFacts {
                input_desktop: Some("Winlogon".into()),
                ..clone(&good)
            },
            DesktopFacts {
                capture: Some([0, 0, 0]),
                ..clone(&good)
            },
            DesktopFacts {
                click_reached_standin: false,
                ..clone(&good)
            },
            DesktopFacts {
                cursor_misses: vec!["1,1->1,0".into()],
                ..clone(&good)
            },
        ] {
            assert_eq!(judge::desktop(&bad).status(), "INCONCLUSIVE");
        }
    }

    fn clone(f: &DesktopFacts) -> DesktopFacts {
        DesktopFacts {
            session_id: f.session_id,
            input_desktop: f.input_desktop.clone(),
            screen: f.screen,
            cursor_misses: f.cursor_misses.clone(),
            cursor_tries: f.cursor_tries,
            click_reached_standin: f.click_reached_standin,
            capture: f.capture,
            standin_fill: f.standin_fill,
        }
    }
}

mod focus {
    use super::*;

    const GAME: u64 = 0x10;
    const HELPER: u64 = 0x20;
    const APP_PID: u32 = 900;

    fn sample(at_ms: u64, hwnd: u64, pid: u32) -> Sample {
        Sample { at_ms, hwnd, pid }
    }

    fn verdict(
        timeline: &[Sample],
        excused: &[Span],
        activations: u32,
        steal_seen: bool,
    ) -> Verdict {
        judge::focus_kept(&FocusFacts {
            timeline,
            ended_ms: 10_000,
            standin: GAME,
            app_pid: APP_PID,
            excused,
            activations,
            steal_seen,
            covered: "the run",
        })
    }

    const GATE: Span = Span {
        from_ms: 2_000,
        to_ms: 3_000,
    };

    #[test]
    fn a_steal_inside_the_deliberate_switch_passes() {
        let timeline = [
            sample(0, GAME, 1),
            sample(2_100, HELPER, 2),
            sample(2_600, GAME, 1),
        ];
        assert!(verdict(&timeline, &[GATE], 0, true).passed());
    }

    #[test]
    fn without_seeing_the_steal_the_watcher_is_not_trusted() {
        let timeline = [sample(0, GAME, 1)];
        assert_eq!(
            verdict(&timeline, &[GATE], 0, false).status(),
            "INCONCLUSIVE"
        );
    }

    #[test]
    fn the_app_in_the_foreground_fails_even_inside_a_switch() {
        let timeline = [
            sample(0, GAME, 1),
            sample(2_100, 0x30, APP_PID),
            sample(2_600, GAME, 1),
        ];
        assert_eq!(verdict(&timeline, &[GATE], 0, true).status(), "FAIL");
    }

    #[test]
    fn one_activation_fails_a_perfect_timeline() {
        assert_eq!(
            verdict(&[sample(0, GAME, 1)], &[GATE], 1, true).status(),
            "FAIL"
        );
    }

    #[test]
    fn another_window_outside_a_switch_or_outlasting_it_fails() {
        let outside = [
            sample(0, GAME, 1),
            sample(5_000, HELPER, 2),
            sample(5_050, GAME, 1),
        ];
        assert_eq!(verdict(&outside, &[GATE], 0, true).status(), "FAIL");
        let outlasting = [
            sample(0, GAME, 1),
            sample(2_100, HELPER, 2),
            sample(3_500, GAME, 1),
        ];
        assert_eq!(verdict(&outlasting, &[GATE], 0, true).status(), "FAIL");
    }

    #[test]
    fn foreign_presses_make_the_run_inconclusive() {
        assert!(judge::undisturbed(0, 12).passed());
        assert_eq!(judge::undisturbed(1, 12).status(), "INCONCLUSIVE");
    }
}

mod scenarios {
    use super::*;
    use judge::{GateFacts, OpaqueFacts, SettingsWindowFacts};

    #[test]
    fn the_gate_needs_both_switches_to_happen() {
        let good = GateFacts {
            helper_took_foreground: true,
            hidden_ms: Some(80),
            standin_took_foreground: true,
            shown_ms: Some(60),
        };
        assert!(judge::wow_gate(&good).passed());
        let no_steal = GateFacts {
            helper_took_foreground: false,
            ..good
        };
        assert_eq!(judge::wow_gate(&no_steal).status(), "INCONCLUSIVE");
        let never_hidden = GateFacts {
            hidden_ms: None,
            ..good
        };
        assert_eq!(judge::wow_gate(&never_hidden).status(), "FAIL");
        let never_back = GateFacts {
            shown_ms: None,
            ..good
        };
        assert_eq!(judge::wow_gate(&never_back).status(), "FAIL");
    }

    fn opaque() -> OpaqueFacts {
        OpaqueFacts {
            control_window_present: true,
            control_passed_through: true,
            misses: Vec::new(),
            grid_points: 70,
            corner_click_toggled: true,
            corner_click_reached_standin: false,
        }
    }

    #[test]
    fn opaque_needs_the_negative_control_first() {
        assert!(judge::opaque_to_clicks(&opaque()).passed());
        let no_control = OpaqueFacts {
            control_passed_through: false,
            ..opaque()
        };
        assert_eq!(
            judge::opaque_to_clicks(&no_control).status(),
            "INCONCLUSIVE"
        );
        let leaky = OpaqueFacts {
            misses: vec!["300,40".into()],
            ..opaque()
        };
        assert_eq!(judge::opaque_to_clicks(&leaky).status(), "FAIL");
        let through = OpaqueFacts {
            corner_click_reached_standin: true,
            ..opaque()
        };
        assert_eq!(judge::opaque_to_clicks(&through).status(), "FAIL");
    }

    #[test]
    fn the_settings_window_has_a_slider_per_slider_and_color_row() {
        assert_eq!(judge::expected_trackbars(), 7);
        let good = SettingsWindowFacts {
            opened: true,
            took_foreground: true,
            trackbars: 7,
            expected_trackbars: 7,
            toggle_applied: true,
            closed: true,
            foreground_returned: true,
        };
        assert!(judge::settings_window(&good).passed());
        let kept = SettingsWindowFacts {
            foreground_returned: false,
            ..good
        };
        assert_eq!(judge::settings_window(&kept).status(), "FAIL");
        let missing_alpha = SettingsWindowFacts {
            trackbars: 4,
            ..good
        };
        assert_eq!(judge::settings_window(&missing_alpha).status(), "FAIL");
    }
}
