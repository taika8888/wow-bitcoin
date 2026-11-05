mod support;

use support::{FakeMeasure, OHLC_1, OHLC_15, TICKER};
use ticker_core::{
    App, Draw, Effects, Env, FeedSchedule, Frame, Frontmost, HttpReply, Input, MenuItem, Point,
    PointerInput, PressTarget, Rect, ScreenArea, Settings, ShellFacts, Time, Timeframe, kraken,
};

const T0: Time = Time(1_793_847_300.0);

fn screen(width: f64, height: f64, scale: f64) -> ScreenArea {
    ScreenArea {
        frame: Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        work: Rect {
            x: 0.0,
            y: 0.0,
            width,
            height: height - 40.0,
        },
        scale,
    }
}

fn env() -> Env {
    Env {
        screens: vec![screen(1920.0, 1080.0, 1.0)],
        utc_offset_seconds: 0,
    }
}

fn answered(body: &[u8]) -> HttpReply {
    HttpReply::Answered {
        status: 200,
        body: body.to_vec(),
    }
}

struct Driver {
    app: App<FakeMeasure>,
    now: Time,
    env: Env,
    frame: Frame,
}

impl Driver {
    fn new(settings: Settings) -> Driver {
        Driver::with_env(settings, env())
    }

    fn with_env(settings: Settings, env: Env) -> Driver {
        let mut app = App::new(Some(&settings.to_json()), Frontmost::Wow, T0, FakeMeasure);
        let frame = app
            .handle(Input::Tick, T0, &env)
            .present
            .expect("the first tick presents a frame");
        Driver {
            app,
            now: T0,
            env,
            frame,
        }
    }

    fn live(settings: Settings) -> Driver {
        let mut driver = Driver::new(settings);
        driver.send(Input::QuoteReply(answered(TICKER)));
        driver
    }

    fn send(&mut self, input: Input) -> Effects {
        let effects = self.app.handle(input, self.now, &self.env);
        if let Some(frame) = &effects.present {
            self.frame = frame.clone();
        }
        effects
    }

    fn at(&mut self, seconds: f64, input: Input) -> Effects {
        self.now = T0.plus(seconds);
        self.send(input)
    }

    fn pointer(&mut self, input: PointerInput) -> Effects {
        self.send(Input::Pointer(input))
    }

    fn click(&mut self, at: Point) -> Effects {
        self.pointer(PointerInput::LeftDown(at));
        self.pointer(PointerInput::LeftUp(at))
    }

    fn target(&self, name: &str) -> Point {
        self.frame
            .targets()
            .into_iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no target {name} in {:?}", self.frame.targets()))
            .1
            .mid()
    }

    fn expand(&mut self) -> Effects {
        self.click(self.target("ticker"))
    }

    fn snapshot(&mut self) -> ticker_core::Snapshot {
        self.app.snapshot(ShellFacts::default())
    }
}

