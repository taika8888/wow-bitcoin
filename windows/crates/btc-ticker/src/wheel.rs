//! Wheel, touchpad and pinch input over the expanded chart. With "Scroll inactive windows when I hover over them" on,
//! the Windows default, `WM_MOUSEWHEEL` goes to the window under the cursor, so the overlay gets it while the game
//! keeps the foreground. With it off, the wheel goes to the game; then a low-level mouse hook, installed only while the
//! cursor is over the chart, takes the wheel there before the game sees it.

use std::cell::Cell;

use ticker_core::{Point, Rect, ZoomStep};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, HHOOK, MOUSEWHEEL_ROUTING_MOUSE_POS, MSLLHOOKSTRUCT, PostMessageW,
    SPI_GETMOUSEWHEELROUTING, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowsHookExW, SystemParametersInfoW,
    UnhookWindowsHookEx, WH_MOUSE_LL, WHEEL_DELTA, WM_MOUSEWHEEL,
};

/// Wheel travel toward the next zoom step. A notched wheel moves `WHEEL_DELTA` a notch, one step. A precision touchpad
/// scrolls, and pinches as Ctrl+wheel, in smaller deltas that add up to a step. Turning the other way starts over.
#[derive(Default)]
pub struct Travel(i32);

impl Travel {
    /// Forward, away from the user, zooms in, as scrolling up does on macOS.
    pub fn add(&mut self, delta: i32) -> Vec<ZoomStep> {
        if self.0.signum() == -delta.signum() {
            self.0 = 0;
        }
        self.0 += delta;
        let steps = self.0 / WHEEL_DELTA as i32;
        self.0 -= steps * WHEEL_DELTA as i32;
        let step = if steps > 0 { ZoomStep::In } else { ZoomStep::Out };
        vec![step; steps.unsigned_abs() as usize]
    }

    pub fn reset(&mut self) {
        self.0 = 0;
    }
}

/// The signed wheel delta in the high word of a `WM_MOUSEWHEEL` wParam or an `MSLLHOOKSTRUCT::mouseData`.
pub fn delta(high_word: usize) -> i32 {
    i32::from((high_word >> 16) as u16 as i16)
}

/// Whether Windows sends the wheel to the window under the cursor. Assumed on when the setting cannot be read.
pub fn follows_cursor() -> bool {
    let mut routing = MOUSEWHEEL_ROUTING_MOUSE_POS;
    // SAFETY: writes one u32.
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETMOUSEWHEELROUTING,
            0,
            Some((&raw mut routing).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    routing == MOUSEWHEEL_ROUTING_MOUSE_POS
}

thread_local! {
    static GRABBED: Cell<Option<(Rect, HWND)>> = const { Cell::new(None) };
}

/// While it lives, wheel input over `chart` (screen pixels) goes to `overlay` and not to the focused window.
pub struct Grab(HHOOK);

impl Grab {
    pub fn new(chart: Rect, overlay: HWND) -> Option<Grab> {
        GRABBED.set(Some((chart, overlay)));
        // SAFETY: the hook procedure is a plain function that lives for the whole process; the hook runs on this
        // thread's message loop, which `GRABBED` belongs to.
        let hook = unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            SetWindowsHookExW(WH_MOUSE_LL, Some(procedure), Some(instance.into()), 0).ok()?
        };
        Some(Grab(hook))
    }

    pub fn retarget(&self, chart: Rect) {
        GRABBED.set(GRABBED.get().map(|(_, overlay)| (chart, overlay)));
    }
}

impl Drop for Grab {
    fn drop(&mut self) {
        GRABBED.set(None);
        // SAFETY: removes the hook `new` installed.
        let _ = unsafe { UnhookWindowsHookEx(self.0) };
    }
}

/// Runs for every mouse event on the system while a `Grab` lives. Windows drops a hook that overruns
/// `LowLevelHooksTimeout`, after which the game would get the wheel, so this posts the wheel to the overlay and returns.
unsafe extern "system" fn procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 == WM_MOUSEWHEEL as usize {
        // SAFETY: for WM_MOUSEWHEEL, lparam points at the event's MSLLHOOKSTRUCT for the duration of the call.
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let at = Point { x: f64::from(info.pt.x), y: f64::from(info.pt.y) };
        if let Some((_, overlay)) = GRABBED.get().filter(|(chart, _)| chart.contains(at)) {
            let wheel = WPARAM(info.mouseData as usize & 0xFFFF_0000);
            let at = LPARAM(((info.pt.y as u16 as isize) << 16) | info.pt.x as u16 as isize);
            // SAFETY: posts to this thread's own window.
            let _ = unsafe { PostMessageW(Some(overlay), WM_MOUSEWHEEL, wheel, at) };
            return LRESULT(1);
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
