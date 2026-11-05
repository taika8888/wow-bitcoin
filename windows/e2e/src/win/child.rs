//! The harness's own windows, each in its own child process (the harness exe re-run with a role argument), so
//! foreground and activation behave between them, the app and the harness as they do between real processes:
//! - the stand-in: copied as Wow.exe, so the app's real classifier sees World of Warcraft; topmost, full screen,
//!   painted with `STANDIN_FILL`;
//! - the helper: copied as Other.exe; a small topmost window the harness clicks to move the foreground away;
//! - the clear window: a layered window whose pixels are alpha 0 below an opaque strip, to show that alpha-0 pixels
//!   pass clicks through on this desktop.
//!
//! The harness reads a child's press count and asks it to re-raise itself with SendMessageTimeout.
use btc_ticker_e2e::pixels::STANDIN_FILL;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetDC, HBRUSH, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_TOPMOST, MA_NOACTIVATE,
    MSG, PostQuitMessage, RegisterClassW, SET_WINDOW_POS_FLAGS, SMTO_ABORTIFHUNG, SW_SHOW,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SendMessageTimeoutW, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WINDOW_EX_STYLE,
    WM_APP, WM_DESTROY, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const Q_PRESSES: u32 = WM_APP + 1;
const Q_RAISE: u32 = WM_APP + 2;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Height of the clear window's opaque strip, which proves the window is there and on top.
const STRIP: i32 = 20;

static PRESSES: AtomicIsize = AtomicIsize::new(0);
static ROLE: OnceLock<Role> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Standin,
    Helper,
    Clear,
}

impl Role {
    const ALL: [Role; 3] = [Role::Standin, Role::Helper, Role::Clear];

    fn arg(self) -> &'static str {
        match self {
            Role::Standin => "standin",
            Role::Helper => "helper",
            Role::Clear => "clear",
        }
    }

    pub fn from_arg(arg: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.arg() == arg)
    }

    fn class(self) -> PCWSTR {
        match self {
            Role::Standin => w!("BTCTickerE2EStandin"),
            Role::Helper => w!("BTCTickerE2EHelper"),
            Role::Clear => w!("BTCTickerE2EClear"),
        }
    }

    fn ex_style(self) -> WINDOW_EX_STYLE {
        match self {
            Role::Standin | Role::Helper => WS_EX_TOPMOST,
            Role::Clear => WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
        }
    }

    /// x, y, width, height on a primary screen of `screen`, placed clear of the overlay's default spot (top
    /// center) and of each other.
    pub fn rect(self, screen: (i32, i32)) -> (i32, i32, i32, i32) {
        let (w, h) = screen;
        match self {
            Role::Standin => (0, 0, w, h),
            Role::Helper => (w - 260, h - 220, 200, 120),
            Role::Clear => (40, h - 260, 240, 120),
        }
    }

    /// For the clear window: a point on its opaque strip, and one on its alpha-0 pixels.
    pub fn clear_points(screen: (i32, i32)) -> ((i32, i32), (i32, i32)) {
        let (x, y, width, _) = Role::Clear.rect(screen);
        ((x + width / 2, y + STRIP / 2), (x + width / 2, y + 80))
    }

    fn fill(self) -> Option<[u8; 3]> {
        match self {
            Role::Standin => Some(STANDIN_FILL),
            Role::Helper => Some([0x30, 0xA0, 0x50]),
            Role::Clear => None,
        }
    }
}

pub struct Win {
    child: Child,
    pub hwnd: HWND,
}

impl Win {
    /// Starts `exe` (a copy of the harness, named for the role it plays) and waits for its window.
    pub fn spawn(exe: &Path, role: Role) -> Result<Win, String> {
        let mut child = Command::new(exe)
            .arg(role.arg())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("starting {}: {e}", exe.display()))?;
        let pid = child.id();
        // SAFETY: the class names are static nul-terminated literals.
        let class = unsafe { role.class().to_string() }.unwrap_or_default();
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            let found = super::os::top_level()
                .into_iter()
                .find(|&h| super::os::pid_of(h) == pid && super::os::class_of(h) == class);
            if let Some(hwnd) = found {
                return Ok(Win { child, hwnd });
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "{} exited {status} before showing a window",
                    role.arg()
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        Err(format!("{} showed no window within 5 s", role.arg()))
    }

