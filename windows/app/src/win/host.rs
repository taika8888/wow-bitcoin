//! The hidden host window: the tick and watch timers, worker replies, store notifications, power and display
//! broadcasts, and the notification-area icon with its native menu. A real hidden top-level window rather than a
//! message-only one, because message-only windows miss broadcasts such as TaskbarCreated, after which the icon
//! would be gone for good once Explorer restarts.
use std::cell::Cell;
use ticker_core::{Draw, Frame, Input, Measure, MenuItem, Point, Rect, Rgba, TextStyle};
use ticker_paint::Fonts;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
    NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    GetSystemMetrics, HICON, ICONINFO, KillTimer, MF_SEPARATOR, MF_STRING, PBT_APMRESUMEAUTOMATIC,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON,
    SPI_SETWORKAREA, SetForegroundWindow, SetTimer, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenuEx, WINDOW_EX_STYLE, WM_ACTIVATE, WM_APP, WM_CONTEXTMENU, WM_DISPLAYCHANGE,
    WM_ENDSESSION, WM_NULL, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMECHANGE, WM_TIMER,
    WNDCLASSW, WS_POPUP,
};
use windows::core::{HSTRING, w};

pub(super) const WM_APP_REPLY: u32 = WM_APP + 1;
pub(super) const WM_APP_STORE: u32 = WM_APP + 2;
const WM_APP_TRAY: u32 = WM_APP + 3;

const TICK_TIMER: usize = 1;
/// Re-asserts topmost while visible and re-checks the foreground window.
const WATCH_TIMER: usize = 2;
const WATCH_MS: u32 = 2_000;
const TRAY_ID: u32 = 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

thread_local! {
    static HOST: Cell<HWND> = Cell::new(HWND::default());
    static ICON: Cell<HICON> = Cell::new(HICON::default());
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

pub(super) fn create(fonts: &Fonts) -> windows::core::Result<HWND> {
    // SAFETY: registers a class whose static strings and window procedure live for the whole process, then
    // creates a window of it on this thread; it is never shown.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: w!("BTCTickerHost"),
            ..Default::default()
        };
        RegisterClassW(&class);
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BTCTickerHost"),
            w!("BTC Ticker"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    HOST.set(hwnd);
    ICON.set(icon(fonts).unwrap_or_default());
    Ok(hwnd)
}

pub(super) fn icon_handle() -> HICON {
    ICON.get()
}

fn tray_data() -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: HOST.get(),
        uID: TRAY_ID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: ICON.get(),
        ..Default::default()
    };
    for (slot, unit) in data.szTip.iter_mut().zip("BTC Ticker".encode_utf16()) {
        *slot = unit;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    data
}

pub(super) fn add_tray_icon() -> bool {
    let data = tray_data();
    // SAFETY: `data` is sized and valid for both calls.
    unsafe {
        let added = Shell_NotifyIconW(NIM_ADD, &data).as_bool();
        if added {
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
        added
    }
}

pub(super) fn remove_tray_icon() {
    // SAFETY: as above.
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &tray_data()) };
}

/// One-shot: the next `Input::Tick` after `seconds`. Re-arming replaces the pending one.
pub(super) fn arm_tick(seconds: f64) {
    let ms = (seconds * 1000.0).ceil().clamp(1.0, f64::from(u32::MAX)) as u32;
    // SAFETY: our own window and timer id.
    unsafe { SetTimer(Some(HOST.get()), TICK_TIMER, ms, None) };
}

