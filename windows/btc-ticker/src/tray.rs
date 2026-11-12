//! The notification-area icon and its menu. Windows 11 puts a new icon in the overflow until the user pins it.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, HICON, MF_STRING, PostMessageW, SetForegroundWindow,
    TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};
use windows::core::PCWSTR;

use crate::menu::{self, Command};
use crate::win::wide;

pub struct Tray {
    data: NOTIFYICONDATAW,
}

impl Tray {
    /// Clicks on the icon arrive at `owner` as `message`, with the event in the low word of lParam.
    pub fn add(owner: HWND, message: u32, icon: HICON) -> Tray {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: owner,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: message,
            hIcon: icon,
            ..Default::default()
        };
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let mut tray = Tray { data };
        tray.write_tip("BTC Ticker");
        tray.show();
        tray
    }

    /// Kept for `show`, so an Explorer restart brings the icon back with the current text.
    pub fn set_tip(&mut self, text: &str) {
        if self.write_tip(text) {
            // SAFETY: passes a fully initialized struct for the icon this Tray added.
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.data) };
        }
    }

    /// Reports whether the text changed.
    fn write_tip(&mut self, text: &str) -> bool {
        let mut tip = [0u16; 128];
        let room = tip.len() - 1;
        for (slot, unit) in tip[..room].iter_mut().zip(text.encode_utf16()) {
            *slot = unit;
        }
        let changed = tip != self.data.szTip;
        self.data.szTip = tip;
        changed
    }

    /// Also called when Explorer restarts, which forgets every icon.
    pub fn show(&self) {
        // SAFETY: passes a fully initialized struct.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.data);
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &self.data);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: removes the icon this Tray added and frees its image.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data);
            let _ = DestroyIcon(self.data.hIcon);
        }
    }
}

/// A classic menu needs its owner in the foreground to close when the user clicks elsewhere, so this one takes
/// focus; the tray is outside the game anyway.
pub fn choose(owner: HWND, at: POINT, locked: bool) -> Option<Command> {
    let items = menu::items(locked);
    // SAFETY: the menu is created, tracked and destroyed here; labels outlive AppendMenuW, which copies them.
    unsafe {
        let popup = CreatePopupMenu().ok()?;
        for (index, (_, label)) in items.iter().enumerate() {
            let label = wide(label);
            let _ = AppendMenuW(popup, MF_STRING, index + 1, PCWSTR(label.as_ptr()));
        }
        let _ = SetForegroundWindow(owner);
        let chosen =
            TrackPopupMenu(popup, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, at.x, at.y, None, owner, None);
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(popup);
        usize::try_from(chosen.0).ok().and_then(|id| items.get(id.checked_sub(1)?)).map(|(command, _)| *command)
    }
}
