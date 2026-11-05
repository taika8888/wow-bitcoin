//! The Settings window: native controls built at runtime from `ticker_core::FORM`, one row per field, under the
//! same sections as the macOS window. A control's id encodes its row and part, so every notification maps straight
//! back to the field's setter; no setting is named here. Edits are live, with no Apply button, like macOS.
//!
//! This is the one window that activates. Opening it remembers the foreground window, normally the game, and
//! closing it hands the foreground back.
use std::cell::{Cell, RefCell};
use ticker_core::{FORM, Field, FieldKind, Input, Rgba, Settings};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, COLOR_WINDOWFRAME, CreateFontIndirectW, CreateSolidBrush, DeleteObject,
    DrawFocusRect, FillRect, FrameRect, GetMonitorInfoW, GetSysColorBrush, HBRUSH, HFONT,
    InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemServices::SS_CENTERIMAGE;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};
use windows::Win32::UI::Controls::{
    BST_CHECKED, BST_UNCHECKED, DRAWITEMSTRUCT, ODS_FOCUS, TBM_SETPAGESIZE, TBM_SETPOS,
    TBM_SETRANGEMAX, TBM_SETRANGEMIN, TBS_NOTICKS, UDM_GETPOS32, UDM_SETPOS32, UDM_SETRANGE32,
    UDS_ARROWKEYS, UDS_NOTHOUSANDS,
};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, SystemParametersInfoForDpi,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_OWNERDRAW,
    BS_PUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GW_CHILD,
    GetDlgCtrlID, GetDlgItem, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, HMENU,
    IDCANCEL, IsWindow, NONCLIENTMETRICSW, RegisterClassW, SPI_GETNONCLIENTMETRICS, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW,
    ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_DPICHANGED,
    WM_DRAWITEM, WM_HSCROLL, WM_SETFONT, WM_VSCROLL, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_GROUP,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, w};

/// commctrl.h defines it as WM_USER; windows-rs does not export it.
const TBM_GETPOS: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER;

const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0);

/// Row geometry in DIPs, multiplied by the window's DPI / 96.
const MARGIN: i32 = 14;
const LABEL_WIDTH: i32 = 120;
const CONTROL_WIDTH: i32 = 210;
const VALUE_WIDTH: i32 = 56;
const GAP: i32 = 8;
const ROW_HEIGHT: i32 = 30;
const CONTROL_HEIGHT: i32 = 24;
const SWATCH_WIDTH: i32 = 40;
const SPIN_WIDTH: i32 = 22;

/// Control id = ID_BASE + row * PARTS + part.
const ID_BASE: i32 = 1000;
const PARTS: i32 = 8;
const MAIN: i32 = 0;
const SECOND: i32 = 1;
const VALUE: i32 = 2;
const LABEL: i32 = 3;
const OPTION: i32 = 4;

struct Window {
    hwnd: HWND,
    return_to: Option<HWND>,
    shown: Settings,
    fonts: [HFONT; 2],
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static CUSTOM_COLORS: Cell<[COLORREF; 16]> = const { Cell::new([COLORREF(0x00FF_FFFF); 16]) };
}

pub(super) fn hwnd() -> Option<HWND> {
    WINDOW.with(|window| window.borrow().as_ref().map(|w| w.hwnd))
}

fn current_settings() -> Option<Settings> {
    super::with_shell(|shell| shell.app.settings().clone())
}

pub(super) fn open() {
    if let Some(hwnd) = hwnd() {
        // SAFETY: our live window.
        let _ = unsafe { SetForegroundWindow(hwnd) };
        return;
    }
    let Some(settings) = current_settings() else {
        return;
    };
    // SAFETY: plain queries.
    let return_to = unsafe {
        let foreground = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(foreground, Some(&mut pid));
        (!foreground.is_invalid() && pid != GetCurrentProcessId()).then_some(foreground)
    };
    // SAFETY: registers a class whose static strings and window procedure live for the whole process (a repeat
    // registration fails harmlessly), then creates the window hidden on this thread.
    let created = unsafe {
        GetModuleHandleW(None).and_then(|module| {
            let instance = module.into();
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hIcon: super::host::icon_handle(),
                hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut _),
                lpszClassName: w!("BTCTickerSettings"),
                ..Default::default()
            });
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BTCTickerSettings"),
                w!("BTC Ticker Settings"),
                STYLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
        })
    };
    let hwnd = match created {
        Ok(hwnd) => hwnd,
        Err(error) => {
            eprintln!("could not open Settings: {error}");
            return;
        }
    };
    WINDOW.with(|window| {
        *window.borrow_mut() = Some(Window {
            hwnd,
            return_to,
            shown: settings.clone(),
            fonts: [HFONT::default(); 2],
        })
    });
    build(hwnd, None);
    set_controls(hwnd, &settings);
    // SAFETY: our live window. The user just chose Settings, so this process received the last input and may
    // take the foreground.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Brings the controls up to date after any change, from any source. A no-op while closed or already current.
