//! EVENT_SYSTEM_FOREGROUND also fires for refused SetForegroundWindow calls, naming a window that never came to the
//! front, so every event is confirmed with GetForegroundWindow before it counts.
use std::cell::Cell;
use ticker_core::{Frontmost, Input};
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT,
};
use windows::core::PWSTR;

thread_local! {
    static LAST: Cell<HWND> = Cell::new(HWND::default());
}

pub(super) fn current() -> Frontmost {
    // SAFETY: no preconditions.
    let hwnd = unsafe { GetForegroundWindow() };
    LAST.set(hwnd);
    classify(hwnd)
}

/// Out of context, so callbacks arrive through this thread's message loop. Not WINEVENT_SKIPOWNPROCESS: our own
/// Settings window must classify as Ticker.
pub(super) fn hook() -> HWINEVENTHOOK {
    // SAFETY: `on_foreground` matches WINEVENTPROC and lives for the whole process.
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    }
}

unsafe extern "system" fn on_foreground(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    recheck();
}

/// Also run by the host's watch timer, so an event that arrived before the switch settled is not lost.
pub(super) fn recheck() {
    // SAFETY: no preconditions.
    let hwnd = unsafe { GetForegroundWindow() };
    // No foreground window is a moment mid-switch, not a change.
    if hwnd.is_invalid() || hwnd == LAST.get() {
        return;
    }
    LAST.set(hwnd);
    super::dispatch(Input::Foreground(classify(hwnd)));
    // The newly active app may have just raised its own topmost window.
    super::overlay::raise();
}

fn classify(hwnd: HWND) -> Frontmost {
    let mut pid = 0;
    // SAFETY: `pid` is valid for the call; any handle is tolerated.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    // SAFETY: no preconditions.
    if pid == unsafe { GetCurrentProcessId() } {
        return Frontmost::classify(None, true);
    }
    Frontmost::classify(image_path(pid).as_deref(), false)
}

/// None when the process cannot be opened, such as an elevated launcher; that classifies as Other.
fn image_path(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning; `buffer` and `length` are valid for the calls.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        queried.ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}
