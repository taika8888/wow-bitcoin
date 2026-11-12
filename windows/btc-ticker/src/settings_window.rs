//! The Settings window: a normal, activating window of standard controls. Each edit goes to the core as
//! `OverlayEvent::SettingsEdited`, and the controls follow the core's settings back, whatever changed them.

use std::cell::{Cell, RefCell};

use ticker_core::{Rgba, Settings};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, CreateFontIndirectW, CreateSolidBrush, DeleteObject, GetMonitorInfoW, HBRUSH, HFONT, InvalidateRect,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW};
use windows::Win32::UI::Controls::{
    BST_CHECKED, ICC_BAR_CLASSES, ICC_STANDARD_CLASSES, ICC_UPDOWN_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx,
    TBM_SETLINESIZE, TBM_SETPAGESIZE, TBM_SETPOS, TBM_SETRANGEMAX, TBM_SETRANGEMIN, TBS_NOTICKS, TRACKBAR_CLASSW,
    UDM_SETRANGE32, UDS_ALIGNRIGHT, UDS_ARROWKEYS, UDS_AUTOBUDDY, UDS_NOTHOUSANDS, UDS_SETBUDDYINT, UPDOWN_CLASSW,
    WC_BUTTONW, WC_COMBOBOXW, WC_EDITW, WC_STATICW,
};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL,
    CBN_SELCHANGE, CBS_DROPDOWNLIST, CreateWindowExW, DefWindowProcW, DestroyWindow, EN_CHANGE, ES_NUMBER,
    GWLP_USERDATA, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW, GetWindowTextW, HMENU, IDCANCEL, IsIconic, IsWindow,
    NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SW_RESTORE, SW_SHOW, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_HSCROLL, WM_NCDESTROY, WM_SETFONT, WS_BORDER, WS_CAPTION,
    WS_CHILD, WS_EX_CLIENTEDGE, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, Result, w};

use crate::autostart;
use crate::shell::{Event, deliver};
use crate::win::{self, wide};

struct Slider {
    id: i32,
    label: &'static str,
    unit: &'static str,
    range: (i32, i32),
    step: i32,
    get: fn(&Settings) -> i32,
    set: fn(&mut Settings, i32),
}

struct Check {
    id: i32,
    label: &'static str,
    get: fn(&Settings) -> bool,
    set: fn(&mut Settings, bool),
}

struct Swatch {
    id: i32,
    label: &'static str,
    get: fn(&Settings) -> Rgba,
    set: fn(&mut Settings, Rgba),
}

const FONT_SIZE: Slider = Slider {
    id: 100,
    label: "Font size",
    unit: "pt",
    range: (9, 72),
    step: 1,
    get: |settings| settings.font_size.round() as i32,
    set: |settings, value| settings.font_size = f64::from(value),
};
const OPACITY: Slider = Slider {
    id: 101,
    label: "Background",
    unit: "%",
    range: (0, 100),
    step: 5,
    get: |settings| (settings.opacity * 100.0).round() as i32,
    set: |settings, value| settings.opacity = f64::from(value) / 100.0,
};
const CHART_WIDTH: Slider = Slider {
    id: 102,
    label: "Chart width",
    unit: "px",
    range: (240, 900),
    step: 10,
    get: |settings| settings.chart_width.round() as i32,
    set: |settings, value| settings.chart_width = f64::from(value),
};
const CHART_HEIGHT: Slider = Slider {
    id: 103,
    label: "Chart height",
    unit: "px",
    range: (120, 600),
    step: 10,
    get: |settings| settings.chart_height.round() as i32,
    set: |settings, value| settings.chart_height = f64::from(value),
};
const SLIDERS: [&Slider; 4] = [&FONT_SIZE, &OPACITY, &CHART_WIDTH, &CHART_HEIGHT];

const SHOW_LABEL: Check = Check {
    id: 200,
    label: "Show BTC label",
    get: |settings| settings.show_label,
    set: |settings, on| settings.show_label = on,
};
const SHOW_CHANGE: Check = Check {
    id: 201,
    label: "Show change since the daily open",
    get: |settings| settings.show_change,
    set: |settings, on| settings.show_change = on,
};
const ONLY_WHEN_WOW: Check = Check {
    id: 202,
    label: "Show only while World of Warcraft is frontmost",
    get: |settings| settings.only_when_wow_active,
    set: |settings, on| settings.only_when_wow_active = on,
};
const LOCKED: Check = Check {
    id: 203,
    label: "Lock position",
    get: |settings| settings.locked,
    set: |settings, on| settings.locked = on,
};
const CHECKS: [&Check; 4] = [&SHOW_LABEL, &SHOW_CHANGE, &ONLY_WHEN_WOW, &LOCKED];