pub(super) fn sync(settings: &Settings) {
    let hwnd = WINDOW.with(|window| {
        let mut window = window.borrow_mut();
        let window = window.as_mut().filter(|w| w.shown != *settings)?;
        window.shown = settings.clone();
        Some(window.hwnd)
    });
    if let Some(hwnd) = hwnd {
        set_controls(hwnd, settings);
    }
}

fn id(row: usize, part: i32) -> i32 {
    ID_BASE + row as i32 * PARTS + part
}

/// Creates every control for the window's DPI and sizes the window around them: centered on its monitor's work
/// area when first built, at `origin` after a DPI change.
fn build(hwnd: HWND, origin: Option<(i32, i32)>) {
    // SAFETY: our live window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let px = |dips: i32| dips * dpi as i32 / 96;
    let fonts = create_fonts(dpi);
    WINDOW.with(|window| {
        if let Some(window) = window.borrow_mut().as_mut() {
            for font in std::mem::replace(&mut window.fonts, fonts) {
                // SAFETY: a font this window created and no control uses any more.
                let _ = unsafe { DeleteObject(font.into()) };
            }
        }
    });
    let [normal, bold] = fonts;
    let label_x = px(MARGIN);
    let control_x = label_x + px(LABEL_WIDTH + GAP);
    let value_x = control_x + px(CONTROL_WIDTH + GAP);
    let full_width = px(LABEL_WIDTH + GAP + CONTROL_WIDTH + GAP + VALUE_WIDTH);
    let mut y = px(MARGIN) / 2;
    let mut section = "";
    let child = |text: &str,
                 class: PCWSTR,
                 style: u32,
                 id: i32,
                 x: i32,
                 y: i32,
                 width: i32,
                 font: HFONT| {
        // SAFETY: creates a child of our live window; the text outlives the call.
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                &HSTRING::from(text),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
                x,
                y + (px(ROW_HEIGHT) - px(CONTROL_HEIGHT)) / 2,
                width,
                px(CONTROL_HEIGHT),
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                None,
                None,
            )
        };
        if let Ok(control) = created {
            // SAFETY: a live control and a live font.
            unsafe {
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(1)),
                )
            };
        }
    };
    let label = |row: usize, text: &str, y: i32| {
        child(
            text,
            w!("STATIC"),
            SS_CENTERIMAGE.0,
            id(row, LABEL),
            label_x,
            y,
            px(LABEL_WIDTH),
            normal,
        )
    };
    let tab = WS_TABSTOP.0;
    for (row, field) in FORM.iter().enumerate() {
        if field.section != section {
            section = field.section;
            y += px(GAP) / 2;
            child(
                section,
                w!("STATIC"),
                SS_CENTERIMAGE.0,
                0,
                label_x,
                y,
                full_width,
                bold,
            );
            y += px(ROW_HEIGHT);
        }
        let Field {
            label: text, kind, ..
        } = field;
        match kind {
            FieldKind::Slider { unit, .. } => {
                label(row, text, y);
                child(
                    "",
                    w!("msctls_trackbar32"),
                    tab | TBS_NOTICKS,
                    id(row, MAIN),
                    control_x,
                    y,
                    px(CONTROL_WIDTH),
                    normal,
                );
                if !unit.is_empty() {
                    child(
                        "",
                        w!("STATIC"),
                        SS_CENTERIMAGE.0,
                        id(row, VALUE),
                        value_x,
                        y,
                        px(VALUE_WIDTH),
                        normal,
                    );
                }
            }
            FieldKind::Stepper { .. } => {
                label(row, text, y);
                child(
                    "",
                    w!("STATIC"),
                    SS_CENTERIMAGE.0,
                    id(row, MAIN),
                    control_x,
                    y,
                    px(CONTROL_WIDTH - SPIN_WIDTH - GAP),
                    normal,
                );
                child(
                    "",
                    w!("msctls_updown32"),
                    tab | UDS_ARROWKEYS | UDS_NOTHOUSANDS,
                    id(row, SECOND),
                    control_x + px(CONTROL_WIDTH - SPIN_WIDTH),
                    y,
                    px(SPIN_WIDTH),
                    normal,
                );
            }
            FieldKind::Choice { options, .. } => {
                label(row, text, y);
                let width = px(CONTROL_WIDTH) / options.len().max(1) as i32;
                for (index, option) in options.iter().enumerate() {
                    let group = if index == 0 { WS_GROUP.0 | tab } else { 0 };
                    child(
                        option,
                        w!("BUTTON"),
                        BS_AUTORADIOBUTTON as u32 | group,
                        id(row, OPTION + index as i32),
                        control_x + width * index as i32,
                        y,
                        width,
                        normal,
                    );
                }
            }
            FieldKind::Toggle { .. } => {
                child(
                    text,
                    w!("BUTTON"),
                    BS_AUTOCHECKBOX as u32 | tab | WS_GROUP.0,
                    id(row, MAIN),
                    label_x,
                    y,
                    full_width,
                    normal,
                );
            }
            FieldKind::Color { .. } => {
                label(row, text, y);
                child(
                    "",
                    w!("BUTTON"),
                    BS_OWNERDRAW as u32 | tab | WS_GROUP.0,
                    id(row, MAIN),
                    control_x,
                    y,
                    px(SWATCH_WIDTH),
                    normal,
                );
                let alpha_x = control_x + px(SWATCH_WIDTH + GAP);
                child(
                    "",
                    w!("msctls_trackbar32"),
                    tab | TBS_NOTICKS,
                    id(row, SECOND),
                    alpha_x,
                    y,
                    px(CONTROL_WIDTH - SWATCH_WIDTH - GAP),
                    normal,
                );
                child(
                    "",
                    w!("STATIC"),
                    SS_CENTERIMAGE.0,
                    id(row, VALUE),
                    value_x,
                    y,
                    px(VALUE_WIDTH),
                    normal,
                );
            }
            FieldKind::Action { .. } => {
                child(
                    text,
                    w!("BUTTON"),
                    BS_PUSHBUTTON as u32 | tab | WS_GROUP.0,
                    id(row, MAIN),
                    control_x,
                    y,
                    px(140),
                    normal,
                );
            }
        }
        y += px(ROW_HEIGHT);
    }
    for (row, field) in FORM.iter().enumerate() {
        let (low, high, page) = match &field.kind {
            FieldKind::Slider { range, step, .. } => (
                0,
                ((range.end() - range.start()) / step).round() as isize,
                5,
            ),
            FieldKind::Color { .. } => (0, 100, 10),
            _ => continue,
        };
        let part = if matches!(field.kind, FieldKind::Color { .. }) {
            SECOND
        } else {
            MAIN
        };
        let control = item(hwnd, row, part);
        // SAFETY: a live trackbar of our window.
        unsafe {
            SendMessageW(control, TBM_SETRANGEMIN, Some(WPARAM(0)), Some(LPARAM(low)));
            SendMessageW(
                control,
                TBM_SETRANGEMAX,
                Some(WPARAM(1)),
                Some(LPARAM(high)),
            );
            SendMessageW(
                control,
                TBM_SETPAGESIZE,
                Some(WPARAM(0)),
                Some(LPARAM(page)),
            );
        }
    }
    for (row, field) in FORM.iter().enumerate() {
        if let FieldKind::Stepper { range, .. } = &field.kind {
            // SAFETY: a live up-down control of our window.
            unsafe {
                SendMessageW(
                    item(hwnd, row, SECOND),
                    UDM_SETRANGE32,
                    Some(WPARAM(*range.start() as usize)),
                    Some(LPARAM(*range.end() as isize)),
                )
            };
        }
    }

    let mut frame = RECT {
        left: 0,
        top: 0,
        right: full_width + px(MARGIN) * 2,
        bottom: y + px(MARGIN),
    };
    // SAFETY: `frame` is valid for the call; style and DPI describe our window.
    let _ = unsafe { AdjustWindowRectExForDpi(&mut frame, STYLE, false, WINDOW_EX_STYLE(0), dpi) };
    let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
    let (x, y) = origin.unwrap_or_else(|| {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is sized and valid for the call.
        let _ = unsafe {
            GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY), &mut info)
        };
        let work = info.rcWork;
        (
            work.left + (work.right - work.left - width) / 2,
            work.top + ((work.bottom - work.top - height) / 2).max(0),
        )
    });
    // SAFETY: our live window.
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn create_fonts(dpi: u32) -> [HFONT; 2] {
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    // SAFETY: `metrics` is sized and valid for the call; the fonts are owned by the Window and deleted by it.
    unsafe {
        let _ = SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some(&mut metrics as *mut NONCLIENTMETRICSW as *mut _),
            0,
            dpi,
        );
        let mut bold = metrics.lfMessageFont;
        bold.lfWeight = 700;
        [
            CreateFontIndirectW(&metrics.lfMessageFont),
            CreateFontIndirectW(&bold),
        ]
    }
}

