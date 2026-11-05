//! What the check sees and does on the real desktop: windows, z-order, foreground, real mouse input and screenshots.

use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOT, GW_HWNDNEXT, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetSystemMetrics, GetTopWindow, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos,
    SetForegroundWindow, WindowFromPoint,
};
use windows::core::BOOL;

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn center(&self) -> POINT {
        POINT { x: (self.x + self.width / 2.0) as i32, y: (self.y + self.height / 2.0) as i32 }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x && other.y >= self.y && other.max_x() <= self.max_x() && other.max_y() <= self.max_y()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.max_x() && other.x < self.max_x() && self.y < other.max_y() && other.y < self.max_y()
    }

    /// Within a pixel on every edge.
    pub fn same(&self, other: &Rect) -> bool {
        [(self.x, other.x), (self.y, other.y), (self.width, other.width), (self.height, other.height)]
            .iter()
            .all(|(a, b)| (a - b).abs() < 1.0)
    }

    /// Live price text changes the ticker's width, so it counts as unmoved when its top edge and any one of its
    /// left, center or right holds.
    pub fn same_spot(&self, other: &Rect) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() < 1.0;
        close(self.y, other.y)
            && (close(self.x, other.x)
                || close(self.x + self.width / 2.0, other.x + other.width / 2.0)
                || close(self.max_x(), other.max_x()))
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{},{} {}x{}", self.x, self.y, self.width, self.height)
    }
}

pub fn per_monitor_dpi() {
    // SAFETY: plain process-wide setting, made before any window exists.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

pub fn hwnd(raw: i64) -> HWND {
    HWND(raw as isize as *mut _)
}

pub fn foreground() -> HWND {
    // SAFETY: plain query.
    unsafe { GetForegroundWindow() }
}

pub fn bring_to_front(window: HWND) -> bool {
    // SAFETY: plain call; it fails rather than misbehaves if Windows refuses.
    let _ = unsafe { SetForegroundWindow(window) };
    std::thread::sleep(Duration::from_millis(300));
    foreground() == window
}

pub fn window_rect(window: HWND) -> Rect {
    let mut rect = RECT::default();
    // SAFETY: writes one RECT.
    let _ = unsafe { GetWindowRect(window, &mut rect) };
    Rect {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        width: f64::from(rect.right - rect.left),
        height: f64::from(rect.bottom - rect.top),
    }
}

pub fn is_visible(window: HWND) -> bool {
    // SAFETY: plain query.
    unsafe { IsWindowVisible(window) }.as_bool()
}

pub fn ex_style(window: HWND) -> u32 {
    // SAFETY: plain query.
    unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 }
}

pub fn owner_pid(window: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: writes one u32.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    pid
}

/// The top-level window that takes a click at `point`.
pub fn window_at(point: POINT) -> HWND {
    // SAFETY: plain queries.
    unsafe { GetAncestor(WindowFromPoint(point), GA_ROOT) }
}

/// Visible top-level windows from the top of the z-order down.
pub fn z_order() -> Vec<HWND> {
    let mut windows = Vec::new();
    // SAFETY: walks the window list with plain queries.
    unsafe {
        let mut window = GetTopWindow(None).unwrap_or_default();
        while !window.is_invalid() {
            if IsWindowVisible(window).as_bool() {
                windows.push(window);
            }
            window = GetWindow(window, GW_HWNDNEXT).unwrap_or_default();
        }
    }
    windows
}

/// Visible top-level windows of `pid` with window class `class`.
pub fn windows_of(pid: u32, class: &str) -> Vec<HWND> {
    unsafe extern "system" fn collect(window: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the Vec below, alive for the whole enumeration.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(window) };
        true.into()
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback only touches `all`, which outlives the call.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&raw mut all as isize)) };
    all.into_iter()
        .filter(|window| owner_pid(*window) == pid && is_visible(*window) && class_name(*window) == class)
        .collect()
}

fn class_name(window: HWND) -> String {
    let mut name = [0u16; 128];
    // SAFETY: fills the buffer.
    let length = unsafe { GetClassNameW(window, &mut name) };
    String::from_utf16_lossy(&name[..length.max(0) as usize])
}

/// Every running process whose executable is `path`.
pub fn processes_running(path: &Path) -> Vec<u32> {
    let mut found = Vec::new();
    // SAFETY: walks a process snapshot and closes every handle it opens.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return found };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID) {
                let mut buffer = [0u16; 1024];
                let mut length = buffer.len() as u32;
                let named = QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    windows::core::PWSTR(buffer.as_mut_ptr()),
                    &mut length,
                );
                if named.is_ok() && Path::new(&String::from_utf16_lossy(&buffer[..length as usize])) == path {
                    found.push(entry.th32ProcessID);
                }
                let _ = CloseHandle(process);
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    found
}

