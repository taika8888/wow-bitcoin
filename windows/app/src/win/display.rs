//! Physical px throughout, because app.manifest makes the process per-monitor-v2 aware.
use ticker_core::{Env, Rect, ScreenArea};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::SystemServices::{TIME_ZONE_ID_DAYLIGHT, TIME_ZONE_ID_STANDARD};
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

pub(super) fn current() -> Env {
    let mut found: Vec<(bool, ScreenArea)> = Vec::new();
    // SAFETY: `found` outlives the call and `each` only pushes into it.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(each),
            LPARAM(&mut found as *mut Vec<(bool, ScreenArea)> as isize),
        )
    };
    found.sort_by_key(|(primary, _)| !primary);
    Env {
        screens: found.into_iter().map(|(_, screen)| screen).collect(),
        utc_offset_seconds: utc_offset_seconds(),
    }
}

unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the `&mut Vec` passed by `current` for the duration of EnumDisplayMonitors.
    let found = unsafe { &mut *(data.0 as *mut Vec<(bool, ScreenArea)>) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is sized and valid for the call; `monitor` comes from the enumeration.
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let (mut dpi_x, mut dpi_y) = (96, 96);
        // SAFETY: both out-pointers are valid for the call.
        let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        found.push((
            info.dwFlags & MONITORINFOF_PRIMARY != 0,
            ScreenArea {
                frame: rect(info.rcMonitor),
                work: rect(info.rcWork),
                scale: f64::from(dpi_x) / 96.0,
            },
        ));
    }
    BOOL(1)
}

fn rect(r: RECT) -> Rect {
    Rect {
        x: f64::from(r.left),
        y: f64::from(r.top),
        width: f64::from(r.right - r.left),
        height: f64::from(r.bottom - r.top),
    }
}

fn utc_offset_seconds() -> i32 {
    let mut zone = TIME_ZONE_INFORMATION::default();
    // SAFETY: `zone` is valid for the call.
    let kind = unsafe { GetTimeZoneInformation(&mut zone) };
    let bias = zone.Bias
        + match kind {
            TIME_ZONE_ID_DAYLIGHT => zone.DaylightBias,
            TIME_ZONE_ID_STANDARD => zone.StandardBias,
            _ => 0,
        };
    -bias * 60
}