const COLORS: [Swatch; 3] = [
    Swatch {
        id: 300,
        label: "Price color",
        get: |settings| settings.text_color,
        set: |settings, color| settings.text_color = color,
    },
    Swatch {
        id: 301,
        label: "Up color",
        get: |settings| settings.up_color,
        set: |settings, color| settings.up_color = color,
    },
    Swatch {
        id: 302,
        label: "Down color",
        get: |settings| settings.down_color,
        set: |settings, color| settings.down_color = color,
    },
];

/// Missing from the windows crate: TBM_GETPOS is WM_USER.
const TBM_GETPOS: u32 = 0x0400;

const DECIMALS: i32 = 400;
const REFRESH: i32 = 401;
const RESET_POSITION: i32 = 402;
const AUTOSTART: i32 = 403;
/// A slider's value label and a color's swatch take their control's id plus this.
const COMPANION: i32 = 50;

/// Device-independent layout, scaled by the window's DPI.
const MARGIN: i32 = 14;
const ROW: i32 = 30;
const LABEL_WIDTH: i32 = 110;
const CONTROL_WIDTH: i32 = 220;
const VALUE_WIDTH: i32 = 56;
const CONTROL_HEIGHT: i32 = 24;

thread_local! {
    static OPEN: Cell<isize> = const { Cell::new(0) };
}

/// The open Settings window, for the message loop's dialog navigation.
pub fn current() -> Option<HWND> {
    Some(HWND(OPEN.get() as *mut _)).filter(|hwnd| !hwnd.is_invalid())
}

/// Closing the window means dropping this, which hands focus back to the window that had it when Settings opened.
pub struct SettingsWindow {
    hwnd: HWND,
    return_to: Option<HWND>,
}

/// The window's own state, so its procedure never needs the shell, which may be busy when the procedure runs.
struct Form {
    /// The settings the controls show, as the core last reported them.
    shown: Cell<Settings>,
    /// Set while the controls are being created or updated, so their change notifications are not taken for edits:
    /// an up-down control, for one, writes 0 into its edit box as it is created.
    syncing: Cell<bool>,
    font: HFONT,
    swatches: RefCell<Vec<HBRUSH>>,
    custom_colors: RefCell<[COLORREF; 16]>,
}

impl SettingsWindow {
    /// Opens centered on the monitor that holds `ticker`.
    pub fn open(settings: Settings, ticker: HWND, return_to: Option<HWND>) -> Result<SettingsWindow> {
        let class = w!("BTCTickerSettings");
        win::register_class(class, Some(procedure), Some(HBRUSH((COLOR_BTNFACE.0 + 1) as usize as *mut _)));
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        // SAFETY: creates a window this struct owns; the Form box is freed in WM_NCDESTROY.
        unsafe {
            let controls = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_BAR_CLASSES | ICC_UPDOWN_CLASS | ICC_STANDARD_CLASSES,
            };
            let _ = InitCommonControlsEx(&controls);
            let mut monitor = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(MonitorFromWindow(ticker, MONITOR_DEFAULTTOPRIMARY), &mut monitor);
            let work = monitor.rcWork;
            // Created on the ticker's monitor, so the controls are laid out for that monitor's DPI.
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("BTC Ticker Settings"),
                style,
                work.left,
                work.top,
                100,
                100,
                None,
                None,
                None,
                None,
            )?;
            let dpi = GetDpiForWindow(hwnd);
            let form = Box::new(Form {
                shown: Cell::new(settings),
                syncing: Cell::new(true),
                font: message_font(dpi),
                swatches: RefCell::new(Vec::new()),
                custom_colors: RefCell::new([COLORREF(0x00ff_ffff); 16]),
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(form) as isize);
            let height = build(hwnd, form_of(hwnd).expect("set above"), dpi);

            let scale = |value: i32| value * dpi as i32 / 96;
            let mut frame = RECT {
                left: 0,
                top: 0,
                right: scale(MARGIN * 2 + LABEL_WIDTH + CONTROL_WIDTH + VALUE_WIDTH),
                bottom: height,
            };
            let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE(0), dpi);
            let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
            let x = (work.left + work.right - width) / 2;
            let y = (work.top + work.bottom - height) / 2;
            let _ = SetWindowPos(hwnd, None, x, y, width, height, SWP_NOZORDER);
            apply(hwnd, settings);
            show_autostart(GetDlgItem(Some(hwnd), AUTOSTART).unwrap_or_default());
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            OPEN.set(hwnd.0 as isize);
            Ok(SettingsWindow { hwnd, return_to })
        }
    }

    pub fn focus(&self) {
        // SAFETY: plain calls on this window.
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    pub fn sync(&self, settings: Settings) {
        if form_of(self.hwnd).is_some_and(|form| form.shown.get() != settings) {
            apply(self.hwnd, settings);
        }
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        OPEN.set(0);
        // SAFETY: destroys the window this struct owns, then hands focus back if that window still exists.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            if let Some(previous) = self.return_to.filter(|previous| IsWindow(Some(*previous)).as_bool()) {
                let _ = SetForegroundWindow(previous);
            }
        }
    }
}

