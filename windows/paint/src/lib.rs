//! Rasterizes a `ticker_core::Frame` into the premultiplied BGRA top-down buffer that `UpdateLayeredWindow` takes.
//! No platform code, so `cargo test` on the Mac inspects the exact pixels Windows will show.
use ticker_core::{Draw, Frame, Measure, Point, Rect, Rgba, TextStyle};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

/// A per-pixel-alpha layered window lets clicks through wherever alpha is 0, so a 0% background would hand clicks
/// to the game. Every pixel of the window gets at least this alpha, which is invisible.
pub const HIT_FLOOR_ALPHA: u8 = 1;

const QUARTER_CIRCLE_BEZIER_K: f32 = 0.552_284_8;

/// The one embedded font. Grayscale coverage only: ClearType cannot work on a per-pixel-alpha window.
pub struct Fonts {
    font: fontdue::Font,
}

impl Fonts {
    pub fn embedded() -> Fonts {
        let font = fontdue::Font::from_bytes(
            include_bytes!("../fonts/NotoSans-SemiBold.ttf").as_slice(),
            fontdue::FontSettings::default(),
        )
        .expect("the embedded font parses");
        Fonts { font }
    }

    pub fn has_glyph(&self, c: char) -> bool {
        self.font.lookup_glyph_index(c) != 0
    }

    fn ascent(&self, px: f32) -> f32 {
        self.font
            .horizontal_line_metrics(px)
            .map_or(px, |m| m.ascent)
    }
}

impl Measure for Fonts {
    fn advance(&self, text: &str, style: TextStyle) -> f64 {
        text.chars()
            .map(|c| f64::from(self.font.metrics(c, style.px as f32).advance_width))
            .sum()
    }

    fn line_height(&self, style: TextStyle) -> f64 {
        let px = style.px as f32;
        self.font
            .horizontal_line_metrics(px)
            .map_or(style.px, |m| f64::from(m.ascent - m.descent))
    }
}

/// Premultiplied BGRA, top-down rows, `width * 4` stride: the DIB section layout UpdateLayeredWindow wants, so the
/// shell copies `data` into the DIB's bits in one memcpy.
pub struct Bgra {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Bgra {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

pub fn paint(frame: &Frame, fonts: &Fonts) -> Bgra {
    let width = (frame.window.width.ceil() as u32).max(1);
    let height = (frame.window.height.ceil() as u32).max(1);
    let mut pixmap = Pixmap::new(width, height).expect("the window has a nonzero size");
    pixmap.fill(Color::from_rgba8(0, 0, 0, HIT_FLOOR_ALPHA));
    for draw in &frame.draw {
        match draw {
            Draw::RoundRect {
                rect,
                radius,
                color,
            } => fill(&mut pixmap, round_rect(rect, *radius), *color),
            Draw::Rect { rect, color } => fill(&mut pixmap, round_rect(rect, 0.0), *color),
            Draw::Line {
                from,
                to,
                width,
                dash,
                color,
            } => line(&mut pixmap, *from, *to, *width, *dash, *color),
            Draw::Text {
                origin,
                text,
                style,
                color,
            } => text_run(&mut pixmap, fonts, *origin, text, *style, *color),
        }
    }
    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Bgra {
        width,
        height,
        data,
    }
}

fn solid(color: Rgba) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color(
        Color::from_rgba(
            color.red as f32,
            color.green as f32,
            color.blue as f32,
            color.alpha as f32,
        )
        .unwrap_or(Color::TRANSPARENT),
    );
    paint
}

fn round_rect(rect: &Rect, radius: f64) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
    );
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = (radius as f32).min(w / 2.0).min(h / 2.0).max(0.0);
    if r == 0.0 {
        return Some(PathBuilder::from_rect(tiny_skia::Rect::from_xywh(
            x, y, w, h,
        )?));
    }
    let k = r * QUARTER_CIRCLE_BEZIER_K;
    let mut path = PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(x + w - r, y);
    path.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    path.line_to(x + w, y + h - r);
    path.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    path.line_to(x + r, y + h);
    path.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    path.line_to(x, y + r);
    path.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    path.close();
    path.finish()
}

fn fill(pixmap: &mut Pixmap, path: Option<tiny_skia::Path>, color: Rgba) {
    if let Some(path) = path {
        pixmap.fill_path(
            &path,
            &solid(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn line(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    width: f64,
    dash: Option<(f64, f64)>,
    color: Rgba,
) {
    let mut path = PathBuilder::new();
    path.move_to(from.x as f32, from.y as f32);
    path.line_to(to.x as f32, to.y as f32);
    let Some(path) = path.finish() else {
        return;
    };
    let stroke = Stroke {
        width: width as f32,
        dash: dash.and_then(|(on, off)| StrokeDash::new(vec![on as f32, off as f32], 0.0)),
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &solid(color), &stroke, Transform::identity(), None);
}

fn text_run(
    pixmap: &mut Pixmap,
    fonts: &Fonts,
    origin: Point,
    text: &str,
    style: TextStyle,
    color: Rgba,
) {
    let px = style.px as f32;
    let baseline = (origin.y as f32 + fonts.ascent(px)).round() as i32;
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let alpha = color.alpha as f32;
    let rgb = [color.red as f32, color.green as f32, color.blue as f32];
    let data = pixmap.data_mut();
    let mut pen = origin.x as f32;
    for c in text.chars() {
        let (metrics, coverage) = fonts.font.rasterize(c, px);
        let left = (pen + metrics.xmin as f32).round() as i32;
        let top = baseline - metrics.ymin - metrics.height as i32;
        for (row, line) in coverage.chunks(metrics.width.max(1)).enumerate() {
            let y = top + row as i32;
            if !(0..height).contains(&y) {
                continue;
            }
            for (col, &cov) in line.iter().enumerate() {
                let x = left + col as i32;
                if cov == 0 || !(0..width).contains(&x) {
                    continue;
                }
                let source_alpha = alpha * f32::from(cov) / 255.0;
                let i = ((y * width + x) * 4) as usize;
                let keep = 1.0 - source_alpha;
                for channel in 0..3 {
                    let dest = f32::from(data[i + channel]) / 255.0;
                    data[i + channel] =
                        ((rgb[channel] * source_alpha + dest * keep) * 255.0).round() as u8;
                }
                let dest = f32::from(data[i + 3]) / 255.0;
                data[i + 3] = ((source_alpha + dest * keep) * 255.0).round() as u8;
            }
        }
        pen += metrics.advance_width;
    }
}
