//! The shell: one UI thread, one `App`, one message loop. `App` is borrowed only inside short closures and never
//! across a Win32 call: SetWindowPos, UpdateLayeredWindow, ReleaseCapture and the modal loops of ChooseColor and the
//! tray menu send messages that re-enter a window procedure, which may dispatch again.
mod display;
mod foreground;
mod host;
mod net;
mod overlay;
mod settings_ui;
mod store;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use ticker_core::{App, Effects, Env, Input, ShellFacts, Snapshot, Time};
use ticker_paint::Fonts;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::UnhookWinEvent;
use windows::Win32::UI::Controls::{
    ICC_BAR_CLASSES, ICC_STANDARD_CLASSES, ICC_UPDOWN_CLASS, INITCOMMONCONTROLSEX,
    InitCommonControlsEx,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsDialogMessageW, MSG, PostQuitMessage, TranslateMessage,
};
use windows::core::HSTRING;

struct Shell {
    app: App<&'static Fonts>,
    fonts: &'static Fonts,
    env: Env,
    net: net::Fetcher,
    store: store::Store,
    state_file: Option<PathBuf>,
    /// The last snapshot written, with `seq` zeroed, so an unchanged state is not rewritten.
    published: Option<Snapshot>,
}

thread_local! {
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
    static FACTS: Cell<ShellFacts> = Cell::new(ShellFacts::default());
}

fn with_shell<R>(f: impl FnOnce(&mut Shell) -> R) -> Option<R> {
    SHELL.with(|shell| shell.borrow_mut().as_mut().map(f))
}

fn update_facts(change: impl FnOnce(&mut ShellFacts)) {
    FACTS.with(|facts| {
        let mut next = facts.get();
        change(&mut next);
        facts.set(next);
    });
}

pub fn run() -> i32 {
    match start() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("btc-ticker could not start: {error}");
            1
        }
    }
}

fn start() -> windows::core::Result<i32> {
    let seed = std::env::var("BTCTICKER_SETTINGS").ok();
    let key = if seed.is_some() {
        ticker_core::SEEDED_SETTINGS_KEY
    } else {
        ticker_core::SETTINGS_KEY
    };
    // One ticker per settings store: two would fight over the same value. Mutex names allow no backslash after
    // the namespace prefix.
    let name = HSTRING::from(format!("Local\\BTCTicker {}", key.replace('\\', "/")));
    // SAFETY: plain arguments; the handle stays open until the process exits, which is what holds the name.
    let _instance: HANDLE = unsafe { CreateMutexW(None, false, &name) }?;
    // SAFETY: reads this thread's last error, set by CreateMutexW just above.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(0);
    }

    let store = store::Store::open(key, seed.as_deref())?;
    let fonts: &'static Fonts = Box::leak(Box::new(Fonts::embedded()));
    let controls = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_BAR_CLASSES | ICC_UPDOWN_CLASS | ICC_STANDARD_CLASSES,
    };
    // SAFETY: `controls` is a valid, sized struct for the duration of the call.
    let _ = unsafe { InitCommonControlsEx(&controls) };

    let host = host::create(fonts)?;
    let overlay = overlay::create()?;
    let tray_icon = host::add_tray_icon();
    update_facts(|facts| {
        facts.hwnd = overlay.0 as u64;
        facts.ex_style = overlay::EX_STYLE.0;
        facts.tray_icon = tray_icon;
    });
    let app = App::new(store.load().as_deref(), foreground::current(), now(), fonts);
    let net = net::Fetcher::spawn(host);
    store.watch(host);
    let hook = foreground::hook();
    host::start_watch_timer();
    SHELL.with(|shell| {
        *shell.borrow_mut() = Some(Shell {
            app,
            fonts,
            env: display::current(),
            net,
            store,
            state_file: std::env::var_os("BTCTICKER_STATE_FILE").map(PathBuf::from),
            published: None,
        })
    });
    dispatch(Input::Tick);

    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid MSG for every call; GetMessageW returns 0 on WM_QUIT and -1 on error.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if let Some(settings) = settings_ui::hwnd() {
            // SAFETY: `settings` is our live Settings window and `msg` was just filled by GetMessageW.
            if unsafe { IsDialogMessageW(settings, &msg) }.as_bool() {
                continue;
            }
        }
        // SAFETY: as above.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    host::remove_tray_icon();
    // SAFETY: `hook` came from SetWinEventHook on this thread.
    let _ = unsafe { UnhookWinEvent(hook) };
    Ok(0)
}

/// The only path from Win32 into `App`.
fn dispatch(input: Input) {
    let effects = SHELL.with(|shell| {
        let mut shell = shell.borrow_mut();
        let shell = shell.as_mut()?;
        Some(shell.app.handle(input, now(), &shell.env))
    });
    if let Some(effects) = effects {
        apply(effects);
    }
}

/// Monitors, DPI, the work area or the clock's offset changed: read them again and let `App` re-lay out.
fn display_changed() {
    let env = display::current();
    with_shell(|shell| shell.env = env);
    dispatch(Input::DisplayChanged);
}

fn apply(effects: Effects) {
    let Effects {
        fetch_quote,
        fetch_candles,
        save_settings,
        present,
        capture,
        open_settings,
        quit,
        wake_in,
    } = effects;
    let painted = with_shell(|shell| {
        if let Some(request) = fetch_quote {
            shell.net.quote(request);
        }
        if let Some((timeframe, request)) = fetch_candles {
            shell.net.candles(timeframe, request);
        }
        if let Some(json) = &save_settings {
            shell.store.save(json);
        }
        present.map(|frame| {
            let pixels = ticker_paint::paint(&frame, shell.fonts);
            (frame, pixels)
        })
    })
    .flatten();
    if let Some((frame, pixels)) = painted {
        overlay::present(&frame, &pixels);
    }
    if let Some(take) = capture {
        overlay::capture(take);
    }
    if open_settings {
        settings_ui::open();
    }
    if let Some(settings) = with_shell(|shell| shell.app.settings().clone()) {
        settings_ui::sync(&settings);
    }
    host::arm_tick(wake_in);
    publish();
    if quit {
        // SAFETY: no arguments; ends the message loop on this thread.
        unsafe { PostQuitMessage(0) };
    }
}

/// Writes the snapshot to `BTCTICKER_STATE_FILE` when anything in it changed. Replaced with a rename, so the
/// harness never reads half a file.
fn publish() {
    let facts = FACTS.with(Cell::get);
    with_shell(|shell| {
        let Some(path) = shell.state_file.clone() else {
            return;
        };
        let snapshot = shell.app.snapshot(facts);
        let unchanged = Snapshot {
            seq: 0,
            ..snapshot.clone()
        };
        if shell.published.as_ref() == Some(&unchanged) {
            return;
        }
        let staging = path.with_extension("tmp");
        let written = std::fs::write(&staging, snapshot.to_json()).is_ok()
            && (0..5).any(|attempt| {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                std::fs::rename(&staging, &path).is_ok()
            });
        if written {
            shell.published = Some(unchanged);
        }
    });
}

fn now() -> Time {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Time(since.as_secs_f64())
}