fn form_of<'a>(hwnd: HWND) -> Option<&'a Form> {
    // SAFETY: GWLP_USERDATA holds the Form box from `open` until WM_NCDESTROY frees it.
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Form).as_ref() }
}

/// Creates every control and returns the client height they need.
fn build(hwnd: HWND, form: &Form, dpi: u32) -> i32 {
    let scale = |value: i32| value * dpi as i32 / 96;
    let mut y = MARGIN;
    let add = |class: PCWSTR, text: &str, style: i32, x: i32, y: i32, width: i32, id: i32| {
        let text = wide(text);
        let extra = if class == WC_EDITW { WS_EX_CLIENTEDGE } else { WINDOW_EX_STYLE(0) };
        // SAFETY: creates a child the parent destroys; the font outlives the window.
        unsafe {
            let child = CreateWindowExW(
                extra,
                class,
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style as u32),
                scale(x),
                scale(y),
                scale(width),
                scale(CONTROL_HEIGHT),
                Some(hwnd),
                Some(HMENU(id as isize as *mut _)),
                None,
                None,
            )
            .unwrap_or_default();
            SendMessageW(child, WM_SETFONT, Some(WPARAM(form.font.0 as usize)), Some(LPARAM(1)));
            child
        }
    };
    let tab = WS_TABSTOP.0 as i32;
    let control_x = MARGIN + LABEL_WIDTH;
    let label = |text: &str, y: i32| add(WC_STATICW, text, 0, MARGIN, y + 3, LABEL_WIDTH, -1);
    let slider = |slider: &Slider, y: i32| {
        label(slider.label, y);
        let bar = add(TRACKBAR_CLASSW, "", tab | TBS_NOTICKS as i32, control_x, y, CONTROL_WIDTH, slider.id);
        // SAFETY: plain control messages.
        unsafe {
            SendMessageW(bar, TBM_SETRANGEMIN, Some(WPARAM(0)), Some(LPARAM(slider.range.0 as isize)));
            SendMessageW(bar, TBM_SETRANGEMAX, Some(WPARAM(1)), Some(LPARAM(slider.range.1 as isize)));
            SendMessageW(bar, TBM_SETLINESIZE, None, Some(LPARAM(slider.step as isize)));
            SendMessageW(bar, TBM_SETPAGESIZE, None, Some(LPARAM((slider.step * 5) as isize)));
        }
        add(WC_STATICW, "", 0, control_x + CONTROL_WIDTH + 6, y + 3, VALUE_WIDTH - 6, slider.id + COMPANION);
    };
    let check = |check: &Check, y: i32| {
        add(WC_BUTTONW, check.label, tab | BS_AUTOCHECKBOX, MARGIN, y, LABEL_WIDTH + CONTROL_WIDTH, check.id);
    };

    slider(&FONT_SIZE, y);
    y += ROW;
    label("Decimals", y);
    let decimals = add(WC_COMBOBOXW, "", tab | CBS_DROPDOWNLIST | WS_VSCROLL.0 as i32, control_x, y, 60, DECIMALS);
    for option in ["0", "1", "2"] {
        let option = wide(option);
        // SAFETY: the combo box copies the string.
        unsafe { SendMessageW(decimals, CB_ADDSTRING, None, Some(LPARAM(option.as_ptr() as isize))) };
    }
    y += ROW;
    for item in [&SHOW_LABEL, &SHOW_CHANGE] {
        check(item, y);
        y += ROW;
    }
    for swatch in &COLORS {
        label(swatch.label, y);
        add(WC_BUTTONW, "Choose…", tab | BS_PUSHBUTTON, control_x, y, 90, swatch.id);
        add(WC_STATICW, "", WS_BORDER.0 as i32, control_x + 100, y + 2, 40, swatch.id + COMPANION);
        y += ROW;
    }
    for item in [&OPACITY, &CHART_WIDTH, &CHART_HEIGHT] {
        slider(item, y);
        y += ROW;
    }
    label("Refresh every", y);
    add(WC_EDITW, "", tab | ES_NUMBER, control_x, y, 60, REFRESH);
    let spinner = UDS_SETBUDDYINT | UDS_ALIGNRIGHT | UDS_ARROWKEYS | UDS_AUTOBUDDY | UDS_NOTHOUSANDS;
    let arrows = add(UPDOWN_CLASSW, "", spinner as i32, 0, y, 0, -1);
    // SAFETY: plain control message.
    unsafe { SendMessageW(arrows, UDM_SETRANGE32, Some(WPARAM(2)), Some(LPARAM(300))) };
    add(WC_STATICW, "seconds", 0, control_x + 70, y + 3, 80, -1);
    y += ROW;
    for item in [&ONLY_WHEN_WOW, &LOCKED] {
        check(item, y);
        y += ROW;
    }
    add(WC_BUTTONW, "Start with Windows", tab | BS_AUTOCHECKBOX, MARGIN, y, LABEL_WIDTH + CONTROL_WIDTH, AUTOSTART);
    y += ROW;
    add(WC_BUTTONW, "Reset position", tab | BS_PUSHBUTTON, MARGIN, y, 130, RESET_POSITION);
    y += ROW;
    scale(y + MARGIN - (ROW - CONTROL_HEIGHT))
}