pub(super) fn start_watch_timer() {
    // SAFETY: our own window and timer id.
    unsafe { SetTimer(Some(HOST.get()), WATCH_TIMER, WATCH_MS, None) };
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == TICK_TIMER => {
            // SAFETY: our own window and timer id.
            let _ = unsafe { KillTimer(Some(hwnd), TICK_TIMER) };
            super::dispatch(Input::Tick);
        }
        WM_TIMER if wparam.0 == WATCH_TIMER => {
            super::foreground::recheck();
            super::overlay::raise();
        }
        // SAFETY: WM_APP_REPLY is posted only by `net::post`, once per boxed reply.
        WM_APP_REPLY => super::dispatch(unsafe { super::net::take(lparam) }),
        WM_APP_STORE => {
            if let Some(json) = super::with_shell(|shell| shell.store.load().unwrap_or_default()) {
                super::dispatch(Input::SettingsStored(json));
            }
        }
        WM_APP_TRAY => match (lparam.0 & 0xFFFF) as u32 {
            WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT => {
                let x = (wparam.0 & 0xFFFF) as u16 as i16;
                let y = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
                tray_menu(hwnd, i32::from(x), i32::from(y));
            }
            _ => {}
        },
        WM_DISPLAYCHANGE | WM_TIMECHANGE => super::display_changed(),
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => super::display_changed(),
        WM_POWERBROADCAST if wparam.0 == PBT_APMRESUMEAUTOMATIC as usize => {
            super::dispatch(Input::Woke)
        }
        WM_ACTIVATE => {
            super::count_activation(wparam);
            // SAFETY: forwards this window's own message unchanged.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        WM_ENDSESSION if wparam.0 != 0 => {
            // SAFETY: no arguments.
            unsafe { PostQuitMessage(0) }
        }
        _ if msg == TASKBAR_CREATED.get() && msg != 0 => {
            let added = add_tray_icon();
            super::update_facts(|facts| facts.tray_icon = added);
            super::publish();
        }
        // SAFETY: forwards this window's own message unchanged.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    LRESULT(0)
}

/// The user clicked the taskbar, so the game has already lost focus and the documented dance is right here:
/// foreground the host, track the menu, then post WM_NULL so it closes when the user clicks away.
fn tray_menu(hwnd: HWND, x: i32, y: i32) {
    let Some(labels) = super::with_shell(|shell| {
        MenuItem::ALL.map(|item| (item, item.label(shell.app.settings())))
    }) else {
        return;
    };
    // SAFETY: the menu is created, tracked and destroyed here; every string outlives its AppendMenuW call.
    let chosen = unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        for (index, (item, label)) in labels.iter().enumerate() {
            if *item == MenuItem::Quit {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            }
            let _ = AppendMenuW(menu, MF_STRING, index + 1, &HSTRING::from(*label));
        }
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON).0,
            x,
            y,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        command.0 as usize
    };
    if let Some((item, _)) = chosen.checked_sub(1).and_then(|index| labels.get(index)) {
        super::dispatch(Input::Menu(*item));
    }
}

/// The Bitcoin mark on an orange disc, drawn by ticker-paint at the small-icon size, so the exe needs no resource
/// file.
fn icon(fonts: &Fonts) -> Option<HICON> {
    // SAFETY: no preconditions.
    let side = f64::from(unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16));
    let mark = if fonts.has_glyph('\u{20BF}') {
        "\u{20BF}"
    } else {
        "B"
    };
    let style = TextStyle { px: side * 0.8 };
    let whole = Rect {
        x: 0.0,
        y: 0.0,
        width: side,
        height: side,
    };
    let frame = Frame {
        visible: true,
        window: whole,
        scale: 1.0,
        ticker: whole,
        timeframes: Vec::new(),
        menu: Vec::new(),
        draw: vec![
            Draw::RoundRect {
                rect: whole,
                radius: side / 2.0,
                color: Rgba::opaque(0.97, 0.58, 0.10),
            },
            Draw::Text {
                origin: Point {
                    x: (side - fonts.advance(mark, style)) / 2.0,
                    y: (side - fonts.line_height(style)) / 2.0,
                },
                text: mark.into(),
                style,
                color: Rgba::opaque(1.0, 1.0, 1.0),
            },
        ],
        bars_drawn: 0,
    };
    let mut pixels = ticker_paint::paint(&frame, fonts);
    // Icons take straight alpha; ticker-paint writes premultiplied.
    for pixel in pixels.data.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            if let Some(straight) = (u32::from(*channel) * 255).checked_div(alpha) {
                *channel = straight.min(255) as u8;
            }
        }
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: pixels.width as i32,
            biHeight: -(pixels.height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: `bits` points at the DIB's width * height * 4 bytes, the length of `pixels.data`; both bitmaps are
    // copied by CreateIconIndirect and deleted here.
    unsafe {
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(pixels.data.as_ptr(), bits.cast(), pixels.data.len());
        let mask = CreateBitmap(pixels.width as i32, pixels.height as i32, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        });
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon.ok()
    }
}
