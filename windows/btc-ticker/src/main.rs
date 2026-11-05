#![windows_subsystem = "windows"]
//! BTC Ticker for Windows: a live Bitcoin price floating above World of Warcraft that never takes focus from the game,
//! and a candlestick chart on click. `ticker-core` decides everything; this crate is the Win32 shell around it.

mod foreground;
mod menu;
mod net;
mod overlay;
mod render;
mod settings_store;
mod settings_window;
mod shell;
mod tray;
mod wheel;
mod win;

use std::path::PathBuf;

use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{PCWSTR, w};

fn main() {
    let config = config();
    let unattended = config.state_file.is_some();
    if let Err(error) = shell::run(config) {
        let message = format!("BTC Ticker could not start: {error}");
        eprintln!("{message}");
        if !unattended {
            let text = win::wide(&message);
            // SAFETY: a plain modal message box with strings that outlive it.
            unsafe { MessageBoxW(None, PCWSTR(text.as_ptr()), w!("BTC Ticker"), MB_OK | MB_ICONERROR) };
        }
        std::process::exit(1);
    }
}

/// Reads the environment. `BTCTICKER_SETTINGS` seeds a test run, which then never touches the user's settings file:
/// it uses `BTCTICKER_SETTINGS_FILE`, or a file of its own under %TEMP%.
fn config() -> shell::Config {
    let variable = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let seed = variable("BTCTICKER_SETTINGS");
    let settings_file = match (variable("BTCTICKER_SETTINGS_FILE"), &seed) {
        (Some(path), _) => PathBuf::from(path),
        (None, Some(_)) => std::env::temp_dir().join(format!("BTCTicker-{}", std::process::id())).join("settings.json"),
        (None, None) => {
            let appdata = variable("APPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
            appdata.join("BTCTicker").join("settings.json")
        }
    };
    shell::Config {
        settings_file,
        seed,
        state_file: variable("BTCTICKER_STATE_FILE").map(PathBuf::from),
        input_tag: variable("BTCTICKER_INPUT_TAG").and_then(|tag| tag.parse().ok()),
    }
}
