use std::cell::{Cell, RefCell};
use ticker_core::{Frame, Input, Point, PointerInput, SYNTHETIC_INPUT_TAG};
use ticker_paint::Bgra;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, GetDC, HBITMAP, HDC,
    HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetMessageExtraInfo, GetMessagePos, HTCLIENT,
    HWND_TOPMOST, IDC_ARROW, LoadCursorW, MA_NOACTIVATE, RegisterClassW, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WINDOW_EX_STYLE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_DPICHANGED,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST, WM_RBUTTONUP,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

/// LAYERED: per-pixel alpha. TOPMOST: above a borderless game. NOACTIVATE: a click never takes focus from the game.
/// TOOLWINDOW: no taskbar button, not in Alt-Tab. Never TRANSPARENT, which would let every click through.
pub(super) const EX_STYLE: WINDOW_EX_STYLE =
    WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0);

/// A memory DC holding a top-down 32-bit DIB section of `size`, kept until the frame size changes.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u8,
    size: SIZE,
}

thread_local! {
    static HWND_CELL: Cell<HWND> = Cell::new(HWND::default());
    static SHOWN: Cell<bool> = const { Cell::new(false) };
    /// TrackMouseEvent(TME_LEAVE) is one-shot; re-armed on the first move after each leave.
    static TRACKING_LEAVE: Cell<bool> = const { Cell::new(false) };
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
}

pub(super) fn create() -> windows::core::Result<HWND> {
    // SAFETY: registers a class whose static strings and window procedure live for the whole process, then
    // creates a window of it on this thread.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("BTCTickerOverlay"),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            EX_STYLE,
            w!("BTCTickerOverlay"),
            w!("BTC Ticker"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    HWND_CELL.set(hwnd);
    Ok(hwnd)
}

fn hwnd() -> HWND {
    HWND_CELL.get()
}

/// Hidden frames hide the window. Otherwise position, size and pixels change in one UpdateLayeredWindow call, so a
/// drag or an expand never shows a half-updated window, and the first frame shows without activating.
pub(super) fn present(frame: &Frame, pixels: &Bgra) {
    let hwnd = hwnd();
    if !frame.visible {
        if SHOWN.replace(false) {
            // SAFETY: our live window, owned by this thread.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        return;
    }
    let size = SIZE {
        cx: pixels.width as i32,
        cy: pixels.height as i32,
    };
    let at = POINT {
        x: frame.window.x.round() as i32,
        y: frame.window.y.round() as i32,
    };
    let updated = SURFACE.with(|surface| {
        let mut surface = surface.borrow_mut();
        if surface.as_ref().is_none_or(|s| s.size != size) {
            if let Some(old) = surface.take() {
                old.release();
            }
            *surface = Surface::new(size);
        }
        let Some(surface) = surface.as_ref() else {
            return false;
        };
        // SAFETY: `bits` points at the DIB's width * height * 4 bytes, the exact length of `pixels.data`.
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.data.as_ptr(), surface.bits, pixels.data.len())
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: the screen DC is released below; every pointer outlives the call; `surface.dc` holds a DIB of
        // `size` with premultiplied BGRA from ticker-paint.
        unsafe {
            let screen = GetDC(None);
            let result = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&at),
                Some(&size),
                Some(surface.dc),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
            if let Err(error) = &result {
                eprintln!("UpdateLayeredWindow failed: {error}");
            }
            result.is_ok()
        }
    });
    if updated && !SHOWN.replace(true) {
        // SAFETY: our live window. SW_SHOWNOACTIVATE, never SW_SHOW, which activates.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    }
    raise();
}

/// Back to the top of the topmost band without activating. After every present, on every confirmed foreground
/// change, and on the host's watch timer: a topmost rival can rise above us without any message reaching us.
pub(super) fn raise() {
    if !SHOWN.get() {
        return;
    }
    // SAFETY: our live window.
    let _ = unsafe {
        SetWindowPos(
            hwnd(),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
}

pub(super) fn capture(take: bool) {
    // SAFETY: our live window; releasing capture we may not hold is harmless.
    unsafe {
        if take {
            SetCapture(hwnd());
        } else {
            let _ = ReleaseCapture();
        }
    }
}

impl Surface {
    fn new(size: SIZE) -> Option<Surface> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size.cx,
                biHeight: -size.cy,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: `info` and `bits` are valid for the calls; the DC and bitmap are owned by the returned Surface.
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .inspect_err(|error| eprintln!("CreateDIBSection failed: {error}"))
                .ok()?;
            let previous = SelectObject(dc, bitmap.into());
            Some(Surface {
                dc,
                bitmap,
                previous,
                bits: bits.cast(),
                size,
            })
        }
    }

    fn release(self) {
        // SAFETY: undoes `new` in reverse order; nothing else holds these handles.
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = windows::Win32::Graphics::Gdi::DeleteDC(self.dc);
        }
    }
}

/// Screen position of the message being handled, in physical px. Taken from the message rather than from client
/// coordinates, so a window that moved since the event was queued does not skew a drag.
fn message_point() -> Point {
    // SAFETY: no arguments; reads this thread's current message state.
    let packed = unsafe { GetMessagePos() };
    Point {
        x: f64::from(packed as u16 as i16),
        y: f64::from((packed >> 16) as u16 as i16),
    }
}

fn cursor_point() -> Point {
    let mut at = POINT::default();
    // SAFETY: `at` is valid for the call.
    let _ = unsafe { GetCursorPos(&mut at) };
    Point {
        x: f64::from(at.x),
        y: f64::from(at.y),
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pointer = |input: PointerInput| {
        super::dispatch(Input::Pointer(input));
        LRESULT(0)
    };
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // Alpha-0 pixels never reach this message, which is why ticker-paint gives every pixel HIT_FLOOR_ALPHA.
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_LBUTTONDOWN => {
            // SAFETY: reads the extra info of the message being handled.
            let tagged = unsafe { GetMessageExtraInfo() }.0 as usize == SYNTHETIC_INPUT_TAG;
            super::update_facts(|facts| {
                facts.presses += 1;
                if !tagged {
                    facts.foreign_presses += 1;
                }
            });
            pointer(PointerInput::LeftDown(message_point()))
        }
        WM_MOUSEMOVE => {
            if !TRACKING_LEAVE.replace(true) {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: `track` is valid for the call and names our own window.
                let _ = unsafe { TrackMouseEvent(&mut track) };
            }
            pointer(PointerInput::Move(message_point()))
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.set(false);
            pointer(PointerInput::Left)
        }
        WM_LBUTTONUP => pointer(PointerInput::LeftUp(message_point())),
        WM_RBUTTONUP => pointer(PointerInput::RightUp(message_point())),
        // `App` ignores it when no press is under way, which covers our own ReleaseCapture.
        WM_CAPTURECHANGED if lparam.0 != hwnd.0 as isize => {
            pointer(PointerInput::CaptureLost(cursor_point()))
        }
        WM_ACTIVATE => {
            super::count_activation(wparam);
            // SAFETY: forwards this window's own message unchanged.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // `App` re-lays out against the new scale and clamps, so the suggested rect is not used.
        WM_DPICHANGED => {
            super::display_changed();
            LRESULT(0)
        }
        // SAFETY: forwards this window's own message unchanged.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
