//! The Windows half: launches the app beside the harness's own windows, drives it with SendInput and registry
//! edits, gathers facts with Win32, and hands them to `btc_ticker_e2e::judge`.
mod child;
mod os;
mod watch;

use btc_ticker_e2e::checks::{Check, Report, Verdict, line};
use btc_ticker_e2e::judge::{self, Span};
use btc_ticker_e2e::pixels::{self, STANDIN_FILL};
use child::{Role, Win};
use os::Button;
use serde_json::{Value, json};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use ticker_core::{Rect, Snapshot, Timeframe};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, PostMessageW, SetForegroundWindow, WM_CLOSE,
};
use windows::core::{PCWSTR, w};

/// The app re-asserts topmost every 2 s; this leaves room for a slow runner.
const TOPMOST_BOUND_MS: u64 = 3_500;

pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    if let Some(role) = args.get(1).and_then(|a| Role::from_arg(a)) {
        return child::run(role);
    }
    let (Some(app), Some(artifacts)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: btc-ticker-e2e <btc-ticker.exe> <artifact dir>");
        return 2;
    };
    os::set_dpi_aware();
    os::ms();
    let artifacts = PathBuf::from(artifacts);
    let _ = std::fs::create_dir_all(&artifacts);
    os::delete_settings();
    let mut report = Report::default();
    if let Err(reason) = run(Path::new(app), &artifacts, &mut report) {
        println!("# {} ms stopped: {reason}", os::ms());
    }
    os::delete_settings();
    let (lines, code) = report.finish();
    print!("{lines}");
    code
}

struct Launched {
    child: Child,
    pid: u32,
    /// Kill-on-close, so the app never outlives the harness.
    job: HANDLE,
}

