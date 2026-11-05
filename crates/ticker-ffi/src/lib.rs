mod input;
mod view;

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use ticker_core::{App, Effect, Input, Settings, Timestamp};

pub struct Ticker {
    app: App,
    chart_revision: u64,
}

fn string(value: &impl serde::Serialize) -> *mut c_char {
    let json = serde_json::to_string(value).unwrap_or_else(|_| r#"{"error":"cannot encode response"}"#.into());
    // JSON escapes embedded NULs, so this cannot fail.
    CString::new(json).map_or(ptr::null_mut(), CString::into_raw)
}

fn error(message: impl AsRef<str>) -> *mut c_char {
    string(&serde_json::json!({"error": message.as_ref()}))
}

unsafe fn text<'a>(value: *const c_char) -> Result<&'a str, &'static str> {
    if value.is_null() {
        return Err("null string");
    }
    // SAFETY: caller supplies a live, NUL-terminated buffer for this call.
    unsafe { CStr::from_ptr(value) }.to_str().map_err(|_| "invalid UTF-8")
}

fn guarded(action: impl FnOnce() -> Result<*mut c_char, String>) -> *mut c_char {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(value)) => value,
        Ok(Err(message)) => error(message),
        Err(_) => error("internal panic"),
    }
}

/// Creates an app without issuing requests. Invalid settings follow Settings::decode's defaults policy.
/// # Safety
/// Strings must be NUL-terminated; `out_error` must be writable. Free its non-null result with ticker_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ticker_create(
    settings: *const c_char,
    frontmost: *const c_char,
    now: f64,
    out_error: *mut *mut c_char,
) -> *mut Ticker {
    if out_error.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: caller provides an output slot.
    unsafe {
        *out_error = ptr::null_mut();
    }
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<Ticker, String> {
        if !now.is_finite() {
            return Err("non-finite time".into());
        }
        // SAFETY: the caller owns both strings for the duration of this call.
        let settings = Settings::decode(Some(unsafe { text(settings)? }));
        let foreground: input::Foreground =
            serde_json::from_str(unsafe { text(frontmost)? }).map_err(|e| e.to_string())?;
        Ok(Ticker { app: App::new(settings, foreground.into(), Timestamp::from_unix_seconds(now)), chart_revision: 0 })
    }));
    match result {
        Ok(Ok(ticker)) => Box::into_raw(Box::new(ticker)),
        failure => {
            let message = match failure {
                Ok(Err(message)) => message,
                _ => "internal panic".into(),
            };
            // SAFETY: caller provides an output slot.
            unsafe {
                *out_error = error(message);
            }
            ptr::null_mut()
        }
    }
}

/// Handles one JSON input and returns effects plus nextWake, or an error object without applying malformed input.
/// # Safety
/// `ticker` must be a live handle, exclusively borrowed for this call; input must be NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ticker_handle(ticker: *mut Ticker, input: *const c_char) -> *mut c_char {
    guarded(|| {
        // SAFETY: validity and exclusive access are the caller's responsibility; null is rejected.
        let ticker = unsafe { ticker.as_mut() }.ok_or("null app")?;
        let input: input::Message = serde_json::from_str(unsafe { text(input)? }).map_err(|e| e.to_string())?;
        let input = Input::from(input);
        let candles = matches!(input, Input::CandlesFinished { .. });
        let effects = ticker.app.handle(input);
        if candles && effects.contains(&Effect::Render) {
            ticker.chart_revision = ticker.chart_revision.wrapping_add(1);
        }
        Ok(string(&view::Reply {
            effects: effects.into_iter().map(view::Action::from).collect(),
            next_wake: ticker.app.next_wake().map(Timestamp::unix_seconds),
        }))
    })
}

/// Rendering metadata, plus bars and ticks when max_bars > 0. UTC offset is seconds east of UTC at chartStart.
/// # Safety
/// `ticker` must be a live handle, with no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ticker_view(ticker: *const Ticker, max_bars: usize, utc_offset: i32) -> *mut c_char {
    guarded(|| {
        // SAFETY: validity is the caller's responsibility; null is rejected.
        let ticker = unsafe { ticker.as_ref() }.ok_or("null app")?;
        Ok(string(&view::View::new(&ticker.app, ticker.chart_revision, max_bars, utc_offset)))
    })
}

/// # Safety
/// Pass only a string returned by this library, once. Null is allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ticker_string_free(value: *mut c_char) {
    if !value.is_null() {
        // SAFETY: ownership transfers back from the caller exactly once.
        drop(unsafe { CString::from_raw(value) });
    }
}

/// # Safety
/// Pass only a live handle returned by ticker_create, once, after all calls using it finish. Null is allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ticker_destroy(ticker: *mut Ticker) {
    if !ticker.is_null() {
        // SAFETY: ownership transfers back from the caller exactly once.
        drop(unsafe { Box::from_raw(ticker) });
    }
}
