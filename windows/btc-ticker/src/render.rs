//! Draws the overlay with Direct2D and DirectWrite into a premultiplied 32-bit DIB and hands it to
//! `UpdateLayeredWindow`, which gives every pixel its own alpha. Everything here is in physical pixels: the render
//! target runs at 96 DPI and the settings' device-independent sizes are multiplied by the monitor's scale.

use std::ops::Range;

use ticker_core::{
    CandleChart, ChartInk, ChartNotice, ExpandedLayout, Rect, Rgba, Settings, Size, TickerDisplay, Timeframe,
    Timestamp, Tone, spaced_labels,
};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DASH_STYLE_CUSTOM, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateBitmap, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO, ULW_ALPHA, UpdateLayeredWindow};
use windows::core::{Result, w};
use windows_numerics::Vector2;

use crate::win;

const TICKER_PADDING: Size = Size { width: 10.0, height: 4.0 };
const AXIS_FONT_SIZE: f64 = 10.0;
/// The lowest alpha that still takes clicks: at 0 a layered window passes clicks through to the game.
const CLICKABLE_ALPHA: f32 = 1.0 / 255.0;

/// The corner away from the ticker, kept inside the panel so the layered window receives its clicks.
pub fn resize_grip(layout: &ExpandedLayout, scale: f64) -> Rect {
    let side = 16.0 * scale;
    Rect::new(
        if layout.opens_right { layout.frame.width - side } else { 0.0 },
        if layout.opens_down { layout.frame.height - side } else { 0.0 },
        side,
        side,
    )
}

/// The ticker line, laid out once per frame and measured before placement decides where it goes.
pub struct TickerText {
    layout: IDWriteTextLayout,
    tones: Vec<(DWRITE_TEXT_RANGE, Tone)>,
    dimmed: bool,
    /// The whole collapsed ticker, padding included.
    pub size: Size,
}

pub enum Panel<'a> {
    Collapsed,
    Expanded {
        layout: &'a ExpandedLayout,
        timeframe: Timeframe,
        chart: Option<&'a CandleChart>,
        notice: Option<ChartNotice>,
    },
}

/// One frame of the overlay: where it goes, how big it is, and what is on it, in panel coordinates.
pub struct Frame<'a> {
    pub origin: POINT,
    pub size: Size,
    pub ticker: Rect,
    pub text: &'a TickerText,
    pub panel: Panel<'a>,
    pub settings: &'a Settings,
    pub scale: f64,
}

pub struct Renderer {
    dwrite: IDWriteFactory,
    target: ID2D1DCRenderTarget,
    dashes: ID2D1StrokeStyle,
}

