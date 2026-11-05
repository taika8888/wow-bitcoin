//! Win32 fact gathering and input for the harness. Nothing here decides a verdict.
use std::sync::OnceLock;
use std::time::Instant;
use ticker_core::{Rect, SEEDED_SETTINGS_KEY, SETTINGS_VALUE, SYNTHETIC_INPUT_TAG, Time};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromWindow, ROP_CODE, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetUserObjectInformationW,
    OpenInputDesktop, UOI_NAME,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    GetWindowDpiAwarenessContext, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetCursorPos,
    GetForegroundWindow, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WindowFromPoint,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

/// Milliseconds since the harness started, shared by the watcher thread and the run.
pub fn ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub fn set_dpi_aware() {
    // SAFETY: no pointers; called before any window exists.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

pub fn hwnd(value: u64) -> HWND {
    HWND(value as usize as *mut _)
}

pub fn id(hwnd: HWND) -> u64 {
    hwnd.0 as usize as u64
}

pub fn screen() -> (i32, i32) {
    // SAFETY: no preconditions.
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
    // SAFETY: `list` is the `&mut Vec<HWND>` passed for the duration of the enumeration.
    unsafe { (*(list.0 as *mut Vec<HWND>)).push(hwnd) };
    BOOL(1)
}

/// Top-level windows from the top of the z-order down.
pub fn top_level() -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    // SAFETY: `list` outlives the call and `collect` only pushes into it.
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<HWND> as isize)) };
    list
}

pub fn children(parent: HWND) -> Vec<HWND> {
    let mut list: Vec<HWND> = Vec::new();
    // SAFETY: as above.
    let _ = unsafe {
        EnumChildWindows(
            Some(parent),
            Some(collect),
            LPARAM(&mut list as *mut Vec<HWND> as isize),
        )
    };
    list
}

pub fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0;
    // SAFETY: `pid` is valid for the call; any handle is tolerated.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

pub fn own_pid() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentProcessId() }
}

pub fn class_of(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: `buffer` is valid for the call.
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

pub fn text_of(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: `buffer` is valid for the call.
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

pub fn style(hwnd: HWND) -> u32 {
    // SAFETY: any handle is tolerated.
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 }
}

pub fn ex_style(hwnd: HWND) -> u32 {
    // SAFETY: any handle is tolerated.
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
}

pub fn foreground() -> HWND {
    // SAFETY: no preconditions.
    unsafe { GetForegroundWindow() }
}

pub fn visible(hwnd: HWND) -> bool {
    // SAFETY: any handle is tolerated.
    unsafe { IsWindowVisible(hwnd) }.as_bool()
}

fn rect(r: RECT) -> Rect {
    Rect {
        x: f64::from(r.left),
        y: f64::from(r.top),
        width: f64::from(r.right - r.left),
        height: f64::from(r.bottom - r.top),
    }
}

pub fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    // SAFETY: `r` is valid for the call.
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some(rect(r))
}

/// The monitor rect and work area of the monitor the window is on.
pub fn monitor_of(hwnd: HWND) -> Option<(Rect, Rect)> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is sized and valid for the call.
    let ok =
        unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) };
    ok.as_bool()
        .then(|| (rect(info.rcMonitor), rect(info.rcWork)))
}

pub fn window_at(x: i32, y: i32) -> HWND {
    // SAFETY: plain value.
    unsafe { WindowFromPoint(POINT { x, y }) }
}

pub fn per_monitor_v2(hwnd: HWND) -> bool {
    // SAFETY: plain handle and context values.
    unsafe {
        AreDpiAwarenessContextsEqual(
            GetWindowDpiAwarenessContext(hwnd),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    }
    .as_bool()
}

pub fn session_id() -> Option<u32> {
    let mut session = 0;
    // SAFETY: `session` is valid for the call.
    unsafe { ProcessIdToSessionId(own_pid(), &mut session) }.ok()?;
    Some(session)
}

pub fn input_desktop() -> Option<String> {
    // SAFETY: the desktop handle is closed below; `buffer` is valid for the call.
    unsafe {
        let desk = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS).ok()?;
        let mut buffer = [0u16; 128];
        let named = GetUserObjectInformationW(
            HANDLE(desk.0),
            UOI_NAME,
            Some(buffer.as_mut_ptr().cast()),
            (buffer.len() * 2) as u32,
            None,
        );
        let _ = CloseDesktop(desk);
        named.ok()?;
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }
}

pub fn cursor() -> Option<(i32, i32)> {
    let mut at = POINT::default();
    // SAFETY: `at` is valid for the call.
    unsafe { GetCursorPos(&mut at) }.ok()?;
    Some((at.x, at.y))
}

#[derive(Clone, Copy, Debug)]
pub enum Button {
    Left,
    Right,
}

/// A virtual-screen pixel in SendInput's 0..=65535 absolute range. The runner probe measured this mapping landing
/// GetCursorPos exactly on 53 of 53 targets, where the rounding over width - 1 missed 2.
fn normalized(p: i32, origin: i32, extent: i32) -> i32 {
    ((i64::from(p - origin) * 65536 + 32768) / i64::from(extent.max(1))) as i32
}

fn mouse(x: i32, y: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    // SAFETY: no preconditions.
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: normalized(x, vx, vw),
                dy: normalized(y, vy, vh),
                mouseData: 0,
                dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: SYNTHETIC_INPUT_TAG,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    // SAFETY: `inputs` is a valid slice for the call and the size is INPUT's.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!(
            "SendInput inserted {sent} of {} events: {}",
            inputs.len(),
            windows::core::Error::from_thread()
        ))
    }
}

pub fn move_to(x: i32, y: i32) -> Result<(), String> {
    send(&[mouse(x, y, MOUSEEVENTF_MOVE)])
}

