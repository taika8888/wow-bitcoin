//! The Win32 shell around the core. Window procedures and hooks turn what Windows reports into `Event`s; the shell
//! turns those into the core's `Input`s and runs the `Effect`s that come back, in order.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use ticker_core::{App, Effect, Frontmost, Input, OverlayEvent, Settings, Snapshot, SnapshotWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Shell::{NIN_SELECT, NINF_KEY};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, IsDialogMessageW, KillTimer, MSG, PBT_APMRESUMEAUTOMATIC, PostMessageW,
    PostQuitMessage, RegisterWindowMessageW, SM_CXSMICON, SPI_SETWORKAREA, SetTimer, TranslateMessage, WINDOW_EX_STYLE,
    WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DISPLAYCHANGE, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMER, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::foreground;
use crate::menu::{Command, Popup};
use crate::net::{Net, Request};
use crate::overlay::{Overlay, Pointer};
use crate::settings_store::SettingsStore;
use crate::settings_window::{self, SettingsWindow};
use crate::tray::{self, Tray};
use crate::win;

#[derive(Debug)]
pub enum Event {
    Pointer(Pointer),
    Foreground(Frontmost),
    Tick,
    Woke,
    /// A worker finished a request.
    Fetched,
    /// Something in the settings folder changed; wait for the writer to finish.
    SettingsFileTouched,
    SettingsFileSettled,
    /// Monitors, work areas or the overlay's DPI changed.
    Redraw,
    Command(Command),
    MenuClosed(Option<Command>),
    TrayClicked(POINT),
    TaskbarCreated,
    SettingsEdited(Settings),
    SettingsClosed,
}

/// Where the app keeps its files, and the hooks the end-to-end check uses.
pub struct Config {
    /// Names the main window, so a second launch on the same settings file can find this one.
    pub instance: String,
    pub settings_file: PathBuf,
    /// Settings to start from, replacing the file's.
    pub seed: Option<String>,
    /// Where to write a `Snapshot` after every change.
    pub state_file: Option<PathBuf>,
    /// The `dwExtraInfo` the end-to-end check puts on its clicks.
    pub input_tag: Option<isize>,
}

const WM_FETCHED: u32 = WM_APP + 1;
const WM_SETTINGS_FILE: u32 = WM_APP + 2;
const WM_TRAY: u32 = WM_APP + 3;
const TIMER_HEARTBEAT: usize = 1;
const TIMER_WAKE: usize = 2;
const TIMER_SETTINGS_FILE: usize = 3;
/// Long enough for an editor's truncate-then-write to finish.
const SETTINGS_FILE_SETTLE_MS: u32 = 100;
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;
const MAIN_CLASS: PCWSTR = w!("BTCTickerMain");
const OPEN_SETTINGS: PCWSTR = w!("BTCTicker.OpenSettings");
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static OPEN_SETTINGS_MESSAGE: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
}

/// Hands an event to the shell. Windows calls window procedures re-entrantly, for example `SetCapture` sends
/// `WM_CAPTURECHANGED` from inside a handler, so an event that arrives while the shell is busy waits in the queue and
/// runs right after the current one.
pub fn deliver(event: Event) {
    QUEUE.with_borrow_mut(|queue| queue.push_back(event));
    SHELL.with(|shell| {
        let Ok(mut shell) = shell.try_borrow_mut() else { return };
        let Some(shell) = shell.as_mut() else { return };
        while let Some(event) = QUEUE.with_borrow_mut(VecDeque::pop_front) {
            shell.on(event);
        }
    });
}

