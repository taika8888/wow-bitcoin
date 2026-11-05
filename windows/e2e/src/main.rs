//! btc-ticker-e2e <path to btc-ticker.exe> <artifact dir>
//! The same exe also runs as the harness's own windows, re-executed with a role argument (see win/child.rs).
#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    std::process::exit(win::main());
    #[cfg(not(windows))]
    {
        eprintln!(
            "the e2e harness drives Win32; on this machine `cargo test -p btc-ticker-e2e` runs its verdict logic"
        );
        std::process::exit(2);
    }
}
