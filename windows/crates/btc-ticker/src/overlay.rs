//! The overlay window: a layered, topmost popup that never activates. It turns raw mouse messages into the core's
//! overlay events and draws whatever the core's state says.

use std::collections::BTreeMap;

use ticker_core::{
    App, DragPhase, ExpandedLayout, OverlayEvent, Point, PressTarget, Rect, Screens, Size, Timeframe, ZoomStep,
    placement,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageExtraInfo, HWND_TOPMOST, IsWindowVisible, MA_NOACTIVATE, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, ShowWindow,
    WM_CAPTURECHANGED, WM_CLOSE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
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
}

pub struct Overlay {
    hwnd: HWND,
    renderer: Renderer,
    drawn: Drawn,
    press: Option<Press>,
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

/// What the last frame put where, in panel coordinates, for hit testing and the snapshot.
#[derive(Default)]
struct Drawn {
    ticker: Rect,
    layout: Option<ExpandedLayout>,
    max_bars: usize,
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
    pub fn pointer(&mut self, pointer: Pointer) -> Option<OverlayEvent> {
        match pointer {
            Pointer::Down { .. } => {
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
            Pointer::RightDown { .. } | Pointer::RightUp { .. } | Pointer::Wheel { .. } => None,
        }
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

    /// Over the chart while Windows sends the wheel to the focused window, grab it until the cursor leaves.
    fn hover(&mut self) {
        let cursor = win::cursor();
        let cursor = Point { x: f64::from(cursor.x), y: f64::from(cursor.y) };
        let chart = self.chart_on_screen().filter(|chart| chart.contains(cursor) && !wheel::follows_cursor());
        match (chart, &self.grab) {
            (None, _) => self.grab = None,
            (Some(chart), Some(grab)) => grab.retarget(chart),
            (Some(chart), None) => {
                self.grab = wheel::Grab::new(chart);
                let mut leave = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: self.hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: asks for one WM_MOUSELEAVE to this thread's own window.
                let _ = unsafe { TrackMouseEvent(&mut leave) };
            }
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
        let settings = app.settings();
        let scale = win::scale(self.hwnd);
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
        self.drawn = Drawn { ticker, layout: expanded.map(|(layout, ..)| layout), max_bars };
        self.show(app.overlay().is_visible());
        match (self.chart_on_screen().filter(|_| app.overlay().is_visible()), &self.grab) {
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