pub fn cursor() -> POINT {
    let mut point = POINT::default();
    // SAFETY: writes one POINT.
    let _ = unsafe { GetCursorPos(&mut point) };
    point
}

/// Real mouse input through `SendInput`, every event tagged so the app can tell it from a person's. It puts the
/// cursor back where it found it when dropped.
pub struct Mouse {
    tag: usize,
    home: POINT,
}

impl Mouse {
    pub fn new(tag: usize) -> Mouse {
        Mouse { tag, home: cursor() }
    }

    /// Clicks `point` only if one of `allowed` is the window there.
    pub fn click(&self, point: POINT, right: bool, allowed: &[HWND]) -> Result<(), String> {
        self.guard(point, allowed)?;
        self.move_to(point)?;
        let (down, up) = if right {
            (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
        } else {
            (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)
        };
        self.send(down, 0, 0, 0)?;
        std::thread::sleep(Duration::from_millis(40));
        self.send(up, 0, 0, 0)?;
        std::thread::sleep(Duration::from_millis(250));
        Ok(())
    }

    pub fn drag(&self, from: POINT, to: POINT, allowed: &[HWND]) -> Result<(), String> {
        self.guard(from, allowed)?;
        self.move_to(from)?;
        self.send(MOUSEEVENTF_LEFTDOWN, 0, 0, 0)?;
        std::thread::sleep(Duration::from_millis(60));
        const STEPS: i32 = 12;
        for step in 1..=STEPS {
            let at = POINT { x: from.x + (to.x - from.x) * step / STEPS, y: from.y + (to.y - from.y) * step / STEPS };
            self.move_to(at)?;
            std::thread::sleep(Duration::from_millis(15));
        }
        self.send(MOUSEEVENTF_LEFTUP, 0, 0, 0)?;
        std::thread::sleep(Duration::from_millis(300));
        Ok(())
    }

    /// Moves onto `point` from a few pixels left, as a hand reaching for the wheel would, then turns the wheel `times`
    /// times by `delta` (`WHEEL_DELTA` is one notch; positive is away from the user).
    pub fn wheel(&self, point: POINT, delta: i32, times: u32, allowed: &[HWND]) -> Result<(), String> {
        self.guard(point, allowed)?;
        self.move_to(POINT { x: point.x - 4, ..point })?;
        self.move_to(point)?;
        std::thread::sleep(Duration::from_millis(100));
        for _ in 0..times {
            self.send(MOUSEEVENTF_WHEEL, 0, 0, delta)?;
            std::thread::sleep(Duration::from_millis(80));
        }
        Ok(())
    }

    fn guard(&self, point: POINT, allowed: &[HWND]) -> Result<(), String> {
        let there = window_at(point);
        if allowed.contains(&there) {
            Ok(())
        } else {
            Err(format!(
                "refused to click ({}, {}): the window there is {:?}, not one of the check's",
                point.x, point.y, there.0
            ))
        }
    }

    fn move_to(&self, point: POINT) -> Result<(), String> {
        // SAFETY: plain metrics queries.
        let (left, top, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let normalize = |value: i32, origin: i32, extent: i32| {
            ((i64::from(value - origin) * 65_536 + i64::from(extent) - 1) / i64::from(extent)) as i32
        };
        let flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        self.send(flags, normalize(point.x, left, width), normalize(point.y, top, height), 0)?;
        std::thread::sleep(Duration::from_millis(30));
        let landed = cursor();
        if (landed.x, landed.y) != (point.x, point.y) {
            return Err(format!(
                "the cursor is at ({}, {}), not ({}, {}): someone may be moving the mouse",
                landed.x, landed.y, point.x, point.y
            ));
        }
        Ok(())
    }

    fn send(&self, flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> Result<(), String> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: self.tag },
            },
        };
        // SAFETY: sends one fully initialized input.
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        if sent == 1 { Ok(()) } else { Err("SendInput was blocked".to_string()) }
    }
}

impl Drop for Mouse {
    fn drop(&mut self) {
        // SAFETY: plain call.
        let _ = unsafe { SetCursorPos(self.home.x, self.home.y) };
    }
}

/// What a screenshot shows, measured from its pixels.
pub struct Picture {
    pub distinct_colors: usize,
    /// Pixels close to the default up (green) and down (red) colors.
    pub up: usize,
    pub down: usize,
}