impl Drop for Launched {
    fn drop(&mut self) {
        os::close(self.job);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Harness<'r> {
    report: &'r mut Report,
    artifacts: PathBuf,
    screen: (i32, i32),
    game: Win,
    helper: Win,
    clear: Win,
    app: Option<Launched>,
    overlay: HWND,
    state: PathBuf,
    excused: Vec<Span>,
    edits: u32,
    clicks: u32,
}

fn run(app: &Path, artifacts: &Path, report: &mut Report) -> Result<(), String> {
    let copies = std::env::temp_dir().join(format!("btc-ticker-e2e-{}", os::own_pid()));
    std::fs::create_dir_all(&copies).map_err(|e| format!("{}: {e}", copies.display()))?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let copy = |name: &str| {
        let to = copies.join(name);
        std::fs::copy(&me, &to)
            .map(|_| to)
            .map_err(|e| format!("copying the harness as {name}: {e}"))
    };
    let (wow, other) = (copy("Wow.exe")?, copy("Other.exe")?);
    let spawned = (|| {
        Ok::<_, String>((
            Win::spawn(&wow, Role::Standin)?,
            Win::spawn(&other, Role::Helper)?,
            Win::spawn(&other, Role::Clear)?,
        ))
    })();
    let (game, helper, clear) = match spawned {
        Ok(windows) => windows,
        Err(error) => {
            report.record(Check::Desktop, Verdict::Fail(error.clone()));
            return Err(error);
        }
    };
    std::thread::sleep(Duration::from_millis(500));
    let mut harness = Harness {
        report,
        artifacts: artifacts.to_path_buf(),
        screen: os::screen(),
        game,
        helper,
        clear,
        app: None,
        overlay: HWND::default(),
        state: artifacts.join("state.json"),
        excused: Vec::new(),
        edits: 0,
        clicks: 0,
    };
    let result = harness.all(app);
    drop(harness);
    let _ = std::fs::remove_dir_all(&copies);
    result
}

/// The seeded settings: the macOS e2e seed, locked so any release on the body is a click, plus `changes`.
fn settings(changes: Value) -> String {
    let mut seed = json!({
        "showLabel": true, "showChange": true, "decimals": 0, "fontSize": 15, "locked": true,
        "onlyWhenWoWActive": false, "refreshSeconds": 5,
    });
    if let (Some(seed), Value::Object(changes)) = (seed.as_object_mut(), changes) {
        seed.extend(changes);
    }
    seed.to_string()
}

fn center(r: &Rect) -> (i32, i32) {
    ((r.x + r.width / 2.0) as i32, (r.y + r.height / 2.0) as i32)
}

fn wait_until<T>(seconds: f64, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        if let Some(found) = probe() {
            return Some(found);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl Harness<'_> {
    fn record(&mut self, check: Check, verdict: Verdict) -> bool {
        println!("# {} ms {}", os::ms(), line(check, &verdict));
        self.report.record(check, verdict)
    }

    fn snapshot(&self) -> Option<Snapshot> {
        let text = std::fs::read_to_string(&self.state).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn last(&self) -> String {
        self.snapshot().map_or("no snapshot".into(), |s| {
            format!(
                "last snapshot: '{}' {} visible {} {}",
                s.text,
                judge::describe(&judge::rect(&s.frame)),
                s.visible,
                judge::describe_chart(&s)
            )
        })
    }

    fn wait(&self, seconds: f64, done: impl Fn(&Snapshot) -> bool) -> Option<Snapshot> {
        wait_until(seconds, || self.snapshot().filter(|s| done(s)))
    }

    fn target(&self, name: &str) -> Option<(i32, i32)> {
        let snapshot = self.snapshot()?;
        snapshot.targets.get(name).map(|t| center(&judge::rect(t)))
    }

    fn edit(&mut self, json: &str) -> Result<(), String> {
        self.edits += 1;
        os::write_settings(json)
    }

    fn app_pid(&self) -> u32 {
        self.app.as_ref().map_or(0, |app| app.pid)
    }

    /// Clicks `at` and waits for `done`. Err is the verdict for the check: Inconclusive when the input could not be
    /// inserted, Fail when the click reached the stand-in or the snapshot never got there.
    fn click(
        &mut self,
        at: (i32, i32),
        button: Button,
        seconds: f64,
        done: impl Fn(&Snapshot) -> bool,
    ) -> Result<Snapshot, Verdict> {
        let before = self.game.presses();
        os::click(at.0, at.1, button)
            .map_err(|e| Verdict::Inconclusive(format!("input did not arrive: {e}")))?;
        self.clicks += 1;
        let reached = self.wait(seconds, done);
        if self.game.presses() != before {
            return Err(Verdict::Fail(format!(
                "the click at {at:?} passed through to the stand-in"
            )));
        }
        reached.ok_or_else(|| {
            Verdict::Fail(format!(
                "nothing changed {seconds} s after the click at {at:?}; {}",
                self.last()
            ))
        })
    }

    fn click_target(
        &mut self,
        name: &str,
        seconds: f64,
        done: impl Fn(&Snapshot) -> bool,
    ) -> Result<Snapshot, Verdict> {
        let at = self
            .target(name)
            .ok_or_else(|| Verdict::Fail(format!("no '{name}' target; {}", self.last())))?;
        self.click(at, Button::Left, seconds, done)
    }

    fn standin_point(&self) -> (i32, i32) {
        (120, self.screen.1 / 2)
    }

    /// SetForegroundWindow from the harness, which the runner probe saw honored every time; a click on the
    /// stand-in otherwise, which input activation always honors.
    fn bring_standin_forward(&self) -> Result<&'static str, String> {
        // SAFETY: a plain window handle.
        let _ = unsafe { SetForegroundWindow(self.game.hwnd) };
        if wait_until(2.0, || (os::foreground() == self.game.hwnd).then_some(())).is_some() {
            return Ok("SetForegroundWindow from the harness");
        }
        let (x, y) = self.standin_point();
        os::click(x, y, Button::Left)?;
        wait_until(2.0, || (os::foreground() == self.game.hwnd).then_some(()))
            .map(|_| "a SendInput click on it, after SetForegroundWindow was refused")
            .ok_or_else(|| {
                format!(
                    "neither SetForegroundWindow nor a click made it foreground; foreground is {:#x}",
                    os::id(os::foreground())
                )
            })
    }

    fn all(&mut self, app: &Path) -> Result<(), String> {
        let route = self.bring_standin_forward();
        let topmost = os::ex_style(self.game.hwnd) & judge::WS_EX_TOPMOST != 0;
        let standin = match &route {
            Ok(route) if topmost => Verdict::Pass(format!(
                "Wow.exe stand-in is foreground and topmost, by {route}"
            )),
            Ok(route) => Verdict::Fail(format!("foreground by {route}, but not topmost")),
            Err(error) => Verdict::Inconclusive(error.clone()),
        };
        self.record(Check::StandinForeground, standin);
        let desktop = judge::desktop(&self.desktop_facts());
        if !self.record(Check::Desktop, desktop) || route.is_err() {
            self.report
                .inconclusive_rest("the desktop cannot show the behavior; see desktop");
            return Err("desktop preconditions failed".into());
        }

        let watcher = watch::start();
        self.launch(app)?;
        self.live_quote();
        self.window_checks();
        self.topmost_recovers();
        self.settings_live();
        self.wow_gate(&watcher);
        self.clamped();
        self.chart_checks()?;
        self.opaque_to_clicks();
        self.focus_kept(&watcher);
        drop(watcher);
        self.undisturbed();
        self.settings_window();
        self.quit();
        Ok(())
    }

    fn desktop_facts(&self) -> judge::DesktopFacts {
        let (w, h) = self.screen;
        let targets = [(1, 1), (w / 2, h / 2), (w - 2, h - 2), (71, 1), (333, 517)];
        let cursor_misses = targets
            .iter()
            .filter_map(|&(x, y)| {
                let moved = os::move_to(x, y);
                std::thread::sleep(Duration::from_millis(30));
                let at = os::cursor();
                (moved.is_err() || at != Some((x, y))).then(|| format!("{x},{y}->{at:?}"))
            })
            .collect();
        let before = self.game.presses();
        let (x, y) = self.standin_point();
        let clicked = os::click(x, y, Button::Left).is_ok();
        let click_reached_standin = clicked
            && wait_until(2.0, || {
                (self.game.presses().zip(before).is_some_and(|(a, b)| a > b)).then_some(())
            })
            .is_some();
        let capture = os::capture(&Rect {
            x: f64::from(x),
            y: f64::from(y),
            width: 1.0,
            height: 1.0,
        })
        .ok()
        .and_then(|shot| shot.rgb_at(0, 0));
        judge::DesktopFacts {
            session_id: os::session_id(),
            input_desktop: os::input_desktop(),
            screen: self.screen,
            cursor_misses,
            cursor_tries: targets.len(),
            click_reached_standin,
            capture,
            standin_fill: STANDIN_FILL,
        }
    }

    fn launch(&mut self, app: &Path) -> Result<(), String> {
        let stderr = std::fs::File::create(self.artifacts.join("app-stderr.log"))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        let _ = std::fs::remove_file(&self.state);
        let started = Command::new(app)
            .env("BTCTICKER_SETTINGS", settings(json!({"timeframe": "h4"})))
            .env("BTCTICKER_STATE_FILE", &self.state)
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn();
        let child = match started {
            Ok(child) => child,
            Err(error) => {
                let detail = format!("could not start {}: {error}", app.display());
                self.record(Check::Launch, Verdict::Fail(detail.clone()));
                return Err(detail);
            }
        };
        let pid = child.id();
        // SAFETY: a fresh job object owned by `Launched`; `limits` and the child's handle are valid for the calls.
        let job = unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).unwrap_or_default();
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let _ = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            let _ = AssignProcessToJobObject(job, HANDLE(child.as_raw_handle()));
            job
        };
        self.app = Some(Launched { child, pid, job });
        let shown = self.wait(10.0, |s| s.hwnd != 0 && s.visible);
        let verdict = match &shown {
            Some(snapshot) => {
                self.overlay = os::hwnd(snapshot.hwnd);
                let owner = os::pid_of(self.overlay);
                Verdict::of(
                    owner == pid,
                    format!(
                        "pid {pid} published a visible overlay {:#x} at {}",
                        snapshot.hwnd,
                        judge::describe(&judge::rect(&snapshot.frame))
                    ),
                    format!(
                        "the snapshot's window {:#x} belongs to pid {owner}, not {pid}",
                        snapshot.hwnd
                    ),
                )
            }
            None => Verdict::Fail(format!(
                "no visible overlay in the snapshot within 10 s; {}",
                self.last()
            )),
        };
        if self.record(Check::Launch, verdict) {
            Ok(())
        } else {
            Err("the app did not launch".into())
        }
    }