fn item(hwnd: HWND, row: usize, part: i32) -> HWND {
    // SAFETY: a lookup; a missing control yields an invalid handle, which every message call tolerates.
    unsafe { GetDlgItem(Some(hwnd), id(row, part)) }.unwrap_or_default()
}

fn send(control: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: a message to one of our controls; any handle is tolerated.
    unsafe { SendMessageW(control, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) }.0
}

fn set_text(control: HWND, text: &str) {
    // SAFETY: as above; the text outlives the call.
    let _ = unsafe { SetWindowTextW(control, &HSTRING::from(text)) };
}

fn check(control: HWND, on: bool) {
    let state = if on { BST_CHECKED } else { BST_UNCHECKED };
    send(control, BM_SETCHECK, state.0 as usize, 0);
}

fn set_controls(hwnd: HWND, settings: &Settings) {
    for (row, field) in FORM.iter().enumerate() {
        match &field.kind {
            FieldKind::Slider {
                range,
                step,
                unit,
                get,
                ..
            } => {
                let value = get(settings);
                let position = ((value - range.start()) / step).round() as isize;
                send(item(hwnd, row, MAIN), TBM_SETPOS, 1, position);
                set_text(item(hwnd, row, VALUE), &format!("{value:.0} {unit}"));
            }
            FieldKind::Stepper { format, get, .. } => {
                let value = get(settings);
                set_text(item(hwnd, row, MAIN), &format(value));
                send(item(hwnd, row, SECOND), UDM_SETPOS32, 0, value as isize);
            }
            FieldKind::Choice { options, get, .. } => {
                for index in 0..options.len() {
                    check(
                        item(hwnd, row, OPTION + index as i32),
                        index == get(settings),
                    );
                }
            }
            FieldKind::Toggle { get, .. } => check(item(hwnd, row, MAIN), get(settings)),
            FieldKind::Color { get, .. } => {
                let alpha = (get(settings).alpha * 100.0).round();
                // SAFETY: repaints our own swatch control later, through WM_DRAWITEM.
                let _ = unsafe { InvalidateRect(Some(item(hwnd, row, MAIN)), None, true) };
                send(item(hwnd, row, SECOND), TBM_SETPOS, 1, alpha as isize);
                set_text(item(hwnd, row, VALUE), &format!("{alpha:.0}%"));
            }
            FieldKind::Action { enabled, .. } => {
                // SAFETY: one of our controls.
                let _ = unsafe { EnableWindow(item(hwnd, row, MAIN), enabled(settings)) };
            }
        }
    }
}

