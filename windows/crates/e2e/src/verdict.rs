//! Turns each check's finding into its log line, once outside input may have disturbed the run.

/// Whether the desktop is still the one the check set up.
#[derive(Default)]
enum Run {
    #[default]
    Clean,
    /// Someone else's input showed up while check `at` ran. That check and every later one measured the desktop with
    /// a person on it, not the app, so none of their verdicts can be trusted.
    Disturbed { at: String },
}

#[derive(Default)]
pub struct Verdicts {
    run: Run,
    failures: u32,
}

impl Verdicts {
    /// The line for check `name`, which found `status` with `detail`. `disturbance` describes the first input the
    /// check saw but did not send, once there has been any.
    pub fn check(&mut self, status: &str, name: &str, detail: &str, disturbance: Option<&str>) -> String {
        match (&self.run, disturbance) {
            (Run::Clean, None) => self.line(status, name, detail),
            (Run::Clean, Some(seen)) => {
                self.run = Run::Disturbed { at: name.to_string() };
                self.line("INCONCLUSIVE", name, &format!("{seen}; rerun without touching the mouse"))
            }
            (Run::Disturbed { at }, _) => {
                let detail = format!("not run: input was disturbed at e2e-{at}");
                self.line("INCONCLUSIVE", name, &detail)
            }
        }
    }

    /// Teardown checks look at what the run left behind, which outside input cannot fake, so they keep their verdict.
    pub fn teardown(&mut self, status: &str, name: &str, detail: &str) -> String {
        self.line(status, name, detail)
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    fn line(&mut self, status: &str, name: &str, detail: &str) -> String {
        if status != "PASS" {
            self.failures += 1;
        }
        format!("{status:<12} e2e-{name}  ({detail})")
    }
}

#[cfg(test)]
mod tests {
    use super::Verdicts;

    const MOVED: &str = "the cursor is at (2560, 27), not (2560, 26): someone may be moving the mouse";

    #[test]
    fn an_undisturbed_run_reports_each_verdict_as_found() {
        let mut verdicts = Verdicts::default();
        let lines = [
            verdicts.check("PASS", "live-quote", "'BTC $80,392  -0.06%'", None),
            verdicts.check("FAIL", "expand", "no menu window appeared within 3 s", None),
            verdicts.check("INCONCLUSIVE", "only-when-wow", "Windows refused a foreground switch", None),
            verdicts.teardown("PASS", "no-leftovers", "no process runs btc-ticker.exe"),
        ];
        assert_eq!(
            lines,
            [
                "PASS         e2e-live-quote  ('BTC $80,392  -0.06%')",
                "FAIL         e2e-expand  (no menu window appeared within 3 s)",
                "INCONCLUSIVE e2e-only-when-wow  (Windows refused a foreground switch)",
                "PASS         e2e-no-leftovers  (no process runs btc-ticker.exe)",
            ]
        );
        assert_eq!(verdicts.failures(), 2);
    }

    #[test]
    fn a_moved_mouse_makes_that_check_and_every_later_one_inconclusive_but_not_teardown() {
        let mut verdicts = Verdicts::default();
        let lines = [
            verdicts.check("PASS", "clamped", "flush with its corner", None),
            verdicts.check("FAIL", "expand", MOVED, Some(MOVED)),
            verdicts.check("FAIL", "timeframe-15m", "sent nothing", Some(MOVED)),
            verdicts.check("PASS", "ticker-anchored", "the ticker stayed put", Some(MOVED)),
            verdicts.teardown("FAIL", "no-leftovers", "still running btc-ticker.exe: pids [4242]"),
            verdicts.teardown("PASS", "user-settings-untouched", "byte for byte what it was"),
        ];
        assert_eq!(
            lines,
            [
                "PASS         e2e-clamped  (flush with its corner)",
                &format!("INCONCLUSIVE e2e-expand  ({MOVED}; rerun without touching the mouse)"),
                "INCONCLUSIVE e2e-timeframe-15m  (not run: input was disturbed at e2e-expand)",
                "INCONCLUSIVE e2e-ticker-anchored  (not run: input was disturbed at e2e-expand)",
                "FAIL         e2e-no-leftovers  (still running btc-ticker.exe: pids [4242])",
                "PASS         e2e-user-settings-untouched  (byte for byte what it was)",
            ]
        );
        assert_eq!(verdicts.failures(), 4, "a disturbed run never passes");
    }
}
