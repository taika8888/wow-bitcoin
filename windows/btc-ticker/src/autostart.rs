//! Start with Windows: a `BTCTicker` value under the user's Run key that holds this exe's quoted path. The registry is
//! the only record of the choice.

use std::io::{Error, Result};
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_ANY, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::PCWSTR;

use crate::win::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const TEST_KEY: &str = r"Software\BTCTicker-test";

/// The key and value name. A test run, one with `BTCTICKER_SETTINGS` set as in `config` in main.rs, writes a value of
/// its own under a key Windows never runs, so a test that dies with the box ticked starts nothing at the next login.
fn location() -> (Vec<u16>, Vec<u16>) {
    match std::env::var_os("BTCTICKER_SETTINGS").filter(|seed| !seed.is_empty()) {
        Some(_) => (wide(TEST_KEY), wide(&format!("BTCTicker-{}", std::process::id()))),
        None => (wide(RUN_KEY), wide("BTCTicker")),
    }
}

/// Whether the value exists, whatever exe it names: the box shows the choice, and ticking it again rewrites the path.
pub fn enabled() -> bool {
    let (key, value) = location();
    // SAFETY: reads no data, only whether the value exists; the strings outlive the call.
    let status = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), RRF_RT_ANY, None, None, None)
    };
    status.is_ok()
}

pub fn set(on: bool) -> Result<()> {
    let (key, value) = location();
    // SAFETY: the strings and the data buffer outlive each call.
    let status = unsafe {
        if on {
            let exe = std::env::current_exe()?;
            let command: Vec<u16> =
                "\"".encode_utf16().chain(exe.as_os_str().encode_wide()).chain("\"\0".encode_utf16()).collect();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * size_of::<u16>()) as u32,
            )
        } else {
            match RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr())) {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                status => status,
            }
        }
    };
    if status.is_ok() { Ok(()) } else { Err(Error::from_raw_os_error(status.0 as i32)) }
}
