//! The overlay window: a layered, topmost popup that never activates. It turns raw mouse messages into the core's
//! overlay events and draws whatever the core's state says.

use std::collections::BTreeMap;

use ticker_core::{
    App, DragPhase, ExpandedLayout, OverlayEvent, Point, PressTarget, Rect, Screens, Settings, Size, Timeframe,
    ZoomStep, placement,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageExtraInfo, HWND_TOPMOST, IDC_ARROW, IDC_SIZENESW, IDC_SIZENWSE,
    IsWindowVisible, LoadCursorW, MA_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetCursor, SetWindowPos, ShowWindow, WM_CAPTURECHANGED, WM_CLOSE, WM_DPICHANGED, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

use crate::menu::Command;
use crate::render::{Frame, Panel, Renderer};
use crate::shell::{Event, deliver};
use crate::{wheel, win};

/// Mouse input on the overlay, in screen pixels. `extra` is the input's `dwExtraInfo`, which tells the end-to-end
/// check's own clicks from a person's.
#[derive(Clone, Copy, Debug)]
pub enum Pointer {
    Down {
        extra: isize,
    },
    Move,
    Up,
    /// Capture went elsewhere before the button came up.
    Lost,
    RightDown {
        extra: isize,
    },
    RightUp {
        at: POINT,
    },
    /// A wheel notch, a touchpad scroll or a touchpad pinch, `delta` in `WHEEL_DELTA` units, at a screen point.
    Wheel {
        delta: i32,
        at: POINT,
    },
    /// The cursor left the overlay.
    Left,
    Cursor,
}

pub struct Overlay {
    hwnd: HWND,
    renderer: Renderer,
    drawn: Drawn,
    press: Option<Press>,
    resize: Option<Resize>,
    travel: wheel::Travel,
    /// Takes the wheel over the chart from the focused game while Windows routes the wheel to focus.
    grab: Option<wheel::Grab>,
}

/// Where the button went down and where the window was then, both on screen.
#[derive(Clone, Copy)]
struct Press {
    cursor: POINT,
    origin: Point,
}

struct Resize {
    cursor: POINT,
    original: Settings,
    preview: Settings,
    layout: ExpandedLayout,
    scale: f64,
    maximum: Size,
}

impl Resize {
    fn moved(&mut self, cursor: POINT) {
        let x = if self.layout.opens_right { 1.0 } else { -1.0 };
        let y = if self.layout.opens_down { 1.0 } else { -1.0 };
        self.preview.chart_width =
            self.layout.chart.width / self.scale + x * f64::from(cursor.x - self.cursor.x) / self.scale;
        self.preview.chart_height =
            self.layout.chart.height / self.scale + y * f64::from(cursor.y - self.cursor.y) / self.scale;
        self.preview.chart_width = self.preview.chart_width.min(self.maximum.width);
        self.preview.chart_height = self.preview.chart_height.min(self.maximum.height);
        self.preview = self.preview.clamped();
    }
}

/// What the last frame put where, in panel coordinates, for hit testing and the snapshot.
#[derive(Default)]
struct Drawn {
    ticker: Rect,
    layout: Option<ExpandedLayout>,
    max_bars: usize,
    resize: Option<Rect>,
}

impl Overlay {
    pub fn create() -> Result<Overlay> {
        let class = w!("BTCTickerOverlay");
        win::register_class(class, Some(procedure), None);
        // SAFETY: creates a window the process owns until exit.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class,
                w!("BTC Ticker"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )?
        };
        Ok(Overlay {
            hwnd,
            renderer: Renderer::new()?,
            drawn: Drawn::default(),
            press: None,
            resize: None,
            travel: wheel::Travel::default(),
            grab: None,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// The overlay event a mouse message means, if any. Whether a press becomes a drag or a click is the core's call.
    pub fn pointer(&mut self, pointer: Pointer, app: &App) -> Option<OverlayEvent> {
        if matches!(pointer, Pointer::Down { .. }) && self.over_resize() {
            let layout = self.drawn.layout?;
            let screens = win::screens();
            let window = win::window_rect(self.hwnd);
            let work = placement::screen_for(&window, &screens).map_or(screens.primary.work, |screen| screen.work);
            let scale = win::scale(self.hwnd);
            self.travel.reset();
            self.resize = Some(Resize {
                cursor: win::cursor(),
                original: *app.settings(),
                preview: *app.settings(),
                layout,
                scale,
                maximum: Size {
                    width: (work.width - 2.0 * ExpandedLayout::INSET) / scale,
                    height: (work.height - layout.ticker.height - ExpandedLayout::INSET) / scale,
                },
            });
            // SAFETY: captures this thread's own window without activating it.
            unsafe { SetCapture(self.hwnd) };
            return None;
        }
        if self.resize.is_some() {
            match pointer {
                Pointer::Move => {
                    self.resize.as_mut()?.moved(win::cursor());
                    if let Err(error) = self.draw(app, &win::screens()) {
                        eprintln!("could not resize the overlay: {error}");
                    }
                    return None;
                }
                Pointer::Up | Pointer::Lost => {
                    let mut resize = self.resize.take()?;
                    // SAFETY: clear the gesture before the resulting WM_CAPTURECHANGED.
                    let _ = unsafe { ReleaseCapture() };
                    if matches!(pointer, Pointer::Up) {
                        resize.moved(win::cursor());
                        // An unchanged setting emits no core Render effect, even if the last preview differed.
                        if resize.preview == *app.settings()
                            && let Err(error) = self.draw(app, &win::screens())
                        {
                            eprintln!("could not finish resizing: {error}");
                        }
                        return Some(OverlayEvent::SettingsEdited(resize.preview));
                    }
                    if let Err(error) = self.draw(app, &win::screens()) {
                        eprintln!("could not cancel resizing: {error}");
                    }
                    return None;
                }
                _ => {}
            }
        }
        match pointer {
            Pointer::Down { .. } => {
                self.travel.reset();
                self.press = Some(Press { cursor: win::cursor(), origin: win::window_rect(self.hwnd).origin() });
                // SAFETY: captures the mouse for this thread's own window.
                unsafe { SetCapture(self.hwnd) };
                Some(OverlayEvent::Pressed)
            }
            Pointer::Move => {
                let Some(press) = self.press else {
                    self.hover();
                    return None;
                };
                let cursor = win::cursor();
                let distance = f64::from(cursor.x - press.cursor.x).hypot(f64::from(cursor.y - press.cursor.y));
                Some(OverlayEvent::PointerMoved { distance })
            }
            Pointer::Up | Pointer::Lost => {
                self.press.take()?;
                // SAFETY: releases this thread's capture; the WM_CAPTURECHANGED it sends finds no press left.
                let _ = unsafe { ReleaseCapture() };
                let over = match pointer {
                    Pointer::Lost => PressTarget::Outside,
                    _ => self.target_at(win::cursor()),
                };
                Some(OverlayEvent::Released { over, ticker_origin: self.dropped_at() })
            }
            Pointer::Left => {
                self.grab = None;
                self.travel.reset();
                None
            }
            Pointer::Cursor => {
                let layout = self
                    .resize
                    .as_ref()
                    .map(|resize| resize.layout)
                    .or_else(|| self.drawn.layout.filter(|_| self.over_resize()));
                let cursor = match layout {
                    Some(layout) if layout.opens_down == layout.opens_right => IDC_SIZENWSE,
                    Some(_) => IDC_SIZENESW,
                    None => IDC_ARROW,
                };
                // SAFETY: standard shared cursors; Windows owns their lifetime.
                unsafe {
                    SetCursor(LoadCursorW(None, cursor).ok());
                }
                None
            }
            Pointer::RightDown { .. } | Pointer::RightUp { .. } | Pointer::Wheel { .. } => None,
        }
    }

    fn over_resize(&self) -> bool {
        let window = win::window_rect(self.hwnd);
        let cursor = win::cursor();
        let point = Point { x: f64::from(cursor.x) - window.x, y: f64::from(cursor.y) - window.y };
        self.drawn.resize.is_some_and(|grip| grip.contains(point))
    }

    /// The zoom steps a wheel delta adds up to, when it lands on the expanded chart.
    pub fn wheel(&mut self, delta: i32, at: POINT) -> Vec<ZoomStep> {
        let at = Point { x: f64::from(at.x), y: f64::from(at.y) };
        if !self.chart_on_screen().is_some_and(|chart| chart.contains(at)) {
            self.travel.reset();
            return Vec::new();
        }
        self.travel.add(delta)
    }

    /// Wheel travel lasts only while the cursor stays on the chart. Over it while Windows sends the wheel to the focused
    /// window, grab the wheel until the cursor leaves.
    fn hover(&mut self) {
        let cursor = win::cursor();
        let cursor = Point { x: f64::from(cursor.x), y: f64::from(cursor.y) };
        let Some(chart) = self.chart_on_screen().filter(|chart| chart.contains(cursor)) else {
            self.travel.reset();
            self.grab = None;
            return;
        };
        let mut leave = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: asks for one WM_MOUSELEAVE to this thread's own window.
        let _ = unsafe { TrackMouseEvent(&mut leave) };
        match (wheel::follows_cursor(), &self.grab) {
            (true, _) => self.grab = None,
            (false, Some(grab)) => grab.retarget(chart),
            (false, None) => self.grab = wheel::Grab::new(chart, self.hwnd),
        }
    }

    fn chart_on_screen(&self) -> Option<Rect> {
        let window = win::window_rect(self.hwnd);
        self.drawn.layout.map(|layout| layout.chart.offset(window.x, window.y))
    }

    /// Keeps the window under the cursor while the core says the press is a drag.
    pub fn follow(&self, drag: DragPhase) {
        let Some(press) = self.press.filter(|_| drag == DragPhase::Dragging) else { return };
        let cursor = win::cursor();
        let x = press.origin.x as i32 + cursor.x - press.cursor.x;
        let y = press.origin.y as i32 + cursor.y - press.cursor.y;
        // SAFETY: moves this thread's own window.
        let _ = unsafe { SetWindowPos(self.hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    pub fn draw(&mut self, app: &App, screens: &Screens) -> Result<()> {
        let scale = win::scale(self.hwnd);
        let visible = app.overlay().is_visible();
        if self.resize.as_ref().is_some_and(|resize| {
            resize.original != *app.settings() || resize.scale != scale || !visible || app.expanded().is_none()
        }) {
            self.resize = None;
            // SAFETY: an external edit or hiding the window cancels its capture.
            let _ = unsafe { ReleaseCapture() };
        }
        // Becoming visible changes the core's overlay state, which renders again, so the first shown frame is fresh.
        if !visible {
            self.travel.reset();
            self.grab = None;
            self.show(false);
            return Ok(());
        }
        let settings = self.resize.as_ref().map_or(app.settings(), |resize| &resize.preview);
        let text = self.renderer.ticker_text(&app.display(), settings, scale)?;
        let ticker_frame = placement::ticker_frame(settings.position, text.size, screens);
        let expanded = match app.expanded() {
            Some((timeframe, load)) => {
                let segment = self.renderer.segment_size(settings, scale, text.size.height)?;
                let chart = Size { width: settings.chart_width * scale, height: settings.chart_height * scale };
                let work =
                    placement::screen_for(&ticker_frame, screens).map_or(screens.primary.work, |screen| screen.work);
                Some((ExpandedLayout::new(ticker_frame, segment, chart, work), timeframe, load.notice(timeframe)))
            }
            None => None,
        };
        let max_bars = expanded.as_ref().map_or(0, |(layout, ..)| layout.max_bars(scale));
        let chart = app.chart(max_bars);
        let (frame, ticker) = match &expanded {
            Some((layout, ..)) => (layout.frame, layout.ticker),
            None => (ticker_frame, Rect::at(Point::default(), ticker_frame.size())),
        };
        let origin = match self.press {
            Some(_) if app.overlay().drag() == DragPhase::Dragging => win::window_rect(self.hwnd).origin(),
            _ => frame.origin(),
        };
        let panel = match &expanded {
            Some((layout, timeframe, notice)) => {
                Panel::Expanded { layout, timeframe: *timeframe, chart: chart.as_ref(), notice: notice.clone() }
            }
            None => Panel::Collapsed,
        };
        let origin = POINT { x: origin.x as i32, y: origin.y as i32 };
        self.renderer
            .present(self.hwnd, &Frame { origin, size: frame.size(), ticker, text: &text, panel, settings, scale })?;
        let layout = expanded.map(|(layout, ..)| layout);
        let resize = layout.filter(|_| !settings.locked).map(|layout| crate::render::resize_grip(&layout, scale));
        self.drawn = Drawn { ticker, layout, max_bars, resize };
        if self.drawn.layout.is_none() {
            self.travel.reset();
        }
        self.show(true);
        match (self.chart_on_screen(), &self.grab) {
            (Some(chart), Some(grab)) => grab.retarget(chart),
            (None, Some(_)) => self.grab = None,
            _ => {}
        }
        Ok(())
    }

    /// Puts the overlay back on top of a game window that just activated, which may itself be topmost.
    pub fn raise(&self) {
        // SAFETY: reorders this thread's own window without activating it.
        let _ = unsafe {
            SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
        };
    }

    /// Hit targets on screen: "ticker", and each timeframe's label and the "chart" while expanded.
    pub fn targets(&self) -> BTreeMap<String, Rect> {
        let origin = win::window_rect(self.hwnd).origin();
        let on_screen = |rect: Rect| rect.offset(origin.x, origin.y);
        let mut targets = BTreeMap::from([("ticker".to_string(), on_screen(self.drawn.ticker))]);
        if let Some(layout) = &self.drawn.layout {
            for timeframe in Timeframe::ALL {
                targets.insert(timeframe.spec().label.to_string(), on_screen(layout.timeframe_rect(timeframe)));
            }
            targets.insert("chart".to_string(), on_screen(layout.chart));
        }
        if let Some(grip) = self.drawn.resize {
            targets.insert("resize".to_string(), on_screen(grip));
        }
        targets
    }

    /// The bar budget of the chart last drawn, 0 while collapsed.
    pub fn max_bars(&self) -> usize {
        self.drawn.max_bars
    }

    fn show(&mut self, visible: bool) {
        // SAFETY: shows or hides this thread's own window without activating it.
        unsafe {
            match (visible, IsWindowVisible(self.hwnd).as_bool()) {
                (true, false) => {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    self.raise();
                }
                (false, true) => {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                    if self.press.take().is_some() {
                        let _ = ReleaseCapture();
                    }
                }
                _ => {}
            }
        }
    }

    fn target_at(&self, cursor: POINT) -> PressTarget {
        let window = win::window_rect(self.hwnd);
        let point = Point { x: f64::from(cursor.x) - window.x, y: f64::from(cursor.y) - window.y };
        if !Rect::at(Point::default(), window.size()).contains(point) {
            return PressTarget::Outside;
        }
        match self.drawn.layout.as_ref().and_then(|layout| layout.timeframe_at(point)) {
            Some(timeframe) => PressTarget::Timeframe(timeframe),
            None => PressTarget::Body,
        }
    }

    /// Where the ticker line lands if this release ends a drag.
    fn dropped_at(&self) -> ticker_core::ScreenPoint {
        let window = win::window_rect(self.hwnd);
        let ticker = self.drawn.ticker.offset(window.x, window.y);
        placement::dropped(&ticker, &window, &win::screens())
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: reads the extra info of the message being handled.
    let extra = || unsafe { GetMessageExtraInfo() }.0;
    let pointer = match message {
        // Any other answer, the default included, makes Windows report the overlay as foreground for a moment,
        // which a "show only while WoW is frontmost" rule would act on.
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => Some(Pointer::Down { extra: extra() }),
        WM_MOUSEMOVE => Some(Pointer::Move),
        WM_LBUTTONUP => Some(Pointer::Up),
        WM_CAPTURECHANGED => Some(Pointer::Lost),
        WM_RBUTTONDOWN => Some(Pointer::RightDown { extra: extra() }),
        WM_RBUTTONUP => Some(Pointer::RightUp { at: win::cursor() }),
        WM_MOUSEWHEEL => Some(Pointer::Wheel {
            delta: wheel::delta(wparam.0),
            at: POINT { x: i32::from(lparam.0 as i16), y: i32::from((lparam.0 >> 16) as i16) },
        }),
        WM_MOUSELEAVE => Some(Pointer::Left),
        WM_SETCURSOR => {
            deliver(Event::Pointer(Pointer::Cursor));
            return LRESULT(1);
        }
        WM_DPICHANGED => {
            deliver(Event::Redraw);
            return LRESULT(0);
        }
        WM_CLOSE => {
            deliver(Event::Command(Command::Quit));
            return LRESULT(0);
        }
        _ => None,
    };
    match pointer {
        Some(pointer) => {
            deliver(Event::Pointer(pointer));
            LRESULT(0)
        }
        // SAFETY: default handling.
        None => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_from_each_corner_scales_and_obeys_limits() {
        for scale in [1.0, 1.5, 2.0] {
            for (x, y) in [(100.0, 100.0), (1600.0, 100.0), (100.0, 900.0), (1600.0, 900.0)] {
                let settings = Settings::default();
                let layout = ExpandedLayout::new(
                    Rect::new(x * scale, y * scale, 180.0 * scale, 28.0 * scale),
                    Size { width: 30.0 * scale, height: 24.0 * scale },
                    Size { width: settings.chart_width * scale, height: settings.chart_height * scale },
                    Rect::new(0.0, 0.0, 1920.0 * scale, 1080.0 * scale),
                );
                let mut resize = Resize {
                    cursor: POINT::default(),
                    original: settings,
                    preview: settings,
                    layout,
                    scale,
                    maximum: Size { width: 800.0, height: 500.0 },
                };
                let dx = if layout.opens_right { 1 } else { -1 };
                let dy = if layout.opens_down { 1 } else { -1 };
                resize.moved(POINT { x: dx * (40.0 * scale) as i32, y: dy * (30.0 * scale) as i32 });
                assert_eq!((resize.preview.chart_width, resize.preview.chart_height), (400.0, 210.0));
                resize.moved(POINT { x: dx * 10000, y: dy * 10000 });
                assert_eq!((resize.preview.chart_width, resize.preview.chart_height), (800.0, 500.0));
                resize.moved(POINT { x: -dx * 10000, y: -dy * 10000 });
                assert_eq!((resize.preview.chart_width, resize.preview.chart_height), (240.0, 120.0));
                let grip = crate::render::resize_grip(&layout, scale);
                assert_eq!(grip.mid_x() > layout.frame.width / 2.0, layout.opens_right);
                assert_eq!(grip.mid_y() > layout.frame.height / 2.0, layout.opens_down);
            }
        }
    }
}
