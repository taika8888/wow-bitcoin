//! A stand-in for the game: this same executable, copied to `Wow.exe` so the ticker takes it for World of Warcraft,
//! showing one plain dark window. A copy under any other name stands in for some other app.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect,
    PAINTSTRUCT, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW, GetWindowTextW, IDC_ARROW,
    LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SetForegroundWindow, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WM_DESTROY, WM_MOUSEWHEEL, WM_PAINT, WNDCLASSW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::desktop::{self, Rect};

pub struct StandIn {
    child: Child,
    pub hwnd: HWND,
    wheel: Arc<AtomicU32>,
}

impl StandIn {
    /// Starts `exe` as a stand-in window over `rect` and waits for its window.
    pub fn spawn(exe: &Path, rect: &Rect, title: &str) -> Result<StandIn, String> {
        let mut child = Command::new(exe)
            .args([
                "standin",
                &rect.x.to_string(),
                &rect.y.to_string(),
                &rect.width.to_string(),
                &rect.height.to_string(),
                title,
            ])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", exe.display()))?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (sender, receiver) = mpsc::channel();
        let wheel = Arc::new(AtomicU32::new(0));
        let turns = Arc::clone(&wheel);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(raw) = line.strip_prefix("HWND ").and_then(|raw| raw.parse::<i64>().ok()) {
                    let _ = sender.send(raw);
                } else if line == "WHEEL" {
                    turns.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(raw) => {
                std::thread::sleep(Duration::from_millis(300));
                Ok(StandIn { child, hwnd: desktop::hwnd(raw), wheel })
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(format!("{} showed no window within 10 s", exe.display()))
            }
        }
    }
}

impl StandIn {
    /// How many `WM_MOUSEWHEEL` messages the stand-in's window has received.
    pub fn wheel_messages(&self) -> u32 {
        self.wheel.load(Ordering::Relaxed)
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        desktop::post_close(self.hwnd);
        for _ in 0..40 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The stand-in process: `standin <x> <y> <width> <height> <title>`.
pub fn run(args: &[String]) {
    let number = |index: usize| args.get(index).and_then(|value| value.parse::<f64>().ok()).unwrap_or(0.0) as i32;
    let title = crate::wide(args.get(4).map_or("stand-in", String::as_str));
    // SAFETY: registers a class and runs a window for the life of this process.
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("BTCTickerE2EStandIn"),
            ..Default::default()
        });
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BTCTickerE2EStandIn"),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            number(0),
            number(1),
            number(2),
            number(3),
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        println!("HWND {}", hwnd.0 as isize);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: standard handling for this process's own window.
    unsafe {
        match message {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let backdrop = CreateSolidBrush(COLORREF(0x0030_2418));
                FillRect(dc, &client, backdrop);
                let _ = DeleteObject(backdrop.into());
                SetBkMode(dc, TRANSPARENT);
                SetTextColor(dc, COLORREF(0x00a0_a0a0));
                let mut title = [0u16; 128];
                let length = GetWindowTextW(hwnd, &mut title);
                DrawTextW(dc, &mut title[..length as usize], &mut client, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                println!("WHEEL");
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
