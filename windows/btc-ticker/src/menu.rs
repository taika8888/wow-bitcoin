//! The overlay's right-click menu. A classic `TrackPopupMenu` needs the foreground, which would take it from the
//! game. This one is a `WS_EX_NOACTIVATE` popup that a low-level mouse hook closes on any click outside it; measured
//! on this PC, the game stays foreground throughout and an outside click still reaches it.

use std::cell::Cell;

use ticker_core::{Rect, Screens, placement};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_MENU, COLOR_MENUTEXT, CreateFontIndirectW, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect, GetSysColor, GetSysColorBrush, HFONT,
    InvalidateRect, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
    GetWindowRect, HHOOK, MA_NOACTIVATE, MSLLHOOKSTRUCT, NONCLIENTMETRICSW, PostMessageW, SPI_GETNONCLIENTMETRICS,
    SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowLongPtrW, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_CLOSE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONDOWN, WS_BORDER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{Result, w};

use crate::shell::{Event, deliver};
use crate::win;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Settings,
    ToggleLock,
    Quit,
}

/// Both menus, the overlay's and the tray icon's, list these.
pub fn items(locked: bool) -> [(Command, &'static str); 3] {
    [
        (Command::Settings, "Settings…"),
        (Command::ToggleLock, if locked { "Unlock Position" } else { "Lock Position" }),
        (Command::Quit, "Quit BTC Ticker"),
    ]
}

pub const CLASS: windows::core::PCWSTR = w!("BTCTickerMenu");
const ROW_HEIGHT: f64 = 26.0;
const WIDTH: f64 = 180.0;
const TEXT_INSET: i32 = 12;

thread_local! {
    static OPEN: Cell<isize> = const { Cell::new(0) };
}

/// An open menu: closing it means dropping it. Inside a 1 px border, its rows are equally tall, in `items` order.
pub struct Popup {
    hwnd: HWND,
    hook: HHOOK,
}

struct Rows {
    items: [(Command, &'static str); 3],
    height: i32,
    font: HFONT,
    hovered: Cell<Option<usize>>,
}

impl Popup {
    pub fn open(owner: HWND, at: POINT, locked: bool, screens: &Screens) -> Result<Popup> {
        win::register_class(CLASS, Some(procedure), None);
        let scale = win::scale(owner);
        let row = (ROW_HEIGHT * scale).round() as i32;
        let size = ((WIDTH * scale).round(), f64::from(row * 3 + 2));
        let wanted = Rect::new(f64::from(at.x), f64::from(at.y), size.0, size.1);
        let area = placement::screen_for(&wanted, screens).map_or(screens.primary.work, |screen| screen.work);
        let spot = placement::clamp(&wanted, &area);
        // SAFETY: creates a window this Popup owns; its Rows box is freed in WM_NCDESTROY.
        unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                CLASS,
                w!(""),
                WS_POPUP | WS_BORDER,
                spot.x as i32,
                spot.y as i32,
                size.0 as i32,
                size.1 as i32,
                Some(owner),
                None,
                None,
                None,
            )?;
            let rows =
                Box::new(Rows { items: items(locked), height: row, font: menu_font(hwnd), hovered: Cell::new(None) });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(rows) as isize);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            OPEN.set(hwnd.0 as isize);
            let hook = match SetWindowsHookExW(WH_MOUSE_LL, Some(outside_click), None, 0) {
                Ok(hook) => hook,
                Err(error) => {
                    let _ = DestroyWindow(hwnd);
                    return Err(error);
                }
            };
            Ok(Popup { hwnd, hook })
        }
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        OPEN.set(0);
        // SAFETY: undoes `open`.
        unsafe {
            let _ = UnhookWindowsHookEx(self.hook);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn outside_click(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let button = matches!(wparam.0 as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN);
    let open = HWND(OPEN.get() as *mut _);
    if code >= 0 && button && !open.is_invalid() {
        // SAFETY: for WH_MOUSE_LL, lparam points at an MSLLHOOKSTRUCT for the duration of the call.
        unsafe {
            let at = (*(lparam.0 as *const MSLLHOOKSTRUCT)).pt;
            let mut rect = RECT::default();
            let _ = GetWindowRect(open, &mut rect);
            let inside = (rect.left..rect.right).contains(&at.x) && (rect.top..rect.bottom).contains(&at.y);
            if !inside {
                let _ = PostMessageW(Some(open), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA holds the Rows box from `open` until WM_NCDESTROY frees it.
    let rows = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Rows).as_ref() };
    let row_at = |lparam: LPARAM| {
        let y = i32::from((lparam.0 >> 16) as i16);
        rows.and_then(|rows| usize::try_from(y / rows.height).ok()).filter(|row| *row < 3)
    };
    match (message, rows) {
        (WM_MOUSEACTIVATE, _) => return LRESULT(MA_NOACTIVATE as isize),
        (WM_MOUSEMOVE, Some(rows)) => {
            if rows.hovered.replace(row_at(lparam)) != row_at(lparam) {
                // SAFETY: plain calls on this window.
                unsafe {
                    let _ = InvalidateRect(Some(hwnd), None, true);
                    let mut leave = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        ..Default::default()
                    };
                    let _ = TrackMouseEvent(&mut leave);
                }
            }
            return LRESULT(0);
        }
        (WM_MOUSELEAVE, Some(rows)) => {
            rows.hovered.set(None);
            // SAFETY: plain call on this window.
            let _ = unsafe { InvalidateRect(Some(hwnd), None, true) };
            return LRESULT(0);
        }
        (WM_LBUTTONUP, Some(rows)) => {
            if let Some(row) = row_at(lparam) {
                deliver(Event::MenuClosed(Some(rows.items[row].0)));
            }
            return LRESULT(0);
        }
        (WM_CLOSE, _) => {
            deliver(Event::MenuClosed(None));
            return LRESULT(0);
        }
        (WM_PAINT, Some(rows)) => {
            // SAFETY: standard paint cycle on this window's DC.
            unsafe { paint(hwnd, rows) };
            return LRESULT(0);
        }
        (WM_NCDESTROY, Some(rows)) => {
            // SAFETY: reclaims the box leaked in `open`, once.
            unsafe {
                let _ = DeleteObject(rows.font.into());
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_ref(rows).cast_mut()));
            }
        }
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

unsafe fn paint(hwnd: HWND, rows: &Rows) {
    // SAFETY: called from WM_PAINT for this window.
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        FillRect(dc, &client, GetSysColorBrush(COLOR_MENU));
        SetBkMode(dc, TRANSPARENT);
        let previous = SelectObject(dc, rows.font.into());
        for (index, (_, label)) in rows.items.iter().enumerate() {
            let top = index as i32 * rows.height;
            let mut rect = RECT { left: 0, top, right: client.right, bottom: top + rows.height };
            let hovered = rows.hovered.get() == Some(index);
            if hovered {
                FillRect(dc, &rect, GetSysColorBrush(COLOR_HIGHLIGHT));
            }
            SetTextColor(dc, COLORREF(GetSysColor(if hovered { COLOR_HIGHLIGHTTEXT } else { COLOR_MENUTEXT })));
            rect.left += TEXT_INSET * rows.height / ROW_HEIGHT as i32;
            let mut text: Vec<u16> = label.encode_utf16().collect();
            DrawTextW(dc, &mut text, &mut rect, DT_SINGLELINE | DT_VCENTER | DT_LEFT);
        }
        SelectObject(dc, previous);
        let _ = EndPaint(hwnd, &paint);
    }
}

fn menu_font(hwnd: HWND) -> HFONT {
    let mut metrics = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    // SAFETY: fills one struct of the size given.
    unsafe {
        let _ = SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some((&raw mut metrics).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0).0,
            GetDpiForWindow(hwnd),
        );
        CreateFontIndirectW(&metrics.lfMenuFont)
    }
}