/// Saves the screen inside `rect` as a PNG and measures it.
pub fn capture(rect: &Rect, path: &Path) -> std::io::Result<Picture> {
    let (width, height) = (rect.width as i32, rect.height as i32);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    // SAFETY: copies the screen into a DIB this block creates and frees. CAPTUREBLT includes layered windows.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let previous = SelectObject(memory, bitmap.into());
        let copied =
            BitBlt(memory, 0, 0, width, height, Some(screen), rect.x as i32, rect.y as i32, SRCCOPY | CAPTUREBLT);
        let _ = GdiFlush();
        let pixels = std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize);
        for bgra in pixels.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[bgra[2], bgra[1], bgra[0]]);
        }
        SelectObject(memory, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        copied.map_err(|error| std::io::Error::other(error.to_string()))?;
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.write_header().and_then(|mut writer| writer.write_image_data(&rgb)).map_err(std::io::Error::other)?;

    let mut colors = std::collections::HashSet::new();
    let (mut up, mut down) = (0, 0);
    for pixel in rgb.as_chunks::<3>().0 {
        let [red, green, blue] = pixel.map(i32::from);
        colors.insert((red >> 4) << 8 | (green >> 4) << 4 | blue >> 4);
        if green > red + 60 && green > blue + 40 {
            up += 1;
        }
        if red > green + 80 && red > blue + 80 {
            down += 1;
        }
    }
    Ok(Picture { distinct_colors: colors.len(), up, down })
}

pub fn child(parent: HWND, class: &str, text: &str) -> Option<HWND> {
    let (class, text) = (crate::wide(class), crate::wide(text));
    // SAFETY: plain query with strings that outlive it.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::FindWindowExW(
            Some(parent),
            None,
            windows::core::PCWSTR(class.as_ptr()),
            windows::core::PCWSTR(text.as_ptr()),
        )
    }
    .ok()
}

pub fn post_close(window: HWND) {
    // SAFETY: plain post; a window that is already gone ignores it.
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(window),
            windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
            WPARAM(0),
            LPARAM(0),
        )
    };
}

/// The window's monitor DPI over 96.
pub fn scale(window: HWND) -> f64 {
    // SAFETY: plain query.
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(window) };
    if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
}

/// Where Windows sends wheel input: `MOUSEWHEEL_ROUTING_MOUSE_POS` (2) is "Scroll inactive windows when I hover over
/// them", `MOUSEWHEEL_ROUTING_FOCUS` (0) the focused window. Setting it lasts until the value is dropped, and never
/// reaches the user's saved settings.
pub struct WheelRouting {
    saved: u32,
}

impl WheelRouting {
    pub fn current() -> Option<u32> {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETMOUSEWHEELROUTING, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };
        let mut routing = 0u32;
        // SAFETY: writes one u32.
        unsafe {
            SystemParametersInfoW(
                SPI_GETMOUSEWHEELROUTING,
                0,
                Some((&raw mut routing).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()
        .map(|()| routing)
    }

    /// Switches to `routing` for this session, or None when Windows refuses or the switch does not read back.
    pub fn set(routing: u32) -> Option<WheelRouting> {
        let saved = Self::current()?;
        let guard = WheelRouting { saved };
        apply_routing(routing);
        (Self::current() == Some(routing)).then_some(guard)
    }
}

impl Drop for WheelRouting {
    fn drop(&mut self) {
        apply_routing(self.saved);
    }
}

/// No SPIF_UPDATEINIFILE, so the change stays in memory for this session and the registry keeps the user's choice.
fn apply_routing(routing: u32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_SETMOUSEWHEELROUTING, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    // SAFETY: SPI_SETMOUSEWHEELROUTING takes the value itself in pvParam.
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_SETMOUSEWHEELROUTING,
            0,
            Some(routing as usize as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
}

pub fn is_topmost(window: HWND) -> bool {
    ex_style(window) & windows::Win32::UI::WindowsAndMessaging::WS_EX_TOPMOST.0 != 0
}

pub fn primary_monitor() -> (Rect, Rect) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    let mut work = RECT::default();
    // SAFETY: plain queries; the second fills one RECT.
    let (width, height) = unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut work).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    };
    let frame = Rect { x: 0.0, y: 0.0, width: f64::from(width), height: f64::from(height) };
    let work = Rect {
        x: f64::from(work.left),
        y: f64::from(work.top),
        width: f64::from(work.right - work.left),
        height: f64::from(work.bottom - work.top),
    };
    (frame, work)
}

/// Where Windows shows the notification-area icon of `pid`'s hidden window of class `owner_class`: the icon itself, or
/// the overflow chevron while the icon sits in the overflow. None when no such icon is registered.
pub fn tray_icon(pid: u32, owner_class: &str) -> Option<Rect> {
    use windows::Win32::UI::Shell::{NOTIFYICONIDENTIFIER, Shell_NotifyIconGetRect};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    let class = crate::wide(owner_class);
    // SAFETY: plain queries with structs and strings that outlive them.
    unsafe {
        let owner = FindWindowW(windows::core::PCWSTR(class.as_ptr()), windows::core::PCWSTR::null())
            .ok()
            .filter(|owner| owner_pid(*owner) == pid)?;
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: owner,
            uID: 1,
            ..Default::default()
        };
        let rect = Shell_NotifyIconGetRect(&identifier).ok()?;
        Some(Rect {
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            width: f64::from(rect.right - rect.left),
            height: f64::from(rect.bottom - rect.top),
        })
    }
}