/// Asks the ticker already running as `instance` to open its Settings window. It may not have created its main
/// window yet, so this looks for up to 2 s.
pub fn open_settings_in(instance: &str) {
    let name = HSTRING::from(instance);
    for _ in 0..20 {
        // SAFETY: plain queries and a post to another process's window, with strings that outlive them.
        unsafe {
            if let Ok(main) = FindWindowW(MAIN_CLASS, &name) {
                let mut pid = 0;
                GetWindowThreadProcessId(main, Some(&mut pid));
                // Lets the running ticker bring Settings to the front, which Windows allows only if this launch may.
                let _ = AllowSetForegroundWindow(pid);
                let _ = PostMessageW(Some(main), RegisterWindowMessageW(OPEN_SETTINGS), WPARAM(0), LPARAM(0));
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

pub fn run(config: Config) -> Result<()> {
    let main = create_main_window(&config.instance)?;
    let (store, settings) = SettingsStore::open(config.settings_file, config.seed.as_deref());
    store.watch(main, WM_SETTINGS_FILE);
    let overlay = Overlay::create()?;
    // SAFETY: plain query.
    let icon_size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, (win::scale(main) * 96.0) as u32) };
    let tray = Tray::add(main, WM_TRAY, overlay.renderer().icon(icon_size)?);
    let shell = Shell {
        app: App::new(settings, foreground::current(), win::now()),
        net: Net::new(main, WM_FETCHED)?,
        main,
        overlay,
        store,
        tray,
        popup: None,
        settings_window: None,
        probe: Probe {
            state_file: config.state_file,
            input_tag: config.input_tag,
            activations: 0,
            foreign_presses: 0,
            written: String::new(),
        },
    };
    let _hook = foreground::watch();
    // SAFETY: a timer on this thread's own window.
    unsafe { SetTimer(Some(main), TIMER_HEARTBEAT, 1000, None) };
    SHELL.set(Some(shell));
    deliver(Event::Redraw);
    deliver(Event::Tick);

    let mut message = MSG::default();
    // SAFETY: the standard message loop; IsDialogMessageW gives the settings window Tab and arrow-key navigation.
    unsafe {
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if settings_window::current().is_some_and(|dialog| IsDialogMessageW(dialog, &message).as_bool()) {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    SHELL.take();
    Ok(())
}

struct Shell {
    app: App,
    /// A hidden window that owns the timers, the tray icon and the worker notifications, and hears broadcasts.
    main: HWND,
    overlay: Overlay,
    store: SettingsStore,
    net: Net,
    tray: Tray,
    popup: Option<Popup>,
    settings_window: Option<SettingsWindow>,
    probe: Probe,
}

/// What the end-to-end check reads.
struct Probe {
    state_file: Option<PathBuf>,
    input_tag: Option<isize>,
    activations: u32,
    foreign_presses: u32,
    written: String,
}

impl Shell {
    fn on(&mut self, event: Event) {
        match event {
            Event::Pointer(pointer) => self.pointer(pointer),
            Event::Foreground(frontmost) => {
                if frontmost == Frontmost::Ticker && self.app.overlay().frontmost() != Frontmost::Ticker {
                    self.probe.activations += 1;
                }
                self.keep_on_top();
                self.handle(Input::Overlay(OverlayEvent::AppActivated(frontmost)));
            }
            Event::Tick => {
                self.keep_on_top();
                self.handle(Input::Tick(win::now()));
            }
            Event::Woke => self.handle(Input::Woke(win::now())),
            Event::Fetched => {
                let finished: Vec<_> = self.net.finished().collect();
                for fetched in finished {
                    let now = win::now();
                    let outcome = fetched.outcome;
                    self.handle(match fetched.request {
                        Request::Quote(request) => Input::QuoteFinished { request, outcome, now },
                        Request::Candles(request, timeframe) => {
                            Input::CandlesFinished { request, timeframe, outcome, now }
                        }
                    });
                }
            }
            Event::SettingsFileTouched => {
                // SAFETY: a timer on this thread's own window; setting it again restarts it.
                unsafe { SetTimer(Some(self.main), TIMER_SETTINGS_FILE, SETTINGS_FILE_SETTLE_MS, None) };
            }
            Event::SettingsFileSettled => {
                // SAFETY: as above.
                let _ = unsafe { KillTimer(Some(self.main), TIMER_SETTINGS_FILE) };
                if let Some(settings) = self.store.changed() {
                    self.handle(Input::SettingsChanged(settings));
                }
            }
            Event::Redraw => self.render(),
            Event::Command(command) => self.command(command, foreground_window()),
            Event::MenuClosed(command) => {
                self.popup = None;
                if let Some(command) = command {
                    self.command(command, foreground_window());
                }
            }
            Event::TrayClicked(at) => {
                let before = foreground_window();
                if let Some(command) = tray::choose(self.main, at, self.app.settings().locked) {
                    self.command(command, before);
                }
            }
            Event::TaskbarCreated => self.tray.show(),
            Event::SettingsEdited(settings) => self.handle(Input::Overlay(OverlayEvent::SettingsEdited(settings))),
            Event::SettingsClosed => self.settings_window = None,
        }
        self.write_snapshot();
    }

    /// Another always-on-top window that raises itself sends no foreground event, so the heartbeat re-raises too.
    fn keep_on_top(&self) {
        self.overlay.raise();
        if let Some(popup) = &self.popup {
            popup.raise();
        }
    }

    fn pointer(&mut self, pointer: Pointer) {
        if let Pointer::Down { extra } | Pointer::RightDown { extra } = pointer
            && Some(extra) != self.probe.input_tag
        {
            self.probe.foreign_presses += 1;
        }
        if let Pointer::Wheel { delta, at } = pointer {
            for step in self.overlay.wheel(delta, at) {
                self.handle(Input::Overlay(OverlayEvent::Zoom(step)));
            }
            return;
        }
        if let Pointer::RightUp { at } = pointer {
            self.popup = None;
            match Popup::open(self.overlay.hwnd(), at, self.app.settings().locked, &win::screens()) {
                Ok(popup) => self.popup = Some(popup),
                Err(error) => eprintln!("could not open the menu: {error}"),
            }
            return;
        }
        if let Some(event) = self.overlay.pointer(pointer, &self.app) {
            self.handle(Input::Overlay(event));
            self.overlay.follow(self.app.overlay().drag());
        }
    }

    /// `foreground` is the window to hand focus back to when a window this command opens closes.
    fn command(&mut self, command: Command, foreground: Option<HWND>) {
        match command {
            Command::Settings => match &self.settings_window {
                Some(window) => window.focus(),
                None => match SettingsWindow::open(*self.app.settings(), foreground) {
                    Ok(window) => self.settings_window = Some(window),
                    Err(error) => eprintln!("could not open Settings: {error}"),
                },
            },
            Command::ToggleLock => self.handle(Input::Overlay(OverlayEvent::ToggleLock)),
            // SAFETY: ends this thread's message loop.
            Command::Quit => unsafe { PostQuitMessage(0) },
        }
    }

    fn handle(&mut self, input: Input) {
        for effect in self.app.handle(input) {
            match effect {
                Effect::SaveSettings(settings) => self.store.save(&settings),
                Effect::FetchQuote { request, url, timeout } => self.net.fetch(Request::Quote(request), url, timeout),
                Effect::FetchCandles { request, timeframe, url, timeout } => {
                    self.net.fetch(Request::Candles(request, timeframe), &url, timeout);
                }
                Effect::Render => self.render(),
            }
        }
        self.arm_wake();
    }

    fn render(&mut self) {
        if let Err(error) = self.overlay.draw(&self.app, &win::screens()) {
            eprintln!("could not draw the overlay: {error}");
        }
        if let Some(window) = &self.settings_window {
            window.sync(*self.app.settings());
        }
    }

    /// A one-shot timer for the core's next deadline, beside the 1 s heartbeat.
    fn arm_wake(&self) {
        // SAFETY: timers on this thread's own window.
        unsafe {
            match self.app.next_wake() {
                Some(at) => {
                    let millis = (at.seconds_since(win::now()) * 1000.0).ceil().clamp(10.0, f64::from(u32::MAX));
                    SetTimer(Some(self.main), TIMER_WAKE, millis as u32, None);
                }
                None => {
                    let _ = KillTimer(Some(self.main), TIMER_WAKE);
                }
            }
        }
    }

    fn write_snapshot(&mut self) {
        let Some(path) = &self.probe.state_file else { return };
        let hwnd = self.overlay.hwnd();
        let window = SnapshotWindow {
            hwnd: hwnd.0 as i64,
            frame: win::window_rect(hwnd),
            targets: self.overlay.targets(),
            max_bars: self.overlay.max_bars(),
            activations: self.probe.activations,
            foreign_presses: self.probe.foreign_presses,
        };
        let json = Snapshot::new(&self.app, window).json();
        if json != self.probe.written {
            match win::write_atomically(path, json.as_bytes()) {
                Ok(()) => self.probe.written = json,
                Err(error) => eprintln!("could not write {}: {error}", path.display()),
            }
        }
    }
}

/// The foreground window when it belongs to another process: where focus goes back to after Settings.
fn foreground_window() -> Option<HWND> {
    let mut pid = 0;
    // SAFETY: plain queries.
    unsafe {
        let hwnd = GetForegroundWindow();
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (!hwnd.is_invalid() && pid != std::process::id()).then_some(hwnd)
    }
}

fn create_main_window(instance: &str) -> Result<HWND> {
    win::register_class(MAIN_CLASS, Some(procedure), None);
    // SAFETY: registers message names and creates a window that is never shown.
    unsafe {
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        OPEN_SETTINGS_MESSAGE.store(RegisterWindowMessageW(OPEN_SETTINGS), Ordering::Relaxed);
        let name = HSTRING::from(instance);
        CreateWindowExW(WINDOW_EX_STYLE(0), MAIN_CLASS, &name, WS_OVERLAPPED, 0, 0, 0, 0, None, None, None, None)
    }
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let low_word = |value: usize| u32::from(value as u16);
    let event = match message {
        WM_TIMER => match wparam.0 {
            TIMER_WAKE => {
                // SAFETY: the wake timer is one-shot; App::next_wake re-arms it after the tick.
                let _ = unsafe { KillTimer(Some(hwnd), TIMER_WAKE) };
                Some(Event::Tick)
            }
            TIMER_HEARTBEAT => Some(Event::Tick),
            TIMER_SETTINGS_FILE => Some(Event::SettingsFileSettled),
            _ => None,
        },
        WM_FETCHED => Some(Event::Fetched),
        WM_SETTINGS_FILE => Some(Event::SettingsFileTouched),
        WM_TRAY => match low_word(lparam.0 as usize) {
            WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT => {
                let anchor = POINT { x: i32::from(wparam.0 as i16), y: i32::from((wparam.0 >> 16) as i16) };
                Some(Event::TrayClicked(anchor))
            }
            _ => None,
        },
        WM_POWERBROADCAST if wparam.0 == PBT_APMRESUMEAUTOMATIC as usize => Some(Event::Woke),
        WM_DISPLAYCHANGE => Some(Event::Redraw),
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => Some(Event::Redraw),
        WM_CLOSE => Some(Event::Command(Command::Quit)),
        _ if message == TASKBAR_CREATED.load(Ordering::Relaxed) => Some(Event::TaskbarCreated),
        _ if message == OPEN_SETTINGS_MESSAGE.load(Ordering::Relaxed) => Some(Event::Command(Command::Settings)),
        _ => None,
    };
    match event {
        Some(event) => {
            deliver(event);
            LRESULT(0)
        }
        // SAFETY: default handling.
        None => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