fn apply(hwnd: HWND, settings: Settings) {
    let Some(form) = form_of(hwnd) else { return };
    form.shown.set(settings);
    form.syncing.set(true);
    // SAFETY: plain messages to this window's own controls.
    unsafe {
        let item = |id: i32| GetDlgItem(Some(hwnd), id).unwrap_or_default();
        for slider in SLIDERS {
            let value = (slider.get)(&settings);
            SendMessageW(item(slider.id), TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(value as isize)));
            let text = wide(&format!("{value} {}", slider.unit));
            let _ = SetWindowTextW(item(slider.id + COMPANION), PCWSTR(text.as_ptr()));
        }
        for check in CHECKS {
            let state = if (check.get)(&settings) { BST_CHECKED.0 } else { 0 };
            SendMessageW(item(check.id), BM_SETCHECK, Some(WPARAM(state as usize)), None);
        }
        SendMessageW(item(DECIMALS), CB_SETCURSEL, Some(WPARAM(usize::from(settings.decimals))), None);
        // Rewriting the field under the caret would fight the person typing in it.
        if GetFocus() != item(REFRESH) {
            let text = wide(&settings.refresh_seconds.to_string());
            let _ = SetWindowTextW(item(REFRESH), PCWSTR(text.as_ptr()));
        }
        let mut swatches = form.swatches.borrow_mut();
        for brush in swatches.drain(..) {
            let _ = DeleteObject(brush.into());
        }
        for swatch in &COLORS {
            swatches.push(CreateSolidBrush(colorref((swatch.get)(&settings))));
            let _ = InvalidateRect(Some(item(swatch.id + COMPANION)), None, true);
        }
        let _ = EnableWindow(item(RESET_POSITION), settings.position.is_some());
    }
    form.syncing.set(false);
}

/// The box follows the registry, so after a failed write it still shows what Windows will do at login.
fn show_autostart(checkbox: HWND) {
    let state = if autostart::enabled() { BST_CHECKED.0 } else { 0 };
    // SAFETY: plain control message.
    unsafe { SendMessageW(checkbox, BM_SETCHECK, Some(WPARAM(state as usize)), None) };
}

