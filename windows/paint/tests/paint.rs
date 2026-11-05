//! Pixel properties of what Windows will show, checked on frames built by driving `ticker_core::App` with the
//! recorded Kraken fixtures. Every render is also written as a PNG under the test tmpdir for review.
use std::collections::BTreeSet;
use ticker_core::{
    App, Draw, Env, Frame, Frontmost, HttpReply, Input, KrakenError, Measure, PointerInput, Rect,
    Rgba, ScreenArea, Settings, TextStyle, Time, Timeframe,
};
use ticker_paint::{Bgra, Fonts, HIT_FLOOR_ALPHA, paint};

const TICKER: &[u8] = include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ticker.json");
const OHLC_1: &[u8] = include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json");
const OHLC_15: &[u8] =
    include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json");
const OHLC_240: &[u8] =
    include_bytes!("../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json");
const T0: Time = Time(1_793_847_300.0);

fn env(scale: f64) -> Env {
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width: 1920.0 * scale,
        height: 1080.0 * scale,
    };
    Env {
        screens: vec![ScreenArea {
            frame: full,
            work: Rect {
                height: full.height - 48.0 * scale,
                ..full
            },
            scale,
        }],
        utc_offset_seconds: 0,
    }
}

fn answered(body: &[u8]) -> HttpReply {
    HttpReply::Answered {
        status: 200,
        body: body.to_vec(),
    }
}

struct Scenes<'f> {
    app: App<&'f Fonts>,
    env: Env,
    now: Time,
    frame: Frame,
    out: Vec<(String, Frame)>,
}

impl<'f> Scenes<'f> {
    fn send(&mut self, input: Input) {
        if let Some(frame) = self.app.handle(input, self.now, &self.env).present {
            self.frame = frame;
        }
    }

    fn click(&mut self, target: &str) {
        let at = self
            .frame
            .targets()
            .into_iter()
            .find(|(name, _)| name == target)
            .unwrap_or_else(|| panic!("no {target} target"))
            .1
            .mid();
        self.send(Input::Pointer(PointerInput::LeftDown(at)));
        self.send(Input::Pointer(PointerInput::LeftUp(at)));
    }

    fn keep(&mut self, name: String) {
        self.out.push((name, self.frame.clone()));
    }
}

/// Collapsed, loading chart, each timeframe with candles, the menu, stale candles and a failure placeholder,
/// at opacity 0 and 1 and at monitor scales 1 and 1.5.
fn scenes(fonts: &Fonts) -> Vec<(String, Frame)> {
    let mut all = Vec::new();
    for opacity in [0.0, 1.0] {
        for scale in [1.0, 1.5] {
            let settings = Settings {
                opacity,
                ..Settings::default()
            };
            let env = env(scale);
            let mut app = App::new(Some(&settings.to_json()), Frontmost::Wow, T0, fonts);
            let frame = app.handle(Input::Tick, T0, &env).present.unwrap();
            let mut s = Scenes {
                app,
                env,
                now: T0,
                frame,
                out: Vec::new(),
            };
            let tag = format!("opacity{opacity}-scale{scale}");
            s.keep(format!("loading-{tag}"));
            s.send(Input::QuoteReply(answered(TICKER)));
            s.keep(format!("collapsed-{tag}"));
            s.click("ticker");
            s.keep(format!("chart-loading-{tag}"));
            s.send(Input::CandlesReply(Timeframe::M15, answered(OHLC_15)));
            s.keep(format!("chart-15m-{tag}"));
            for (label, timeframe, body) in [
                ("1m", Timeframe::M1, OHLC_1),
                ("4h", Timeframe::H4, OHLC_240),
            ] {
                s.click(label);
                s.send(Input::CandlesReply(timeframe, answered(body)));
                s.keep(format!("chart-{label}-{tag}"));
            }
            let ticker = s.frame.targets()[0].1.mid();
            s.send(Input::Pointer(PointerInput::RightUp(ticker)));
            s.keep(format!("menu-{tag}"));
            s.click("menu:lock");
            s.send(Input::Pointer(PointerInput::RightUp(ticker)));
            s.keep(format!("menu-locked-{tag}"));
            s.send(Input::Pointer(PointerInput::Left));
            s.now = T0.plus(300.0);
            s.send(Input::Tick);
            s.send(Input::CandlesReply(Timeframe::H4, HttpReply::Unreachable));
            s.keep(format!("chart-stale-{tag}"));
            s.click("15m");
            s.send(Input::CandlesReply(
                Timeframe::M15,
                HttpReply::Answered {
                    status: 503,
                    body: Vec::new(),
                },
            ));
            s.click("1m");
            s.click("15m");
            s.keep(format!("chart-15m-stale-{tag}"));
            all.extend(s.out);
        }
    }
    all
}