/// The edit a control made, applied to a copy of the current settings and dispatched whole.
fn edited(hwnd: HWND, control: HWND, control_id: i32) {
    let offset = control_id - ID_BASE;
    let (row, part) = (offset / PARTS, offset % PARTS);
    let (Some(field), Some(current), true) =
        (FORM.get(row as usize), current_settings(), offset >= 0)
    else {
        return;
    };
    let mut next = current.clone();
    match (&field.kind, part) {
        (
            FieldKind::Slider {
                range, step, set, ..
            },
            MAIN,
        ) => {
            let position = send(control, TBM_GETPOS, 0, 0) as f64;
            set(
                &mut next,
                (range.start() + position * step).min(*range.end()),
            );
        }
        (FieldKind::Stepper { range, set, .. }, SECOND) => {
            let position = send(control, UDM_GETPOS32, 0, 0) as u32;
            set(&mut next, position.clamp(*range.start(), *range.end()));
        }
        (FieldKind::Choice { set, .. }, part) if part >= OPTION => {
            set(&mut next, (part - OPTION) as usize)
        }
        (FieldKind::Toggle { set, .. }, MAIN) => set(
            &mut next,
            send(control, BM_GETCHECK, 0, 0) == BST_CHECKED.0 as isize,
        ),
        (FieldKind::Color { get, set }, MAIN) => match choose_color(hwnd, get(&current)) {
            Some(color) => set(&mut next, color),
            None => return,
        },
        (FieldKind::Color { get, set }, SECOND) => {
            let alpha = send(control, TBM_GETPOS, 0, 0) as f64 / 100.0;
            set(&mut next, get(&current).with_alpha(alpha));
        }
        (FieldKind::Action { apply, .. }, MAIN) => apply(&mut next),
        _ => return,
    }
    super::dispatch(Input::SettingsEdited(next));
}

