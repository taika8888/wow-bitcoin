use std::path::{Path, PathBuf};

use ticker_core::kraken::parse_candles;
use ticker_core::{
    CandleChart, DisplaySegment, ExpandedLayout, Rect, Rgba, Settings, Size, TickerDisplay, Timeframe, Tone, Zoom,
};
use windows::Win32::Foundation::POINT;

use super::{Frame, Panel, Renderer};

/// Kraken responses recorded for the macOS tests, the same bytes the core tests read.
const RECORDED: [(&str, Timeframe); 3] = [
    (include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_1.json"), Timeframe::M1),
    (include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_15.json"), Timeframe::M15),
    (include_str!("../../../../Tests/TickerCoreTests/Fixtures/kraken/ohlc_240.json"), Timeframe::H4),
];

/// A mid-grey stand-in for the game scene the overlay floats over.
const BACKDROP: u8 = 0x80;

/// One painted overlay: premultiplied BGRA rows, top-down.
struct Painted {
    width: usize,
    height: usize,
    bgra: Vec<u8>,
    chart: Rect,
}

impl Painted {
    /// The brightest color channel in a band of rows, in premultiplied terms: how much light the overlay adds there.
    fn brightest(&self, rows: std::ops::Range<f64>) -> u8 {
        let rows = rows.start as usize * self.width * 4..rows.end as usize * self.width * 4;
        self.bgra[rows].as_chunks::<4>().0.iter().flat_map(|pixel| pixel[..3].iter().copied()).max().unwrap()
    }
}

fn paint(renderer: &Renderer, settings: &Settings, body: &str, scale: f64) -> Painted {
    let display = TickerDisplay {
        segments: vec![DisplaySegment::new("BTC $80,412", Tone::Neutral), DisplaySegment::new("  +1.24%", Tone::Up)],
        dimmed: false,
    };
    let text = renderer.ticker_text(&display, settings, scale).unwrap();
    let segment = renderer.segment_size(settings, scale, text.size.height).unwrap();
    let ticker = Rect::new(20.0, 20.0, text.size.width, text.size.height);
    let chart_size = Size { width: settings.chart_width * scale, height: settings.chart_height * scale };
    let work = Rect::new(0.0, 0.0, 1920.0 * scale, 1080.0 * scale);
    let layout = ExpandedLayout::new(ticker, segment, chart_size, work);
    let candles = parse_candles(body.as_bytes()).unwrap();
    let chart = CandleChart::new(&candles, settings.timeframe, settings.zoom, layout.max_bars(scale)).unwrap();
    let frame = Frame {
        origin: POINT::default(),
        size: layout.frame.size(),
        ticker: layout.ticker,
        text: &text,
        panel: Panel::Expanded { layout: &layout, timeframe: settings.timeframe, chart: Some(&chart), notice: None },
        settings,
        scale,
    };
    let mut dib = renderer.paint(&frame).unwrap();
    Painted {
        width: dib.width as usize,
        height: dib.height as usize,
        bgra: dib.pixels_mut().to_vec(),
        chart: layout.chart,
    }
}

/// Composites the overlay over the backdrop, as the desktop would, and writes it as an RGB PNG.
fn save(painted: &Painted, path: &Path) {
    let mut rgb = Vec::with_capacity(painted.width * painted.height * 3);
    for [blue, green, red, alpha] in painted.bgra.as_chunks::<4>().0.iter().copied() {
        let under = u32::from(BACKDROP) * (255 - u32::from(alpha)) / 255;
        rgb.extend([red, green, blue].map(|channel| (u32::from(channel) + under).min(255) as u8));
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, painted.width as u32, painted.height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.write_header().unwrap().write_image_data(&rgb).unwrap();
}

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/parity")
}

/// Renders the narrowest chart the settings allow, at every timeframe and zoom, from the recorded candles, with the
/// price color at 35% opacity, and saves each frame under `target/parity` for a person to look at.
#[test]
fn the_narrowest_chart_renders_from_recorded_candles() {
    let renderer = Renderer::new().unwrap();
    for (body, timeframe) in RECORDED {
        for zoom in Zoom::ALL {
            let settings = Settings {
                timeframe,
                zoom,
                chart_width: *Settings::CHART_WIDTH_RANGE.start(),
                text_color: Rgba::new(1.0, 1.0, 1.0, 0.35),
                ..Settings::default()
            };
            let painted = paint(&renderer, &settings, body, 1.0);
            let label = timeframe.spec().label;
            save(&painted, &parity_dir().join(format!("narrow-{label}-{zoom:?}.png").to_lowercase()));
        }
    }
    for scale in [1.0, 2.0] {
        let settings = Settings { timeframe: Timeframe::M15, ..Settings::default() };
        let painted = paint(&renderer, &settings, RECORDED[1].0, scale);
        save(&painted, &parity_dir().join(format!("default-15m-{}x.png", scale)));
    }
}

/// The time labels sit in the bottom rows of the chart, below the plot, in the axis label ink: 70% of the text
/// color's own opacity. White text at 35% must draw them at 35% of the brightness opaque white text does.
#[test]
fn a_translucent_price_color_fades_the_time_labels() {
    let renderer = Renderer::new().unwrap();
    let brightest = |alpha| {
        let settings =
            Settings { timeframe: Timeframe::M15, text_color: Rgba::new(1.0, 1.0, 1.0, alpha), ..Settings::default() };
        let painted = paint(&renderer, &settings, RECORDED[1].0, 1.0);
        painted.brightest(painted.chart.max_y() - 12.0..painted.chart.max_y())
    };
    let opaque = brightest(1.0);
    let faded = brightest(0.35);
    assert!((170..=179).contains(&opaque), "opaque white labels peak at 70% of 255, not {opaque}");
    assert!((50..=63).contains(&faded), "35% white labels peak at 35% of 70% of 255, not {faded}");
}
