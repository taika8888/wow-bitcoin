//! The app under test: launched with a throwaway settings file and a state file it rewrites after every change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::desktop::Rect;

/// The app's `ticker_core::Snapshot`, as written to `BTCTICKER_STATE_FILE`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub text: String,
    pub feed: String,
    pub fetched_at: Option<f64>,
    pub mode: String,
    pub locked: bool,
    pub opacity: f64,
    pub visible: bool,
    pub hwnd: i64,
    pub frame: Rect,
    pub timeframe: Option<String>,
    pub zoom: Option<String>,
    pub chart: Option<String>,
    pub candles: usize,
    pub visible_candles: usize,
    pub max_bars: usize,
    pub targets: BTreeMap<String, Rect>,
    pub activations: u32,
    pub foreign_presses: u32,
}

impl Snapshot {
    pub fn ticker(&self) -> Rect {
        self.targets.get("ticker").copied().unwrap_or(self.frame)
    }
}

pub struct Ticker {
    child: Child,
    pub pid: u32,
    pub exe: PathBuf,
    state: PathBuf,
    settings: PathBuf,
}

impl Ticker {
    /// The settings file sits in a folder of its own that does not exist yet, as on a fresh install.
    pub fn launch(exe: &Path, work: &Path, seed: Option<&Value>, input_tag: usize) -> std::io::Result<Ticker> {
        let (state, settings) = (work.join("state.json"), work.join("BTCTicker").join("settings.json"));
        let mut command = Command::new(exe);
        if let Some(seed) = seed {
            command.env("BTCTICKER_SETTINGS", seed.to_string());
        }
        let child = command
            .env("BTCTICKER_SETTINGS_FILE", &settings)
            .env("BTCTICKER_STATE_FILE", &state)
            .env("BTCTICKER_INPUT_TAG", input_tag.to_string())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(work.join("app-stderr.log"))?)
            .spawn()?;
        let exe = std::fs::canonicalize(exe)?;
        let exe = PathBuf::from(exe.to_string_lossy().trim_start_matches(r"\\?\"));
        Ok(Ticker { pid: child.id(), child, exe, state, settings })
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        serde_json::from_slice(&std::fs::read(&self.state).ok()?).ok()
    }

    pub fn wait_for(&self, seconds: f64, wanted: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        wait_until(seconds, || self.snapshot().filter(|snapshot| wanted(snapshot)))
    }

    /// Writes the settings file in place, the way a text editor saving it would.
    pub fn write_settings(&self, settings: &Value) {
        let _ = std::fs::write(&self.settings, settings.to_string());
    }

    pub fn saved_settings(&self) -> Option<Value> {
        serde_json::from_slice(&std::fs::read(&self.settings).ok()?).ok()
    }

    pub fn exited_within(&mut self, seconds: f64) -> Option<ExitStatus> {
        wait_until(seconds, || self.child.try_wait().ok().flatten())
    }

    /// Kills the app if it is still running, and says whether it had to.
    pub fn stop(&mut self) -> bool {
        let running = matches!(self.child.try_wait(), Ok(None));
        if running {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        running
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn wait_until<T>(seconds: f64, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        if let Some(found) = probe() {
            return Some(found);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