    fn live_quote(&mut self) {
        let live = self.wait(20.0, |s| {
            s.feed == "live" && judge::parse_ticker(&s.text).is_some_and(|t| t.label && t.change)
        });
        let verdict = match live {
            Some(snapshot) => judge::live_quote(&snapshot, &os::kraken_price()),
            None => Verdict::Fail(format!("no live quote within 20 s; {}", self.last())),
        };
        self.record(Check::LiveQuote, verdict);
    }

    fn window_checks(&mut self) {
        let Some(snapshot) = self.snapshot() else {
            return;
        };
        let frame = judge::rect(&snapshot.frame);
        let real = os::window_rect(self.overlay);
        let monitor = os::monitor_of(self.overlay);
        let visible = os::visible(self.overlay);
        let inside = monitor.is_some_and(|(screen, _)| screen.contains_rect(&frame));
        let on_screen = Verdict::of(
            visible && inside && real.is_some_and(|r| judge::same_rect(&r, &frame)),
            format!(
                "IsWindowVisible, GetWindowRect {} matches the snapshot, inside the {} monitor",
                judge::describe(&frame),
                monitor.map_or("?".into(), |(m, _)| judge::describe(&m))
            ),
            format!(
                "visible {visible}, GetWindowRect {:?} vs snapshot {}, monitor {:?}",
                real.map(|r| judge::describe(&r)),
                judge::describe(&frame),
                monitor.map(|(m, _)| judge::describe(&m))
            ),
        );
        self.record(Check::OnScreen, on_screen);

        let pid = self.app_pid();
        let others = os::top_level()
            .into_iter()
            .filter(|&h| h != self.overlay && os::pid_of(h) == pid && os::visible(h))
            .map(|h| (os::class_of(h), os::ex_style(h)))
            .collect();
        let styles = judge::styles(&judge::StyleFacts {
            real: os::ex_style(self.overlay),
            published: snapshot.ex_style,
            per_monitor_v2: os::per_monitor_v2(self.overlay),
            others,
        });
        self.record(Check::Styles, styles);

        let order: Vec<u64> = os::top_level().into_iter().map(os::id).collect();
        let above = judge::above(&order, snapshot.hwnd, os::id(self.game.hwnd));
        self.record(Check::AboveStandin, above);

        // SAFETY: literal class name, no title.
        let taskbar = unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }
            .is_ok_and(|h| !h.is_invalid());
        let tray = match (snapshot.tray_icon, taskbar) {
            (true, _) => {
                Verdict::Pass("Shell_NotifyIconW(NIM_ADD) succeeded, per the snapshot".into())
            }
            (false, true) => Verdict::Fail("Shell_TrayWnd exists but NIM_ADD failed".into()),
            (false, false) => Verdict::Inconclusive(
                "no Shell_TrayWnd on this desktop, so there is no tray to add to".into(),
            ),
        };
        self.record(Check::Tray, tray);
    }

    fn overlay_above_game(&self) -> bool {
        let order: Vec<u64> = os::top_level().into_iter().map(os::id).collect();
        judge::above(&order, os::id(self.overlay), os::id(self.game.hwnd)).passed()
    }

    fn topmost_recovers(&mut self) {
        let mut seen_below = false;
        for _ in 0..3 {
            if self.game.raise() && !self.overlay_above_game() {
                seen_below = true;
                break;
            }
            wait_until(3.0, || self.overlay_above_game().then_some(()));
        }
        let raised_at = Instant::now();
        let recovered = seen_below
            .then(|| wait_until(5.0, || self.overlay_above_game().then_some(())))
            .flatten()
            .map(|_| raised_at.elapsed().as_millis() as u64);
        let verdict = judge::topmost_recovers(seen_below, recovered, TOPMOST_BOUND_MS);
        self.record(Check::TopmostRecovers, verdict);
    }

    fn settings_live(&mut self) {
        let before = self.snapshot();
        let edited = settings(json!({"showLabel": false, "decimals": 2, "fontSize": 30}));
        let verdict = match self.edit(&edited) {
            Err(error) => Verdict::Fail(format!("writing the registry value: {error}")),
            Ok(()) => {
                let taller = before.as_ref().map_or(f64::INFINITY, |b| b.frame.height);
                match self.wait(5.0, |s| {
                    s.frame.height > taller
                        && judge::parse_ticker(&s.text).is_some_and(|t| !t.label && t.decimals == 2)
                }) {
                    Some(after) => Verdict::Pass(format!(
                        "'{}' {} px tall -> '{}' {} px tall within 5 s of the registry edit",
                        before.as_ref().map_or("", |b| b.text.as_str()),
                        taller,
                        after.text,
                        after.frame.height
                    )),
                    None => Verdict::Fail(format!(
                        "the snapshot did not reflect the edit within 5 s; {}",
                        self.last()
                    )),
                }
            }
        };
        self.record(Check::SettingsLive, verdict);
    }

    fn wow_gate(&mut self, watcher: &watch::Watcher) {
        let gated = settings(
            json!({"showLabel": false, "decimals": 1, "fontSize": 30, "onlyWhenWoWActive": true}),
        );
        let landed = self.edit(&gated).is_ok()
            && self
                .wait(5.0, |s| {
                    judge::parse_ticker(&s.text).is_some_and(|t| t.decimals == 1)
                })
                .is_some();
        if !landed {
            let detail = format!(
                "the onlyWhenWoWActive edit never reached the app; {}",
                self.last()
            );
            self.record(Check::WowGate, Verdict::Fail(detail));
            return;
        }
        let from_ms = os::ms();
        let overlay = self.overlay;
        let visible =
            |shown: bool| move |s: &Snapshot| s.visible == shown && os::visible(overlay) == shown;
        self.helper.raise();
        let (x, y, width, height) = Role::Helper.rect(self.screen);
        let helper_took_foreground = os::click(x + width / 2, y + height / 2, Button::Left).is_ok()
            && wait_until(3.0, || (os::foreground() == self.helper.hwnd).then_some(())).is_some();
        let started = Instant::now();
        let hidden_ms = helper_took_foreground
            .then(|| self.wait(5.0, visible(false)))
            .flatten()
            .map(|_| started.elapsed().as_millis() as u64);
        let (sx, sy) = self.standin_point();
        let standin_took_foreground = os::click(sx, sy, Button::Left).is_ok()
            && wait_until(3.0, || (os::foreground() == self.game.hwnd).then_some(())).is_some();
        let started = Instant::now();
        let shown_ms = self
            .wait(5.0, visible(true))
            .map(|_| started.elapsed().as_millis() as u64);
        if !standin_took_foreground {
            let _ = self.bring_standin_forward();
        }
        self.excused.push(Span {
            from_ms,
            to_ms: os::ms(),
        });
        let mut verdict = judge::wow_gate(&judge::GateFacts {
            helper_took_foreground,
            hidden_ms,
            standin_took_foreground,
            shown_ms,
        });
        if verdict.passed() && !watcher.saw(self.helper.hwnd) {
            verdict = Verdict::Fail(
                "the helper took the foreground but the watcher never recorded it".into(),
            );
        }
        self.record(Check::WowGate, verdict);
    }

    fn clamped(&mut self) {
        let Some((_, work)) = os::monitor_of(self.overlay) else {
            self.record(
                Check::Clamped,
                Verdict::Fail("no monitor for the overlay".into()),
            );
            return;
        };
        let hanging = json!({"position": {"x": work.max_x() - 20.0, "y": work.y - 5.0}});
        let verdict = match self.edit(&settings(hanging)) {
            Err(error) => Verdict::Fail(format!("writing the registry value: {error}")),
            Ok(()) => {
                let landed = self.wait(5.0, |s| judge::clamped(&judge::rect(&s.frame), &work));
                let real = os::window_rect(self.overlay);
                match (landed, real) {
                    (Some(_), Some(real)) if judge::clamped(&real, &work) => {
                        Verdict::Pass(format!(
                            "a saved spot 20 px from the right and 5 px above the top of the work area {} put the \
                         window at {}, flush with its top right corner",
                            judge::describe(&work),
                            judge::describe(&real)
                        ))
                    }
                    (_, real) => Verdict::Fail(format!(
                        "saved spot hanging off the top right of {}: GetWindowRect {:?}; {}",
                        judge::describe(&work),
                        real.map(|r| judge::describe(&r)),
                        self.last()
                    )),
                }
            }
        };
        self.record(Check::Clamped, verdict);
    }

    fn chart_checks(&mut self) -> Result<(), String> {
        let ready = self.edit(&settings(json!({"timeframe": "h4"}))).is_ok()
            && self
                .wait(5.0, |s| {
                    s.mode == "collapsed"
                        && s.locked
                        && s.visible
                        && s.targets.contains_key("ticker")
                        && judge::parse_ticker(&s.text).is_some_and(|t| t.label && t.decimals == 0)
                })
                .is_some();
        if !ready {
            let detail = format!(
                "the collapsed, locked ticker never came back; {}",
                self.last()
            );
            self.record(Check::Expand, Verdict::Fail(detail.clone()));
            return Err(detail);
        }
        let expanded = self.click_target("ticker", 15.0, |s| judge::charted(s, Timeframe::H4));
        let verdict = expanded.map_or_else(|v| v, |s| self.chart_verdict(&s, Timeframe::H4));
        if !self.record(Check::Expand, verdict) {
            return Err("the chart did not open".into());
        }
        for (timeframe, chosen, captured) in [
            (Timeframe::M15, Check::Timeframe15m, Check::Capture15m),
            (Timeframe::H4, Check::Timeframe4h, Check::Capture4h),
            (Timeframe::M1, Check::Timeframe1m, Check::Capture1m),
        ] {
            let label = timeframe.spec().label;
            let shown = self.click_target(label, 15.0, |s| judge::charted(s, timeframe));
            let verdict = shown.map_or_else(|v| v, |s| self.chart_verdict(&s, timeframe));
            if self.record(chosen, verdict) {
                let verdict = self.capture(label);
                self.record(captured, verdict);
            }
        }

        let band = self.snapshot().and_then(|s| {
            let ticker = judge::rect(s.targets.get("ticker")?);
            Some(center(&judge::chart_band(&judge::rect(&s.frame), &ticker)))
        });
        let collapse = match band {
            None => Verdict::Fail(format!("no chart to click; {}", self.last())),
            Some(at) => match self.click(at, Button::Left, 5.0, |s| s.mode == "collapsed") {
                Ok(s) => Verdict::Pass(format!(
                    "a click on the chart body at {at:?} closed it; window back to {}",
                    judge::describe(&judge::rect(&s.frame))
                )),
                Err(verdict) => verdict,
            },
        };
        self.record(Check::Collapse, collapse);

        let reopened = self.click_target("ticker", 15.0, |s| judge::charted(s, Timeframe::M1));
        let stored = os::read_settings().unwrap_or_default();
        let reexpand = match reopened {
            Ok(s) => Verdict::of(
                stored.contains(r#""timeframe":"m1""#),
                format!(
                    "reopened on 1m, the last chosen, with {} candles; the stored settings hold timeframe m1",
                    s.candles
                ),
                format!("reopened on 1m, but the stored settings are {stored}"),
            ),
            Err(verdict) => verdict,
        };
        self.record(Check::Reexpand, reexpand);
        let _ = self.click_target("ticker", 5.0, |s| s.mode == "collapsed");
        Ok(())
    }

    fn chart_verdict(&self, snapshot: &Snapshot, timeframe: Timeframe) -> Verdict {
        judge::chart(snapshot, timeframe, os::unix_now(), &os::kraken_price())
    }

    fn capture(&mut self, label: &str) -> Verdict {
        std::thread::sleep(Duration::from_millis(300));
        let Some(snapshot) = self.snapshot() else {
            return Verdict::Fail("no snapshot".into());
        };
        let frame = judge::rect(&snapshot.frame);
        let Some(ticker) = snapshot.targets.get("ticker").map(judge::rect) else {
            return Verdict::Fail("no ticker target".into());
        };
        let shot = match os::capture(&frame) {
            Ok(shot) => shot,
            Err(error) => return Verdict::Fail(format!("capture failed: {error}")),
        };
        let name = format!("expanded-{label}.png");
        let path = self.artifacts.join(&name);
        if let Err(error) = shot.write_png(&path) {
            return Verdict::Fail(format!("writing {}: {error}", path.display()));
        }
        let band = judge::chart_band(&frame, &ticker);
        let local = (
            (band.x - frame.x) as usize,
            (band.y - frame.y) as usize,
            band.width as usize,
            band.height as usize,
        );
        let found = pixels::inspect(&pixels::crop(&shot.bgra, shot.width as usize, local));
        judge::capture(&found, &name)
    }

    fn opaque_to_clicks(&mut self) {
        let (strip, hole) = Role::clear_points(self.screen);
        self.clear.raise();
        let control_window_present = os::window_at(strip.0, strip.1) == self.clear.hwnd;
        let (game_before, clear_before) = (self.game.presses(), self.clear.presses());
        let control_passed_through = os::click(hole.0, hole.1, Button::Left).is_ok()
            && wait_until(2.0, || {
                (self
                    .game
                    .presses()
                    .zip(game_before)
                    .is_some_and(|(a, b)| a > b))
                .then_some(())
            })
            .is_some()
            && self.clear.presses() == clear_before;
        wait_until(TOPMOST_BOUND_MS as f64 / 1000.0, || {
            self.overlay_above_game().then_some(())
        });

        let clear = settings(json!({"timeframe": "m1", "opacity": 0}));
        let collapsed = self.edit(&clear).is_ok()
            && self
                .wait(5.0, |s| {
                    s.opacity == 0.0 && s.mode == "collapsed" && s.visible
                })
                .is_some();
        let mut misses = Vec::new();
        let mut grid_points = 0;
        let mut corner_click_toggled = false;
        let mut corner_click_reached_standin = false;
        if collapsed {
            grid_points += self.grid_misses(&mut misses);
            let frame = self
                .snapshot()
                .map(|s| judge::rect(&s.frame))
                .unwrap_or_default();
            let corner = (frame.x as i32 + 2, frame.y as i32 + 2);
            match self.click(corner, Button::Left, 10.0, |s| s.mode == "expanded") {
                Ok(_) => {
                    corner_click_toggled = true;
                    grid_points += self.grid_misses(&mut misses);
                }
                Err(Verdict::Fail(detail)) if detail.contains("stand-in") => {
                    corner_click_reached_standin = true
                }
                Err(_) => {}
            }
            let _ = self.click_target("ticker", 5.0, |s| s.mode == "collapsed");
        } else {
            misses.push(format!("the opacity 0 edit never landed; {}", self.last()));
        }
        let verdict = judge::opaque_to_clicks(&judge::OpaqueFacts {
            control_window_present,
            control_passed_through,
            misses,
            grid_points,
            corner_click_toggled,
            corner_click_reached_standin,
        });
        self.record(Check::OpaqueToClicks, verdict);
    }

    /// WindowFromPoint over a 7 by 5 grid of the overlay, 1 px inside its edges. Returns the points tried.
    fn grid_misses(&self, misses: &mut Vec<String>) -> usize {
        let Some(frame) = self.snapshot().map(|s| judge::rect(&s.frame)) else {
            misses.push("no snapshot".into());
            return 0;
        };
        let mut tried = 0;
        for i in 0..7 {
            for j in 0..5 {
                let x = (frame.x + 1.0 + (frame.width - 3.0) * f64::from(i) / 6.0) as i32;
                let y = (frame.y + 1.0 + (frame.height - 3.0) * f64::from(j) / 4.0) as i32;
                tried += 1;
                let found = os::window_at(x, y);
                if found != self.overlay {
                    misses.push(format!("{x},{y}->{}", os::class_of(found)));
                }
            }
        }
        tried
    }

    fn focus_kept(&mut self, watcher: &watch::Watcher) {
        let timeline = watcher.timeline();
        let activations = self.snapshot().map_or(u32::MAX, |s| s.activations);
        let covered = format!(
            "launch, {} settings edits and {} clicks on the overlay",
            self.edits, self.clicks
        );
        let verdict = judge::focus_kept(&judge::FocusFacts {
            timeline: &timeline,
            ended_ms: os::ms(),
            standin: os::id(self.game.hwnd),
            app_pid: self.app_pid(),
            excused: &self.excused,
            activations,
            steal_seen: watcher.saw(self.helper.hwnd),
            covered: &covered,
        });
        self.record(Check::FocusKept, verdict);
    }

    fn undisturbed(&mut self) {
        let verdict = match self.snapshot() {
            Some(s) => judge::undisturbed(s.foreign_presses, s.presses),
            None => Verdict::Fail("no snapshot".into()),
        };
        self.record(Check::Undisturbed, verdict);
    }

    fn open_menu(&mut self) -> Result<(), Verdict> {
        let at = self
            .target("ticker")
            .ok_or_else(|| Verdict::Fail(format!("no ticker target; {}", self.last())))?;
        self.click(at, Button::Right, 3.0, |s| {
            s.menu_open && s.targets.contains_key("menu:quit")
        })
        .map(|_| ())
    }

    fn settings_window(&mut self) {
        if let Err(verdict) = self.open_menu() {
            self.record(Check::SettingsWindow, verdict);
            return;
        }
        if let Err(verdict) = self.click_target("menu:settings", 3.0, |s| !s.menu_open) {
            self.record(Check::SettingsWindow, verdict);
            return;
        }
        let pid = self.app_pid();
        let window = wait_until(5.0, || {
            os::top_level().into_iter().find(|&h| {
                os::pid_of(h) == pid && os::class_of(h) == "BTCTickerSettings" && os::visible(h)
            })
        });
        let Some(window) = window else {
            let verdict = judge::settings_window(&judge::SettingsWindowFacts {
                opened: false,
                took_foreground: false,
                trackbars: 0,
                expected_trackbars: judge::expected_trackbars(),
                toggle_applied: false,
                closed: false,
                foreground_returned: false,
            });
            self.record(Check::SettingsWindow, verdict);
            return;
        };
        let took_foreground =
            wait_until(3.0, || (os::foreground() == window).then_some(())).is_some();
        std::thread::sleep(Duration::from_millis(300));
        let controls = os::children(window);
        let trackbars = controls
            .iter()
            .filter(|&&h| os::class_of(h) == "msctls_trackbar32")
            .count();
        if let Some(shot) = os::window_rect(window).and_then(|r| os::capture(&r).ok()) {
            let _ = shot.write_png(&self.artifacts.join("settings.png"));
        }
        let checkbox = controls
            .iter()
            .copied()
            .find(|&h| os::text_of(h) == "Show BTC label")
            .and_then(os::window_rect);
        let toggle_applied = checkbox.is_some_and(|r| {
            os::click(center(&r).0, center(&r).1, Button::Left).is_ok()
                && self
                    .wait(5.0, |s| {
                        judge::parse_ticker(&s.text).is_some_and(|t| !t.label)
                    })
                    .is_some()
                && os::read_settings().is_some_and(|s| s.contains(r#""showLabel":false"#))
        });
        // SAFETY: posts to a window of the app's; a stale handle fails harmlessly.
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        let closed = wait_until(3.0, || (!os::visible(window)).then_some(())).is_some();
        let foreground_returned =
            wait_until(3.0, || (os::foreground() == self.game.hwnd).then_some(())).is_some();
        let verdict = judge::settings_window(&judge::SettingsWindowFacts {
            opened: true,
            took_foreground,
            trackbars,
            expected_trackbars: judge::expected_trackbars(),
            toggle_applied,
            closed,
            foreground_returned,
        });
        self.record(Check::SettingsWindow, verdict);
        if !foreground_returned {
            let _ = self.bring_standin_forward();
        }
    }

    fn quit(&mut self) {
        let verdict = match self
            .open_menu()
            .and_then(|_| self.click_target("menu:quit", 0.0, |_| true))
        {
            Err(verdict) => verdict,
            Ok(_) => {
                let started = Instant::now();
                let exited = wait_until(10.0, || {
                    let app = self.app.as_mut()?;
                    app.child.try_wait().ok().flatten()
                });
                match exited {
                    Some(status) => Verdict::Pass(format!(
                        "Quit in the overlay menu ended the process in {} ms ({status})",
                        started.elapsed().as_millis()
                    )),
                    None => Verdict::Fail("the process was still running 10 s after Quit".into()),
                }
            }
        };
        self.record(Check::Quit, verdict);
    }
}
