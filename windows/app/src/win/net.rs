//! HTTPS through WinHTTP, so TLS and certificates are the OS's. One worker thread per kind, quote and candles, each
//! strictly sequential like the macOS feed; `App` already keeps at most one request per kind in flight. Replies go
//! back to the host window as a boxed `Input`.
use std::sync::mpsc::{Sender, channel};
use ticker_core::{HttpReply, Input, Request, Timeframe};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{HSTRING, PCWSTR, w};

pub(super) struct Fetcher {
    quote: Sender<Request>,
    candles: Sender<(Timeframe, Request)>,
}

impl Fetcher {
    pub fn spawn(host: HWND) -> Fetcher {
        // HWND is not Send; the workers only post to it, which any thread may do.
        let target = host.0 as isize;
        let (quote, quotes) = channel::<Request>();
        let (candles, charts) = channel::<(Timeframe, Request)>();
        std::thread::spawn(move || {
            for request in quotes {
                post(target, Input::QuoteReply(get(&request, "quote")));
            }
        });
        std::thread::spawn(move || {
            for (timeframe, request) in charts {
                let reply = get(&request, timeframe.spec().label);
                post(target, Input::CandlesReply(timeframe, reply));
            }
        });
        Fetcher { quote, candles }
    }

    pub fn quote(&self, request: Request) {
        let _ = self.quote.send(request);
    }

    pub fn candles(&self, timeframe: Timeframe, request: Request) {
        let _ = self.candles.send((timeframe, request));
    }
}

fn post(target: isize, input: Input) {
    let boxed = Box::into_raw(Box::new(input));
    // SAFETY: the host reclaims the box in its WM_APP_REPLY handler; if posting fails the box is reclaimed here.
    unsafe {
        let posted = PostMessageW(
            Some(HWND(target as *mut _)),
            super::host::WM_APP_REPLY,
            WPARAM(0),
            LPARAM(boxed as isize),
        );
        if posted.is_err() {
            drop(Box::from_raw(boxed));
        }
    }
}

/// Reclaims a reply posted by `post`.
///
/// # Safety
/// `lparam` must be the pointer of exactly one WM_APP_REPLY message posted by `post`.
pub(super) unsafe fn take(lparam: LPARAM) -> Input {
    // SAFETY: per the contract, a Box<Input> leaked by `post` and not yet reclaimed.
    *unsafe { Box::from_raw(lparam.0 as *mut Input) }
}

fn get(request: &Request, what: &str) -> HttpReply {
    match fetch(request) {
        Ok((status, body)) => {
            if status != 200 {
                eprintln!("{what} fetch answered HTTP {status}");
            }
            HttpReply::Answered { status, body }
        }
        Err(error) => {
            eprintln!("{what} fetch failed: {error}");
            HttpReply::Unreachable
        }
    }
}

fn fetch(request: &Request) -> Result<(u16, Vec<u8>), String> {
    let rest = request
        .url
        .strip_prefix("https://")
        .ok_or_else(|| format!("not an https URL: {}", request.url))?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (host, path) = (HSTRING::from(host), HSTRING::from(path));
    let timeout = (request.timeout_seconds * 1000.0).round() as i32;
    // SAFETY: every handle opened here is closed before returning; every buffer outlives its call.
    unsafe {
        let session = WinHttpOpen(
            w!("BTCTicker"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err(format!(
                "WinHttpOpen: {}",
                windows::core::Error::from_thread()
            ));
        }
        let _ = WinHttpSetTimeouts(session, timeout, timeout, timeout, timeout);
        let connection = WinHttpConnect(session, &host, INTERNET_DEFAULT_HTTPS_PORT, 0);
        let result = if connection.is_null() {
            Err(format!(
                "WinHttpConnect: {}",
                windows::core::Error::from_thread()
            ))
        } else {
            let handle = WinHttpOpenRequest(
                connection,
                w!("GET"),
                &path,
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            );
            let result = if handle.is_null() {
                Err(format!(
                    "WinHttpOpenRequest: {}",
                    windows::core::Error::from_thread()
                ))
            } else {
                let result = exchange(handle);
                let _ = WinHttpCloseHandle(handle);
                result
            };
            let _ = WinHttpCloseHandle(connection);
            result
        };
        let _ = WinHttpCloseHandle(session);
        result
    }
}

/// # Safety
/// `handle` must be an open WinHTTP request handle.
unsafe fn exchange(handle: *mut core::ffi::c_void) -> Result<(u16, Vec<u8>), String> {
    // SAFETY: `handle` is open per the contract; `status`, `length`, `available`, `read` and `chunk` are valid for
    // each call.
    unsafe {
        WinHttpSendRequest(handle, None, None, 0, 0, 0).map_err(|e| format!("send: {e}"))?;
        WinHttpReceiveResponse(handle, std::ptr::null_mut())
            .map_err(|e| format!("receive: {e}"))?;
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            handle,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut length,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("status: {e}"))?;
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(handle, &mut available).map_err(|e| format!("size: {e}"))?;
            if available == 0 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(handle, chunk.as_mut_ptr().cast(), available, &mut read)
                .map_err(|e| format!("read: {e}"))?;
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok((status as u16, body))
    }
}
