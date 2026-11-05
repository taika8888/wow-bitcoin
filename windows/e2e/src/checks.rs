//! Every check the harness must report, declared once. `Report::finish` prints one line per check in this order and
//! turns a check that never reported into FAIL "not run", so an early return can never pass by omission.
use std::collections::BTreeMap;
use std::fmt::Write;

macro_rules! checks {
    ($($id:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Check { $($id),+ }
        impl Check {
            pub const ALL: &[Check] = &[$(Check::$id),+];
            pub fn name(self) -> &'static str { match self { $(Check::$id => $name),+ } }
        }
    };
}

checks! {
    Desktop => "desktop",
    StandinForeground => "standin-foreground",
    Launch => "launch",
    LiveQuote => "live-quote",
    OnScreen => "window-on-screen",
    Styles => "styles",
    AboveStandin => "above-standin",
    TopmostRecovers => "topmost-recovers",
    Tray => "tray",
    SettingsLive => "settings-live (registry edit)",
    WowGate => "wow-gate (SendInput clicks move the real foreground)",
    Clamped => "clamped",
    Expand => "expand (SendInput, synthesized OS input)",
    Timeframe15m => "timeframe-15m (SendInput, synthesized OS input)",
    Capture15m => "capture-15m",
    Timeframe4h => "timeframe-4h (SendInput, synthesized OS input)",
    Capture4h => "capture-4h",
    Timeframe1m => "timeframe-1m (SendInput, synthesized OS input)",
    Capture1m => "capture-1m",
    Collapse => "collapse (SendInput, synthesized OS input)",
    Reexpand => "reexpand (SendInput, synthesized OS input)",
    OpaqueToClicks => "opaque-to-clicks (SendInput, synthesized OS input)",
    FocusKept => "focus-kept",
    Undisturbed => "undisturbed",
    SettingsWindow => "settings-window (SendInput, synthesized OS input)",
    Quit => "quit (SendInput, overlay menu)",
}

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Pass(String),
    Fail(String),
    /// The runner could not show the behavior either way. Fails the run, like verify.sh.
    Inconclusive(String),
}

impl Verdict {
    pub fn status(&self) -> &'static str {
        match self {
            Verdict::Pass(_) => "PASS",
            Verdict::Fail(_) => "FAIL",
            Verdict::Inconclusive(_) => "INCONCLUSIVE",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Verdict::Pass(d) | Verdict::Fail(d) | Verdict::Inconclusive(d) => d,
        }
    }

    pub fn passed(&self) -> bool {
        matches!(self, Verdict::Pass(_))
    }

    /// Pass with `pass` when `ok`, else Fail with `fail`.
    pub fn of(ok: bool, pass: impl Into<String>, fail: impl Into<String>) -> Verdict {
        if ok {
            Verdict::Pass(pass.into())
        } else {
            Verdict::Fail(fail.into())
        }
    }
}

pub fn line(check: Check, verdict: &Verdict) -> String {
    format!(
        "{:<12} e2e-{}  ({})",
        verdict.status(),
        check.name(),
        verdict.detail()
    )
}

#[derive(Default)]
pub struct Report {
    results: BTreeMap<Check, Verdict>,
}

impl Report {
    /// First verdict wins; reporting a check twice is a harness bug and turns it into FAIL. Returns whether the
    /// check passed, so a caller can skip what depends on it.
    pub fn record(&mut self, check: Check, verdict: Verdict) -> bool {
        let verdict = match self.results.get(&check) {
            Some(first) => Verdict::Fail(format!(
                "reported twice: first {} ({}), then {} ({})",
                first.status(),
                first.detail(),
                verdict.status(),
                verdict.detail()
            )),
            None => verdict,
        };
        let passed = verdict.passed();
        self.results.insert(check, verdict);
        passed
    }

    pub fn get(&self, check: Check) -> Option<&Verdict> {
        self.results.get(&check)
    }

    /// Every check not yet reported becomes Inconclusive with `reason`, for a desktop that cannot show anything.
    pub fn inconclusive_rest(&mut self, reason: &str) {
        for check in Check::ALL {
            self.results
                .entry(*check)
                .or_insert_with(|| Verdict::Inconclusive(reason.to_string()));
        }
    }

    /// One line per check in `Check::ALL` order, and exit code 0 only when every check passed.
    pub fn finish(&self) -> (String, i32) {
        let not_run = Verdict::Fail("not run".into());
        let mut out = String::new();
        let mut all_passed = true;
        for check in Check::ALL {
            let verdict = self.results.get(check).unwrap_or(&not_run);
            all_passed &= verdict.passed();
            let _ = writeln!(out, "{}", line(*check, verdict));
        }
        (out, if all_passed { 0 } else { 1 })
    }
}
