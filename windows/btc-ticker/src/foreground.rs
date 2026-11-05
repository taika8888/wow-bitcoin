//! Tracks which process owns the foreground window, which decides whether the overlay shows.

use ticker_core::Frontmost;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT,
};

use crate::shell::{Event, deliver};
use crate::win;

/// Out of context, the hook runs on this thread's message loop, 0 to 47 ms after the switch as measured on this PC.
pub fn watch() -> HWINEVENTHOOK {
    // Windows sends this event even for a SetForegroundWindow it refuses, naming the window that asked, so the event's
    // window is only a hint that the foreground may have moved. Measured on windows-latest: 11 of 11 refusals.
    unsafe extern "system" fn changed(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
        deliver(Event::Foreground(current()));
    }
    // SAFETY: the callback is a plain function that lives for the whole process.
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    }
}

pub fn current() -> Frontmost {
    // SAFETY: plain query.
    classify(unsafe { GetForegroundWindow() })
}

fn classify(hwnd: HWND) -> Frontmost {
    let mut pid = 0;
    // SAFETY: writes one u32; a stale window leaves it 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let executable = win::process_image(pid).unwrap_or_default();
    Frontmost::classify(&executable, pid, std::process::id())
}
