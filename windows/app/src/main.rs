#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    std::process::exit(win::run());
    #[cfg(not(windows))]
    {
        eprintln!(
            "btc-ticker is a Windows app. On this machine run windows/scripts/check.sh, which lints it for Windows."
        );
        std::process::exit(2);
    }
}
