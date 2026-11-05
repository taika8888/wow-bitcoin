//! The settings file: read at startup, written atomically on every in-app change, and watched for edits from outside.

use std::path::PathBuf;

use ticker_core::Settings;
use windows::Win32::Foundation::{HWND, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindFirstChangeNotificationW,
    FindNextChangeNotification,
};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::PCWSTR;

use crate::win::{wide, write_atomically};

pub struct SettingsStore {
    path: PathBuf,
    /// The file's text as this process last wrote or read it, so its own writes are not taken for outside edits.
    last_text: Option<String>,
}

impl SettingsStore {
    /// A `seed` replaces whatever the file holds, for a test run that must start from known settings.
    pub fn open(path: PathBuf, seed: Option<&str>) -> (SettingsStore, Settings) {
        let mut store = SettingsStore { path, last_text: None };
        if let Some(seed) = seed {
            store.save(&Settings::decode(Some(seed)));
        }
        let text = std::fs::read_to_string(&store.path).ok();
        let settings = Settings::decode(text.as_deref());
        store.last_text = text;
        (store, settings)
    }

    pub fn save(&mut self, settings: &Settings) {
        let text = settings.json();
        match write_atomically(&self.path, text.as_bytes()) {
            Ok(()) => self.last_text = Some(text),
            Err(error) => eprintln!("could not save settings to {}: {error}", self.path.display()),
        }
    }

    /// The file's settings if someone else changed it since this process last touched it. A file mid-write is not
    /// yet a JSON object, and reading it as one would reset every setting to its default.
    pub fn changed(&mut self) -> Option<Settings> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        if self.last_text.as_deref() == Some(text.as_str())
            || serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text).is_err()
        {
            return None;
        }
        let settings = Settings::decode(Some(&text));
        self.last_text = Some(text);
        Some(settings)
    }

    /// Posts `message` to `notify` whenever anything in the settings file's folder changes.
    pub fn watch(&self, notify: HWND, message: u32) {
        let Some(folder) = self.path.parent().map(|folder| wide(&folder.to_string_lossy())) else { return };
        let notify = notify.0 as isize;
        std::thread::spawn(move || {
            // SAFETY: the folder name outlives the call, and the handle is used only on this thread.
            unsafe {
                let Ok(change) = FindFirstChangeNotificationW(
                    PCWSTR(folder.as_ptr()),
                    false,
                    FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
                ) else {
                    return;
                };
                while WaitForSingleObject(change, INFINITE) == WAIT_OBJECT_0 {
                    let posted = PostMessageW(Some(HWND(notify as *mut _)), message, WPARAM(0), LPARAM(0));
                    if posted.is_err() || FindNextChangeNotification(change).is_err() {
                        break;
                    }
                }
            }
        });
    }
}