    fn ask(&self, msg: u32) -> Option<isize> {
        let mut result = 0usize;
        // SAFETY: `result` is valid for the call; the timeout bounds a hung child.
        let answered = unsafe {
            SendMessageTimeoutW(
                self.hwnd,
                msg,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                2_000,
                Some(&mut result),
            )
        };
        (answered.0 != 0).then_some(result as isize)
    }

    /// Left presses the window has received.
    pub fn presses(&self) -> Option<isize> {
        self.ask(Q_PRESSES)
    }

    /// SetWindowPos(HWND_TOPMOST) from inside the child, the way a game re-raises itself.
    pub fn raise(&self) -> bool {
        self.ask(Q_RAISE) == Some(1)
    }
}

impl Drop for Win {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let role = ROLE.get().copied();
    match msg {
        WM_LBUTTONDOWN => {
            PRESSES.fetch_add(1, Ordering::SeqCst);
        }
        WM_MOUSEACTIVATE if role == Some(Role::Clear) => return LRESULT(MA_NOACTIVATE as isize),
        Q_PRESSES => return LRESULT(PRESSES.load(Ordering::SeqCst)),
        Q_RAISE => {
            // Only the stand-in activates as it rises, as a game would; the harness's other windows never do.
            let activate = if role == Some(Role::Standin) {
                SET_WINDOW_POS_FLAGS(0)
            } else {
                SWP_NOACTIVATE
            };
            // SAFETY: our own window.
            let raised = unsafe {
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | activate,
                )
            };
            return LRESULT(raised.is_ok() as isize);
        }
        // The self-destruct timer, should the harness die without killing us.
        WM_TIMER | WM_DESTROY => {
            // SAFETY: no arguments.
            unsafe { PostQuitMessage(0) }
        }
        _ => {}
    }
    // SAFETY: forwards this window's own message unchanged.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn run(role: Role) -> i32 {
    let _ = ROLE.set(role);
    super::os::set_dpi_aware();
    let (x, y, width, height) = role.rect(super::os::screen());
    // SAFETY: plain window creation and a message loop on this thread; every pointer outlives its call.
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return 2;
        };
        let brush = role.fill().map_or(HBRUSH::default(), |[r, g, b]| {
            CreateSolidBrush(COLORREF(
                u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16,
            ))
        });
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: role.class(),
            hbrBackground: brush,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return 3;
        }
        let Ok(hwnd) = CreateWindowExW(
            role.ex_style(),
            role.class(),
            w!("btc-ticker-e2e"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return 4;
        };
        SetTimer(Some(hwnd), 1, 600_000, None);
        if role == Role::Clear {
            paint_clear(hwnd, x, y, width, height);
        }
        let show = if role == Role::Standin {
            SW_SHOW
        } else {
            SW_SHOWNOACTIVATE
        };
        let _ = ShowWindow(hwnd, show);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
}

/// An opaque magenta strip on top, alpha 0 everywhere else. Premultiplied BGRA, as UpdateLayeredWindow takes.
///
/// # Safety
/// `hwnd` must be this thread's layered window.
unsafe fn paint_clear(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the DIB's bits are valid for width * height * 4 bytes until DeleteObject; DCs are released below.
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits = std::ptr::null_mut();
        if let Ok(bitmap) =
            CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        {
            let pixels =
                std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
            for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let row = index as i32 / width;
                *pixel = if row < STRIP {
                    [255, 0, 255, 255]
                } else {
                    [0, 0, 0, 0]
                };
            }
            let previous = SelectObject(memory, bitmap.into());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&POINT { x, y }),
                Some(&SIZE {
                    cx: width,
                    cy: height,
                }),
                Some(memory),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(memory, previous);
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}
