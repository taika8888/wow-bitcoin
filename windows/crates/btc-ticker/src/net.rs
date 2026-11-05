//! Runs the core's GET requests through WinHTTP on worker threads and hands the outcomes back to the UI thread.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use ticker_core::{HttpOutcome, RequestId, Timeframe};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{PCWSTR, w};

use crate::win::wide;

#[derive(Clone, Copy, Debug)]
pub enum Request {
    Quote(RequestId),
    Candles(RequestId, Timeframe),
}

pub struct Fetched {
    pub request: Request,
    pub outcome: HttpOutcome,
}

pub struct Net {
    session: Arc<Handle>,
    sender: Sender<Fetched>,
    receiver: Receiver<Fetched>,
    notify: isize,
    message: u32,
}

impl Net {
    /// Each finished request posts `message` to `notify`; the UI thread then drains `finished`.
    pub fn new(notify: HWND, message: u32) -> windows::core::Result<Net> {
        // SAFETY: opens a session handle owned by `Handle`.
        let session = unsafe {
            WinHttpOpen(w!("BTCTicker/1.0"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0)
        };
        let session = Handle::new(session).ok_or_else(windows::core::Error::from_thread)?;
        let (sender, receiver) = channel();
        Ok(Net { session: Arc::new(session), sender, receiver, notify: notify.0 as isize, message })
    }

    pub fn fetch(&self, request: Request, url: &str, timeout: Duration) {
        let (session, sender, url) = (Arc::clone(&self.session), self.sender.clone(), url.to_string());
        let (notify, message) = (self.notify, self.message);
        std::thread::spawn(move || {
            let outcome = get(&session, &url, timeout).unwrap_or(HttpOutcome::Unreachable);
            if sender.send(Fetched { request, outcome }).is_ok() {
                // SAFETY: posting to a window that may already be gone fails harmlessly.
                let _ = unsafe { PostMessageW(Some(HWND(notify as *mut c_void)), message, WPARAM(0), LPARAM(0)) };
            }
        });
    }

    pub fn finished(&self) -> impl Iterator<Item = Fetched> + '_ {
        self.receiver.try_iter()
    }
}

/// None for anything short of a full answer: no route, a timeout, TLS failure, or a cut-off body.
fn get(session: &Handle, url: &str, timeout: Duration) -> Option<HttpOutcome> {
    let (host, path) = url.strip_prefix("https://")?.split_once('/')?;
    let (host, path) = (wide(host), wide(&format!("/{path}")));
    let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: every handle is owned by a `Handle` and closed on drop; buffers outlive the calls that fill them.
    unsafe {
        let connection = Handle::new(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
        let request = Handle::new(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        WinHttpSetTimeouts(request.0, millis, millis, millis, millis).ok()?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut status).cast()),
            &mut length,
            std::ptr::null_mut(),
        )
        .ok()?;
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).ok()?;
            if available == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(request.0, body[start..].as_mut_ptr().cast(), available, &mut read).ok()?;
            body.truncate(start + read as usize);
        }
        Some(HttpOutcome::Answered { status: u16::try_from(status).unwrap_or(0), body })
    }
}

struct Handle(*mut c_void);

// WinHTTP session handles may be used from any thread.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    fn new(raw: *mut c_void) -> Option<Handle> {
        (!raw.is_null()).then_some(Handle(raw))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: closes a handle this wrapper owns.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}