fn renders() -> &'static [(String, Frame, Bgra)] {
    static RENDERS: std::sync::OnceLock<Vec<(String, Frame, Bgra)>> = std::sync::OnceLock::new();
    RENDERS.get_or_init(|| {
        let fonts = Fonts::embedded();
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("paint");
        std::fs::create_dir_all(&dir).unwrap();
        scenes(&fonts)
            .into_iter()
            .map(|(name, frame)| {
                let image = paint(&frame, &fonts);
                save_png(&image, &dir.join(format!("{name}.png")));
                (name, frame, image)
            })
            .collect()
    })
}

fn save_png(image: &Bgra, path: &std::path::Path) {
    let mut rgba = image.data.clone();
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let size = tiny_skia::IntSize::from_wh(image.width, image.height).unwrap();
    tiny_skia::Pixmap::from_vec(rgba, size)
        .unwrap()
        .save_png(path)
        .unwrap();
}

#[test]
fn every_pixel_catches_clicks_at_opacity_zero_and_one() {
    for (name, frame, image) in renders() {
        assert_eq!(
            (image.width, image.height),
            (frame.window.width as u32, frame.window.height as u32),
            "{name}: the buffer covers the whole window"
        );
        let open = image.data.chunks(4).position(|p| p[3] < HIT_FLOOR_ALPHA);
        assert_eq!(
            open, None,
            "{name}: a zero-alpha pixel would pass clicks to the game"
        );
    }
}

#[test]
fn pixels_are_premultiplied() {
    for (name, _, image) in renders() {
        assert!(
            image
                .data
                .chunks(4)
                .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]),
            "{name}"
        );
    }
}

#[test]
fn opacity_zero_is_invisible_and_opacity_one_is_opaque_behind_the_text() {
    for (name, _, image) in renders() {
        let corner = image.pixel(image.width / 2, 0)[3];
        if name.starts_with("collapsed-opacity0-") {
            assert_eq!(corner, HIT_FLOOR_ALPHA, "{name}: only the hit floor at 0%");
        }
        if name.starts_with("collapsed-opacity1-") {
            assert_eq!(corner, 255, "{name}: solid background at 100%");
        }
    }
}

#[test]
fn digits_are_tabular() {
    let fonts = Fonts::embedded();
    for px in [9.0, 15.0, 22.5, 72.0] {
        let style = TextStyle { px };
        let widths: Vec<f64> = "0123456789"
            .chars()
            .map(|c| fonts.advance(&c.to_string(), style))
            .collect();
        assert!(
            widths.iter().all(|w| (w - widths[0]).abs() < 1e-9),
            "{px} px: {widths:?}"
        );
    }
}

#[test]
fn every_character_the_app_draws_has_a_glyph() {
    let fonts = Fonts::embedded();
    let mut chars: BTreeSet<char> = scenes(&fonts)
        .iter()
        .flat_map(|(_, frame)| frame.draw.iter())
        .filter_map(|d| match d {
            Draw::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .flat_map(|t| t.chars().collect::<Vec<_>>())
        .collect();
    let messages = [
        ticker_core::FetchFailure::Unreachable,
        ticker_core::FetchFailure::Status(503),
        ticker_core::FetchFailure::Rejected(KrakenError::Malformed(String::new())),
    ]
    .map(|f| f.message());
    chars.extend(messages.iter().flat_map(|m| m.chars()));
    chars.extend("BTC $0123456789,.+-% stale offline Loading No candles retrying Unlock Lock Position Quit Ticker Settings\u{2026} 1m 15m 4h :/".chars());
    let missing: Vec<char> = chars
        .into_iter()
        .filter(|c| !fonts.has_glyph(*c) && *c != ' ')
        .collect();
    assert!(missing.is_empty(), "no glyph for {missing:?}");
}

#[test]
fn candles_show_up_and_down_colors() {
    let settings = Settings::default();
    let bgra = |c: Rgba| [c.blue, c.green, c.red, c.alpha].map(|v| (v * 255.0).round() as i32);
    let (up, down) = (bgra(settings.up_color), bgra(settings.down_color));
    for (name, frame, image) in renders() {
        let close = |target: [i32; 4]| {
            image
                .data
                .chunks(4)
                .filter(|p| {
                    p.iter()
                        .zip(target)
                        .all(|(&a, b)| (i32::from(a) - b).abs() <= 3)
                })
                .count()
        };
        let (ups, downs) = (close(up), close(down));
        if frame.bars_drawn > 0 {
            assert!(
                ups >= 50 && downs >= 50,
                "{name}: {} bars, {ups} up and {downs} down pixels",
                frame.bars_drawn
            );
        } else if name.starts_with("collapsed-") {
            assert_eq!(downs, 0, "{name}: no candles, and the change is up");
        }
    }
}

#[test]
fn the_ticker_text_is_drawn_in_the_text_color() {
    for (name, frame, image) in renders() {
        if !name.starts_with("collapsed-") {
            continue;
        }
        let ticker = frame.ticker;
        let white = (ticker.y as u32..(ticker.y + ticker.height) as u32)
            .flat_map(|y| (ticker.x as u32..(ticker.x + ticker.width) as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| image.pixel(x, y).iter().all(|&v| v >= 250))
            .count();
        assert!(white >= 100, "{name}: {white} white text pixels");
    }
}
