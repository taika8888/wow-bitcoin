#![windows_subsystem = "windows"]
//! BTC Ticker for Windows: a live Bitcoin price floating above World of Warcraft that never takes focus from the game,
//! and a candlestick chart on click. `ticker-core` decides everything; this crate is the Win32 shell around it.

mod autostart;
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

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HSTRING, PCWSTR, w};

fn main() {
    let config = config();
    // SAFETY: the mutex handle is never closed, so Windows releases it only when this process exits.
    let running = unsafe {
        let _ = CreateMutexW(None, false, &HSTRING::from(format!(r"Local\{}", config.instance)));
        GetLastError() == ERROR_ALREADY_EXISTS
    };
    if running {
        shell::open_settings_in(&config.instance);
        return;
    }
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
        instance: instance(&settings_file),
        settings_file,
        seed,
        state_file: variable("BTCTICKER_STATE_FILE").map(PathBuf::from),
        input_tag: variable("BTCTICKER_INPUT_TAG").and_then(|tag| tag.parse().ok()),
    }
}

/// Names the one ticker allowed per settings file. FNV-1a rather than std's hasher, which may change between builds.
fn instance(settings_file: &Path) -> String {
    let path = std::path::absolute(settings_file).unwrap_or_else(|_| settings_file.to_path_buf());
    let hash = path
        .to_string_lossy()
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    format!("BTCTicker-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::instance;

    #[test]
    fn one_settings_file_names_one_instance_however_it_is_spelled() {
        let file = instance(Path::new(r"C:\Users\a\AppData\Roaming\BTCTicker\settings.json"));
        assert_eq!(file, instance(Path::new(r"c:\users\A\AppData\Roaming\BTCTicker\..\BTCTicker\Settings.json")));
        assert_ne!(file, instance(Path::new(r"C:\Users\a\AppData\Local\Temp\BTCTicker\settings.json")));
    }
}