impl Renderer {
    pub fn new() -> Result<Renderer> {
        // SAFETY: factory and resource creation with valid, stack-held descriptions.
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let target = d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            })?;
            let dashes = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES { dashStyle: D2D1_DASH_STYLE_CUSTOM, ..Default::default() },
                Some(&[4.0, 3.0]),
            )?;
            Ok(Renderer { dwrite, target, dashes })
        }
    }

    pub fn ticker_text(&self, display: &TickerDisplay, settings: &Settings, scale: f64) -> Result<TickerText> {
        let format = self.format(settings.font_size * scale, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
        let text: Vec<u16> = display.text().encode_utf16().collect();
        let mut tones = Vec::new();
        let mut start = 0u32;
        for segment in &display.segments {
            let length = segment.text.encode_utf16().count() as u32;
            tones.push((DWRITE_TEXT_RANGE { startPosition: start, length }, segment.tone));
            start += length;
        }
        // SAFETY: plain DirectWrite calls on owned objects.
        let layout = unsafe {
            let layout = self.dwrite.CreateTextLayout(&text, &format, 100_000.0, 100_000.0)?;
            // Tabular figures keep the line from twitching sideways as the digits change.
            let typography = self.dwrite.CreateTypography()?;
            typography.AddFontFeature(DWRITE_FONT_FEATURE {
                nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                parameter: 1,
            })?;
            layout.SetTypography(&typography, DWRITE_TEXT_RANGE { startPosition: 0, length: text.len() as u32 })?;
            layout
        };
        let measured = measure(&layout)?;
        let size = Size {
            width: (measured.width + 2.0 * TICKER_PADDING.width * scale).ceil(),
            height: (measured.height + 2.0 * TICKER_PADDING.height * scale).ceil(),
        };
        Ok(TickerText { layout, tones, dimmed: display.dimmed, size })
    }

    /// One timeframe control: room for the widest label, no taller than the ticker line.
    pub fn segment_size(&self, settings: &Settings, scale: f64, ticker_height: f64) -> Result<Size> {
        let format = self.format(segment_font_size(settings) * scale, DWRITE_FONT_WEIGHT_BOLD)?;
        let mut widest = Size::default();
        for timeframe in Timeframe::ALL {
            let label = self.text_layout(timeframe.spec().label, &format)?;
            let size = measure(&label)?;
            widest = Size { width: widest.width.max(size.width), height: widest.height.max(size.height) };
        }
        Ok(Size {
            width: widest.width.ceil() + 14.0 * scale,
            height: (widest.height.ceil() + 6.0 * scale).min(ticker_height),
        })
    }

    pub fn present(&self, hwnd: HWND, frame: &Frame) -> Result<()> {
        let bitmap = self.paint(frame)?;
        // SAFETY: hands the painted DIB, which outlives the call, to the layered window.
        unsafe {
            let screen = GetDC(None);
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
                ..Default::default()
            };
            let presented = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&frame.origin),
                Some(&SIZE { cx: bitmap.width, cy: bitmap.height }),
                Some(bitmap.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
            presented
        }
    }

    /// Draws the frame into a new premultiplied DIB on the CPU.
    fn paint(&self, frame: &Frame) -> Result<Dib> {
        let width = frame.size.width.max(1.0) as i32;
        let height = frame.size.height.max(1.0) as i32;
        let bitmap = Dib::new(width, height)?;
        // SAFETY: the render target draws into the DIB selected into `bitmap.dc`, which outlives the frame.
        unsafe {
            self.target.BindDC(bitmap.dc, &RECT { left: 0, top: 0, right: width, bottom: height })?;
            self.target.BeginDraw();
            self.target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            self.target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: CLICKABLE_ALPHA }));
            self.draw(frame)?;
            self.target.EndDraw(None, None)?;
            GdiFlush().ok()?;
        }
        Ok(bitmap)
    }

    /// The tray icon: an orange disc with a white "B", `size` pixels square.
    pub fn icon(&self, size: i32) -> Result<HICON> {
        let mut bitmap = Dib::new(size, size)?;
        let side = size as f32;
        // SAFETY: draws into the DIB, then hands its bits to CreateIconIndirect, which copies them.
        unsafe {
            self.target.BindDC(bitmap.dc, &RECT { left: 0, top: 0, right: size, bottom: size })?;
            self.target.BeginDraw();
            self.target.Clear(Some(&D2D1_COLOR_F::default()));
            let orange = self.brush(D2D1_COLOR_F { r: 0.969, g: 0.576, b: 0.102, a: 1.0 })?;
            let center = Vector2 { X: side / 2.0, Y: side / 2.0 };
            let radius = side / 2.0 - 0.5;
            self.target.FillEllipse(&D2D1_ELLIPSE { point: center, radiusX: radius, radiusY: radius }, &orange);
            let format = self.format(f64::from(side) * 0.72, DWRITE_FONT_WEIGHT_BOLD)?;
            let letter = self.text_layout("B", &format)?;
            let measured = measure(&letter)?;
            let white = self.brush(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })?;
            let origin = Vector2 {
                X: ((side - measured.width as f32) / 2.0).round(),
                Y: ((side - measured.height as f32) / 2.0).round(),
            };
            self.target.DrawTextLayout(origin, &letter, &white, D2D1_DRAW_TEXT_OPTIONS_NONE);
            self.target.EndDraw(None, None)?;
            GdiFlush().ok()?;
            // Icons take straight alpha; Direct2D wrote premultiplied.
            for pixel in bitmap.pixels_mut().as_chunks_mut::<4>().0 {
                let alpha = u32::from(pixel[3]);
                if alpha > 0 && alpha < 255 {
                    for channel in &mut pixel[..3] {
                        *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                    }
                }
            }
            let mask = CreateBitmap(size, size, 1, 1, None);
            let icon = CreateIconIndirect(&ICONINFO {
                fIcon: true.into(),
                hbmMask: mask,
                hbmColor: bitmap.bitmap,
                ..Default::default()
            });
            let _ = DeleteObject(mask.into());
            icon
        }
    }

    fn draw(&self, frame: &Frame) -> Result<()> {
        let (settings, scale) = (frame.settings, frame.scale);
        let whole = Rect::new(0.0, 0.0, frame.size.width, frame.size.height);
        let radius = (8.0 * scale).min(frame.ticker.height / 2.0) as f32;
        let background = self.brush(D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: settings.opacity as f32 })?;
        // SAFETY: drawing between BeginDraw and EndDraw.
        unsafe {
            self.target.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: rect_f(whole), radiusX: radius, radiusY: radius },
                &background,
            );
            let fade = if frame.text.dimmed { 0.5 } else { 1.0 };
            for (range, tone) in &frame.text.tones {
                let brush = self.brush(color(tone_color(settings, *tone), fade))?;
                frame.text.layout.SetDrawingEffect(&brush, *range)?;
            }
            let neutral = self.brush(color(settings.text_color, fade))?;
            let origin = Vector2 {
                X: (frame.ticker.x + TICKER_PADDING.width * scale) as f32,
                Y: (frame.ticker.y + TICKER_PADDING.height * scale) as f32,
            };
            self.target.DrawTextLayout(origin, &frame.text.layout, &neutral, D2D1_DRAW_TEXT_OPTIONS_NONE);

            if let Panel::Expanded { layout, timeframe, chart, notice } = &frame.panel {
                self.draw_segments(layout, *timeframe, settings, scale)?;
                self.draw_chart(layout.chart, *chart, notice.as_ref(), settings, scale)?;
                if !settings.locked {
                    let grip = resize_grip(layout, scale);
                    let ink = self.brush(color(settings.text_color, 0.65))?;
                    let x = if layout.opens_right { grip.max_x() } else { grip.min_x() };
                    let y = if layout.opens_down { grip.max_y() } else { grip.min_y() };
                    let dx = if layout.opens_right { -scale } else { scale };
                    let dy = if layout.opens_down { -scale } else { scale };
                    for length in [6.0, 10.0] {
                        self.target.DrawLine(
                            Vector2 { X: (x + 3.0 * dx) as f32, Y: (y + length * dy) as f32 },
                            Vector2 { X: (x + length * dx) as f32, Y: (y + 3.0 * dy) as f32 },
                            &ink,
                            scale as f32,
                            None,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn draw_segments(&self, layout: &ExpandedLayout, chosen: Timeframe, settings: &Settings, scale: f64) -> Result<()> {
        for timeframe in Timeframe::ALL {
            let selected = timeframe == chosen;
            let rect = layout.timeframe_rect(timeframe);
            let fill = self.brush(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: if selected { 0.26 } else { 0.08 } })?;
            let weight = if selected { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_MEDIUM };
            let label =
                self.text_layout(timeframe.spec().label, &self.format(segment_font_size(settings) * scale, weight)?)?;
            let size = measure(&label)?;
            let ink = self.brush(color(settings.text_color, if selected { 1.0 } else { 0.6 }))?;
            let corner = (4.0 * scale) as f32;
            let inset = Rect::new(rect.x + 1.0, rect.y, rect.width - 2.0, rect.height);
            // SAFETY: drawing between BeginDraw and EndDraw.
            unsafe {
                self.target.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: rect_f(inset), radiusX: corner, radiusY: corner },
                    &fill,
                );
                let origin = Vector2 {
                    X: (rect.mid_x() - size.width / 2.0).round() as f32,
                    Y: (rect.mid_y() - size.height / 2.0).round() as f32,
                };
                self.target.DrawTextLayout(origin, &label, &ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
        Ok(())
    }

    fn draw_chart(
        &self,
        area: Rect,
        chart: Option<&CandleChart>,
        notice: Option<&ChartNotice>,
        settings: &Settings,
        scale: f64,
    ) -> Result<()> {
        let axis_format = self.format(AXIS_FONT_SIZE * scale, DWRITE_FONT_WEIGHT_MEDIUM)?;
        let Some(chart) = chart else {
            let text = match notice {
                Some(ChartNotice::Placeholder(text)) => text.as_str(),
                _ => "",
            };
            let label = self.text_layout(text, &self.format(12.0 * scale, DWRITE_FONT_WEIGHT_MEDIUM)?)?;
            let size = measure(&label)?;
            let ink = self.brush(color(ChartInk::Placeholder.color(settings.text_color), 1.0))?;
            let origin = Vector2 {
                X: (area.mid_x() - size.width / 2.0).round() as f32,
                Y: (area.mid_y() - size.height / 2.0).round() as f32,
            };
            // SAFETY: drawing between BeginDraw and EndDraw.
            unsafe { self.target.DrawTextLayout(origin, &label, &ink, D2D1_DRAW_TEXT_OPTIONS_NONE) };
            return Ok(());
        };

        let (low, high) = (*chart.price_domain.start(), *chart.price_domain.end());
        let prices: Vec<(f64, IDWriteTextLayout)> = chart
            .price_ticks()
            .into_iter()
            .map(|tick| Ok((tick.value, self.text_layout(&tick.label, &axis_format)?)))
            .collect::<Result<_>>()?;
        let mut label_width: f64 = 0.0;
        let mut label_height: f64 = 0.0;
        for (_, label) in &prices {
            let size = measure(label)?;
            label_width = label_width.max(size.width);
            label_height = label_height.max(size.height);
        }
        let gap = 6.0 * scale;
        let plot = Rect::new(
            area.x,
            area.y + label_height / 2.0,
            (area.width - label_width - gap).max(1.0),
            (area.height - label_height * 1.5 - gap).max(1.0),
        );
        let (start, end) = (*chart.time_domain.start(), *chart.time_domain.end());
        let span = end.seconds_since(start).max(1.0);
        let x_of = |time: Timestamp| plot.x + time.seconds_since(start) / span * plot.width;
        let y_of = |price: f64| plot.max_y() - (price - low) / (high - low).max(f64::EPSILON) * plot.height;

        let times: Vec<(f64, IDWriteTextLayout, Range<f64>)> = chart
            .time_ticks(win::utc_offset(end), win::hour12())
            .into_iter()
            .map(|tick| {
                let x = x_of(tick.value).round() + 0.5;
                let label = self.text_layout(&tick.label, &axis_format)?;
                let width = measure(&label)?.width;
                let left = (x - width / 2.0).clamp(plot.x, plot.max_x() - width);
                Ok((x, label, left..left + width))
            })
            .collect::<Result<_>>()?;
        let extents: Vec<Range<f64>> = times.iter().map(|(.., extent)| extent.clone()).collect();

        let grid = self.brush(color(ChartInk::Grid.color(settings.text_color), 1.0))?;
        let axis_ink = self.brush(color(ChartInk::AxisLabel.color(settings.text_color), 1.0))?;
        // SAFETY: drawing between BeginDraw and EndDraw.
        unsafe {
            for (price, label) in &prices {
                let y = y_of(*price).round() + 0.5;
                self.target.DrawLine(point(plot.x, y), point(plot.max_x(), y), &grid, 1.0, None);
                let origin = point(plot.max_x() + gap, y - label_height / 2.0);
                self.target.DrawTextLayout(origin, label, &axis_ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            for ((x, label, extent), drawn) in times.iter().zip(spaced_labels(&extents, gap)) {
                self.target.DrawLine(point(*x, plot.y), point(*x, plot.max_y()), &grid, 1.0, None);
                if drawn {
                    let origin = point(extent.start, plot.max_y() + gap / 2.0);
                    self.target.DrawTextLayout(origin, label, &axis_ink, D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
            }

            let body_width = (plot.width / chart.bars.len().max(1) as f64 * 0.7).max(1.0);
            for bar in &chart.bars {
                let brush = self.brush(color(tone_color(settings, bar.tone), 1.0))?;
                let x = x_of(bar.time);
                let wick = Rect::new(x.round(), y_of(bar.high), 1.0, (y_of(bar.low) - y_of(bar.high)).max(1.0));
                self.target.FillRectangle(&rect_f(wick), &brush);
                let top = y_of(bar.body_high);
                let body = Rect::new(x - body_width / 2.0, top, body_width, (y_of(bar.body_low) - top).max(1.0));
                self.target.FillRectangle(&rect_f(body), &brush);
            }

            let last = self.brush(color(ChartInk::LastPrice.color(settings.text_color), 1.0))?;
            let y = y_of(chart.last_price).round() + 0.5;
            self.target.DrawLine(point(plot.x, y), point(plot.max_x(), y), &last, 1.0, &self.dashes);

            if let Some(ChartNotice::Stale(marker)) = notice {
                let label =
                    self.text_layout(marker, &self.format(AXIS_FONT_SIZE * scale, DWRITE_FONT_WEIGHT_SEMI_BOLD)?)?;
                let size = measure(&label)?;
                let (pad_x, pad_y) = (5.0 * scale, 1.0 * scale);
                let capsule = Rect::new(plot.x, plot.y, size.width + 2.0 * pad_x, size.height + 2.0 * pad_y);
                let radius = (capsule.height / 2.0) as f32;
                let shade = self.brush(D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.6 })?;
                self.target.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: rect_f(capsule), radiusX: radius, radiusY: radius },
                    &shade,
                );
                let ink = self.brush(color(settings.down_color, 1.0))?;
                self.target.DrawTextLayout(
                    point(capsule.x + pad_x, capsule.y + pad_y),
                    &label,
                    &ink,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        Ok(())
    }

    fn format(&self, size: f64, weight: DWRITE_FONT_WEIGHT) -> Result<IDWriteTextFormat> {
        // SAFETY: plain factory call with static strings.
        unsafe {
            self.dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size as f32,
                w!("en-us"),
            )
        }
    }

    fn text_layout(&self, text: &str, format: &IDWriteTextFormat) -> Result<IDWriteTextLayout> {
        let text: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: plain factory call; the layout copies the text.
        unsafe { self.dwrite.CreateTextLayout(&text, format, 100_000.0, 100_000.0) }
    }

    fn brush(&self, color: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush> {
        // SAFETY: plain resource creation on the render target.
        unsafe { self.target.CreateSolidColorBrush(&color, None) }
    }
}

/// A 32-bit top-down DIB selected into its own memory DC, released on drop.
struct Dib {
    width: i32,
    height: i32,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    length: usize,
    previous: windows::Win32::Graphics::Gdi::HGDIOBJ,
}

impl Dib {
    fn new(width: i32, height: i32) -> Result<Dib> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: creates and selects GDI objects that Drop releases.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let previous = SelectObject(dc, bitmap.into());
            Ok(Dib { width, height, dc, bitmap, bits: bits.cast(), length: (width * height * 4) as usize, previous })
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        // SAFETY: the DIB section owns `length` bytes at `bits` for as long as `self` lives.
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.length) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        // SAFETY: undoes `new` in reverse order.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

fn measure(layout: &IDWriteTextLayout) -> Result<Size> {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: writes one struct.
    unsafe { layout.GetMetrics(&mut metrics)? };
    Ok(Size { width: f64::from(metrics.widthIncludingTrailingWhitespace), height: f64::from(metrics.height) })
}

fn segment_font_size(settings: &Settings) -> f64 {
    (settings.font_size * 0.75).max(9.0)
}

fn tone_color(settings: &Settings, tone: Tone) -> Rgba {
    match tone {
        Tone::Neutral => settings.text_color,
        Tone::Up => settings.up_color,
        Tone::Down => settings.down_color,
    }
}

fn color(rgba: Rgba, fade: f64) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: rgba.red as f32, g: rgba.green as f32, b: rgba.blue as f32, a: (rgba.alpha * fade) as f32 }
}

fn rect_f(rect: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: rect.x as f32, top: rect.y as f32, right: rect.max_x() as f32, bottom: rect.max_y() as f32 }
}

fn point(x: f64, y: f64) -> Vector2 {
    Vector2 { X: x as f32, Y: y as f32 }
}

#[cfg(test)]
mod tests;