fn colorref(color: Rgba) -> COLORREF {
    let byte = |v: f64| (v * 255.0).round() as u32;
    COLORREF(byte(color.red) | byte(color.green) << 8 | byte(color.blue) << 16)
}

/// ChooseColor has no alpha, so the color's own alpha is kept; the slider beside the swatch edits it.
fn choose_color(hwnd: HWND, current: Rgba) -> Option<Rgba> {
    let mut custom = CUSTOM_COLORS.get();
    let mut dialog = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: colorref(current),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    // SAFETY: `dialog` and `custom` outlive the modal call. No RefCell is borrowed here, so messages the modal
    // loop dispatches can reach `App`.
    let chosen = unsafe { ChooseColorW(&mut dialog) }.as_bool();
    CUSTOM_COLORS.set(custom);
    let rgb = dialog.rgbResult.0;
    let unit = |shift: u32| f64::from((rgb >> shift) & 0xFF) / 255.0;
    chosen.then(|| Rgba {
        red: unit(0),
        green: unit(8),
        blue: unit(16),
        alpha: current.alpha,
    })
}

fn draw_swatch(draw: &DRAWITEMSTRUCT) {
    let offset = draw.CtlID as i32 - ID_BASE;
    let color = match (FORM.get((offset / PARTS) as usize), current_settings()) {
        (
            Some(Field {
                kind: FieldKind::Color { get, .. },
                ..
            }),
            Some(settings),
        ) => get(&settings),
        _ => return,
    };
    // SAFETY: `draw` describes a DC and rect the system hands us for this paint; the brush is deleted here.
    unsafe {
        let brush = CreateSolidBrush(colorref(color));
        FillRect(draw.hDC, &draw.rcItem, brush);
        let _ = DeleteObject(brush.into());
        FrameRect(draw.hDC, &draw.rcItem, GetSysColorBrush(COLOR_WINDOWFRAME));
        if draw.itemState.0 & ODS_FOCUS.0 != 0 {
            let mut inner = draw.rcItem;
            inner.left += 3;
            inner.top += 3;
            inner.right -= 3;
            inner.bottom -= 3;
            let _ = DrawFocusRect(draw.hDC, &inner);
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let control = HWND(lparam.0 as *mut _);
    match msg {
        WM_COMMAND => {
            let (command, code) = (
                (wparam.0 & 0xFFFF) as i32,
                ((wparam.0 >> 16) & 0xFFFF) as u32,
            );
            if command == IDCANCEL.0 {
                // SAFETY: our live window.
                let _ = unsafe { DestroyWindow(hwnd) };
            } else if code == BN_CLICKED {
                edited(hwnd, control, command);
            }
        }
        WM_HSCROLL | WM_VSCROLL if !control.is_invalid() => {
            // SAFETY: `control` is the scrolling child the message names.
            edited(hwnd, control, unsafe { GetDlgCtrlID(control) });
        }
        WM_DRAWITEM => {
            // SAFETY: for WM_DRAWITEM, lParam points at a DRAWITEMSTRUCT valid for this message.
            draw_swatch(unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) });
            return LRESULT(1);
        }
        WM_DPICHANGED => {
            // SAFETY: for WM_DPICHANGED, lParam points at the suggested window RECT; the children are ours.
            unsafe {
                let suggested = *(lparam.0 as *const RECT);
                while let Ok(child) = GetWindow(hwnd, GW_CHILD) {
                    if DestroyWindow(child).is_err() {
                        break;
                    }
                }
                build(hwnd, Some((suggested.left, suggested.top)));
            }
            if let Some(settings) = current_settings() {
                set_controls(hwnd, &settings);
            }
        }
        WM_CLOSE => {
            // SAFETY: our live window.
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        WM_DESTROY => {
            if let Some(window) = WINDOW.with(|window| window.borrow_mut().take()) {
                // SAFETY: fonts this window created; `return_to` is checked to still be a window. We are the
                // foreground process at this moment, so handing the foreground back is allowed.
                unsafe {
                    for font in window.fonts {
                        let _ = DeleteObject(font.into());
                    }
                    if let Some(previous) =
                        window.return_to.filter(|h| IsWindow(Some(*h)).as_bool())
                    {
                        let _ = SetForegroundWindow(previous);
                    }
                }
            }
        }
        // SAFETY: forwards this window's own message unchanged.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    LRESULT(0)
}