fn edit(form: &Form, change: impl FnOnce(&mut Settings)) {
    if form.syncing.get() {
        return;
    }
    let mut settings = form.shown.get();
    change(&mut settings);
    deliver(Event::SettingsEdited(settings));
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(form) = form_of(hwnd) else {
        // SAFETY: default handling before the Form exists.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    // SAFETY: plain queries and messages on this window's own controls.
    unsafe {
        match message {
            WM_HSCROLL => {
                let bar = HWND(lparam.0 as *mut _);
                let id = GetDlgCtrlID(bar);
                if let Some(slider) = SLIDERS.into_iter().find(|slider| slider.id == id) {
                    let value = SendMessageW(bar, TBM_GETPOS, None, None).0 as i32;
                    edit(form, |settings| (slider.set)(settings, value));
                }
                return LRESULT(0);
            }
            WM_COMMAND => {
                let (id, code) = (i32::from(wparam.0 as u16), u32::from((wparam.0 >> 16) as u16));
                let control = HWND(lparam.0 as *mut _);
                if let Some(check) = CHECKS.into_iter().find(|check| check.id == id) {
                    let on = SendMessageW(control, BM_GETCHECK, None, None).0 == BST_CHECKED.0 as isize;
                    edit(form, |settings| (check.set)(settings, on));
                } else if let Some(swatch) = COLORS.iter().find(|swatch| swatch.id == id) {
                    let current = (swatch.get)(&form.shown.get());
                    if let Some(color) = choose_color(hwnd, form, current) {
                        edit(form, |settings| (swatch.set)(settings, color));
                    }
                } else if id == DECIMALS && code == CBN_SELCHANGE {
                    let index = SendMessageW(control, CB_GETCURSEL, None, None).0;
                    if let Ok(decimals) = u8::try_from(index) {
                        edit(form, |settings| settings.decimals = decimals);
                    }
                } else if id == REFRESH && code == EN_CHANGE {
                    let mut text = [0u16; 16];
                    let length = GetWindowTextW(control, &mut text);
                    if let Ok(seconds) = String::from_utf16_lossy(&text[..length as usize]).parse::<u32>() {
                        edit(form, |settings| settings.refresh_seconds = seconds);
                    }
                } else if id == RESET_POSITION && code == BN_CLICKED {
                    edit(form, |settings| settings.position = None);
                } else if id == AUTOSTART && code == BN_CLICKED {
                    let on = SendMessageW(control, BM_GETCHECK, None, None).0 == BST_CHECKED.0 as isize;
                    if let Err(error) = autostart::set(on) {
                        eprintln!("could not change Start with Windows: {error}");
                    }
                    show_autostart(control);
                } else if id == IDCANCEL.0 {
                    deliver(Event::SettingsClosed);
                }
                return LRESULT(0);
            }
            WM_CTLCOLORSTATIC => {
                let id = GetDlgCtrlID(HWND(lparam.0 as *mut _));
                if let Some(index) = COLORS.iter().position(|swatch| swatch.id + COMPANION == id)
                    && let Some(brush) = form.swatches.borrow().get(index)
                {
                    return LRESULT(brush.0 as isize);
                }
            }
            WM_CLOSE => {
                deliver(Event::SettingsClosed);
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                for brush in form.swatches.borrow_mut().drain(..) {
                    let _ = DeleteObject(brush.into());
                }
                let _ = DeleteObject(form.font.into());
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(std::ptr::from_ref(form).cast_mut()));
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

/// The color dialog is modal; it runs its own message loop, so the ticker keeps updating while it is open.
fn choose_color(owner: HWND, form: &Form, current: Rgba) -> Option<Rgba> {
    let mut custom = form.custom_colors.borrow_mut();
    let mut dialog = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: colorref(current),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN,
        ..Default::default()
    };
    // SAFETY: the struct and the custom color array outlive the modal call.
    let chosen = unsafe { ChooseColorW(&mut dialog) }.as_bool();
    chosen.then(|| {
        let [red, green, blue, _] = dialog.rgbResult.0.to_le_bytes();
        let unit = |value: u8| f64::from(value) / 255.0;
        Rgba::new(unit(red), unit(green), unit(blue), current.alpha)
    })
}

fn colorref(color: Rgba) -> COLORREF {
    let byte = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(byte(color.red) | byte(color.green) << 8 | byte(color.blue) << 16)
}

fn message_font(dpi: u32) -> HFONT {
    let mut metrics = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    // SAFETY: fills one struct of the size given.
    unsafe {
        let _ = SystemParametersInfoForDpi(
            SPI_GETNONCLIENTMETRICS.0,
            metrics.cbSize,
            Some((&raw mut metrics).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0).0,
            dpi,
        );
        CreateFontIndirectW(&metrics.lfMessageFont)
    }
}
