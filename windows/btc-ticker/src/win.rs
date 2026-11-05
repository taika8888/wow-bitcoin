//! Small safe wrappers over the Win32 calls more than one module needs.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ticker_core::{Rect, ScreenArea, Screens, Timestamp};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, POINT, RECT, SYSTEMTIME};
use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_ITIME};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, GetCursorPos, GetWindowRect, IDC_ARROW, LoadCursorW, MONITORINFOF_PRIMARY, RegisterClassW,
    WNDCLASSW, WNDPROC,
};
use windows::core::{BOOL, PCWSTR, PWSTR};

/// A null-terminated UTF-16 copy of `text`, to pass as a PCWSTR while it lives.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

pub fn now() -> Timestamp {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
    Timestamp::from_unix_seconds(seconds)
}

pub fn register_class(name: PCWSTR, proc: WNDPROC, background: Option<windows::Win32::Graphics::Gdi::HBRUSH>) {
    // SAFETY: the class name is a static string and the procedure lives for the whole process.
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        RegisterClassW(&WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: proc,
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: background.unwrap_or_default(),
            lpszClassName: name,
            ..Default::default()
        });
    }
}

pub fn cursor() -> POINT {
    let mut point = POINT::default();
    // SAFETY: writes one POINT.
    let _ = unsafe { GetCursorPos(&mut point) };
    point
}

pub fn window_rect(hwnd: HWND) -> Rect {
    let mut rect = RECT::default();
    // SAFETY: writes one RECT; an invalid window leaves it zeroed.
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    rect_from(rect)
}

pub fn rect_from(rect: RECT) -> Rect {
    Rect::new(
        f64::from(rect.left),
        f64::from(rect.top),
        f64::from(rect.right - rect.left),
        f64::from(rect.bottom - rect.top),
    )
}

/// The window's monitor DPI over 96: the factor from the settings' device-independent sizes to physical pixels.
pub fn scale(hwnd: HWND) -> f64 {
    // SAFETY: plain query.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// Every monitor with its work area, in virtual-screen physical pixels.
pub fn screens() -> Screens {
    unsafe extern "system" fn collect(monitor: HMONITOR, _: HDC, _: *mut RECT, areas: LPARAM) -> BOOL {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: `areas` is the Vec passed below, alive for the whole enumeration.
        unsafe {
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                let areas = &mut *(areas.0 as *mut Vec<(ScreenArea, bool)>);
                let area = ScreenArea { frame: rect_from(info.rcMonitor), work: rect_from(info.rcWork) };
                areas.push((area, info.dwFlags & MONITORINFOF_PRIMARY != 0));
            }
        }
        true.into()
    }
    let mut areas: Vec<(ScreenArea, bool)> = Vec::new();
    // SAFETY: the callback only touches `areas`, which outlives the call.
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(collect), LPARAM(&raw mut areas as isize)) };
    let primary_index = areas.iter().position(|(_, primary)| *primary).unwrap_or(0);
    if areas.is_empty() {
        let fallback = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        return Screens { primary: ScreenArea { frame: fallback, work: fallback }, others: Vec::new() };
    }
    let (primary, _) = areas.remove(primary_index);
    Screens { primary, others: areas.into_iter().map(|(area, _)| area).collect() }
}

/// The full path of a process's executable, or None when the process is gone or protected.
pub fn process_image(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning and the buffer outlives the query.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let found = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length);
        let _ = CloseHandle(process);
        found.ok().map(|()| String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

/// Seconds east of UTC that local clocks read at `time`, daylight saving included.
pub fn utc_offset(time: Timestamp) -> i32 {
    let to_file_time = |ticks: u64| FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let utc_ticks = (time.unix_seconds().floor() + 11_644_473_600.0) as u64 * 10_000_000;
    let (mut utc, mut local, mut local_file) = (SYSTEMTIME::default(), SYSTEMTIME::default(), FILETIME::default());
    // SAFETY: each call reads and writes stack structs that outlive it.
    unsafe {
        if FileTimeToSystemTime(&to_file_time(utc_ticks), &mut utc).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err()
            || SystemTimeToFileTime(&local, &mut local_file).is_err()
        {
            return 0;
        }
    }
    let local_ticks = u64::from(local_file.dwHighDateTime) << 32 | u64::from(local_file.dwLowDateTime);
    ((local_ticks as i64 - utc_ticks as i64) / 10_000_000) as i32
}

/// Whether the user's locale writes times on a 12-hour clock, as "2:30 PM".
pub fn hour12() -> bool {
    let mut value = [0u16; 2];
    // SAFETY: the buffer outlives the call, and a null locale name means the user's default locale.
    let written = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_ITIME, Some(&mut value)) };
    written > 0 && value[0] == u16::from(b'0')
}

/// Replaces the file in one rename, so a reader sees the old bytes or the new ones, never a torn write.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}