fn texts(frame: &Frame) -> Vec<String> {
    frame
        .draw
        .iter()
        .filter_map(|d| match d {
            Draw::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn center_of_chart(frame: &Frame) -> Point {
    let content = frame
        .targets()
        .iter()
        .filter(|(name, _)| name != "ticker")
        .fold(frame.ticker.offset(frame.window.origin()), |acc, (_, r)| {
            acc.union(r)
        });
    let window = frame.window;
    let below = content.max_y() < window.mid().y;
    Point {
        x: window.mid().x,
        y: if below {
            (content.max_y() + window.max_y()) / 2.0
        } else {
            (window.y + content.y) / 2.0
        },
    }
}

mod quote {
    use super::*;

    #[test]
    fn the_first_tick_fetches_the_quote_and_shows_loading() {
        let mut app = App::new(None, Frontmost::Wow, T0, FakeMeasure);
        let effects = app.handle(Input::Tick, T0, &env());
        assert_eq!(
            effects.fetch_quote.map(|r| (r.url, r.timeout_seconds)),
            Some((kraken::TICKER_URL.to_string(), 5.0))
        );
        assert_eq!(texts(&effects.present.unwrap()), ["BTC ..."]);
        let again = app.handle(Input::Tick, T0.plus(1.0), &env());
        assert_eq!(again.fetch_quote, None, "one quote request at a time");
        assert_eq!(again.present, None, "nothing on screen changed");
    }

    #[test]
    fn a_quote_shows_the_price_and_the_change() {
        let mut driver = Driver::live(Settings::default());
        assert_eq!(texts(&driver.frame), ["BTC $80,490", "  ", "+0.06%"]);
        let snapshot = driver.snapshot();
        assert_eq!(
            (snapshot.text.as_str(), snapshot.feed.as_str()),
            ("BTC $80,490  +0.06%", "live")
        );
        assert_eq!(snapshot.fetched_at, Some(T0.0));
    }

    #[test]
    fn the_next_quote_starts_one_interval_after_the_last_start() {
        let mut driver = Driver::new(Settings::default());
        let reply = driver.at(0.2, Input::QuoteReply(answered(TICKER)));
        assert!(
            (reply.wake_in - 4.8).abs() < 1e-6,
            "wakes for the next start, got {}",
            reply.wake_in
        );
        assert_eq!(driver.at(4.9, Input::Tick).fetch_quote, None);
        assert!(driver.at(5.0, Input::Tick).fetch_quote.is_some());
    }

    #[test]
    fn failures_back_off_and_waking_resets_the_backoff() {
        let mut driver = Driver::new(Settings::default());
        let mut starts = vec![0.0];
        let mut clock = 0.0;
        for _ in 0..4 {
            driver.at(clock + 0.1, Input::QuoteReply(HttpReply::Unreachable));
            while driver.at(clock, Input::Tick).fetch_quote.is_none() {
                clock += 0.5;
            }
            starts.push(clock);
        }
        let gaps: Vec<f64> = starts.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(gaps, [5.0, 10.0, 20.0, 40.0]);
        driver.at(clock + 0.1, Input::QuoteReply(HttpReply::Unreachable));
        assert_eq!(
            driver.at(clock + 5.0, Input::Tick).fetch_quote,
            None,
            "backing off"
        );
        assert!(
            driver.at(clock + 5.0, Input::Woke).fetch_quote.is_some(),
            "waking cuts the backoff short"
        );
    }

    #[test]
    fn the_quote_turns_stale_and_the_feed_says_so() {
        let mut driver = Driver::live(Settings::default());
        let overdue = driver.at(14.0, Input::Tick);
        assert!(
            overdue.fetch_quote.is_some(),
            "an overdue quote starts on the next input"
        );
        assert!(
            (overdue.wake_in - 1.0).abs() < 0.01,
            "wakes when the quote turns stale, got {}",
            overdue.wake_in
        );
        let stale = driver
            .at(15.01, Input::Tick)
            .present
            .expect("stale redraws");
        assert_eq!(texts(&stale).last().map(String::as_str), Some("  stale"));
    }

    #[test]
    fn an_error_status_or_bad_body_counts_as_a_failure() {
        let mut driver = Driver::new(Settings::default());
        driver.send(Input::QuoteReply(HttpReply::Answered {
            status: 503,
            body: TICKER.to_vec(),
        }));
        assert_eq!(driver.snapshot().feed, "loading");
        assert!(
            driver.at(5.0, Input::Tick).fetch_quote.is_some(),
            "the first retry keeps the cadence"
        );
        driver.at(5.1, Input::QuoteReply(answered(b"not json")));
        assert_eq!(
            driver.at(14.0, Input::Tick).fetch_quote,
            None,
            "the second failure backs off to 10 s"
        );
        assert!(driver.at(15.0, Input::Tick).fetch_quote.is_some());
        driver.at(15.1, Input::Tick);
        assert_eq!(driver.snapshot().feed, "offline");
    }

    #[test]
    fn a_new_refresh_interval_applies_to_the_wait_already_running() {
        let mut driver = Driver::live(Settings::default());
        let slower = Settings {
            refresh_seconds: 300,
            ..Settings::default()
        };
        driver.send(Input::SettingsEdited(slower.clone()));
        assert_eq!(driver.at(5.0, Input::Tick).fetch_quote, None);
        assert!(driver.at(300.0, Input::Tick).fetch_quote.is_some());
        let faster = Settings {
            refresh_seconds: 2,
            ..slower
        };
        driver.at(300.5, Input::QuoteReply(answered(TICKER)));
        let effects = driver.at(302.0, Input::SettingsEdited(faster));
        assert_eq!(
            effects.fetch_quote.map(|r| r.timeout_seconds),
            Some(FeedSchedule::request_timeout(2)),
            "a shorter interval starts the overdue request at once"
        );
    }

    #[test]
    fn a_reply_with_nothing_in_flight_is_ignored() {
        let mut driver = Driver::live(Settings::default());
        driver.at(1.0, Input::QuoteReply(HttpReply::Unreachable));
        assert_eq!(driver.snapshot().feed, "live");
        assert!(driver.at(5.0, Input::Tick).fetch_quote.is_some());
    }
}

mod chart {
    use super::*;

    #[test]
    fn a_click_on_the_ticker_expands_and_fetches_the_last_chosen_timeframe() {
        let mut driver = Driver::live(Settings::default());
        let at = driver.target("ticker");
        let down = driver.pointer(PointerInput::LeftDown(at));
        assert_eq!(down.capture, Some(true), "a press captures the pointer");
        let up = driver.pointer(PointerInput::LeftUp(at));
        assert_eq!(up.capture, Some(false), "the release lets it go");
        assert_eq!(up.save_settings, None, "expanding is not a settings change");
        assert_eq!(
            up.fetch_candles.map(|(t, r)| (t, r.url)),
            Some((Timeframe::M15, kraken::ohlc_url(Timeframe::M15)))
        );
        let snapshot = driver.snapshot();
        assert_eq!(snapshot.mode, "expanded");
        assert_eq!(snapshot.timeframe.as_deref(), Some("15m"));
        assert_eq!(snapshot.chart.as_deref(), Some("loading"));
        assert!(texts(&driver.frame).contains(&"Loading 15m candles".to_string()));
    }

    #[test]
    fn candles_load_and_draw() {
        let mut driver = Driver::live(Settings::default());
        driver.expand();
        driver.send(Input::CandlesReply(Timeframe::M15, answered(OHLC_15)));
        assert_eq!(driver.frame.bars_drawn, 96);
        let snapshot = driver.snapshot();
        assert_eq!(
            (snapshot.chart.as_deref(), snapshot.candles),
            (Some("loaded"), 96)
        );
        let newest = kraken::parse_candles(OHLC_15, Timeframe::M15)
            .unwrap()
            .last()
            .unwrap()
            .time;
        assert_eq!(snapshot.newest_candle, Some(newest.0));
        let labels = texts(&driver.frame);
        assert!(
            labels.iter().any(|t| t.starts_with('$')),
            "price axis labels: {labels:?}"
        );
        assert!(
            labels.iter().any(|t| t.contains(':')),
            "15m spans a day, so hours label the time axis: {labels:?}"
        );
    }

    #[test]
    fn a_click_anywhere_on_the_open_chart_outside_the_timeframe_controls_closes_it() {
        let mut driver = Driver::live(Settings::default());
        driver.expand();
        driver.send(Input::CandlesReply(Timeframe::M15, answered(OHLC_15)));
        let on_chart = center_of_chart(&driver.frame);
        assert_eq!(driver.frame.hit(on_chart), PressTarget::Body);
        driver.click(on_chart);
        assert_eq!(driver.snapshot().mode, "collapsed");
    }

    #[test]
    fn a_click_on_a_timeframe_switches_saves_and_fetches_it() {
        let mut driver = Driver::live(Settings::default());
        driver.expand();
        let effects = driver.click(driver.target("1m"));
        let saved = Settings::decode(effects.save_settings.as_deref());
        assert_eq!(saved.timeframe, Timeframe::M1, "the choice persists");
        assert_eq!(effects.fetch_candles.map(|(t, _)| t), Some(Timeframe::M1));
        driver.send(Input::CandlesReply(Timeframe::M1, answered(OHLC_1)));
        assert_eq!(driver.snapshot().candles, 120);
        driver.expand();
        driver.expand();
        assert_eq!(
            driver.snapshot().timeframe.as_deref(),
            Some("1m"),
            "re-expanding returns to the last choice"
        );
    }

    #[test]
    fn a_failed_fetch_says_why_in_place_of_the_chart() {
        let mut driver = Driver::live(Settings::default());
        driver.expand();
        driver.send(Input::CandlesReply(
            Timeframe::M15,
            HttpReply::Answered {
                status: 503,
                body: Vec::new(),
            },
        ));
        assert!(texts(&driver.frame).contains(&"Kraken answered HTTP 503, retrying".to_string()));
        assert_eq!(driver.snapshot().chart.as_deref(), Some("failed"));
        assert!(
            driver.at(10.0, Input::Tick).fetch_candles.is_some(),
            "retries after 10 s"
        );
    }

    #[test]
    fn a_failed_refresh_keeps_the_candles_and_marks_them_stale() {
        let mut driver = Driver::live(Settings::default());
        driver.expand();
        driver.send(Input::CandlesReply(Timeframe::M15, answered(OHLC_15)));
        driver.at(60.0, Input::Tick);
        driver.at(
            60.5,
            Input::CandlesReply(Timeframe::M15, HttpReply::Unreachable),
        );
        assert_eq!(driver.frame.bars_drawn, 96);
        assert!(texts(&driver.frame).contains(&"Stale: Kraken is not answering".to_string()));
    }
}

mod pointer {
    use super::*;

    #[test]
    fn a_drag_moves_the_window_and_saves_where_the_ticker_landed() {
        let mut driver = Driver::live(Settings::default());
        let start = driver.frame.window;
        let press = driver.target("ticker");
        driver.pointer(PointerInput::LeftDown(press));
        let moved = driver.pointer(PointerInput::Move(Point {
            x: press.x - 300.0,
            y: press.y + 200.0,
        }));
        let window = moved
            .present
            .expect("the window follows the pointer")
            .window;
        assert_eq!((window.x, window.y), (start.x - 300.0, start.y + 200.0));
        let drop = driver.pointer(PointerInput::LeftUp(Point {
            x: press.x - 300.0,
            y: press.y + 200.0,
        }));
        let saved = Settings::decode(drop.save_settings.as_deref())
            .position
            .expect("the drop is saved");
        assert_eq!((saved.x, saved.y), (start.x - 300.0, start.y + 200.0));
        assert_eq!(driver.snapshot().mode, "collapsed", "a drag is not a click");
    }

    #[test]
    fn a_drop_past_the_edge_is_clamped_into_the_work_area() {
        let mut driver = Driver::live(Settings::default());
        let press = driver.target("ticker");
        let corner = Point {
            x: 1919.0,
            y: 1079.0,
        };
        driver.pointer(PointerInput::LeftDown(press));
        driver.pointer(PointerInput::Move(corner));
        let drop = driver.pointer(PointerInput::LeftUp(corner));
        let saved = Settings::decode(drop.save_settings.as_deref())
            .position
            .unwrap();
        let work = env().screens[0].work;
        assert!(
            work.contains_rect(&driver.frame.window),
            "{:?} is inside {work:?}",
            driver.frame.window
        );
        assert_eq!(
            (
                saved.x + driver.frame.window.width,
                saved.y + driver.frame.window.height
            ),
            (work.max_x(), work.max_y()),
            "the least move back inside: flush with the bottom-right of the work area"
        );
    }

    #[test]
    fn a_chart_dragged_until_the_ticker_is_off_screen_lands_on_the_screen_under_the_window() {
        let mut driver = Driver::live(Settings {
            chart_height: 600.0,
            ..Settings::default()
        });
        driver.expand();
        let window = driver.frame.window;
        let press = Point {
            x: window.mid().x,
            y: window.max_y() - 5.0,
        };
        let top = Point { x: press.x, y: 0.0 };
        driver.pointer(PointerInput::LeftDown(press));
        driver.pointer(PointerInput::Move(top));
        driver.pointer(PointerInput::LeftUp(top));
        let ticker = driver.frame.ticker.offset(driver.frame.window.origin());
        assert_eq!(
            ticker.y, 0.0,
            "pulled down onto the screen, not reset to the default spot"
        );
        assert_eq!(ticker.x, window.x + driver.frame.ticker.x);
    }

    #[test]
    fn a_locked_ticker_treats_any_release_as_a_click() {
        let mut driver = Driver::live(Settings {
            locked: true,
            ..Settings::default()
        });
        let press = driver.target("ticker");
        driver.pointer(PointerInput::LeftDown(press));
        let moved = driver.pointer(PointerInput::Move(Point {
            x: press.x + 2.0,
            y: press.y + 60.0,
        }));
        assert_eq!(
            moved.present, None,
            "a locked ticker does not follow the pointer"
        );
        let up = driver.pointer(PointerInput::LeftUp(press));
        assert_eq!(up.save_settings, None);
        assert_eq!(driver.snapshot().mode, "expanded");
    }

    #[test]
    fn a_press_outside_the_window_is_ignored() {
        let mut driver = Driver::live(Settings::default());
        let outside = Point {
            x: driver.frame.window.max_x() + 5.0,
            y: driver.frame.window.y,
        };
        assert_eq!(
            driver.pointer(PointerInput::LeftDown(outside)).capture,
            None
        );
        driver.pointer(PointerInput::LeftUp(outside));
        assert_eq!(driver.snapshot().mode, "collapsed");
    }

    #[test]
    fn every_corner_of_the_window_catches_clicks_at_zero_opacity() {
        let driver = Driver::live(Settings {
            opacity: 0.0,
            ..Settings::default()
        });
        let w = driver.frame.window;
        for corner in [
            Point { x: w.x, y: w.y },
            Point {
                x: w.max_x() - 1.0,
                y: w.y,
            },
            Point {
                x: w.x,
                y: w.max_y() - 1.0,
            },
            Point {
                x: w.max_x() - 1.0,
                y: w.max_y() - 1.0,
            },
        ] {
            assert_eq!(driver.frame.hit(corner), PressTarget::Body, "{corner:?}");
        }
    }
}

mod menu {
    use super::*;

    fn open(driver: &mut Driver) {
        let at = driver.target("ticker");
        driver.pointer(PointerInput::RightUp(at));
        assert!(driver.snapshot().menu_open, "a right click opens the menu");
    }

    #[test]
    fn the_menu_draws_its_items_inside_the_window() {
        let mut driver = Driver::live(Settings::default());
        open(&mut driver);
        let labels = texts(&driver.frame);
        for label in ["Settings\u{2026}", "Lock Position", "Quit BTC Ticker"] {
            assert!(labels.contains(&label.to_string()), "{label} in {labels:?}");
        }
        for item in MenuItem::ALL {
            let at = driver.target(&format!("menu:{}", item.name()));
            assert_eq!(driver.frame.hit(at), PressTarget::Menu(item));
        }
        assert_eq!(
            driver.frame.hit(driver.target("ticker")),
            PressTarget::Body,
            "the ticker stays clickable beside the menu"
        );
    }

    #[test]
    fn lock_in_the_menu_toggles_and_saves_the_lock() {
        let mut driver = Driver::live(Settings::default());
        open(&mut driver);
        let effects = driver.click(driver.target("menu:lock"));
        assert!(Settings::decode(effects.save_settings.as_deref()).locked);
        assert!(
            !driver.snapshot().menu_open,
            "choosing an item closes the menu"
        );
        open(&mut driver);
        assert!(texts(&driver.frame).contains(&"Unlock Position".to_string()));
    }

    #[test]
    fn settings_and_quit_in_the_menu_are_effects() {
        let mut driver = Driver::live(Settings::default());
        open(&mut driver);
        assert!(driver.click(driver.target("menu:settings")).open_settings);
        open(&mut driver);
        assert!(driver.click(driver.target("menu:quit")).quit);
    }

    #[test]
    fn a_press_elsewhere_on_the_overlay_closes_the_menu_without_clicking() {
        let mut driver = Driver::live(Settings::default());
        open(&mut driver);
        let effects = driver.click(driver.target("ticker"));
        assert!(!effects.open_settings && !effects.quit);
        let snapshot = driver.snapshot();
        assert!(!snapshot.menu_open);
        assert_eq!(
            snapshot.mode, "collapsed",
            "the closing click does not open the chart"
        );
    }

    #[test]
    fn the_pointer_leaving_closes_the_menu() {
        let mut driver = Driver::live(Settings::default());
        open(&mut driver);
        driver.pointer(PointerInput::Left);
        assert!(!driver.snapshot().menu_open);
        assert!(driver.frame.menu.is_empty());
    }

    #[test]
    fn a_menu_near_the_bottom_opens_upward_and_keeps_the_ticker_put() {
        let mut driver = Driver::live(Settings {
            position: Some(ticker_core::ScreenPoint { x: 800.0, y: 990.0 }),
            ..Settings::default()
        });
        let ticker = driver.frame.ticker.offset(driver.frame.window.origin());
        open(&mut driver);
        assert_eq!(
            driver.frame.ticker.offset(driver.frame.window.origin()),
            ticker
        );
        assert!(
            driver.target("menu:quit").y < ticker.y,
            "opens toward the middle"
        );
    }

    #[test]
    fn the_tray_menu_acts_without_pointer_input() {
        let mut driver = Driver::live(Settings::default());
        assert!(
            Settings::decode(
                driver
                    .send(Input::Menu(MenuItem::Lock))
                    .save_settings
                    .as_deref()
            )
            .locked
        );
        assert!(driver.send(Input::Menu(MenuItem::Settings)).open_settings);
        assert!(driver.send(Input::Menu(MenuItem::Quit)).quit);
    }
}

mod settings_and_visibility {
    use super::*;

    #[test]
    fn a_settings_edit_is_saved_and_a_stored_change_is_not_echoed() {
        let mut driver = Driver::live(Settings::default());
        let bigger = Settings {
            font_size: 30.0,
            ..Settings::default()
        };
        let edited = driver.send(Input::SettingsEdited(bigger.clone()));
        assert_eq!(edited.save_settings, Some(bigger.to_json()));
        let width = driver.frame.window.width;
        let stored = driver.send(Input::SettingsStored(r#"{"fontSize":40}"#.into()));
        assert_eq!(stored.save_settings, None, "no echo back to the store");
        assert!(
            driver.frame.window.width > width,
            "the stored change applies"
        );
    }

    #[test]
    fn only_while_wow_is_frontmost_hides_and_shows_the_overlay() {
        let mut driver = Driver::live(Settings {
            only_when_wow_active: true,
            ..Settings::default()
        });
        driver.expand();
        let hidden = driver
            .send(Input::Foreground(Frontmost::Other))
            .present
            .unwrap();
        assert!(!hidden.visible);
        assert_eq!(hidden.hit(driver.target("ticker")), PressTarget::Outside);
        assert_eq!(
            driver
                .pointer(PointerInput::LeftDown(driver.target("ticker")))
                .capture,
            None,
            "a hidden overlay takes no presses"
        );
        assert!(
            driver
                .send(Input::Foreground(Frontmost::Ticker))
                .present
                .unwrap()
                .visible
        );
        assert_eq!(driver.snapshot().mode, "expanded", "hiding keeps the mode");
    }

    #[test]
    fn hiding_mid_press_releases_the_capture() {
        let mut driver = Driver::live(Settings {
            only_when_wow_active: true,
            ..Settings::default()
        });
        driver.pointer(PointerInput::LeftDown(driver.target("ticker")));
        assert_eq!(
            driver.send(Input::Foreground(Frontmost::Other)).capture,
            Some(false)
        );
    }

    #[test]
    fn a_saved_spot_is_clamped_when_the_display_shrinks() {
        let mut driver = Driver::live(Settings {
            position: Some(ticker_core::ScreenPoint {
                x: 1700.0,
                y: 1000.0,
            }),
            ..Settings::default()
        });
        driver.env = Env {
            screens: vec![screen(1280.0, 720.0, 1.0)],
            utc_offset_seconds: 0,
        };
        let frame = driver.send(Input::DisplayChanged).present.unwrap();
        assert!(driver.env.screens[0].work.contains_rect(&frame.window));
    }

    #[test]
    fn sizes_follow_the_monitor_scale() {
        let at = |scale: f64| {
            Driver::with_env(
                Settings::default(),
                Env {
                    screens: vec![screen(3840.0, 2160.0, scale)],
                    utc_offset_seconds: 0,
                },
            )
            .frame
            .window
        };
        for scale in [1.25, 1.5, 2.0] {
            let (one, scaled) = (at(1.0), at(scale));
            assert!(
                (scaled.height / one.height - scale).abs() < 0.1,
                "height at {scale}: {} vs {}",
                scaled.height,
                one.height
            );
            assert!(
                (scaled.width / one.width - scale).abs() < 0.1,
                "width at {scale}"
            );
        }
    }

    #[test]
    fn every_target_hit_tests_to_itself_and_uses_whole_pixels() {
        let mut driver = Driver::live(Settings::default());
        let mut frames = vec![driver.frame.clone()];
        driver.expand();
        driver.send(Input::CandlesReply(Timeframe::M15, answered(OHLC_15)));
        frames.push(driver.frame.clone());
        driver.pointer(PointerInput::RightUp(driver.target("ticker")));
        frames.push(driver.frame.clone());
        for frame in frames {
            let w = frame.window;
            assert_eq!(
                [w.x, w.y, w.width, w.height].map(f64::fract),
                [0.0; 4],
                "UpdateLayeredWindow takes whole pixels: {w:?}"
            );
            for (name, rect) in frame.targets() {
                let expected = match name.as_str() {
                    "ticker" => PressTarget::Body,
                    "menu:settings" => PressTarget::Menu(MenuItem::Settings),
                    "menu:lock" => PressTarget::Menu(MenuItem::Lock),
                    "menu:quit" => PressTarget::Menu(MenuItem::Quit),
                    label => PressTarget::Timeframe(
                        Timeframe::ALL
                            .into_iter()
                            .find(|t| t.spec().label == label)
                            .unwrap(),
                    ),
                };
                assert_eq!(frame.hit(rect.mid()), expected, "{name}");
                assert!(w.contains_rect(&rect), "{name} lies inside the window");
            }
        }
    }

    #[test]
    fn the_snapshot_counts_up_and_carries_the_shell_facts() {
        let mut driver = Driver::live(Settings::default());
        let facts = ShellFacts {
            hwnd: 42,
            ex_style: 0x0808_0088,
            activations: 0,
            presses: 3,
            foreign_presses: 1,
            tray_icon: true,
        };
        let first = driver.app.snapshot(facts);
        let second = driver.app.snapshot(facts);
        assert_eq!(second.seq, first.seq + 1);
        assert_eq!(
            (second.hwnd, second.presses, second.foreign_presses),
            (42, 3, 1)
        );
        assert!(second.tray_icon);
        assert!(second.targets.contains_key("ticker"));
        let json = second.to_json();
        assert!(
            json.starts_with(r#"{"activations":0,"candles":0,"chart":null,"exStyle":"#),
            "{json}"
        );
    }
}
