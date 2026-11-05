//! The foreground timeline, on its own thread: an EVENT_SYSTEM_FOREGROUND hook plus a 20 ms poll. Both record what
//! GetForegroundWindow returns, never the window an event names, because refused SetForegroundWindow calls fire the
//! event too.
use btc_ticker_e2e::judge::Sample;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    WINEVENT_OUTOFCONTEXT,
};

static TIMELINE: Mutex<Vec<Sample>> = Mutex::new(Vec::new());
static STOP: AtomicBool = AtomicBool::new(false);

pub struct Watcher {
    thread: Option<JoinHandle<()>>,
}

pub fn start() -> Watcher {
    record();
    let thread = std::thread::spawn(|| {
        // SAFETY: `on_foreground` matches WINEVENTPROC; the hook is removed on this thread before it ends, and this
        // thread pumps its own messages, which is where out-of-context events arrive.
        unsafe {
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            let mut msg = MSG::default();
            while !STOP.load(Ordering::SeqCst) {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                record();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let _ = UnhookWinEvent(hook);
        }
    });
    Watcher {
        thread: Some(thread),
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
    record();
}

fn record() {
    let hwnd = super::os::foreground();
    // No foreground window is a moment mid-switch, not a change.
    if hwnd.is_invalid() {
        return;
    }
    let Ok(mut timeline) = TIMELINE.lock() else {
        return;
    };
    let id = super::os::id(hwnd);
    if timeline.last().is_none_or(|last| last.hwnd != id) {
        timeline.push(Sample {
            at_ms: super::os::ms(),
            hwnd: id,
            pid: super::os::pid_of(hwnd),
        });
    }
}

impl Watcher {
    pub fn timeline(&self) -> Vec<Sample> {
        record();
        TIMELINE.lock().map(|t| t.clone()).unwrap_or_default()
    }

    pub fn saw(&self, hwnd: HWND) -> bool {
        let id = super::os::id(hwnd);
        self.timeline().iter().any(|sample| sample.hwnd == id)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        STOP.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
