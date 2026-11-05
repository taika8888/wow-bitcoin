//! Settings persistence: a REG_SZ value under HKEY_CURRENT_USER holding the macOS JSON blob. Changes made from
//! outside (the e2e harness, regedit) arrive through RegNotifyChangeKeyValue on a watcher thread, which posts
//! WM_APP_STORE to the host; the host reads the value and dispatches `Input::SettingsStored`. Our own writes echo
//! back the same way and are no-ops in `App`, because the decoded settings are equal.
use ticker_core::{SETTINGS_VALUE, Settings};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, KEY_WRITE, REG_NOTIFY_CHANGE_LAST_SET,
    REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegGetValueW, RegNotifyChangeKeyValue, RegSetValueExW,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::HSTRING;

pub(super) struct Store {
    key: HKEY,
    path: &'static str,
}

impl Store {
    /// With a seed, the seed is decoded with the usual fallbacks and written canonically before the app reads it,
    /// as on macOS.
    pub fn open(path: &'static str, seed: Option<&str>) -> windows::core::Result<Store> {
        let store = Store {
            key: create(path, KEY_READ | KEY_WRITE)?,
            path,
        };
        if let Some(seed) = seed {
            store.save(&Settings::decode(Some(seed)).to_json());
        }
        Ok(store)
    }

    /// None when the value is missing, which decodes to the defaults, as on macOS.
    pub fn load(&self) -> Option<String> {
        read(self.key)
    }

    pub fn save(&self, json: &str) {
        let wide: Vec<u16> = json.encode_utf16().chain([0]).collect();
        // SAFETY: `wide` is a nul-terminated UTF-16 buffer viewed as bytes for the duration of the call.
        let result = unsafe {
            RegSetValueExW(
                self.key,
                &HSTRING::from(SETTINGS_VALUE),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    wide.as_ptr().cast(),
                    wide.len() * 2,
                )),
            )
        };
        if let Err(error) = result.ok() {
            eprintln!("saving settings failed: {error}");
        }
    }

    pub fn watch(&self, host: HWND) {
        let path = self.path;
        let target = host.0 as isize;
        std::thread::spawn(move || {
            let Ok(key) = create(path, KEY_NOTIFY) else {
                return;
            };
            // SAFETY: the event and key live as long as this thread, which loops for the life of the process; the
            // notification is re-armed before each wait.
            unsafe {
                let Ok(changed) = CreateEventW(None, false, false, None) else {
                    return;
                };
                loop {
                    if RegNotifyChangeKeyValue(
                        key,
                        false,
                        REG_NOTIFY_CHANGE_LAST_SET,
                        Some(changed),
                        true,
                    )
                    .is_err()
                    {
                        return;
                    }
                    WaitForSingleObject(changed, INFINITE);
                    let _ = PostMessageW(
                        Some(HWND(target as *mut _)),
                        super::host::WM_APP_STORE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        });
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `create` and is not used after this.
        let _ = unsafe { RegCloseKey(self.key) };
    }
}

fn create(path: &str, access: REG_SAM_FLAGS) -> windows::core::Result<HKEY> {
    let mut key = HKEY::default();
    // SAFETY: `key` is valid for the call and owned by the caller afterwards.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(path),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            access,
            None,
            &mut key,
            None,
        )
    }
    .ok()?;
    Ok(key)
}

fn read(key: HKEY) -> Option<String> {
    let name = HSTRING::from(SETTINGS_VALUE);
    let mut bytes = 0u32;
    // SAFETY: a size query; `bytes` is valid for the call.
    unsafe {
        RegGetValueW(
            key,
            None,
            &name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    }
    .ok()
    .ok()?;
    let mut buffer = vec![0u16; bytes as usize / 2 + 1];
    let mut length = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` holds `length` bytes and outlives the call.
    unsafe {
        RegGetValueW(
            key,
            None,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut length),
        )
    }
    .ok()
    .ok()?;
    let text = &buffer[..(length as usize / 2)];
    let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
    Some(String::from_utf16_lossy(&text[..end]))
}