/// Move, press and release in one SendInput call, so nothing interleaves.
pub fn click(x: i32, y: i32, button: Button) -> Result<(), String> {
    let (down, up) = match button {
        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
    };
    send(&[
        mouse(x, y, MOUSEEVENTF_MOVE),
        mouse(x, y, down),
        mouse(x, y, up),
    ])
}

pub struct Shot {
    pub width: i32,
    pub height: i32,
    /// Top-down BGRA.
    pub bgra: Vec<u8>,
}

impl Shot {
    pub fn rgb_at(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y * self.width + x) * 4) as usize;
        Some([self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]])
    }

    pub fn write_png(&self, path: &std::path::Path) -> Result<(), String> {
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            self.width as u32,
            self.height as u32,
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let rgb: Vec<u8> = self
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect();
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgb).map_err(|e| e.to_string())
    }
}

/// What the compositor shows in a screen rect: BitBlt from the screen DC with CAPTUREBLT, which includes layered
/// windows. PrintWindow would ask the app to repaint and prove nothing about the screen.
pub fn capture(area: &Rect) -> Result<Shot, String> {
    let (x, y) = (area.x as i32, area.y as i32);
    let (width, height) = (area.width as i32, area.height as i32);
    if width <= 0 || height <= 0 {
        return Err(format!("empty capture rect {width}x{height}"));
    }
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
    // SAFETY: the DCs and the bitmap are released below; `bits` is valid for width * height * 4 bytes until the
    // bitmap is deleted, and is copied out before that.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        {
            Err(e) => Err(format!("CreateDIBSection: {e}")),
            Ok(bitmap) => {
                let previous = SelectObject(memory, bitmap.into());
                let copied = BitBlt(
                    memory,
                    0,
                    0,
                    width,
                    height,
                    Some(screen),
                    x,
                    y,
                    ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
                );
                let bgra =
                    std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize)
                        .to_vec();
                SelectObject(memory, previous);
                let _ = DeleteObject(bitmap.into());
                copied
                    .map(|_| Shot {
                        width,
                        height,
                        bgra,
                    })
                    .map_err(|e| format!("BitBlt: {e}"))
            }
        };
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        result
    }
}

/// The harness's own fetch of Kraken's last trade price, independent of the app's.
pub fn kraken_price() -> Result<f64, String> {
    let (status, body) = https_get(w!("api.kraken.com"), w!("/0/public/Ticker?pair=XBTUSD"))?;
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    ticker_core::kraken::parse_quote(&body, Time(unix_now()))
        .map(|quote| quote.price)
        .map_err(|e| format!("{e:?}"))
}

fn https_get(host: PCWSTR, path: PCWSTR) -> Result<(u32, Vec<u8>), String> {
    // SAFETY: every handle opened here is closed before returning; every buffer outlives its call.
    unsafe {
        let session = WinHttpOpen(
            w!("btc-ticker-e2e"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err(format!(
                "WinHttpOpen: {}",
                windows::core::Error::from_thread()
            ));
        }
        let _ = WinHttpSetTimeouts(session, 10_000, 10_000, 10_000, 10_000);
        let connection = WinHttpConnect(session, host, INTERNET_DEFAULT_HTTPS_PORT, 0);
        let result = if connection.is_null() {
            Err(format!(
                "WinHttpConnect: {}",
                windows::core::Error::from_thread()
            ))
        } else {
            let request = WinHttpOpenRequest(
                connection,
                w!("GET"),
                path,
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            );
            let result = if request.is_null() {
                Err(format!(
                    "WinHttpOpenRequest: {}",
                    windows::core::Error::from_thread()
                ))
            } else {
                let result = exchange(request);
                let _ = WinHttpCloseHandle(request);
                result
            };
            let _ = WinHttpCloseHandle(connection);
            result
        };
        let _ = WinHttpCloseHandle(session);
        result
    }
}

/// # Safety
/// `request` must be an open WinHTTP request handle.
unsafe fn exchange(request: *mut core::ffi::c_void) -> Result<(u32, Vec<u8>), String> {
    // SAFETY: `request` is open per the contract; every out-parameter is valid for its call.
    unsafe {
        WinHttpSendRequest(request, None, None, 0, 0, 0).map_err(|e| format!("send: {e}"))?;
        WinHttpReceiveResponse(request, std::ptr::null_mut())
            .map_err(|e| format!("receive: {e}"))?;
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut length,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("status: {e}"))?;
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request, &mut available).map_err(|e| format!("size: {e}"))?;
            if available == 0 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(request, chunk.as_mut_ptr().cast(), available, &mut read)
                .map_err(|e| format!("read: {e}"))?;
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok((status, body))
    }
}

/// The seeded run's settings value, as the app reads and writes it.
pub fn write_settings(json: &str) -> Result<(), String> {
    let wide: Vec<u16> = json.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` outlives the call and its byte length is passed.
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(SEEDED_SETTINGS_KEY),
            &HSTRING::from(SETTINGS_VALUE),
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
    }
    .ok()
    .map_err(|e| e.to_string())
}

pub fn read_settings() -> Option<String> {
    let mut buffer = vec![0u16; 4096];
    let mut length = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` holds `length` bytes and outlives the call.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(SEEDED_SETTINGS_KEY),
            &HSTRING::from(SETTINGS_VALUE),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut length),
        )
    }
    .ok()
    .ok()?;
    buffer.truncate(length as usize / 2);
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

pub fn delete_settings() {
    // SAFETY: plain key path.
    let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(SEEDED_SETTINGS_KEY)) };
}

pub fn close(handle: HANDLE) {
    // SAFETY: the caller owns `handle` and does not use it afterwards.
    let _ = unsafe { CloseHandle(handle) };
}
